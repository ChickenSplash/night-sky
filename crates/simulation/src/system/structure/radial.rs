//! Adaptive Dormand-Prince 5(4) in cube-root mass coordinate. Dimensionless
//! volume v=(r/R0)^3 regularises the centre; I is integrated independently.

use super::super::{composition::Inventory, materials::Material};
use super::dry::{Controls, Region, Sample, SolveError, Work, WorkLimit};
use super::profile;
use crate::units::GRAVITATIONAL_CONSTANT;
use std::f64::consts::PI;

/// Internal kernel policy. Dry callers retain their original tolerance policy;
/// shallow water uses an absolute pressure scale independent of core pressure.
#[derive(Clone, Default)]
pub(super) struct Policy {
    pub pressure_floor_pa: f64,
    pub boundary_tolerance_pa: Option<f64>,
    pub pressure_error_scale_pa: Option<f64>,
    /// Descending pressure order within each material.
    pub interfaces: Vec<PressureInterface>,
}

/// A phase or model discontinuity, located before changing density.
#[derive(Clone, Copy)]
pub(super) struct PressureInterface {
    pub material: Material,
    pub pressure_pa: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum DensitySide {
    Equilibrium,
    /// Number of this material's pressure events already crossed outwards.
    Branch(usize),
}

// The kernel charges dispatch; a nested EOS must additionally debit its probes,
// even on failure, without exceeding the supplied request ceiling.
type Density<'a> =
    &'a dyn Fn(Material, f64, DensitySide, &mut Work, usize) -> Result<f64, SolveError>;

/// Dimensionless volume, pressure and axial moment of inertia.
type State = [f64; 3];

struct Step {
    state: State,
    error: State,
}

/// An invalid RK stage rejects only the proposed step, never the central load.
#[derive(Debug, PartialEq)]
enum StepError {
    InvalidState,
    Solve(SolveError),
}

pub(super) struct Trial {
    pub regions: Vec<Region>,
    pub density_sides: Vec<DensitySide>,
    pub inertia_kg_m2: f64,
    pub hydrogen_inner_inertia_kg_m2: Option<f64>,
}

/// Chooses the next trial central pressure inside a shooting bracket. The
/// boundary residual is only defined on the high side, so two high-side
/// trials give a secant aimed inside the accepted residual band; anything
/// doubtful falls back to bisection, which alone guarantees progress.
#[derive(Default)]
pub(super) struct Shooting {
    /// Most recent high-side (central pressure, boundary residual), newest first.
    high: [Option<(f64, f64)>; 2],
    /// Central pressure of the current upper bound, with or without a residual.
    bound: Option<f64>,
    /// The last trial fell below the root: bisect before trusting the slope again.
    undershot: bool,
}

impl Shooting {
    /// Record the trial now bounding the bracket from above. `None` means its
    /// residual is unknown (a classified high-side refusal). Repeating the
    /// current bound changes nothing.
    pub fn high(&mut self, central_pressure: f64, residual: Option<f64>) {
        if self.bound == Some(central_pressure) {
            return;
        }
        self.bound = Some(central_pressure);
        self.high = [
            residual.map(|r| (central_pressure, r)),
            self.high[0].filter(|_| residual.is_some()),
        ];
        self.undershot = false;
    }

    pub fn low(&mut self) {
        self.undershot = true;
    }

    /// Strictly inside (low, high) whenever such a value is representable.
    pub fn next(&self, low: f64, high: f64, tolerance: f64) -> f64 {
        let bisection = low + (high - low) * 0.5;
        let [Some((p1, r1)), Some((p0, r0))] = self.high else {
            return bisection;
        };
        let target = 0.5 * tolerance;
        if self.undershot || p1 != high || p0 <= p1 || r0 <= r1 || r1 <= target {
            return bisection;
        }
        let secant = p1 - (r1 - target) * (p0 - p1) / (r0 - r1);
        if secant > low && secant < high {
            secant
        } else {
            bisection
        }
    }
}

pub(super) struct Solver<'a> {
    inventory: &'a Inventory,
    outer_pressure_pa: f64,
    controls: Controls,
    eos: Density<'a>,
    policy: Policy,
    mass: f64,
    radius_scale: f64,
    pressure_scale: f64,
    pub work: Work,
    side: DensitySide,
    next_interface: Option<PressureInterface>,
}

