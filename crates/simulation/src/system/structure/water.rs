//! Prescribed-isothermal water shell over cold Fe/MgSiO3 proxies.
//! Opt-in only: no atmosphere or climate. Deep ice is an explicit approximation.

use super::super::{
    composition::Inventory,
    materials::{Material, cold, water as eos},
};
use super::{dry, radial};
use crate::units::{EARTH_MASS_KG, GRAVITATIONAL_CONSTANT};
use std::f64::consts::PI;

pub mod protected;

pub use dry::{Controls, Diagnostics, Sample, SolveError, Work, WorkLimit};

pub const MODEL_ID: &str = "cold_refractories_isothermal_iapws_effective_ice_v5";
pub const BOUNDARY_TOLERANCE_PA: f64 = 0.01;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PhaseKnowledge {
    /// Cold refractory compression establishes neither temperature nor phase.
    Unresolved,
    Water(eos::Phase),
    /// Compression proxy, not an identified high-pressure polymorph.
    EffectiveHighPressureIce,
    /// Hot-fluid proxy, no resolved equilibrium phase or solidity claim.
    UnresolvedHotWater,
    /// Pressure-only envelope-loaded water compression, no temperature/phase EOS.
    UnresolvedCompression,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Region {
    profile: dry::Region,
    temperature_k: Option<f64>,
    phase: PhaseKnowledge,
    density_model_id: &'static str,
}

impl Region {
    pub fn inventory(&self) -> &Inventory {
        self.profile.inventory()
    }
    pub fn samples(&self) -> &[Sample] {
        self.profile.samples()
    }
    pub fn inner(&self) -> Sample {
        self.profile.inner()
    }
    pub fn outer(&self) -> Sample {
        self.profile.outer()
    }
    /// Prescribed host temperature only. Entrained-origin thermal states are unknown.
    pub fn temperature_k(&self) -> Option<f64> {
        self.temperature_k
    }
    /// Host phase only, never a phase assignment to entrained secondary origins.
    pub fn phase(&self) -> PhaseKnowledge {
        self.phase
    }
    pub fn origin_phase(&self, material: Material) -> Option<PhaseKnowledge> {
        (self.inventory().mass_kg(material) > 0.0).then_some({
            if matches!(
                material,
                Material::Metal | Material::Silicate | Material::Water
            ) {
                self.phase
            } else {
                PhaseKnowledge::Unresolved
            }
        })
    }
    /// Host density route; the structure model records any entrainment prescription.
    pub fn density_model_id(&self) -> &'static str {
        self.density_model_id
    }
    /// Physical-model limitations, not numerical error bars.
    pub fn approximation_notes(&self) -> &'static [&'static str] {
        match self.density_model_id {
            eos::loaded::MODEL_ID => eos::loaded::APPROXIMATION_NOTES,
            super::envelope::WATER_MODEL_ID => &[
                "Pressure-only effective water compression under at least 1 GPa envelope load",
                "Water phase, thermal expansion and H/He mixing unresolved; no solid surface claim",
                "Temperature is the outer boundary value, not an interior thermal solution",
            ],
            eos::cold_ice::INTERMEDIATE_MODEL_ID => eos::cold_ice::APPROXIMATION_NOTES,
            eos::cold_ice::IH_MODEL_ID => {
                &["Ice-Ih pressure domain extended beyond the original 100 MPa mask"]
            }
            eos::subfreezing_liquid::MODEL_ID => eos::subfreezing_liquid::APPROXIMATION_NOTES,
            eos::surface::MODEL_ID => eos::surface::APPROXIMATION_NOTES,
            eos::hot::MODEL_ID => eos::hot::APPROXIMATION_NOTES,
            eos::deep_ice::WARM_MODEL_ID => &[
                "Temperature-independent effective ice above 1 GPa, not a melting curve",
                "Warm fluid, ice polymorphs and superionic states are unresolved",
            ],
            eos::deep_ice::MODEL_ID => &[
                "Effective high-pressure ice compression; individual polymorphs unresolved",
                "No ice thermal-expansion correction or matched Gibbs potentials",
            ],
            _ => &[],
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct WaterStructure {
    model_id: &'static str,
    regions: Vec<Region>,
    inertia_kg_m2: f64,
    diagnostics: Diagnostics,
}

impl WaterStructure {
    pub fn model_id(&self) -> &'static str {
        self.model_id
    }
    pub fn host_model_id(&self) -> &'static str {
        MODEL_ID
    }
    pub fn regions(&self) -> &[Region] {
        &self.regions
    }
    pub fn retained_inventory(&self) -> Inventory {
        self.regions.iter().fold(Inventory::default(), |a, r| {
            a.checked_add(r.inventory()).unwrap()
        })
    }
    pub fn mass_kg(&self) -> f64 {
        self.retained_inventory().total_mass_kg()
    }
    /// Outer condensed-material boundary at the imposed pressure, not a
    /// predicted surface pressure, atmospheric reference level or landable rock.
    pub fn radius_m(&self) -> f64 {
        self.regions.last().unwrap().outer().radius_m
    }
    pub fn mean_density_kg_m3(&self) -> f64 {
        self.mass_kg() / (4.0 * PI / 3.0 * self.radius_m().powi(3))
    }
    pub fn reference_gravity_m_s2(&self) -> f64 {
        GRAVITATIONAL_CONSTANT * self.mass_kg() / self.radius_m().powi(2)
    }
    pub fn inertia_kg_m2(&self) -> f64 {
        self.inertia_kg_m2
    }
    pub fn inertia_factor(&self) -> f64 {
        self.inertia_kg_m2 / (self.mass_kg() * self.radius_m().powi(2))
    }
    pub fn diagnostics(&self) -> Diagnostics {
        self.diagnostics
    }
}

