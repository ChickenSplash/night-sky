//! The gas parcel as a shell integrated inward from its outer boundary, and the
//! search for the outer radius that seats it on a given core. Log pressure is
//! the independent variable: the outer boundary, one bar and model events are
//! exact, and steps end on the H/He table's knots instead of straddling them.

use super::mixture::ComponentRoute;
use super::*;
use crate::system::materials::hhe::KNOT_DEX;
use std::f64::consts::PI;

/// Agreement between the gravity the thermal profile assumed and the one found.
const GRAVITY_MATCH: f64 = 1e-10;
/// Radius gap to the core that ends the search, and the widest gap accepted
/// when rounding stops it first. Both relative to the core radius.
const RADIUS_TARGET: f64 = 1e-11;
pub(super) const RADIUS_TOLERANCE: f64 = 1e-10;
/// Smallest step in ln P before an EOS refusal is taken as the domain edge.
const MIN_STEP: f64 = 1e-10;
/// A step in ln P this small means the tolerance cannot be met at all.
const UNDERFLOW: f64 = 1e-14;
const MAX_FIT_PASSES: usize = 96;

pub(super) struct Gas<'a> {
    pub eos: Density<'a>,
    pub inventory: Inventory,
    pub helium_fraction: f64,
    pub mixture: Option<hhe_mixture::Mixture>,
}

impl Gas<'_> {
    fn density(
        &self,
        thermal: Thermal,
        route: Option<&ComponentRoute>,
        p: f64,
        side: radial::DensitySide,
        work: &mut dry::Work,
        limit: usize,
    ) -> Result<f64, SolveError> {
        if work.eos_evaluations >= limit {
            return Err(SolveError::WorkBudgetExhausted(WorkLimit::EosEvaluations));
        }
        work.eos_evaluations += 1;
        let t = thermal
            .temperature_k(p)
            .map_err(|_| SolveError::InvalidInput("envelope thermal domain"))?;
        let density = (self.eos)(p, t, self.helium_fraction).map_err(SolveError::Envelope)?;
        match (self.mixture, route) {
            (Some(mix), Some(route)) if route.effective(p, side) => {
                route.density(mix, p, t, density, work, limit)
            }
            (Some(_), Some(route)) if route.suspended => {
                suspended::State::new(&self.inventory, p, t, density)
                    .map(|s| s.density_kg_m3)
                    .map_err(|e| SolveError::Mixture(hhe_mixture::Error::Suspended(e)))
            }
            (Some(mix), _) => mix
                .density_kg_m3(p, t, density)
                .map_err(SolveError::Mixture),
            (None, _) => Ok(density),
        }
    }
}

/// A refused shell and the pressure its integration had reached. Refusals grow
/// with depth, so a core whose surface is at least that deep cannot be seated.
#[derive(Clone, Copy)]
pub(super) struct Failure {
    pub error: Error,
    pub pressure_pa: f64,
    /// The trial outer radius was too large, not too small, to be solved.
    oversized: bool,
}

impl Failure {
    fn top(error: Error) -> Self {
        Self {
            error,
            pressure_pa: OUTER_PRESSURE_PA,
            oversized: false,
        }
    }
    fn unresolved() -> Self {
        Self {
            error: Error::NonConvergence,
            pressure_pa: f64::INFINITY,
            oversized: false,
        }
    }
    fn at(error: SolveError, pressure_pa: f64) -> Self {
        Self {
            error: match error {
                SolveError::WorkBudgetExhausted(_) => Error::WorkBudget,
                error => Error::Radial(error),
            },
            pressure_pa,
            oversized: false,
        }
    }
    /// No outer radius or central pressure can change this outcome.
    pub fn is_final(&self) -> bool {
        self.error == Error::WorkBudget || self.pressure_pa == OUTER_PRESSURE_PA
    }
}

pub(super) struct Shell {
    /// Outward order with hydrogen as the carrier, like the radial kernel.
    pub regions: Vec<Region>,
    pub inertia_kg_m2: f64,
    pub thermal: Thermal,
    pub route: Option<ComponentRoute>,
}

impl Shell {
    pub fn base(&self) -> dry::Sample {
        self.regions[0].inner()
    }
}

pub(super) enum Fit {
    Matched(Box<Shell>),
    /// Every shell reaching the core is deeper than the core's surface pressure.
    Heavier,
}

