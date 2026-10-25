//! Opt-in, spherical cold Fe/MgSiO3 structure. Never called by generation.
//! See docs/simulation/planet-composition.md#headless-dry-body-solver.

mod integrate;

use super::super::{
    composition::Inventory,
    materials::{Material, cold},
};
use crate::units::{EARTH_MASS_KG, GRAVITATIONAL_CONSTANT};
use std::{f64::consts::PI, fmt};

pub const MODEL_ID: &str = "cold_differentiated_seager_2007_v1";

/// Ceilings may be reduced for callers/tests, never increased past the contract.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Controls {
    pub relative_tolerance: f64,
    /// Maximum step in x = (enclosed mass / total mass)^(1/3).
    pub max_step: f64,
    pub max_bracket_expansions: usize,
    pub max_shooting_iterations: usize,
    pub max_steps_per_trial: usize,
    pub max_eos_evaluations: usize,
}

impl Default for Controls {
    fn default() -> Self {
        Self {
            relative_tolerance: 1e-10,
            max_step: 1.0 / 128.0,
            max_bracket_expansions: 64,
            max_shooting_iterations: 128,
            max_steps_per_trial: 32_768,
            max_eos_evaluations: 2_000_000,
        }
    }
}

impl Controls {
    fn validate(self) -> Result<(), SolveError> {
        let valid = self.relative_tolerance.is_finite()
            && (1e-13..=1e-10).contains(&self.relative_tolerance)
            && self.max_step.is_finite()
            && (1e-5..=1.0 / 32.0).contains(&self.max_step)
            && self.max_bracket_expansions <= 64
            && self.max_shooting_iterations <= 128
            && self.max_steps_per_trial <= 32_768
            && self.max_eos_evaluations <= 2_000_000;
        if valid {
            Ok(())
        } else {
            Err(SolveError::InvalidInput("numerical controls"))
        }
    }
}

