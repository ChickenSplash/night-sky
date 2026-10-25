//! Adaptive Dormand-Prince 5(4) in cube-root mass coordinate. Dimensionless
//! volume v=(r/R0)^3 regularises the centre; I is integrated independently.

use super::*;

/// Dimensionless volume, pressure and axial moment of inertia.
type State = [f64; 3];

struct Step {
    state: State,
    error: State,
}

pub(super) struct Trial {
    pub regions: Vec<Region>,
    pub inertia_kg_m2: f64,
}

pub(super) struct Solver<'a> {
    inventory: &'a Inventory,
    outer_pressure_pa: f64,
    controls: Controls,
    eos: Density,
    mass: f64,
    radius_scale: f64,
    pressure_scale: f64,
    pub work: Work,
}

impl<'a> Solver<'a> {
    pub fn new(
        inventory: &'a Inventory,
        outer_pressure_pa: f64,
        controls: Controls,
        eos: Density,
    ) -> Self {
        let mass = inventory.total_mass_kg();
        let radius_scale = (3.0 * mass / (4.0 * PI * 4100.0)).cbrt();
        let pressure_scale =
            3.0 * GRAVITATIONAL_CONSTANT * mass.powi(2) / (4.0 * PI * radius_scale.powi(4));
        Self {
            inventory,
            outer_pressure_pa,
            controls,
            eos,
            mass,
            radius_scale,
            pressure_scale,
            work: Work::default(),
        }
    }

    pub fn pressure_scale(&self) -> f64 {
        self.pressure_scale
    }

    fn density(&mut self, material: Material, pressure: f64) -> Result<f64, SolveError> {
        if self.work.eos_evaluations == self.controls.max_eos_evaluations {
            return Err(SolveError::WorkBudgetExhausted(WorkLimit::EosEvaluations));
        }
        self.work.eos_evaluations += 1;
        (self.eos)(material, pressure).map_err(SolveError::Eos)
    }

    fn sample(&mut self, material: Material, x: f64, y: [f64; 3]) -> Result<Sample, SolveError> {
        let pressure_pa = y[1] * self.pressure_scale;
        Ok(Sample {
            radius_m: y[0].cbrt() * self.radius_scale,
            enclosed_mass_kg: x.powi(3) * self.mass,
            pressure_pa,
            density_kg_m3: self.density(material, pressure_pa)?,
        })
    }

    /// None means central pressure is too low, not a valid partial structure.
    pub fn trial(&mut self, central_pressure: f64) -> Result<Option<Trial>, SolveError> {
        let mut regions = Vec::new();
        let mut x = 0.0;
        let mut y = [0.0, central_pressure / self.pressure_scale, 0.0];
        let mut cumulative = 0.0;
        let mut steps = 0;
        for material in [Material::Metal, Material::Silicate] {
            let mass = self.inventory.mass_kg(material);
            if mass == 0.0 {
                continue;
            }
            cumulative += mass;
            let end = (cumulative / self.mass).cbrt();
            if end <= x {
                return Err(SolveError::InvalidClosure {
                    material,
                    quantity: "unrepresentable mass interval",
                });
            }
            let mut samples = vec![self.sample(material, x, y)?];
            if x == 0.0 {
                // Regular centre expansion. O(x^2) density corrections are
                // below default solver tolerance at x<=1e-6. This expansion
                // covers <=1e-18 M, including its mass and inertia, not a void.
                x = 1e-6_f64.min(end * 1e-4);
                let a = 4100.0 / samples[0].density_kg_m3;
                y = [
                    a * x.powi(3),
                    y[1] - x.powi(2) / (2.0 * a.powf(4.0 / 3.0)),
                    0.4 * a.powf(2.0 / 3.0) * x.powi(5),
                ];
                if y[1] < self.outer_pressure_pa / self.pressure_scale {
                    return Ok(None);
                }
                samples.push(self.sample(material, x, y)?);
            }
            let mut h = self.controls.max_step.min(x * 0.25).min(end - x);
            while x < end {
                if steps == self.controls.max_steps_per_trial {
                    return Err(SolveError::WorkBudgetExhausted(WorkLimit::Integration));
                }
                steps += 1;
                self.work.attempted_steps += 1;
                h = h.min(end - x);
                if h < 1e-14 * x.max(1e-6) || x + h == x {
                    if y[1] * self.pressure_scale - self.outer_pressure_pa
                        < 1.0_f64.max(1e-8 * central_pressure)
                    {
                        return Ok(None);
                    }
                    return Err(SolveError::NonConvergence {
                        material,
                        mass_fraction: x.powi(3),
                    });
                }
                let Some(Step { state: next, error }) = self.step(material, x, y, h)? else {
                    h *= 0.25;
                    continue;
                };
                let norm = error
                    .iter()
                    .enumerate()
                    .map(|(i, e)| {
                        let scale = if i == 1 {
                            y[1].abs().max(next[1].abs()).max(1.0 / self.pressure_scale)
                        } else {
                            (next[i] - y[i]).abs()
                        };
                        e.abs() / (self.controls.relative_tolerance * scale.max(1e-100))
                    })
                    .fold(0.0_f64, f64::max);
                if norm <= 1.0 {
                    x = if h == end - x { end } else { x + h };
                    y = next;
                    if y[1] * self.pressure_scale < self.outer_pressure_pa {
                        return Ok(None);
                    }
                    samples.push(self.sample(material, x, y)?);
                }
                h = (h * (0.9 * norm.max(1e-10).powf(-0.2)).clamp(0.2, 4.0))
                    .min(self.controls.max_step);
            }
            regions.push(Region {
                inventory: Inventory::new(&[(material, mass)]).unwrap(),
                samples,
            });
        }
        Ok(Some(Trial {
            regions,
            inertia_kg_m2: y[2] * self.mass * self.radius_scale.powi(2),
        }))
    }

