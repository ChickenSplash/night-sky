//! A core integrated outward by the radial kernel and a gas shell integrated
//! inward from its outer boundary, matched where they meet. H/He self-gravity,
//! interface load, volumes and inertia are all part of the one fit.
use super::{
    coupled,
    dry::{self, Region, SolveError, WorkLimit},
    radial, water,
};
use crate::{
    system::{
        composition::Inventory,
        materials::{Material, cold, hhe, hhe_mixture, suspended, water::loaded},
    },
    units::{EARTH_MASS_KG, GRAVITATIONAL_CONSTANT},
};

mod mixture;
mod shell;
mod thermal;
pub use thermal::MODEL_ID as THERMAL_MODEL_ID;
pub use thermal::Thermal;
pub use thermal::{INTRINSIC_RANGE_K, intrinsic_k};
#[cfg(test)]
mod tests;

pub const MODEL_ID: &str = "self_gravitating_loaded_hhe_v1";
pub const WATER_MODEL_ID: &str = "loaded_effective_water_compression_v1";
pub const TRACE_MODEL_ID: &str = "passive_secondary_envelope_host_volume_v1";
pub const OUTER_PRESSURE_PA: f64 = 1000.0;
// Operational 0.01-bar shell boundary, not a physical surface or radius
// reference. The shell starts exactly there; this is only the pressure below
// which a core trial counts as having run out before its surface.
const PRESSURE_TOLERANCE_PA: f64 = 1.0;
/// Pressure step allowed where the gas meets the core: the layer-boundary
/// tolerance of `physical::validate_record`.
fn interface_tolerance_pa(pressure_pa: f64) -> f64 {
    0.02_f64.max(1e-9 * pressure_pa)
}
const GRAVITY_TOLERANCE: f64 = 1e-8;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Error {
    InvalidInput(&'static str),
    Eos(hhe::Error),
    Mixture(hhe_mixture::Error),
    Radial(SolveError),
    TraceDomain,
    MissingOneBarBoundary,
    NonConvergence,
    BoundaryResolution { residual_pa: f64, central_pa: f64 },
    WorkBudget,
    DensityInversion,
}

impl Error {
    pub fn label(self) -> &'static str {
        match self {
            Self::Eos(e) | Self::Radial(SolveError::Envelope(e)) => e.label(),
            Self::Mixture(e) | Self::Radial(SolveError::Mixture(e)) => e.label(),
            Self::InvalidInput(reason) => reason,
            Self::TraceDomain => "secondary envelope trace domain",
            Self::MissingOneBarBoundary => "one-bar event unresolved",
            Self::BoundaryResolution { .. } => "central shooting resolution",
            Self::NonConvergence => "reference gravity convergence",
            Self::DensityInversion => "outward envelope density inversion",
            Self::WorkBudget | Self::Radial(SolveError::WorkBudgetExhausted(_)) => {
                "envelope work budget"
            }
            Self::Radial(SolveError::NonConvergence { .. }) => "radial integration convergence",
            Self::Radial(_) => "heavy radial domain",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Envelope {
    pub(super) regions: Vec<Region>,
    pub(super) inventory: Inventory,
    pub(super) inertia_kg_m2: f64,
    pub(super) reference_radius_m: f64,
    reference_enclosed_mass_kg: f64,
    reference_pressure_pa: f64,
    pub(super) thermal: Thermal,
    pub(super) evaluations: usize,
    pub(super) iterations: usize,
    pub(super) gravity_residual: f64,
    interface_radius_residual_m: f64,
    interface_pressure_residual_pa: f64,
    molecular_mixture: bool,
    secondary_enriched: bool,
    effective_boundary_pa: Option<f64>,
    suspended: bool,
    /// Disjoint phase SUB-allocations, already included in each region inventory.
    condensates: Vec<Inventory>,
}

impl Envelope {
    pub fn density_model_id(&self) -> &'static str {
        if self.suspended {
            if self.uses_effective_components() {
                suspended::DEEP_MODEL_ID
            } else {
                suspended::MODEL_ID
            }
        } else if self.uses_effective_components() {
            hhe_mixture::COMPOSITE_MODEL_ID
        } else if self.secondary_enriched {
            hhe_mixture::ENRICHED_MODEL_ID
        } else if self.molecular_mixture {
            hhe_mixture::MODEL_ID
        } else {
            hhe::MODEL_ID
        }
    }
    pub fn uses_effective_components(&self) -> bool {
        self.effective_boundary_pa
            .is_some_and(|p| self.regions.iter().any(|r| r.inner().pressure_pa > p))
    }
    pub fn uses_extended_mixture(&self) -> bool {
        self.secondary_enriched || self.uses_effective_components()
    }
    pub fn region_density_model_id(&self, region: &Region) -> &'static str {
        if self
            .effective_boundary_pa
            .is_some_and(|p| region.inner().pressure_pa > p)
        {
            hhe_mixture::EFFECTIVE_MODEL_ID
        } else if self.suspended {
            suspended::MODEL_ID
        } else if self.secondary_enriched {
            hhe_mixture::ENRICHED_MODEL_ID
        } else if self.molecular_mixture {
            hhe_mixture::MODEL_ID
        } else {
            hhe::MODEL_ID
        }
    }
    pub fn uses_molecular_mixture(&self) -> bool {
        self.molecular_mixture
    }
    pub fn uses_suspended_condensates(&self) -> bool {
        self.suspended
    }
    pub fn region_condensate(&self, index: usize) -> Inventory {
        self.condensates[index]
    }
    pub fn reference_gravity_m_s2(&self) -> f64 {
        GRAVITATIONAL_CONSTANT * self.reference_enclosed_mass_kg / self.reference_radius_m.powi(2)
    }
    /// None when the whole gas shell is below one bar. Its reference is then
    /// the condensed boundary, not an extrapolated pressure level inside rock.
    pub fn one_bar_radius_m(&self) -> Option<f64> {
        (self.reference_pressure_pa == 1e5).then_some(self.reference_radius_m)
    }
    pub fn reference_pressure_pa(&self) -> f64 {
        self.reference_pressure_pa
    }
    pub fn outer_pressure_residual_pa(&self) -> f64 {
        self.regions.last().unwrap().outer().pressure_pa - OUTER_PRESSURE_PA
    }
    /// Gas base minus core surface, where the two integrations meet.
    pub fn interface_radius_residual_m(&self) -> f64 {
        self.interface_radius_residual_m
    }
    /// Core surface minus gas base.
    pub fn interface_pressure_residual_pa(&self) -> f64 {
        self.interface_pressure_residual_pa
    }
    pub fn regions(&self) -> &[Region] {
        &self.regions
    }
    pub fn thermal(&self) -> Thermal {
        self.thermal
    }
    pub fn gravity_residual(&self) -> f64 {
        self.gravity_residual
    }
}

/// Generation's retained budget is immutable. Protected traces remain heavy;
/// Accessible secondaries either use the original bounded host-volume trace
/// approximation or the explicitly screened H/He-rich molecular mixture.
pub fn solve(
    retained: &Inventory,
    protected: &Inventory,
    equilibrium_k: f64,
    intrinsic_k: f64,
    controls: coupled::Controls,
) -> Result<(water::WaterStructure, Envelope), Error> {
    let table = hhe::Table::local().map_err(Error::Eos)?;
    solve_with_density(
        retained,
        protected,
        equilibrium_k,
        intrinsic_k,
        controls,
        &|p, t, y| table.density_kg_m3(p, t, y),
    )
}

type Density<'a> = &'a dyn Fn(f64, f64, f64) -> Result<f64, hhe::Error>;

fn solve_with_density(
    retained: &Inventory,
    protected: &Inventory,
    equilibrium_k: f64,
    intrinsic_k: f64,
    controls: coupled::Controls,
    eos: Density<'_>,
) -> Result<(water::WaterStructure, Envelope), Error> {
    if !equilibrium_k.is_finite() || equilibrium_k <= 0.0 {
        return Err(Error::InvalidInput("equilibrium temperature"));
    }
    controls.radial.validate().map_err(Error::Radial)?;
    if controls.max_iterations > 32 || controls.max_total_eos_evaluations > 16_000_000 {
        return Err(Error::InvalidInput("controls"));
    }
    let host = Inventory::new(&Material::ALL.map(|m| {
        (
            m,
            if matches!(m, Material::Metal | Material::Silicate | Material::Water) {
                retained.mass_kg(m)
            } else {
                0.0
            },
        )
    }))
    .unwrap();
    for m in Material::ALL {
        if protected.mass_kg(m) > retained.mass_kg(m)
            || (protected.mass_kg(m) > 0.0
                && matches!(
                    m,
                    Material::Metal
                        | Material::Silicate
                        | Material::Water
                        | Material::Hydrogen
                        | Material::Helium
                ))
        {
            return Err(Error::InvalidInput("protected allocation"));
        }
    }
    let heavy = host
        .checked_add(protected)
        .map_err(|_| Error::InvalidInput("heavy inventory"))?;
    let gas = Inventory::new(&Material::ALL.map(|m| (m, retained.mass_kg(m) - heavy.mass_kg(m))))
        .map_err(|_| Error::InvalidInput("envelope inventory"))?;
    let light = gas.mass_kg(Material::Hydrogen) + gas.mass_kg(Material::Helium);
    if host.total_mass_kg() <= 0.0
        || light <= 0.0
        || heavy.total_mass_kg() / EARTH_MASS_KG > 100.0
        || !(0.0001..=1200.0).contains(&(retained.total_mass_kg() / EARTH_MASS_KG))
    {
        return Err(Error::InvalidInput("mass domain"));
    }
    if protected.total_mass_kg() / heavy.total_mass_kg() > dry::protected::MAX_TRACE_FRACTION {
        return Err(Error::TraceDomain);
    }
    let secondary_fraction = (gas.total_mass_kg() - light) / gas.total_mass_kg();
    let mixture = if secondary_fraction > dry::protected::MAX_TRACE_FRACTION {
        Some(hhe_mixture::Mixture::new_enriched(&gas).map_err(Error::Mixture)?)
    } else {
        None
    };
    let scale = heavy.total_mass_kg() / host.total_mass_kg();
    // Validates the temperatures before any work is spent.
    Thermal::new(equilibrium_k.max(20.0), intrinsic_k, 10.0)?;
    let heavy_parcels =
        Inventory::new(&Material::ALL.map(|m| (m, host.mass_kg(m) * scale))).unwrap();
    let heavy_density = |m, p: f64, side, _: &mut dry::Work, _| {
        if m == Material::Water {
            // Separate shallow and deep barotropes, never phase/thermal EOS.
            // Honour the one-sided branch at the located model boundary.
            let shallow = match side {
                radial::DensitySide::Branch(i) => i > 0,
                radial::DensitySide::Equilibrium => p < loaded::BOUNDARY_PA,
            };
            if shallow {
                return loaded::density_kg_m3(p)
                    .ok_or(SolveError::InvalidInput("shallow water pressure"));
            }
            if !(0.0..=cold::MAX_PRESSURE_PA).contains(&p) {
                return Err(SolveError::InvalidInput("heavy water pressure"));
            }
            Ok(1460.0 + 0.00311 * p.powf(0.513))
        } else {
            cold::density_kg_m3(m, p).map_err(SolveError::Eos)
        }
    };
    let radial_controls = dry::Controls {
        max_eos_evaluations: controls
            .radial
            .max_eos_evaluations
            .min(controls.max_total_eos_evaluations),
        ..controls.radial
    };
    // The core is integrated outward from the centre and the gas inward from
    // its outer boundary; the two meet at the core's surface. Neither then has
    // to resolve a 10 mbar boundary as the difference of two interior loads.
    let mut solver = radial::Solver::new(
        &heavy_parcels,
        OUTER_PRESSURE_PA,
        radial_controls,
        &heavy_density,
        radial::Policy {
            pressure_floor_pa: OUTER_PRESSURE_PA,
            boundary_tolerance_pa: Some(PRESSURE_TOLERANCE_PA),
            interfaces: vec![radial::PressureInterface {
                material: Material::Water,
                pressure_pa: loaded::BOUNDARY_PA,
            }],
            ..Default::default()
        },
    );
    let carrier = shell::Gas {
        eos,
        inventory: gas,
        helium_fraction: gas.mass_kg(Material::Helium) / light,
        mixture,
    };
    let mut matcher = shell::Matcher::new(
        &carrier,
        equilibrium_k.max(20.0),
        intrinsic_k,
        heavy_parcels.total_mass_kg(),
        controls,
    );
    let ceiling = cold::MAX_PRESSURE_PA;
    let seed = (2.0 * solver.pressure_scale()).clamp(2.0 * OUTER_PRESSURE_PA, ceiling);
    let mut search = shell::Search::new(OUTER_PRESSURE_PA, ceiling, Some(seed), 0.5 * seed);
    let mut refusal = None;
    let mut closest: Option<(f64, f64)> = None;
    let mut solved = None;
    for _ in 0..radial_controls.max_shooting_iterations {
        let Some(central) = search.next() else {
            break;
        };
        solver.work.shooting_iterations += 1;
        let trial = solver.trial(central);
        if solver.work.eos_evaluations + matcher.work.eos_evaluations
            >= controls.max_total_eos_evaluations
        {
            return Err(Error::WorkBudget);
        }
        let trial = match trial {
            Err(SolveError::WorkBudgetExhausted(_)) => return Err(Error::WorkBudget),
            Err(error) => {
                refusal = Some(Error::Radial(error));
                search.record(central, true, None);
                continue;
            }
            Ok(None) => {
                search.record(central, false, None);
                continue;
            }
            Ok(Some(trial)) => trial,
        };
        let core = trial.regions.last().unwrap().outer();
        matcher.limit = controls
            .max_total_eos_evaluations
            .saturating_sub(solver.work.eos_evaluations);
        match matcher.fit(core) {
            Err(failure) if failure.is_final() => return Err(failure.error),
            Err(failure) => {
                refusal = Some(failure.error);
                search.record(central, true, None);
            }
            Ok(shell::Fit::Heavier) => search.record(central, false, None),
            Ok(shell::Fit::Matched(shell)) => {
                let base = shell.base().pressure_pa;
                let residual = core.pressure_pa - base;
                #[cfg(test)]
                if std::env::var_os("NS_ENVELOPE_TRACE").is_some() {
                    eprintln!(
                        "trial {}: central={central} base={base} residual={residual} gas work={}",
                        solver.work.shooting_iterations, matcher.work.eos_evaluations
                    );
                }
                if residual.abs() <= interface_tolerance_pa(base) {
                    solved = Some((trial, *shell, central));
                    break;
                }
                if closest.is_none_or(|(r, _)| residual.abs() < r.abs()) {
                    closest = Some((residual, central));
                }
                search.record(central, residual > 0.0, Some(residual));
            }
        }
    }
    let Some((mut trial, shell, central)) = solved else {
        return Err(match (refusal, closest) {
            (Some(error), _) => error,
            (None, Some((residual_pa, central_pa))) => Error::BoundaryResolution {
                residual_pa,
                central_pa,
            },
            (None, None) => Error::Radial(SolveError::Unbracketed {
                maximum_central_pressure_pa: ceiling,
            }),
        });
    };
    let mut total_work = solver.work.eos_evaluations + matcher.work.eos_evaluations;
    let cumulative = dry::Work {
        eos_evaluations: total_work,
        attempted_steps: solver.work.attempted_steps + matcher.work.attempted_steps,
        ..solver.work
    };
    let core = trial.regions.last().unwrap().outer();
    let base = shell.base();
    let thermal = shell.thermal;
    // A shell wholly below one bar is referenced to the condensed boundary itself.
    let reference = match reference_sample(&shell.regions)? {
        sample if sample == base => core,
        sample => sample,
    };
    let residual = GRAVITATIONAL_CONSTANT * reference.enclosed_mass_kg
        / reference.radius_m.powi(2)
        / thermal.gravity_m_s2
        - 1.0;
    if residual.abs() > GRAVITY_TOLERANCE {
        return Err(Error::NonConvergence);
    }
    let heavy_inertia = trial.inertia_kg_m2;
    let inertia = heavy_inertia + shell.inertia_kg_m2;
    let mut envelope_regions = shell.regions;
    for region in &mut envelope_regions {
        region.inventory = gas
            .scaled(region.inventory.total_mass_kg() / gas.total_mass_kg())
            .map_err(|_| Error::InvalidInput("gas origins"))?;
    }
    let mut heavy_regions = Vec::new();
    for mut region in trial.regions.drain(..) {
        let full = region.inventory.total_mass_kg();
        region.inventory = region
            .inventory
            .scaled(1.0 / scale)
            .unwrap()
            .checked_add(&protected.scaled(full / heavy.total_mass_kg()).unwrap())
            .unwrap();
        heavy_regions.push(region);
    }
    validate(retained, &heavy_regions, &envelope_regions, inertia)?;
    let suspended = shell.route.as_ref().is_some_and(|r| r.suspended);
    let condensates = if suspended {
        phase_allocations(
            &envelope_regions,
            &gas,
            thermal,
            shell.route.as_ref().unwrap().boundary_pa,
            &mut total_work,
            controls.max_total_eos_evaluations,
        )?
    } else {
        vec![Inventory::default(); envelope_regions.len()]
    };
    let diagnostics = dry::Diagnostics {
        central_pressure_pa: central,
        boundary_residual_pa: core.pressure_pa - base.pressure_pa,
        work: dry::Work {
            eos_evaluations: total_work,
            ..cumulative
        },
    };
    let interior = water::envelope_interior(heavy_regions, heavy_inertia, diagnostics, thermal);
    Ok((
        interior,
        Envelope {
            regions: envelope_regions,
            inventory: gas,
            inertia_kg_m2: inertia,
            reference_radius_m: reference.radius_m,
            reference_enclosed_mass_kg: reference.enclosed_mass_kg,
            reference_pressure_pa: reference.pressure_pa,
            thermal,
            evaluations: total_work,
            iterations: cumulative.shooting_iterations,
            gravity_residual: residual,
            interface_radius_residual_m: base.radius_m - core.radius_m,
            interface_pressure_residual_pa: core.pressure_pa - base.pressure_pa,
            molecular_mixture: mixture.is_some(),
            secondary_enriched: secondary_fraction
                > hhe_mixture::MAX_SECONDARY_MASS_FRACTION * (1.0 + 8.0 * f64::EPSILON),
            effective_boundary_pa: shell.route.as_ref().map(|r| r.boundary_pa),
            suspended,
            condensates,
        },
    ))
}

/// Use the located one-bar event when present. A genuinely sub-bar shell uses
/// its inner condensed boundary; a missed event in a thicker shell stays an
/// error. No pressure interpolation, radius rescaling or new physical cutoff.
fn reference_sample(regions: &[Region]) -> Result<dry::Sample, Error> {
    let mut gas = regions
        .iter()
        .filter(|r| r.inventory().mass_kg(Material::Hydrogen) > 0.0);
    let base = gas.next().ok_or(Error::MissingOneBarBoundary)?.inner();
    if base.pressure_pa <= 1e5 {
        return Ok(base);
    }
    regions
        .iter()
        .filter(|r| r.inventory().mass_kg(Material::Hydrogen) > 0.0)
        .map(|r| r.outer())
        .find(|s| s.pressure_pa == 1e5)
        .ok_or(Error::MissingOneBarBoundary)
}

/// Mass-coordinate trapezoid of local phase fractions. This is a reporting
/// suballocation only: the RK density already includes both phases at every
/// evaluation, including self-gravity. No material is moved between parcels.
fn phase_allocations(
    regions: &[Region],
    gas: &Inventory,
    thermal: Thermal,
    boundary: f64,
    work: &mut usize,
    limit: usize,
) -> Result<Vec<Inventory>, Error> {
    let fractions = gas.scaled(1.0 / gas.total_mass_kg()).unwrap();
    regions
        .iter()
        .map(|region| {
            if region.outer().pressure_pa >= boundary {
                return Ok(Inventory::default());
            }
            let mut average = [0.0; 5];
            let mut mass = 0.0;
            let mut previous: Option<(dry::Sample, Inventory)> = None;
            for &sample in region.samples() {
                if *work >= limit {
                    return Err(Error::WorkBudget);
                }
                *work += 1;
                let p = sample.pressure_pa;
                let condensate = if p >= boundary {
                    Inventory::default()
                } else {
                    suspended::State::new(gas, p, thermal.temperature_k(p)?, 1.0)
                        .map_err(|e| Error::Mixture(hhe_mixture::Error::Suspended(e)))?
                        .condensate
                };
                if let Some((old, old_c)) = previous {
                    let dm = sample.enclosed_mass_kg - old.enclosed_mass_kg;
                    let weight = dm / (mass + dm);
                    for (i, m) in hhe_mixture::SECONDARIES.into_iter().enumerate() {
                        if fractions.mass_kg(m) == 0.0 {
                            continue;
                        }
                        let ratio = (old_c.mass_kg(m) + condensate.mass_kg(m))
                            / (2.0 * fractions.mass_kg(m));
                        average[i] = (1.0 - weight) * average[i] + weight * ratio;
                    }
                    mass += dm;
                }
                previous = Some((sample, condensate));
            }
            let entries = hhe_mixture::SECONDARIES.map(|m| {
                let i = hhe_mixture::SECONDARIES
                    .iter()
                    .position(|&a| a == m)
                    .unwrap();
                (m, region.inventory.mass_kg(m) * average[i])
            });
            let result =
                Inventory::new(&entries).map_err(|_| Error::InvalidInput("phase allocations"))?;
            if Material::ALL
                .into_iter()
                .any(|m| result.mass_kg(m) > region.inventory.mass_kg(m))
            {
                return Err(Error::InvalidInput("phase allocation closure"));
            }
            Ok(result)
        })
        .collect()
}

fn integrate_inertia(regions: &[Region]) -> f64 {
    regions
        .iter()
        .flat_map(|r| r.samples.windows(2))
        .map(|p| {
            (p[1].enclosed_mass_kg - p[0].enclosed_mass_kg)
                * (p[0].radius_m.powi(2) + p[1].radius_m.powi(2))
                / 3.0
        })
        .sum()
}

fn validate(
    requested: &Inventory,
    heavy: &[Region],
    gas: &[Region],
    inertia: f64,
) -> Result<(), Error> {
    let mut allocated = Inventory::default();
    let mut previous = None;
    for (index, r) in heavy.iter().chain(gas).enumerate() {
        allocated = allocated
            .checked_add(r.inventory())
            .map_err(|_| Error::InvalidInput("allocation closure"))?;
        let represented = r.outer().enclosed_mass_kg - r.inner().enclosed_mass_kg;
        if (represented / r.inventory().total_mass_kg() - 1.0).abs() > 1e-8 {
            return Err(Error::InvalidInput("region mass resolution"));
        }
        if let Some(p) = previous {
            let p: dry::Sample = p;
            let inner = r.inner();
            // The core and the gas are integrated towards each other, so
            // their meeting point alone closes to a tolerance, not exactly.
            let closed = if index == heavy.len() {
                (inner.radius_m - p.radius_m).abs() <= shell::RADIUS_TOLERANCE * p.radius_m
                    && (inner.pressure_pa - p.pressure_pa).abs()
                        <= interface_tolerance_pa(inner.pressure_pa)
            } else {
                p.radius_m == inner.radius_m && p.pressure_pa == inner.pressure_pa
            };
            if !closed || p.enclosed_mass_kg != inner.enclosed_mass_kg {
                return Err(Error::InvalidInput("interface closure"));
            }
        }
        for pair in r.samples.windows(2) {
            if pair[1].density_kg_m3 > pair[0].density_kg_m3 * (1.0 + 1e-9) {
                return Err(Error::DensityInversion);
            }
            if pair[1].radius_m <= pair[0].radius_m
                || pair[1].enclosed_mass_kg <= pair[0].enclosed_mass_kg
                || pair[1].pressure_pa > pair[0].pressure_pa
                || ![
                    pair[1].radius_m,
                    pair[1].enclosed_mass_kg,
                    pair[1].pressure_pa,
                    pair[1].density_kg_m3,
                ]
                .iter()
                .all(|v| v.is_finite())
            {
                return Err(Error::InvalidInput("profile ordering"));
            }
        }
        previous = Some(r.outer());
    }
    for m in Material::ALL {
        if (allocated.mass_kg(m) - requested.mass_kg(m)).abs() > 1e-10 * requested.mass_kg(m) {
            return Err(Error::InvalidInput("origin closure"));
        }
    }
    let top = gas.last().unwrap().outer();
    if !inertia.is_finite()
        || inertia <= 0.0
        || inertia > 2.0 / 3.0 * requested.total_mass_kg() * top.radius_m.powi(2)
    {
        return Err(Error::InvalidInput("inertia"));
    }
    let independent = integrate_inertia(heavy) + integrate_inertia(gas);
    if (independent / inertia - 1.0).abs() > 1e-3 {
        return Err(Error::InvalidInput("independent inertia check"));
    }
    Ok(())
}