/// The water models the solver uses along one isotherm, and the pressures
/// where one hands over to the next. Generation walks the same list to place
/// its phase layers, so both name the same phases.
pub(crate) struct Isotherm {
    temperature_k: f64,
    /// Deepest first. Branch `i` holds from `boundaries[i]` up to
    /// `boundaries[i - 1]`; the last branch reaches the surface.
    boundaries: Vec<f64>,
    branches: Vec<(PhaseKnowledge, &'static str)>,
}

impl Isotherm {
    pub(crate) fn new(temperature_k: f64, outer_pressure_pa: f64) -> Result<Self, SolveError> {
        let deep_boundary = eos::deep_ice::prototype_boundary_pressure_pa(temperature_k).ok();
        let hot = temperature_k > eos::MAX_TEMPERATURE_K;
        let cold_path = eos::cold_ice::supports_temperature(temperature_k);
        // Below the API's 250 K floor the surface model replaces the top Ih branch.
        let frozen = cold_path && temperature_k < eos::MIN_TEMPERATURE_K;
        // Below its 1 kPa floor the surface model takes over the top API branch.
        let thin = !frozen && outer_pressure_pa < eos::MIN_PRESSURE_PA;
        let subfreezing_path = !cold_path && temperature_k > 251.165 && temperature_k < 273.15;
        let mut boundaries = Vec::new();
        let mut branches = Vec::new();
        let liquid = PhaseKnowledge::Water(eos::Phase::Liquid);
        let ih = PhaseKnowledge::Water(eos::Phase::IceIh);
        let effective_ice = PhaseKnowledge::EffectiveHighPressureIce;
        if hot {
            boundaries.push(eos::hot::DEEP_BOUNDARY_PA);
            branches.extend([
                (effective_ice, eos::deep_ice::WARM_MODEL_ID),
                (PhaseKnowledge::UnresolvedHotWater, eos::hot::MODEL_ID),
            ]);
        } else if cold_path {
            boundaries.extend([
                eos::cold_ice::DEEP_BOUNDARY_PA,
                eos::cold_ice::IH_BOUNDARY_PA,
                eos::MAX_PRESSURE_PA,
            ]);
            branches.extend([
                (effective_ice, eos::deep_ice::MODEL_ID),
                (effective_ice, eos::cold_ice::INTERMEDIATE_MODEL_ID),
                (ih, eos::cold_ice::IH_MODEL_ID),
                (
                    ih,
                    if frozen {
                        eos::surface::MODEL_ID
                    } else {
                        eos::MODEL_ID
                    },
                ),
            ]);
        } else if subfreezing_path {
            let low = eos::melting_pressure_ih_pa(temperature_k).map_err(SolveError::Water)?;
            let high = eos::subfreezing_liquid::high_boundary_pa(temperature_k)
                .map_err(SolveError::Water)?;
            boundaries.extend([eos::cold_ice::DEEP_BOUNDARY_PA, high, low]);
            branches.extend([
                (effective_ice, eos::deep_ice::MODEL_ID),
                (effective_ice, eos::cold_ice::INTERMEDIATE_MODEL_ID),
                (liquid, eos::subfreezing_liquid::MODEL_ID),
            ]);
            if low > eos::MAX_PRESSURE_PA {
                boundaries.push(eos::MAX_PRESSURE_PA);
                branches.push((ih, eos::cold_ice::IH_MODEL_ID));
            }
            branches.push((ih, eos::MODEL_ID));
        } else {
            if let Some(p) = deep_boundary {
                boundaries.push(p);
                branches.push((
                    effective_ice,
                    if temperature_k > 300.0 {
                        eos::deep_ice::WARM_MODEL_ID
                    } else {
                        eos::deep_ice::MODEL_ID
                    },
                ));
            }
            if temperature_k >= 273.15 && temperature_k.is_finite() {
                boundaries.push(eos::MAX_PRESSURE_PA);
                let model = if temperature_k >= eos::dense_liquid::MIN_TEMPERATURE_K {
                    eos::dense_liquid::MODEL_ID
                } else {
                    eos::subfreezing_liquid::MODEL_ID
                };
                branches.push((liquid, model));
            }
            if let Some(p) = eos::melting_pressure_ih_pa(temperature_k).ok().filter(|p| {
                temperature_k >= 273.15 && *p >= eos::MIN_PRESSURE_PA && *p <= eos::MAX_PRESSURE_PA
            }) {
                boundaries.push(p);
                branches.push((liquid, eos::MODEL_ID));
                branches.push((ih, eos::MODEL_ID));
            } else {
                let phase = if outer_pressure_pa >= eos::MAX_PRESSURE_PA && temperature_k >= 273.15
                {
                    // Used only for the liquid branches. The explicit deep-ice branch
                    // takes precedence where available; density validation follows below.
                    eos::Phase::Liquid
                } else {
                    eos::phase(outer_pressure_pa.max(eos::MIN_PRESSURE_PA), temperature_k)
                        .map_err(SolveError::Water)?
                };
                // At a coexistence outer boundary, the inward one-sided limit is liquid,
                // as under a saturated steam column. No zero-mass ice skin, vapour
                // film or mixture is introduced.
                let phase = if matches!(
                    phase,
                    eos::Phase::IceIhLiquidCoexistence | eos::Phase::LiquidVapourCoexistence
                ) {
                    eos::Phase::Liquid
                } else {
                    phase
                };
                branches.push((PhaseKnowledge::Water(phase), eos::MODEL_ID));
            }
        }
        if thin && !hot {
            *branches.last_mut().unwrap() = (
                PhaseKnowledge::Water(eos::surface::phase(temperature_k)),
                eos::surface::MODEL_ID,
            );
        }
        Ok(Self {
            temperature_k,
            boundaries,
            branches,
        })
    }

