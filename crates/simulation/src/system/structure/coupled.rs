//! Headless condensed-body/finite outer-column matching. No generation switch,
//! dense-gas EOS, climate or hidden envelope-surrogate fallback.

mod column;
#[cfg(test)]
mod tests;

use super::{dry, water};
use crate::{
    system::{
        atmosphere::{AtmosphereError, Column, GasAtmosphere, ONE_BAR_PA},
        composition::{CompositionError, Inventory},
        materials::Material,
    },
    units::{EARTH_MASS_KG, GRAVITATIONAL_CONSTANT},
};
use std::{f64::consts::PI, fmt};

pub const MODEL_ID: &str = "loaded_condensed_spherical_isothermal_column_v1";
/// Engineering restriction on neglected atmospheric self-gravity, not a class.
pub const MAX_GAS_MASS_FRACTION: f64 = 0.01;
/// Conservative engineering mask, not a validated nonideal-gas error bound.
pub const MAX_GAS_DENSITY_KG_M3: f64 = 10.0;
pub const PRESSURE_TOLERANCE_PA: f64 = 0.01;
pub const ALLOCATION_RELATIVE_TOLERANCE: f64 = 1e-9;

/// The caller specifies supply eligible for gas exchange, not mutable reservoirs.
/// Everything else stays condensed. Non-water condensates have no radial EOS.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Request {
    pub retained: Inventory,
    pub exchangeable: Inventory,
    pub column_temperature_k: f64,
    /// Required when water remains condensed; independent of the gas temperature.
    pub water_temperature_k: Option<f64>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Controls {
    pub radial: dry::Controls,
    pub max_iterations: usize,
    /// Aggregate ceiling across all radial solves, each still capped at 2M.
    pub max_total_eos_evaluations: usize,
}