    fn derivative(
        &mut self,
        material: Material,
        x: f64,
        y: [f64; 3],
    ) -> Result<Option<[f64; 3]>, SolveError> {
        // Trial stages outside physical pressure/volume are rejected and retried
        // at smaller step. Never clamp pressure or extrapolate the EOS.
        if y[1] < 0.0 || y[0] <= 0.0 {
            return Ok(None);
        }
        let rho = self.density(material, y[1] * self.pressure_scale)?;
        Ok(Some([
            3.0 * x.powi(2) * 4100.0 / rho,
            -x.powi(5) / y[0].powf(4.0 / 3.0),
            2.0 * x.powi(2) * y[0].powf(2.0 / 3.0),
        ]))
    }

    fn step(
        &mut self,
        material: Material,
        x: f64,
        y: [f64; 3],
        h: f64,
    ) -> Result<Option<Step>, SolveError> {
        const C: [f64; 7] = [0.0, 1.0 / 5.0, 3.0 / 10.0, 4.0 / 5.0, 8.0 / 9.0, 1.0, 1.0];
        const A: [[f64; 6]; 7] = [
            [0.0; 6],
            [1.0 / 5.0, 0.0, 0.0, 0.0, 0.0, 0.0],
            [3.0 / 40.0, 9.0 / 40.0, 0.0, 0.0, 0.0, 0.0],
            [44.0 / 45.0, -56.0 / 15.0, 32.0 / 9.0, 0.0, 0.0, 0.0],
            [
                19372.0 / 6561.0,
                -25360.0 / 2187.0,
                64448.0 / 6561.0,
                -212.0 / 729.0,
                0.0,
                0.0,
            ],
            [
                9017.0 / 3168.0,
                -355.0 / 33.0,
                46732.0 / 5247.0,
                49.0 / 176.0,
                -5103.0 / 18656.0,
                0.0,
            ],
            [
                35.0 / 384.0,
                0.0,
                500.0 / 1113.0,
                125.0 / 192.0,
                -2187.0 / 6784.0,
                11.0 / 84.0,
            ],
        ];
        const B4: [f64; 7] = [
            5179.0 / 57600.0,
            0.0,
            7571.0 / 16695.0,
            393.0 / 640.0,
            -92097.0 / 339200.0,
            187.0 / 2100.0,
            1.0 / 40.0,
        ];
        let mut k = [[0.0; 3]; 7];
        let mut next = y;
        for stage in 0..7 {
            let mut state = y;
            for (j, derivative) in k.iter().enumerate().take(stage) {
                for i in 0..3 {
                    state[i] += h * A[stage][j] * derivative[i];
                }
            }
            let Some(derivative) = self.derivative(material, x + C[stage] * h, state)? else {
                return Ok(None);
            };
            k[stage] = derivative;
            if stage == 6 {
                next = state;
            }
        }
        let mut error = [0.0; 3];
        for i in 0..3 {
            // Form the embedded difference before adding the large accumulated
            // state, preserving the error estimate for thin trace regions.
            error[i] = h * k
                .iter()
                .enumerate()
                .map(|(j, k)| k[i] * (if j < 6 { A[6][j] } else { 0.0 } - B4[j]))
                .sum::<f64>();
        }
        Ok(Some(Step { state: next, error }))
    }
}