    pub(crate) fn boundaries(&self) -> &[f64] {
        &self.boundaries
    }

    pub(crate) fn branch_at(&self, pressure_pa: f64) -> usize {
        self.boundaries.partition_point(|p| pressure_pa < *p)
    }

    pub(crate) fn phase(&self, branch: usize) -> PhaseKnowledge {
        self.branches[branch].0
    }

    pub(crate) fn model(&self, branch: usize) -> &'static str {
        self.branches[branch].1
    }

    /// One-sided density of `branch`, whatever phase the pressure itself
    /// would select.
    pub(crate) fn density_kg_m3(
        &self,
        pressure: f64,
        branch: usize,
        work: &mut Work,
        limit: usize,
    ) -> Result<f64, SolveError> {
        let model = self.model(branch);
        if model == eos::hot::MODEL_ID {
            return eos::hot::density_kg_m3(pressure, self.temperature_k)
                .map_err(SolveError::Water);
        }
        if model == eos::deep_ice::WARM_MODEL_ID {
            return eos::deep_ice::warm_density_kg_m3(pressure, self.temperature_k)
                .map_err(SolveError::Water);
        }
        if model == eos::deep_ice::MODEL_ID {
            return eos::deep_ice::density_kg_m3(pressure, self.temperature_k)
                .map_err(SolveError::Water);
        }
        if matches!(
            model,
            eos::cold_ice::INTERMEDIATE_MODEL_ID | eos::cold_ice::IH_MODEL_ID
        ) {
            return eos::cold_ice::density_kg_m3(pressure, self.temperature_k, model)
                .map_err(SolveError::Water);
        }
        if model == eos::subfreezing_liquid::MODEL_ID {
            return eos::subfreezing_liquid::density_kg_m3(pressure, self.temperature_k)
                .map_err(SolveError::Water);
        }
        if model == eos::surface::MODEL_ID {
            return eos::surface::density_kg_m3(pressure, self.temperature_k)
                .map_err(SolveError::Water);
        }
        if model == eos::dense_liquid::MODEL_ID {
            let result = eos::dense_liquid::liquid_limit_with_budget(
                pressure,
                self.temperature_k,
                limit - work.eos_evaluations,
            );
            work.eos_evaluations += match &result {
                Ok(s) => s.eos_evaluations,
                Err(e) => e.eos_evaluations,
            };
            return result.map(|s| s.state.density_kg_m3).map_err(|e| {
                if e.reason == eos::dense_liquid::Failure::EvaluationLimit {
                    SolveError::WorkBudgetExhausted(WorkLimit::EosEvaluations)
                } else {
                    SolveError::DenseWater(e)
                }
            });
        }
        let expected = match self.phase(branch) {
            PhaseKnowledge::Water(p) => p,
            _ => unreachable!(),
        };
        let actual = eos::phase(pressure, self.temperature_k).map_err(SolveError::Water)?;
        let one_sided = actual == eos::Phase::IceIhLiquidCoexistence
            || (actual == eos::Phase::LiquidVapourCoexistence && expected == eos::Phase::Liquid);
        if actual != expected && !one_sided {
            return Err(SolveError::WaterPhaseCrossing {
                pressure_pa: pressure,
                expected,
                actual,
            });
        }
        Ok(eos::condensed_state(pressure, self.temperature_k, expected)
            .map_err(SolveError::Water)?
            .density_kg_m3)
    }
}