enum Pass {
    Done {
        regions: Vec<Region>,
        inertia_kg_m2: f64,
    },
    /// Reference gravity found, when it differs from the one assumed.
    Gravity(f64),
}

/// The thermal profile, component route and model branch a step is taken on.
#[derive(Clone, Copy)]
struct Column<'r> {
    thermal: Thermal,
    route: Option<&'r ComponentRoute>,
    /// Model events crossed outwards so far.
    branch: usize,
}

struct Step {
    state: radial::State,
    error: radial::State,
    density: f64,
}

pub(super) struct Matcher<'a> {
    gas: &'a Gas<'a>,
    equilibrium_k: f64,
    intrinsic_k: f64,
    heavy_mass_kg: f64,
    gas_mass_kg: f64,
    relative_tolerance: f64,
    max_iterations: usize,
    pub work: dry::Work,
    pub limit: usize,
    knots: Vec<f64>,
    /// Every shell solved so far, by outer radius: outer radius, base radius
    /// and base pressure. Base radius rises and base pressure falls with it.
    solved: Vec<[f64; 3]>,
    /// Largest outer radius refused for depth, and smallest refused for size.
    refused: Option<(f64, Failure)>,
    oversized: Option<(f64, Failure)>,
    /// Outer radius and converged gravity of the last solved shell.
    gravity: Option<(f64, f64)>,
    /// d(base radius)/d(outer radius) and d(found gravity)/d(assumed gravity).
    slope: f64,
    contraction: f64,
}

impl<'a> Matcher<'a> {
    pub fn new(
        gas: &'a Gas<'a>,
        equilibrium_k: f64,
        intrinsic_k: f64,
        heavy_mass_kg: f64,
        controls: coupled::Controls,
    ) -> Self {
        Self {
            gas,
            equilibrium_k,
            intrinsic_k,
            heavy_mass_kg,
            gas_mass_kg: gas.inventory.total_mass_kg(),
            relative_tolerance: controls.radial.relative_tolerance,
            max_iterations: controls.max_iterations,
            work: dry::Work::default(),
            limit: controls.max_total_eos_evaluations,
            knots: (0..=280)
                .map(|j| 10.0_f64.powf(f64::from(j) * KNOT_DEX))
                .filter(|&p| p > OUTER_PRESSURE_PA)
                .collect(),
            solved: Vec::new(),
            refused: None,
            oversized: None,
            gravity: None,
            slope: 1.0,
            contraction: 0.0,
        }
    }