/// Adjacent regions deliberately carry separate one-sided interface samples.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sample {
    pub radius_m: f64,
    pub enclosed_mass_kg: f64,
    pub pressure_pa: f64,
    pub density_kg_m3: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Region {
    inventory: Inventory,
    samples: Vec<Sample>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PhaseKnowledge {
    /// Mechanical compression does not establish melting or solidity.
    Unresolved,
}

impl Region {
    pub fn inventory(&self) -> &Inventory {
        &self.inventory
    }
    pub fn samples(&self) -> &[Sample] {
        &self.samples
    }
    pub fn inner(&self) -> Sample {
        self.samples[0]
    }
    pub fn outer(&self) -> Sample {
        *self.samples.last().unwrap()
    }
    /// Neither temperature nor solidity is established by a cold density EOS.
    pub fn temperature_k(&self) -> Option<f64> {
        None
    }
    pub fn phase(&self) -> PhaseKnowledge {
        PhaseKnowledge::Unresolved
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Work {
    pub bracket_expansions: usize,
    pub shooting_iterations: usize,
    pub attempted_steps: usize,
    pub eos_evaluations: usize,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Diagnostics {
    pub central_pressure_pa: f64,
    /// Raw residual, never fixed by translating the pressure profile.
    pub boundary_residual_pa: f64,
    pub work: Work,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DryStructure {
    regions: Vec<Region>,
    mass_kg: f64,
    inertia_kg_m2: f64,
    diagnostics: Diagnostics,
}

impl DryStructure {
    pub fn model_id(&self) -> &'static str {
        MODEL_ID
    }
    pub fn regions(&self) -> &[Region] {
        &self.regions
    }
    /// Derived from disjoint region inventories, not an independent allocation.
    pub fn retained_inventory(&self) -> Inventory {
        self.regions.iter().fold(Inventory::default(), |a, r| {
            a.checked_add(&r.inventory).unwrap()
        })
    }
    pub fn mass_kg(&self) -> f64 {
        self.mass_kg
    }
    /// Outer material boundary, not a claim about solidity or a one-bar radius.
    pub fn radius_m(&self) -> f64 {
        self.regions.last().unwrap().outer().radius_m
    }
    pub fn mean_density_kg_m3(&self) -> f64 {
        self.mass_kg / (4.0 * PI / 3.0 * self.radius_m().powi(3))
    }
    pub fn reference_gravity_m_s2(&self) -> f64 {
        GRAVITATIONAL_CONSTANT * self.mass_kg / self.radius_m().powi(2)
    }
    pub fn inertia_kg_m2(&self) -> f64 {
        self.inertia_kg_m2
    }
    pub fn inertia_factor(&self) -> f64 {
        self.inertia_kg_m2 / (self.mass_kg * self.radius_m().powi(2))
    }
    pub fn diagnostics(&self) -> Diagnostics {
        self.diagnostics
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorkLimit {
    Bracketing,
    Shooting,
    Integration,
    EosEvaluations,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SolveError {
    InvalidInput(&'static str),
    UnsupportedMaterial(Material),
    OutsideMassDomain(f64),
    Eos(cold::ColdEosError),
    Unbracketed {
        maximum_central_pressure_pa: f64,
    },
    WorkBudgetExhausted(WorkLimit),
    NonConvergence {
        material: Material,
        mass_fraction: f64,
    },
    InvalidClosure {
        material: Material,
        quantity: &'static str,
    },
}

impl fmt::Display for SolveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "dry structure solve: {self:?}")
    }
}
impl std::error::Error for SolveError {}

/// Fixed cold thermal prescription and metal-inside-silicate differentiation.
/// An imposed external load carries no atmospheric inventory. Every positive
/// unsupported constituent, however small, is refused before numerical work.
pub fn solve(
    retained: &Inventory,
    outer_pressure_pa: f64,
    controls: Controls,
) -> Result<DryStructure, SolveError> {
    solve_with_eos(retained, outer_pressure_pa, controls, cold::density_kg_m3)
}

type Density = fn(Material, f64) -> Result<f64, cold::ColdEosError>;

fn solve_with_eos(
    retained: &Inventory,
    outer_pressure_pa: f64,
    controls: Controls,
    eos: Density,
) -> Result<DryStructure, SolveError> {
    controls.validate()?;
    for material in Material::ALL {
        if !matches!(material, Material::Metal | Material::Silicate)
            && retained.mass_kg(material) > 0.0
        {
            return Err(SolveError::UnsupportedMaterial(material));
        }
    }
    let mass = retained.total_mass_kg();
    if !super::in_domain(mass / EARTH_MASS_KG, 0.0001, 20.0) {
        return Err(SolveError::OutsideMassDomain(mass));
    }
    if !outer_pressure_pa.is_finite() || !(0.0..=cold::MAX_PRESSURE_PA).contains(&outer_pressure_pa)
    {
        return Err(SolveError::InvalidInput("outer pressure"));
    }
    let mut solver = integrate::Solver::new(retained, outer_pressure_pa, controls, eos);
    let mut low = outer_pressure_pa;
    // Ambient-density sphere is only an initial bracket scale, never a radius fallback.
    let mut high = (low + solver.pressure_scale()).min(cold::MAX_PRESSURE_PA);
    let mut high_trial = loop {
        if let Some(trial) = solver.trial(high)? {
            break trial;
        }
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
        high = (outer_pressure_pa + 2.0 * (high - outer_pressure_pa)).min(cold::MAX_PRESSURE_PA);
    };
    loop {
        let residual = high_trial.regions.last().unwrap().outer().pressure_pa - outer_pressure_pa;
        // Require the converged solution to approach the requested boundary from
        // above, so the EOS never evaluates a negative physical pressure.
        if residual <= 1.0_f64.max(1e-8 * high) {
            let result = DryStructure {
                regions: high_trial.regions,
                mass_kg: mass,
                inertia_kg_m2: high_trial.inertia_kg_m2,
                diagnostics: Diagnostics {
                    central_pressure_pa: high,
                    boundary_residual_pa: residual,
                    work: solver.work,
                },
            };
            validate(&result, retained)?;
            return Ok(result);
        }
        if solver.work.shooting_iterations == controls.max_shooting_iterations {
            return Err(SolveError::WorkBudgetExhausted(WorkLimit::Shooting));
        }
        solver.work.shooting_iterations += 1;
        let mid = low + (high - low) * 0.5;
        if mid == high || mid == low {
            return Err(SolveError::NonConvergence {
                material: if retained.mass_kg(Material::Silicate) > 0.0 {
                    Material::Silicate
                } else {
                    Material::Metal
                },
                mass_fraction: 1.0,
            });
        }
        if let Some(trial) = solver.trial(mid)? {
            high = mid;
            high_trial = trial;
        } else {
            low = mid;
        }
    }
}

fn validate(result: &DryStructure, requested: &Inventory) -> Result<(), SolveError> {
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
        let material = if region.inventory.mass_kg(Material::Metal) > 0.0 {
            Material::Metal
        } else {
            Material::Silicate
        };
        let fail = |quantity| SolveError::InvalidClosure { material, quantity };
        let dm = region.outer().enclosed_mass_kg - region.inner().enclosed_mass_kg;
        if (dm / region.inventory.total_mass_kg() - 1.0).abs() > 1e-8 {
            return Err(fail("region mass resolution"));
        }
        for s in &region.samples {
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
                || !(0.0..=cold::MAX_PRESSURE_PA).contains(&s.pressure_pa)
            {
                return Err(fail("profile domain"));
            }
        }
        for pair in region.samples.windows(2) {
            if pair[1].radius_m <= pair[0].radius_m
                || pair[1].enclosed_mass_kg <= pair[0].enclosed_mass_kg
                || pair[1].pressure_pa > pair[0].pressure_pa
            {
                return Err(fail("profile ordering"));
            }
        }
    }
    if !result.inertia_factor().is_finite()
        || result.inertia_factor() <= 0.0
        || result.inertia_factor() > 2.0 / 3.0
    {
        return Err(SolveError::InvalidClosure {
            material: Material::Silicate,
            quantity: "inertia",
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests;