/// Projection of a unified envelope radial integration, not a second interior
/// solve. Water uses explicitly unresolved pressure-only effective compression.
pub(super) fn envelope_interior(
    regions: Vec<dry::Region>,
    inertia_kg_m2: f64,
    diagnostics: Diagnostics,
    thermal: super::envelope::Thermal,
) -> WaterStructure {
    WaterStructure {
        model_id: "envelope_loaded_heavy_regions_v1",
        regions: regions
            .into_iter()
            .map(|profile| {
                let water = profile.inventory().mass_kg(Material::Water) > 0.0;
                Region {
                    temperature_k: if water {
                        thermal.temperature_k(profile.outer().pressure_pa).ok()
                    } else {
                        None
                    },
                    phase: if water {
                        PhaseKnowledge::UnresolvedCompression
                    } else {
                        PhaseKnowledge::Unresolved
                    },
                    density_model_id: if water
                        && profile.inner().pressure_pa <= eos::loaded::BOUNDARY_PA
                    {
                        eos::loaded::MODEL_ID
                    } else if water {
                        super::envelope::WATER_MODEL_ID
                    } else {
                        cold::MODEL_ID
                    },
                    profile,
                }
            })
            .collect(),
        inertia_kg_m2,
        diagnostics,
    }
}

/// Differentiate metal, silicate, then pure water. Water temperature
/// is imposed only on water, not inferred for the cold refractory proxies.
/// All positive unsupported allocations are refused, including gas traces.
/// Ih/liquid interfaces, the 100 MPa liquid-model seam and the approximate
/// deep-ice melting boundary are located with separate one-sided densities.
/// Above 300 K a labelled depth proxy uses effective ice beyond 1 GPa,
/// regardless of physical phase. Above 623.15 K a hot-fluid proxy extends to
/// 2000 K, without resolved phase knowledge. Below 250 K
/// or 1 kPa a labelled surface-water model extends the IAPWS API. Intermediate
/// cold ice, pressure-melted subfreezing liquid and extended Ih density masks
/// carry explicit approximation notes. Original local IAPWS APIs are unchanged.
/// Unsupported high shooting trials bound the search;
/// they never become truncated or extrapolated successful structures.
pub fn solve(
    retained: &Inventory,
    water_temperature_k: f64,
    outer_pressure_pa: f64,
    controls: Controls,
) -> Result<WaterStructure, SolveError> {
    solve_seeded(
        retained,
        water_temperature_k,
        outer_pressure_pa,
        controls,
        None,
    )
}