    /// The shell whose base radius equals the core's, or the side the core is on.
    /// A shell depends on its outer radius alone, never on the core, so every
    /// shell solved for an earlier core narrows the search for this one.
    pub fn fit(&mut self, core: dry::Sample) -> Result<Fit, Failure> {
        let radius = core.radius_m;
        self.heavy_mass_kg = core.enclosed_mass_kg;
        let split = self.solved.partition_point(|shell| shell[1] < radius);
        let below = split.checked_sub(1).map(|i| self.solved[i]);
        let above = self.solved.get(split).copied();
        let nearest = [below, above]
            .into_iter()
            .flatten()
            .map(|shell| (shell[1] - radius).abs())
            .fold(f64::INFINITY, f64::min);
        let (seed, step) = if nearest.is_finite() {
            (None, 2.0 * nearest / self.slope)
        } else {
            (Some(radius * 1.02), radius * 0.02)
        };
        let mut search = Search::new(radius, f64::INFINITY, seed, step);
        if let Some((outer, _)) = self.oversized
            && above.is_none()
        {
            search.record(outer, true, None);
        }
        if let Some((outer, _)) = self.refused
            && below.is_none_or(|shell| shell[0] < outer)
        {
            search.record(outer, false, None);
        }
        for shell in [above, below].into_iter().flatten() {
            search.record(shell[0], shell[1] > radius, Some(shell[1] - radius));
        }
        let mut best: Option<(f64, Shell)> = None;
        for _ in 0..MAX_FIT_PASSES {
            // Refusals deepen as the shell shrinks. Once a solved shell this
            // close above a refused one is still clearly too large, the match
            // itself is refused; which side the core is on follows from whether
            // even that lighter shell outweighs the core's surface pressure.
            if let Some((inner, failure)) = self.refused {
                let above = self
                    .solved
                    .get(self.solved.partition_point(|s| s[1] < radius));
                match (above, self.oversized) {
                    (Some(&[outer, base, pressure]), _)
                        if base - radius > 8.0 * self.slope.max(1.0) * (outer - inner) =>
                    {
                        return if pressure > core.pressure_pa {
                            Ok(Fit::Heavier)
                        } else {
                            Err(failure)
                        };
                    }
                    (None, Some((outer, _))) if outer - inner <= 1e-3 * outer => {
                        return Err(failure);
                    }
                    _ => {}
                }
            }
            let Some(outer) = search.next() else {
                break;
            };
            match self.shell(outer) {
                Err(failure) if failure.is_final() => return Err(failure),
                Err(failure) => {
                    search.record(outer, failure.oversized, None);
                    if failure.oversized {
                        if self.oversized.is_none_or(|(old, _)| outer < old) {
                            self.oversized = Some((outer, failure));
                        }
                    } else if self.refused.is_none_or(|(old, _)| outer > old) {
                        self.refused = Some((outer, failure));
                    }
                }
                Ok(shell) => {
                    let base = shell.base();
                    let gap = base.radius_m - radius;
                    let index = self.solved.partition_point(|s| s[0] < outer);
                    if let Some(near) = index
                        .checked_sub(1)
                        .map(|i| self.solved[i])
                        .or_else(|| self.solved.get(index).copied())
                    {
                        let slope = (base.radius_m - near[1]) / (outer - near[0]);
                        if slope.is_finite() && slope > 0.0 {
                            self.slope = slope;
                        }
                    }
                    self.solved
                        .insert(index, [outer, base.radius_m, base.pressure_pa]);
                    self.gravity = Some((outer, shell.thermal.gravity_m_s2));
                    if gap.abs() <= RADIUS_TARGET * radius {
                        return Ok(Fit::Matched(Box::new(shell)));
                    }
                    search.step = search.step.max(2.0 * gap.abs() / self.slope);
                    search.record(outer, gap > 0.0, Some(gap));
                    if best.as_ref().is_none_or(|(g, _)| gap.abs() < g.abs()) {
                        best = Some((gap, shell));
                    }
                }
            }
        }
        match (best, self.refused) {
            (Some((gap, shell)), _) if gap.abs() <= RADIUS_TOLERANCE * radius => {
                Ok(Fit::Matched(Box::new(shell)))
            }
            (_, Some((_, failure))) => Err(failure),
            _ => Err(Failure::unresolved()),
        }
    }

    /// One shell below a trial outer radius, with the thermal profile's
    /// gravity brought into agreement with the shell's own reference level.
    fn shell(&mut self, outer_radius_m: f64) -> Result<Shell, Failure> {
        let total = self.heavy_mass_kg + self.gas_mass_kg;
        let mut gravity = match self.gravity {
            Some((outer, gravity)) => gravity * (outer / outer_radius_m).powi(2),
            None => GRAVITATIONAL_CONSTANT * total / outer_radius_m.powi(2),
        };
        let mut previous: Option<(f64, f64)> = None;
        for _ in 0..self.max_iterations {
            // Gravity outside the thermal profile's range reflects the trial
            // radius, not the planet: the search moves away from it.
            let thermal =
                Thermal::new(self.equilibrium_k, self.intrinsic_k, gravity).map_err(|error| {
                    Failure {
                        error,
                        pressure_pa: f64::INFINITY,
                        oversized: gravity < 1.0,
                    }
                })?;
            let route = self
                .gas
                .mixture
                .map(|_| ComponentRoute::new(thermal, self.gas.inventory))
                .transpose()
                .map_err(Failure::top)?;
            match self.integrate(outer_radius_m, thermal, route.as_ref())? {
                Pass::Done {
                    regions,
                    inertia_kg_m2,
                } => {
                    return Ok(Shell {
                        regions,
                        inertia_kg_m2,
                        thermal,
                        route,
                    });
                }
                Pass::Gravity(found) => {
                    if let Some((old, old_found)) = previous {
                        let contraction = (found - old_found) / (gravity - old);
                        if contraction.is_finite() && contraction.abs() < 0.5 {
                            self.contraction = contraction;
                        }
                    }
                    previous = Some((gravity, found));
                    gravity += (found - gravity) / (1.0 - self.contraction);
                }
            }
        }
        Err(Failure::unresolved())
    }