impl<'a> Solver<'a> {
    pub fn new(
        inventory: &'a Inventory,
        outer_pressure_pa: f64,
        controls: Controls,
        eos: Density<'a>,
        policy: Policy,
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
            policy,
            mass,
            radius_scale,
            pressure_scale,
            work: Work::default(),
            side: DensitySide::Equilibrium,
            next_interface: None,
        }
    }

    pub fn pressure_scale(&self) -> f64 {
        self.pressure_scale
    }

    /// Re-integrate a previous solution on the new inventory/load, then grow a
    /// local bracket in the necessary direction. The hint is never a solution
    /// or a domain override. Every probe uses this solver's existing budgets.
    pub fn seeded_bracket(
        &mut self,
        seed: Option<f64>,
        maximum: f64,
        classify_upper: impl Fn(Result<Option<Trial>, SolveError>) -> Result<Option<Trial>, SolveError>,
    ) -> Result<Option<(f64, f64, Option<Trial>)>, SolveError> {
        let Some(seed) =
            seed.filter(|p| p.is_finite() && *p > self.outer_pressure_pa && *p <= maximum)
        else {
            return Ok(None);
        };
        let initial = self.trial(seed);
        let mut width = (seed * 1e-6).max(1.0);
        let mut low = self.outer_pressure_pa;
        let mut high = seed;
        let upwards = matches!(initial, Ok(None));
        let mut upper = if upwards {
            low = seed;
            None
        } else {
            classify_upper(initial)?
        };
        loop {
            // An already acceptable hint needs no extra bracketing probes.
            if let Some(trial) = &upper {
                let residual =
                    trial.regions.last().unwrap().outer().pressure_pa - self.outer_pressure_pa;
                if (0.0..=self.policy.boundary_tolerance_pa.unwrap_or(0.0)).contains(&residual) {
                    return Ok(Some((low, high, upper)));
                }
            }
            let next = if upwards {
                (seed + width).min(maximum)
            } else {
                (seed - width).max(self.outer_pressure_pa)
            };
            if !upwards && next == self.outer_pressure_pa {
                return Ok(Some((next, high, upper)));
            }
            if upwards && next <= low {
                return Err(SolveError::Unbracketed {
                    maximum_central_pressure_pa: maximum,
                });
            }
            if self.work.bracket_expansions == self.controls.max_bracket_expansions {
                return Err(SolveError::WorkBudgetExhausted(WorkLimit::Bracketing));
            }
            self.work.bracket_expansions += 1;
            match self.trial(next) {
                Ok(None) => {
                    low = next;
                    if !upwards {
                        return Ok(Some((low, high, upper)));
                    }
                }
                trial => {
                    high = next;
                    upper = classify_upper(trial)?;
                    if upwards {
                        return Ok(Some((low, high, upper)));
                    }
                }
            }
            width *= 2.0;
        }
    }

    fn density(&mut self, material: Material, pressure: f64) -> Result<f64, SolveError> {
        if self.work.eos_evaluations == self.controls.max_eos_evaluations {
            return Err(SolveError::WorkBudgetExhausted(WorkLimit::EosEvaluations));
        }
        self.work.eos_evaluations += 1;
        (self.eos)(
            material,
            pressure,
            self.side,
            &mut self.work,
            self.controls.max_eos_evaluations,
        )
    }

    fn sample(&mut self, material: Material, x: f64, y: [f64; 3]) -> Result<Sample, SolveError> {
        let pressure_pa = self.physical_pressure(material, y[1]);
        Ok(Sample {
            radius_m: y[0].cbrt() * self.radius_scale,
            enclosed_mass_kg: x.powi(3) * self.mass,
            pressure_pa,
            density_kg_m3: self.density(material, pressure_pa)?,
        })
    }

    /// Recover an exactly imposed event endpoint after nondimensionalisation.
    /// Multiplying P/scale back by scale can round one ulp onto the wrong EOS
    /// branch. Only exact coordinate equality is recognised, never a tolerance
    /// band, pressure offset or an extrapolated state.
    fn physical_pressure(&self, material: Material, coordinate: f64) -> f64 {
        self.policy
            .interfaces
            .iter()
            .find(|i| i.material == material && i.pressure_pa / self.pressure_scale == coordinate)
            .map_or(coordinate * self.pressure_scale, |i| i.pressure_pa)
    }

    /// None means central pressure is too low, not a valid partial structure.
    pub fn trial(&mut self, central_pressure: f64) -> Result<Option<Trial>, SolveError> {
        profile::time(profile::Stage::Trial, || self.integrate(central_pressure))
    }

    fn integrate(&mut self, central_pressure: f64) -> Result<Option<Trial>, SolveError> {
        let mut regions = Vec::new();
        let mut density_sides = Vec::new();
        let mut x = 0.0;
        let mut y = [0.0, central_pressure / self.pressure_scale, 0.0];
        let mut cumulative = 0.0;
        let mut steps = 0;
        let mut hydrogen_inner_inertia_kg_m2 = None;
        // Hydrogen is also an internal carrier coordinate for a full H/He
        // mixture parcel. Its caller restores the separate origin inventories.
        for material in [
            Material::Metal,
            Material::Silicate,
            Material::Water,
            Material::Hydrogen,
        ] {
            let mass = self.inventory.mass_kg(material);
            if mass == 0.0 {
                continue;
            }
            if material == Material::Hydrogen {
                hydrogen_inner_inertia_kg_m2 = Some(y[2] * self.mass * self.radius_scale.powi(2));
            }
            cumulative += mass;
            let end = (cumulative / self.mass).cbrt();
            if end <= x {
                return Err(SolveError::InvalidClosure {
                    material,
                    quantity: "unrepresentable mass interval",
                });
            }
            let interfaces: Vec<_> = self
                .policy
                .interfaces
                .iter()
                .copied()
                .filter(|i| i.material == material)
                .collect();
            let mut branch =
                interfaces.partition_point(|i| y[1] * self.pressure_scale <= i.pressure_pa);
            self.side = if interfaces.is_empty() {
                DensitySide::Equilibrium
            } else {
                DensitySide::Branch(branch)
            };
            self.next_interface = interfaces
                .get(branch)
                .copied()
                .filter(|i| i.pressure_pa > self.outer_pressure_pa);
            let mut region_mass_start = x.powi(3) * self.mass;
            let mut remaining_mass = mass;
            let mut samples = vec![self.sample(material, x, y)?];
            if x == 0.0 {
                // Regular centre expansion. O(x^2) density corrections are
                // below default solver tolerance at x<=1e-6. This expansion
                // covers <=1e-18 M, including its mass and inertia, not a void.
                x = 1e-6_f64.min(end * 1e-4);
                let a = 4100.0 / samples[0].density_kg_m3;
                if let Some(i) = self.next_interface {
                    // Keep the regular centre expansion strictly on its branch.
                    let gap = (y[1] * self.pressure_scale - i.pressure_pa) / self.pressure_scale;
                    x = x.min((0.5 * gap * a.powf(4.0 / 3.0)).sqrt());
                }
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
            let mut pressure_blocked = false;
            let mut invalid_stage = false;
            while x < end {
                if steps == self.controls.max_steps_per_trial {
                    return Err(SolveError::WorkBudgetExhausted(WorkLimit::Integration));
                }
                steps += 1;
                self.work.attempted_steps += 1;
                h = h.min(end - x);
                if let Some(i) = self.next_interface {
                    let gap = y[1] * self.pressure_scale - i.pressure_pa;
                    // Finish in pressure coordinates: P is the independent
                    // variable, so the endpoint is exactly the curve without
                    // shifting P, crossing a jump or extrapolating a branch.
                    if gap <= 1.0 {
                        let bridge = self.interface_step(material, x, y, i.pressure_pa)?;
                        let next_x = bridge.state[0];
                        if next_x <= x
                            || !bridge
                                .state
                                .iter()
                                .chain(bridge.error.iter())
                                .all(|v| v.is_finite())
                        {
                            return Err(SolveError::NonConvergence {
                                material,
                                mass_fraction: x.powi(3),
                            });
                        }
                        let norm = bridge
                            .error
                            .iter()
                            .enumerate()
                            .map(|(j, e)| {
                                let start = [x, y[0], y[2]][j];
                                e.abs()
                                    / (self.controls.relative_tolerance
                                        * (bridge.state[j] - start).abs().max(1e-100))
                            })
                            .fold(0.0_f64, f64::max);
                        if norm <= 1.0 && next_x - x <= self.controls.max_step && next_x < end {
                            x = next_x;
                            y = [
                                bridge.state[1],
                                i.pressure_pa / self.pressure_scale,
                                bridge.state[2],
                            ];
                            samples.push(self.sample_at_pressure(material, x, y, i.pressure_pa)?);
                            let allocated = x.powi(3) * self.mass - region_mass_start;
                            if !(0.0 < allocated && allocated < remaining_mass) {
                                return Err(SolveError::InvalidClosure {
                                    material,
                                    quantity: "unrepresentable phase allocation",
                                });
                            }
                            regions.push(Region {
                                inventory: Inventory::new(&[(material, allocated)]).unwrap(),
                                samples,
                            });
                            density_sides.push(self.side);
                            remaining_mass -= allocated;
                            region_mass_start = x.powi(3) * self.mass;
                            branch += 1;
                            self.side = DensitySide::Branch(branch);
                            self.next_interface = interfaces
                                .get(branch)
                                .copied()
                                .filter(|i| i.pressure_pa > self.outer_pressure_pa);
                            samples =
                                vec![self.sample_at_pressure(material, x, y, i.pressure_pa)?];
                            h = self.controls.max_step.min(end - x);
                            continue;
                        }
                        if steps == self.controls.max_steps_per_trial {
                            return Err(SolveError::WorkBudgetExhausted(WorkLimit::Integration));
                        }
                        steps += 1;
                        self.work.attempted_steps += 1;
                    }
                    let slope = self.pressure_scale * x.powi(5) / y[0].powf(4.0 / 3.0);
                    h = h.min(0.5 * gap / slope);
                }
                if h < 1e-14 * x.max(1e-6) || x + h == x {
                    if self.next_interface.is_some() || invalid_stage {
                        // Failure to resolve an interface is numerical, not a
                        // low central-pressure bracket or a missing phase mass.
                        return Err(SolveError::NonConvergence {
                            material,
                            mass_fraction: x.powi(3),
                        });
                    }
                    if (self.policy.boundary_tolerance_pa.is_some() && pressure_blocked)
                        || y[1] * self.pressure_scale - self.outer_pressure_pa
                            < self
                                .policy
                                .boundary_tolerance_pa
                                .unwrap_or_else(|| 1.0_f64.max(1e-8 * central_pressure))
                    {
                        return Ok(None);
                    }
                    return Err(SolveError::NonConvergence {
                        material,
                        mass_fraction: x.powi(3),
                    });
                }
                let Step { state: next, error } = match self.step(material, x, y, h) {
                    Ok(Some(step)) => step,
                    Ok(None) => {
                        pressure_blocked = true;
                        invalid_stage = false;
                        h *= 0.25;
                        continue;
                    }
                    Err(StepError::InvalidState) => {
                        pressure_blocked = false;
                        invalid_stage = true;
                        h *= 0.25;
                        continue;
                    }
                    Err(StepError::Solve(error)) => return Err(error),
                };
                pressure_blocked = false;
                invalid_stage = false;
                let norm = error
                    .iter()
                    .enumerate()
                    .map(|(i, e)| {
                        let scale = if i == 1 {
                            self.policy
                                .pressure_error_scale_pa
                                .map(|p| p / self.pressure_scale)
                                .unwrap_or_else(|| {
                                    y[1].abs().max(next[1].abs()).max(1.0 / self.pressure_scale)
                                })
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
                inventory: Inventory::new(&[(material, remaining_mass)]).unwrap(),
                samples,
            });
            density_sides.push(self.side);
        }
        Ok(Some(Trial {
            regions,
            density_sides,
            inertia_kg_m2: y[2] * self.mass * self.radius_scale.powi(2),
            hydrogen_inner_inertia_kg_m2,
        }))
    }

    fn derivative(
        &mut self,
        material: Material,
        x: f64,
        y: [f64; 3],
    ) -> Result<Option<[f64; 3]>, StepError> {
        // Pressure-floor stages are rejected and retried at smaller step.
        // Invalid volume/nonfinite stages also reject the proposed step. If
        // shrinking cannot recover, fail numerically, never classify the load
        // as too low. EOS errors and work limits remain fatal.
        // Never clamp pressure or extrapolate the EOS.
        if self.policy.boundary_tolerance_pa.is_some()
            && (y[0] <= 0.0 || !y.iter().all(|value| value.is_finite()))
        {
            return Err(StepError::InvalidState);
        }
        let pressure = self.physical_pressure(material, y[1]);
        if pressure < self.policy.pressure_floor_pa || y[0] <= 0.0 {
            return Ok(None);
        }
        if self
            .next_interface
            .is_some_and(|i| pressure < i.pressure_pa)
        {
            return Ok(None);
        }
        let rho = self.density(material, pressure).map_err(StepError::Solve)?;
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
    ) -> Result<Option<Step>, StepError> {
        rk_step(x, y, h, |x, y| self.derivative(material, x, y))
    }

    fn sample_at_pressure(
        &mut self,
        material: Material,
        x: f64,
        y: State,
        pressure_pa: f64,
    ) -> Result<Sample, SolveError> {
        Ok(Sample {
            radius_m: y[0].cbrt() * self.radius_scale,
            enclosed_mass_kg: x.powi(3) * self.mass,
            pressure_pa,
            density_kg_m3: self.density(material, pressure_pa)?,
        })
    }

    fn interface_step(
        &mut self,
        material: Material,
        x: f64,
        y: State,
        target_pa: f64,
    ) -> Result<Step, SolveError> {
        let start_pa = y[1] * self.pressure_scale;
        // Independent coordinate runs 0..1; use the exact target for its final
        // stage, avoiding cancellation in start + (target - start).
        let scale = self.pressure_scale;
        let step = rk_step(0.0, [x, y[0], y[2]], 1.0, |t, z| {
            if z[0] <= 0.0 || z[1] <= 0.0 || !z.iter().all(|v| v.is_finite()) {
                return Err(SolveError::NonConvergence {
                    material,
                    mass_fraction: z[0].powi(3),
                });
            }
            let pressure = if t == 1.0 {
                target_pa
            } else {
                start_pa + t * (target_pa - start_pa)
            };
            let rho = self.density(material, pressure)?;
            let dx = (start_pa - target_pa) / scale * z[1].powf(4.0 / 3.0) / z[0].powi(5);
            Ok(Some([
                dx,
                dx * 3.0 * z[0].powi(2) * 4100.0 / rho,
                dx * 2.0 * z[0].powi(2) * z[1].powf(2.0 / 3.0),
            ]))
        })?
        .unwrap();
        Ok(step)
    }
}

fn rk_step<E>(
    x: f64,
    y: State,
    h: f64,
    mut derivative: impl FnMut(f64, State) -> Result<Option<State>, E>,
) -> Result<Option<Step>, E> {
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
        let Some(derivative) = derivative(x + C[stage] * h, state)? else {
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

#[cfg(test)]
mod tests;