pub(super) fn solve_seeded(
    retained: &Inventory,
    water_temperature_k: f64,
    outer_pressure_pa: f64,
    controls: Controls,
    central_pressure_pa: Option<f64>,
) -> Result<WaterStructure, SolveError> {
    solve_with_volume_factor(
        retained,
        water_temperature_k,
        outer_pressure_pa,
        controls,
        1.0,
        central_pressure_pa,
    )
}

fn solve_with_volume_factor(
    retained: &Inventory,
    water_temperature_k: f64,
    outer_pressure_pa: f64,
    controls: Controls,
    volume_factor: f64,
    central_pressure_pa: Option<f64>,
) -> Result<WaterStructure, SolveError> {
    controls.validate()?;
    for material in Material::ALL {
        if !matches!(
            material,
            Material::Metal | Material::Silicate | Material::Water
        ) && retained.mass_kg(material) > 0.0
        {
            return Err(SolveError::UnsupportedMaterial(material));
        }
    }
    if retained.mass_kg(Material::Water) == 0.0 {
        return Err(SolveError::InvalidInput(
            "positive water allocation required; use dry::solve",
        ));
    }
    let mass = retained.total_mass_kg();
    if !super::in_domain(mass / EARTH_MASS_KG, 0.0001, 20.0) {
        return Err(SolveError::OutsideMassDomain(mass));
    }
    let isotherm = Isotherm::new(water_temperature_k, outer_pressure_pa)?;
    let branch_at = |pressure: f64| isotherm.branch_at(pressure);
    let branch_phase = |branch: usize| isotherm.phase(branch);
    let branch_model = |branch: usize| isotherm.model(branch);
    let density = |material, pressure, side, work: &mut Work, limit| {
        if material != Material::Water {
            return cold::density_kg_m3(material, pressure).map_err(SolveError::Eos);
        }
        let branch = match side {
            radial::DensitySide::Branch(i) => i,
            radial::DensitySide::Equilibrium => branch_at(pressure),
        };
        isotherm.density_kg_m3(pressure, branch, work, limit)
    };
    // All parcel mass gravitates and displaces volume during integration. The
    // audit-only factor changes density, not host masks, event pressures or work.
    let host_density = density;
    let density = |material, pressure, side, work: &mut Work, limit| {
        host_density(material, pressure, side, work, limit).map(|rho| rho / volume_factor)
    };
    if controls.max_eos_evaluations == 0 {
        return Err(SolveError::WorkBudgetExhausted(WorkLimit::EosEvaluations));
    }
    let mut initial_work = Work {
        eos_evaluations: 1,
        ..Work::default()
    };
    density(
        Material::Water,
        outer_pressure_pa,
        radial::DensitySide::Equilibrium,
        &mut initial_work,
        controls.max_eos_evaluations,
    )?;
    let policy = radial::Policy {
        pressure_floor_pa: outer_pressure_pa,
        boundary_tolerance_pa: Some(BOUNDARY_TOLERANCE_PA),
        // Absolute pressure error scale, including the refractory interior.
        // Do not let a large central pressure weaken shallow-shell accuracy.
        pressure_error_scale_pa: Some(eos::MAX_PRESSURE_PA),
        interfaces: isotherm
            .boundaries
            .iter()
            .map(|p| radial::PressureInterface {
                material: Material::Water,
                pressure_pa: *p,
            })
            .collect(),
    };
    let mut solver = radial::Solver::new(retained, outer_pressure_pa, controls, &density, policy);
    solver.work = initial_work;
    let seeded =
        solver.seeded_bracket(central_pressure_pa, cold::MAX_PRESSURE_PA, classify_upper)?;
    let (mut low, mut high, mut upper) = if let Some(bracket) = seeded {
        bracket
    } else {
        let mut low = outer_pressure_pa;
        let mut high = (low + solver.pressure_scale()).min(cold::MAX_PRESSURE_PA);
        let upper = loop {
            let trial = solver.trial(high);
            match trial {
                Ok(None) => {
                    low = high;
                    if high == cold::MAX_PRESSURE_PA {
                        return Err(SolveError::Unbracketed {
                            maximum_central_pressure_pa: high,
                        });
                    }
                    if solver.work.bracket_expansions == controls.max_bracket_expansions {
                        return Err(SolveError::WorkBudgetExhausted(WorkLimit::Bracketing));
                    }
                    solver.work.bracket_expansions += 1;
                    high = (outer_pressure_pa + 2.0 * (high - outer_pressure_pa))
                        .min(cold::MAX_PRESSURE_PA);
                }
                _ => break classify_upper(trial)?,
            }
        };
        (low, high, upper)
    };
    let mut shooting = radial::Shooting::default();
    loop {
        shooting.high(
            high,
            upper
                .as_ref()
                .map(|t| t.regions.last().unwrap().outer().pressure_pa - outer_pressure_pa),
        );
        if let Some(trial) = upper.as_ref() {
            let residual = trial.regions.last().unwrap().outer().pressure_pa - outer_pressure_pa;
            if (0.0..=BOUNDARY_TOLERANCE_PA).contains(&residual) {
                let trial = upper.take().unwrap();
                let result = WaterStructure {
                    model_id: MODEL_ID,
                    regions: trial
                        .regions
                        .into_iter()
                        .zip(trial.density_sides)
                        .map(|(profile, side)| {
                            let is_water = profile.inventory().mass_kg(Material::Water) > 0.0;
                            let branch = match side {
                                radial::DensitySide::Branch(i) => i,
                                radial::DensitySide::Equilibrium => {
                                    branch_at(profile.inner().pressure_pa)
                                }
                            };
                            Region {
                                profile,
                                temperature_k: is_water.then_some(water_temperature_k),
                                density_model_id: if !is_water {
                                    cold::MODEL_ID
                                } else {
                                    branch_model(branch)
                                },
                                phase: if is_water {
                                    branch_phase(branch)
                                } else {
                                    PhaseKnowledge::Unresolved
                                },
                            }
                        })
                        .collect(),
                    inertia_kg_m2: trial.inertia_kg_m2,
                    diagnostics: Diagnostics {
                        central_pressure_pa: high,
                        boundary_residual_pa: residual,
                        work: solver.work,
                    },
                };
                validate(&result, retained)?;
                return Ok(result);
            }
        }
        if solver.work.shooting_iterations == controls.max_shooting_iterations {
            return Err(SolveError::WorkBudgetExhausted(WorkLimit::Shooting));
        }
        solver.work.shooting_iterations += 1;
        let mid = shooting.next(low, high, BOUNDARY_TOLERANCE_PA);
        if mid == low || mid == high {
            return Err(if upper.is_none() {
                SolveError::UnsupportedWaterProfile
            } else {
                SolveError::NonConvergence {
                    material: Material::Water,
                    mass_fraction: 1.0,
                }
            });
        }
        match solver.trial(mid) {
            Ok(None) => {
                low = mid;
                shooting.low();
            }
            trial => {
                high = mid;
                upper = classify_upper(trial)?;
            }
        }
    }
}