    /// Ascending pressures where a step must end: model events (flagged) and
    /// the places the density interpolation or thermal profile has a kink.
    fn breakpoints(&self, thermal: Thermal, events: &[f64]) -> Vec<(f64, bool)> {
        let mut points: Vec<(f64, bool)> = events.iter().map(|&p| (p, true)).collect();
        points.extend(self.knots.iter().map(|&p| (p, false)));
        points.extend(
            (0..=60)
                .filter_map(|i| {
                    thermal.pressure_at_pa(10.0_f64.powf(2.0 + f64::from(i) * KNOT_DEX))
                })
                .chain([thermal.boundary_pressure_pa()])
                .filter(|&p| p > OUTER_PRESSURE_PA && p < 1e14)
                .map(|p| (p, false)),
        );
        points.sort_by(|a, b| a.0.total_cmp(&b.0).then(b.1.cmp(&a.1)));
        let mut kept: Vec<(f64, bool)> = Vec::with_capacity(points.len());
        for point in points {
            match kept.last_mut() {
                Some(last) if point.0 - last.0 <= 1e-9 * point.0 => {
                    if point.1 && !last.1 {
                        *last = point;
                    }
                }
                _ => kept.push(point),
            }
        }
        kept
    }

    fn density(&mut self, column: Column<'_>, pressure: f64) -> Result<f64, SolveError> {
        self.gas.density(
            column.thermal,
            column.route,
            pressure,
            radial::DensitySide::Branch(column.branch),
            &mut self.work,
            self.limit,
        )
    }

    fn enclosed(&self, above_kg: f64) -> f64 {
        self.heavy_mass_kg + (self.gas_mass_kg - above_kg)
    }