impl Default for Controls {
    fn default() -> Self {
        Self {
            radial: dry::Controls::default(),
            max_iterations: 32,
            max_total_eos_evaluations: 16_000_000,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum CondensedStructure {
    Dry(dry::DryStructure),
    Water(water::WaterStructure),
}

impl CondensedStructure {
    pub fn retained_inventory(&self) -> Inventory {
        match self {
            Self::Dry(s) => s.retained_inventory(),
            Self::Water(s) => s.retained_inventory(),
        }
    }
    pub fn radius_m(&self) -> f64 {
        match self {
            Self::Dry(s) => s.radius_m(),
            Self::Water(s) => s.radius_m(),
        }
    }
    pub fn mass_kg(&self) -> f64 {
        self.retained_inventory().total_mass_kg()
    }
    pub fn diagnostics(&self) -> dry::Diagnostics {
        match self {
            Self::Dry(s) => s.diagnostics(),
            Self::Water(s) => s.diagnostics(),
        }
    }
    pub fn outer(&self) -> dry::Sample {
        match self {
            Self::Dry(s) => s.regions().last().unwrap().outer(),
            Self::Water(s) => s.regions().last().unwrap().outer(),
        }
    }
    pub fn inertia_kg_m2(&self) -> f64 {
        match self {
            Self::Dry(s) => s.inertia_kg_m2(),
            Self::Water(s) => s.inertia_kg_m2(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Diagnostics {
    pub iterations: usize,
    pub total_eos_evaluations: usize,
    /// Actual one-sided condensed pressure minus inventory-normalised gas pressure.
    pub interface_residual_pa: f64,
    pub partition_relative_residual: f64,
}

/// Gas inventory owns the entire represented finite shell. The atmosphere and
/// its pressure-level diagnostics borrow that allocation, never add more mass.
#[derive(Clone, Debug, PartialEq)]
pub struct CoupledStructure {
    condensed: CondensedStructure,
    gas: Inventory,
    atmosphere: Option<GasAtmosphere>,
    diagnostics: Diagnostics,
}

impl CoupledStructure {
    pub fn model_id(&self) -> &'static str {
        MODEL_ID
    }
    pub fn condensed(&self) -> &CondensedStructure {
        &self.condensed
    }
    pub fn gas_inventory(&self) -> &Inventory {
        &self.gas
    }
    pub fn retained_inventory(&self) -> Inventory {
        self.condensed
            .retained_inventory()
            .checked_add(&self.gas)
            .unwrap()
    }
    pub fn atmosphere(&self) -> Option<&GasAtmosphere> {
        self.atmosphere.as_ref()
    }
    /// Atmospheric gravity ignores the shell's own mass, consistently with the
    /// central-field hydrostatic prescription and its explicit <=1% mass mask.
    pub fn column_gm_m3_s2(&self) -> f64 {
        GRAVITATIONAL_CONSTANT * self.condensed.mass_kg()
    }
    pub fn column(&self) -> Result<Option<Column>, AtmosphereError> {
        self.atmosphere
            .map(|a| a.column(self.column_gm_m3_s2(), self.condensed.radius_m()))
            .transpose()
    }
    /// Absent when one bar lies below the condensed boundary or there is no gas.
    pub fn one_bar_radius_m(&self) -> Result<Option<f64>, AtmosphereError> {
        self.atmosphere
            .map(|a| {
                a.pressure_level_altitude_m(
                    ONE_BAR_PA,
                    self.column_gm_m3_s2(),
                    self.condensed.radius_m(),
                )
            })
            .transpose()
            .map(|h| h.flatten().map(|h| self.condensed.radius_m() + h))
    }
    pub fn diagnostics(&self) -> Diagnostics {
        self.diagnostics
    }
    pub fn mass_kg(&self) -> f64 {
        self.retained_inventory().total_mass_kg()
    }
    /// Includes the finite spherical gas shell, not just the condensed body.
    pub fn inertia_kg_m2(&self) -> f64 {
        self.condensed.inertia_kg_m2() + column::inertia_kg_m2(self)
    }
    /// Referenced to the condensed boundary, not the optical or one-bar radius.
    pub fn inertia_factor(&self) -> f64 {
        self.inertia_kg_m2() / (self.mass_kg() * self.condensed.radius_m().powi(2))
    }
    pub fn approximation_notes(&self) -> &'static [&'static str] {
        &[
            "Prescribed isothermal ideal-gas mixture, not a climate solution",
            "Gas self-gravity neglected within the 1% mass mask",
            "Cold-trap caps are approximate, not full phase equilibrium",
            "Entire gas inventory normalised to an operational finite shell",
            "Water and refractory thermal/phase limitations remain those of the radial model",
        ]
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Error {
    InvalidInput(&'static str),
    Composition(CompositionError),
    Atmosphere(AtmosphereError),
    Radial(dry::SolveError),
    UnsupportedCondensate(Material),
    /// Use an explicitly identified envelope approximation until a dense EOS exists.
    GasMassOutsideDomain,
    DenseGasUnsupported,
    WeakBindingUnsupported,
    WorkBudgetExhausted,
    NonConvergence,
    InvalidClosure(Material),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "coupled structure solve: {self:?}")
    }
}
impl std::error::Error for Error {}

/// Match gas mass/partition, condensed loading and radius. Surface pressure is
/// always an output. Repeated evaluations start from immutable offered supply.
pub fn solve(request: &Request, controls: Controls) -> Result<CoupledStructure, Error> {
    controls.radial.validate().map_err(Error::Radial)?;
    if controls.max_iterations > 32 || controls.max_total_eos_evaluations > 16_000_000 {
        return Err(Error::InvalidInput("coupling controls"));
    }
    if !request.column_temperature_k.is_finite()
        || !(50.0..=1000.0).contains(&request.column_temperature_k)
    {
        return Err(Error::InvalidInput("column temperature (50..=1000 K)"));
    }
    if let Some(t) = request.water_temperature_k
        && (!t.is_finite() || t <= 0.0)
    {
        return Err(Error::InvalidInput("water temperature"));
    }
    for material in Material::ALL {
        let offered = request.exchangeable.mass_kg(material);
        if offered > request.retained.mass_kg(material) {
            return Err(Error::InvalidInput("exchangeable exceeds retained"));
        }
        if offered > 0.0 && material.properties().gas.is_none() {
            return Err(Error::InvalidInput("exchangeable refractory"));
        }
        if !matches!(
            material,
            Material::Metal | Material::Silicate | Material::Water
        ) && offered != request.retained.mass_kg(material)
        {
            return Err(Error::UnsupportedCondensate(material));
        }
    }
    let total = request.retained.total_mass_kg();
    if !super::in_domain(total / EARTH_MASS_KG, 0.0001, 20.0) {
        return Err(Error::Radial(dry::SolveError::OutsideMassDomain(total)));
    }
    // Ambient density is only an iteration seed, never a returned radius fit.
    let mut radius = (3.0 * total / (4.0 * PI * 4100.0)).cbrt();
    let mut condensed_mass = total;
    let mut work = 0;
    for iteration in 1..=controls.max_iterations {
        let gas = column::partition(request, condensed_mass, radius)?;
        let inventory = subtract(&request.retained, &gas)?;
        let gm = GRAVITATIONAL_CONSTANT * inventory.total_mass_kg();
        let air = column::atmosphere(&gas, request.column_temperature_k, gm, radius)?;
        let pressure = air.map_or(0.0, |a| a.pressure_pa());
        let mut radial_controls = controls.radial;
        radial_controls.max_eos_evaluations = radial_controls
            .max_eos_evaluations
            .min(controls.max_total_eos_evaluations - work);
        if radial_controls.max_eos_evaluations == 0 {
            return Err(Error::WorkBudgetExhausted);
        }
        let condensed = if inventory.mass_kg(Material::Water) > 0.0 {
            let t = request
                .water_temperature_k
                .ok_or(Error::InvalidInput("missing water temperature"))?;
            CondensedStructure::Water(
                water::solve(&inventory, t, pressure, radial_controls).map_err(Error::Radial)?,
            )
        } else {
            CondensedStructure::Dry(
                dry::solve_loaded(&inventory, pressure, radial_controls).map_err(Error::Radial)?,
            )
        };
        work += condensed.diagnostics().work.eos_evaluations;
        radius = condensed.radius_m();
        condensed_mass = condensed.mass_kg();
        let actual_air = column::atmosphere(
            &gas,
            request.column_temperature_k,
            GRAVITATIONAL_CONSTANT * condensed_mass,
            radius,
        )?;
        let next_gas = column::partition(request, condensed_mass, radius)?;
        let partition_residual = Material::ALL
            .iter()
            .map(|&m| {
                let a = gas.mass_kg(m);
                let b = next_gas.mass_kg(m);
                if a == b {
                    0.0
                } else {
                    (a - b).abs() / a.max(b)
                }
            })
            .fold(0.0_f64, f64::max);
        let actual_pressure = actual_air.map_or(0.0, |a| a.pressure_pa());
        if (pressure - actual_pressure).abs() <= PRESSURE_TOLERANCE_PA
            && partition_residual <= ALLOCATION_RELATIVE_TOLERANCE
        {
            column::validate(&gas, actual_air.as_ref(), gm, radius)?;
            let interface_residual = condensed.outer().pressure_pa - actual_pressure;
            if !interface_residual.is_finite()
                || interface_residual.abs() > 2.0 * PRESSURE_TOLERANCE_PA
            {
                return Err(Error::NonConvergence);
            }
            let result = CoupledStructure {
                condensed,
                gas,
                atmosphere: actual_air,
                diagnostics: Diagnostics {
                    iterations: iteration,
                    total_eos_evaluations: work,
                    interface_residual_pa: interface_residual,
                    partition_relative_residual: partition_residual,
                },
            };
            let allocated = result.retained_inventory();
            for m in Material::ALL {
                if (allocated.mass_kg(m) - request.retained.mass_kg(m)).abs()
                    > 1e-10 * request.retained.mass_kg(m)
                {
                    return Err(Error::InvalidClosure(m));
                }
            }
            return Ok(result);
        }
    }
    Err(Error::NonConvergence)
}

fn subtract(total: &Inventory, gas: &Inventory) -> Result<Inventory, Error> {
    let inventory = Inventory::new(&Material::ALL.map(|m| (m, total.mass_kg(m) - gas.mass_kg(m))))
        .map_err(Error::Composition)?;
    for m in Material::ALL {
        if gas.mass_kg(m) > 0.0 && inventory.mass_kg(m) == total.mass_kg(m) {
            return Err(Error::InvalidClosure(m));
        }
        if !matches!(m, Material::Metal | Material::Silicate | Material::Water)
            && inventory.mass_kg(m) > 0.0
        {
            return Err(Error::UnsupportedCondensate(m));
        }
    }
    Ok(inventory)
}