// Pressure declines outwards. Water domain/phase failures above the prescribed
// boundary can thus serve as an upper bracket, not an excuse to use another EOS.
// Budgets and numerical errors are never reclassified as physical brackets.
fn classify_upper(
    trial: Result<Option<radial::Trial>, SolveError>,
) -> Result<Option<radial::Trial>, SolveError> {
    match trial {
        Err(SolveError::Water(_) | SolveError::WaterPhaseCrossing { .. }) => Ok(None),
        Err(SolveError::DenseWater(e))
            if matches!(
                e.reason,
                eos::dense_liquid::Failure::PressureDomain
                    | eos::dense_liquid::Failure::TemperatureDomain
                    | eos::dense_liquid::Failure::SolidDomain
                    | eos::dense_liquid::Failure::CoexistenceDensity
            ) =>
        {
            Ok(None)
        }
        other => other,
    }
}

fn validate(result: &WaterStructure, requested: &Inventory) -> Result<(), SolveError> {
    let allocated = result.retained_inventory();
    for material in Material::ALL {
        if (allocated.mass_kg(material) - requested.mass_kg(material)).abs()
            > 1e-10 * requested.mass_kg(material)
        {
            return Err(SolveError::InvalidClosure {
                material,
                quantity: "inventory",
            });
        }
    }
    for region in &result.regions {
        let material = Material::ALL
            .into_iter()
            .find(|m| region.inventory().mass_kg(*m) > 0.0)
            .unwrap();
        let fail = |quantity| SolveError::InvalidClosure { material, quantity };
        let dm = region.outer().enclosed_mass_kg - region.inner().enclosed_mass_kg;
        if (dm / region.inventory().total_mass_kg() - 1.0).abs() > 1e-8 {
            return Err(fail("region mass resolution"));
        }
        for s in region.samples() {
            if ![
                s.radius_m,
                s.enclosed_mass_kg,
                s.pressure_pa,
                s.density_kg_m3,
            ]
            .iter()
            .all(|v| v.is_finite())
                || s.radius_m < 0.0
                || s.enclosed_mass_kg < 0.0
                || s.density_kg_m3 <= 0.0
            {
                return Err(fail("profile domain"));
            }
        }
        for pair in region.samples().windows(2) {
            if pair[1].radius_m <= pair[0].radius_m
                || pair[1].enclosed_mass_kg <= pair[0].enclosed_mass_kg
                || pair[1].pressure_pa > pair[0].pressure_pa
            {
                return Err(fail("profile ordering"));
            }
        }
    }
    for pair in result.regions.windows(2) {
        let a = pair[0].outer();
        let b = pair[1].inner();
        if a.radius_m != b.radius_m
            || a.enclosed_mass_kg != b.enclosed_mass_kg
            || a.pressure_pa != b.pressure_pa
        {
            return Err(SolveError::InvalidClosure {
                material: Material::Water,
                quantity: "interface continuity",
            });
        }
    }
    if !result.inertia_factor().is_finite()
        || !(0.0..=2.0 / 3.0).contains(&result.inertia_factor())
        || result.inertia_factor() == 0.0
    {
        return Err(SolveError::InvalidClosure {
            material: Material::Water,
            quantity: "inertia",
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests;