    fn integrate(
        &mut self,
        outer_radius_m: f64,
        thermal: Thermal,
        route: Option<&ComponentRoute>,
    ) -> Result<Pass, Failure> {
        let events: Vec<f64> = route.map_or_else(
            || vec![1e5],
            |r| r.interfaces().iter().map(|i| i.pressure_pa).collect(),
        );
        let points = self.breakpoints(thermal, &events);
        // Every event lies below the outer boundary, so all are crossed there.
        let mut column = Column {
            thermal,
            route,
            branch: events.len(),
        };
        let mut p = OUTER_PRESSURE_PA;
        // Mass above, depth below the outer boundary and the shell's own moment
        // of inertia: each starts at zero, so a thin shell keeps its precision.
        let mut y = [0.0; 3];
        let top = self.density(column, p).map_err(|e| Failure::at(e, p))?;
        let mut samples = vec![dry::Sample {
            radius_m: outer_radius_m,
            enclosed_mass_kg: self.enclosed(0.0),
            pressure_pa: p,
            density_kg_m3: top,
        }];
        let mut regions: Vec<Vec<dry::Sample>> = Vec::new();
        let mut h = f64::INFINITY;
        let mut referenced = false;
        let mut closed = false;
        'shell: for &(target, event) in &points {
            while p < target {
                let span = (target / p).ln();
                let (step, end) = if h >= span {
                    (span, target)
                } else {
                    (h, p * h.exp())
                };
                self.work.attempted_steps += 1;
                let trial = match self.step(column, outer_radius_m, p, y, step, end) {
                    Ok(trial) => trial,
                    Err(error @ SolveError::WorkBudgetExhausted(_)) => {
                        return Err(Failure::at(error, p));
                    }
                    Err(error) => {
                        // A step may reach past the base into a refused state.
                        h = step * 0.25;
                        if h < MIN_STEP {
                            return Err(Failure::at(error, p));
                        }
                        continue;
                    }
                };
                let norm = self.norm(&trial, y);
                if norm <= 1.0 && trial.state[0] >= self.gas_mass_kg {
                    // The base lies inside this step: finish in mass coordinate.
                    if let Some(base) = self.bridge(column, outer_radius_m, p, y, target)? {
                        samples.push(dry::Sample {
                            radius_m: outer_radius_m - base.state[1],
                            enclosed_mass_kg: self.heavy_mass_kg,
                            pressure_pa: p * base.state[0].exp(),
                            density_kg_m3: base.density,
                        });
                        y = [self.gas_mass_kg, base.state[1], base.state[2]];
                        closed = true;
                        break 'shell;
                    }
                    h = step * 0.5;
                    if h < UNDERFLOW {
                        return Err(Failure::unresolved());
                    }
                    continue;
                }
                if norm <= 1.0 {
                    p = end;
                    y = trial.state;
                    samples.push(dry::Sample {
                        radius_m: outer_radius_m - y[1],
                        enclosed_mass_kg: self.enclosed(y[0]),
                        pressure_pa: p,
                        density_kg_m3: trial.density,
                    });
                    if event && p == target {
                        if p == 1e5 {
                            let found = GRAVITATIONAL_CONSTANT * self.enclosed(y[0])
                                / (outer_radius_m - y[1]).powi(2);
                            if (found / thermal.gravity_m_s2 - 1.0).abs() > GRAVITY_MATCH {
                                return Ok(Pass::Gravity(found));
                            }
                            referenced = true;
                        }
                        regions.push(std::mem::take(&mut samples));
                        column.branch -= 1;
                        let density = self.density(column, p).map_err(|e| Failure::at(e, p))?;
                        samples.push(dry::Sample {
                            density_kg_m3: density,
                            ..*regions.last().unwrap().last().unwrap()
                        });
                    }
                }
                h = step * (0.9 * norm.max(1e-10).powf(-0.2)).clamp(0.2, 4.0);
                if h < UNDERFLOW {
                    return Err(Failure::unresolved());
                }
            }
        }
        if !closed {
            return Err(Failure::at(
                SolveError::InvalidInput("envelope thermal domain"),
                p,
            ));
        }
        if !referenced {
            // A shell wholly below one bar is referenced to its base.
            let found =
                GRAVITATIONAL_CONSTANT * self.heavy_mass_kg / (outer_radius_m - y[1]).powi(2);
            if (found / thermal.gravity_m_s2 - 1.0).abs() > GRAVITY_MATCH {
                return Ok(Pass::Gravity(found));
            }
        }
        regions.push(samples);
        let count = regions.len();
        let mut outward = Vec::with_capacity(count);
        for (index, mut samples) in regions.into_iter().enumerate().rev() {
            samples.reverse();
            // The innermost region takes the remainder, so the parcel closes exactly.
            let mass = if index == count - 1 {
                self.gas_mass_kg - (self.enclosed(0.0) - samples.last().unwrap().enclosed_mass_kg)
            } else {
                samples.last().unwrap().enclosed_mass_kg - samples[0].enclosed_mass_kg
            };
            if mass.is_nan() || mass <= 0.0 || samples.len() < 2 {
                return Err(Failure::at(
                    SolveError::InvalidClosure {
                        material: Material::Hydrogen,
                        quantity: "unrepresentable phase allocation",
                    },
                    p,
                ));
            }
            outward.push(Region {
                inventory: Inventory::new(&[(Material::Hydrogen, mass)]).unwrap(),
                samples,
            });
        }
        Ok(Pass::Done {
            regions: outward,
            inertia_kg_m2: y[2],
        })
    }

    fn norm(&self, step: &Step, from: radial::State) -> f64 {
        (0..3)
            .map(|i| {
                step.error[i].abs()
                    / (self.relative_tolerance * (step.state[i] - from[i]).abs().max(1e-100))
            })
            .fold(0.0_f64, f64::max)
    }

    /// One Dormand-Prince step of `step` in ln P from `p` to exactly `end`.
    fn step(
        &mut self,
        column: Column<'_>,
        outer_radius_m: f64,
        p: f64,
        y: radial::State,
        step: f64,
        end: f64,
    ) -> Result<Step, SolveError> {
        let (heavy, gas) = (self.heavy_mass_kg, self.gas_mass_kg);
        let mut density = 0.0;
        let result = radial::rk_step(0.0, y, 1.0, |t, z| {
            let mass = heavy + (gas - z[0]);
            let radius = outer_radius_m - z[1];
            if !(radius > 0.0 && mass > 0.0 && z.iter().all(|v| v.is_finite())) {
                return Err(SolveError::NonConvergence {
                    material: Material::Hydrogen,
                    mass_fraction: 1.0,
                });
            }
            let pressure = if t == 0.0 {
                p
            } else if t == 1.0 {
                end
            } else {
                p * (t * step).exp()
            };
            density = self.density(column, pressure)?;
            let weight = pressure / (GRAVITATIONAL_CONSTANT * mass) * step;
            let above = 4.0 * PI * radius.powi(4) * weight;
            Ok(Some([
                above,
                radius.powi(2) * weight / density,
                2.0 / 3.0 * radius.powi(2) * above,
            ]))
        })?
        .unwrap();
        Ok(Step {
            state: result.state,
            error: result.error,
            density,
        })
    }

    /// One step in mass above, from the current state to exactly the whole gas
    /// parcel. Returns ln(P/p), depth and inertia there, or `None` when the
    /// base is too far for one accurate step or lies past `limit`.
    fn bridge(
        &mut self,
        column: Column<'_>,
        outer_radius_m: f64,
        p: f64,
        y: radial::State,
        limit: f64,
    ) -> Result<Option<Step>, Failure> {
        let (heavy, gas) = (self.heavy_mass_kg, self.gas_mass_kg);
        let span = gas - y[0];
        let mut density = 0.0;
        let from = [0.0, y[1], y[2]];
        let result = radial::rk_step(0.0, from, 1.0, |t, z| {
            let radius = outer_radius_m - z[1];
            if !(radius > 0.0 && z.iter().all(|v| v.is_finite())) {
                return Err(SolveError::NonConvergence {
                    material: Material::Hydrogen,
                    mass_fraction: 1.0,
                });
            }
            let mass = if t == 1.0 {
                heavy
            } else {
                heavy + (gas - (y[0] + t * span))
            };
            let pressure = p * z[0].exp();
            density = self.density(column, pressure)?;
            let area = 4.0 * PI * radius.powi(2);
            Ok(Some([
                span * GRAVITATIONAL_CONSTANT * mass / (area * radius.powi(2) * pressure),
                span / (area * density),
                span * 2.0 / 3.0 * radius.powi(2),
            ]))
        });
        let result = match result {
            Ok(result) => result.unwrap(),
            Err(error @ SolveError::WorkBudgetExhausted(_)) => {
                return Err(Failure::at(error, p));
            }
            Err(_) => return Ok(None),
        };
        let step = Step {
            state: result.state,
            error: result.error,
            density,
        };
        Ok((self.norm(&step, from) <= 1.0 && p * step.state[0].exp() <= limit).then_some(step))
    }
}

/// Root search on a residual that rises with its argument. Each probe is
/// classified low or high, with a residual where one is defined: a secant
/// through the two newest residuals when it lands inside the known bounds,
/// otherwise bisection, or a doubling step while one side is still open.
pub(super) struct Search {
    floor: f64,
    ceiling: f64,
    seed: Option<f64>,
    step: f64,
    low: Option<f64>,
    high: Option<f64>,
    recent: [Option<(f64, f64)>; 2],
    stalled: u8,
}

impl Search {
    /// `floor` is known to be low and `ceiling` is the largest argument allowed.
    pub fn new(floor: f64, ceiling: f64, seed: Option<f64>, step: f64) -> Self {
        Self {
            floor,
            ceiling,
            seed,
            step,
            low: None,
            high: None,
            recent: [None; 2],
            stalled: 0,
        }
    }

    pub fn record(&mut self, x: f64, high: bool, residual: Option<f64>) {
        if let Some(residual) = residual {
            match self.recent[0] {
                Some((_, last)) if residual.abs() > 0.5 * last.abs() => self.stalled += 1,
                _ => self.stalled = 0,
            }
            self.recent = [Some((x, residual)), self.recent[0]];
        }
        if high {
            self.high = Some(x);
        } else {
            self.low = Some(x);
        }
    }

    /// Strictly inside the known bounds, or `None` when nothing is left there.
    pub fn next(&mut self) -> Option<f64> {
        if let Some(seed) = self.seed.take() {
            return Some(seed);
        }
        let lower = self.low.unwrap_or(self.floor);
        let upper = self.high.unwrap_or(self.ceiling);
        if self.stalled < 2
            && let [Some((x1, f1)), Some((x0, f0))] = self.recent
        {
            let x = x1 - f1 * (x1 - x0) / (f1 - f0);
            let reach = 8.0 * self.step;
            if x > lower
                && x < upper
                && (self.high.is_some() || x <= lower + reach)
                && (self.low.is_some() || x >= upper - reach)
            {
                return Some(x);
            }
        }
        self.stalled = 0;
        let x = match (self.low, self.high) {
            (Some(low), None) => {
                let x = (low + self.step).min(self.ceiling);
                self.step *= 2.0;
                x
            }
            (None, Some(high)) if high - self.step > self.floor => {
                let x = high - self.step;
                self.step *= 2.0;
                x
            }
            _ => lower + (upper - lower) * 0.5,
        };
        (x > lower && x < upper || self.high.is_none() && x > lower && x <= upper).then_some(x)
    }
}
