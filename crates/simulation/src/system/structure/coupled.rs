//! Headless condensed-body/finite outer-column matching. No generation switch,
//! dense-gas EOS, climate or hidden envelope-surrogate fallback. An optional
//! grey greenhouse estimate sets the surface temperature.

mod column;
mod deposit;
pub mod escape;
#[cfg(test)]
mod tests;
mod warming;

use super::{dry, water};
use crate::{
    system::{
        atmosphere::{AtmosphereError, Column, GasAtmosphere, ONE_BAR_PA, greenhouse},
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
/// Generation's column floor. Cold-trap caps are extrapolated below 50 K.
pub const MIN_COLUMN_TEMPERATURE_K: f64 = 20.0;
pub const MAX_COLUMN_TEMPERATURE_K: f64 = 2000.0;
/// Labelled ideal-steam extension, not a general dense-mixture EOS.
pub const HOT_STEAM_MODEL_ID: &str = "bounded_hot_ideal_steam_column_v1";
pub const MAX_HOT_STEAM_DENSITY_KG_M3: f64 = 50.0;
pub const TEMPERATURE_RELATIVE_TOLERANCE: f64 = 1e-9;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Temperature {
    /// Imposed isothermal column and, separately, condensed-water temperature.
    /// Water's is required when water remains condensed.
    Prescribed { column_k: f64, water_k: Option<f64> },
    /// Column and water both take the grey-greenhouse surface temperature
    /// solved over this equilibrium temperature, on the coupled column.
    Greenhouse { equilibrium_k: f64 },
}

/// The caller specifies supply eligible for gas exchange, not mutable reservoirs.
/// Everything else stays condensed. Exchangeable secondaries above their cold-trap
/// cap settle as a surface deposit; other non-water condensates have no EOS.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Request {
    pub retained: Inventory,
    pub exchangeable: Inventory,
    pub temperature: Temperature,
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
    /// Actual one-sided condensed pressure minus the gas pressure and deposit load.
    pub interface_residual_pa: f64,
    pub partition_relative_residual: f64,
}

/// Gas inventory owns the entire represented finite shell. The atmosphere and
/// its pressure-level diagnostics borrow that allocation, never add more mass.
/// The gas sits on the deposit, which sits on the condensed body.
#[derive(Clone, Debug, PartialEq)]
pub struct CoupledStructure {
    condensed: CondensedStructure,
    deposit: deposit::Deposit,
    gas: Inventory,
    atmosphere: Option<GasAtmosphere>,
    surface_temperature_k: f64,
    optical_depth: Option<f64>,
    diagnostics: Diagnostics,
}

impl CoupledStructure {
    pub fn model_id(&self) -> &'static str {
        MODEL_ID
    }
    pub fn column_model_id(&self) -> &'static str {
        if self.atmosphere.is_some_and(|air| column::hot_steam(&air)) {
            HOT_STEAM_MODEL_ID
        } else {
            MODEL_ID
        }
    }
    pub fn condensed(&self) -> &CondensedStructure {
        &self.condensed
    }
    pub fn gas_inventory(&self) -> &Inventory {
        &self.gas
    }
    /// Frozen secondaries above their vapour-pressure cap, not in the gas.
    pub fn deposit_inventory(&self) -> &Inventory {
        self.deposit.inventory()
    }
    pub fn deposit_thickness_m(&self) -> f64 {
        self.deposit.thickness_m(self.condensed.radius_m())
    }
    /// Top of the deposit, where the gas column starts.
    pub fn surface_radius_m(&self) -> f64 {
        self.deposit.top_radius_m(self.condensed.radius_m())
    }
    /// Gas pressure plus the deposit's weight: the condensed body's outer load.
    pub fn condensed_load_pa(&self) -> f64 {
        self.atmosphere.map_or(0.0, |a| a.pressure_pa())
            + self
                .deposit
                .load_pa(self.condensed.mass_kg(), self.condensed.radius_m())
    }
    pub fn retained_inventory(&self) -> Inventory {
        self.condensed
            .retained_inventory()
            .checked_add(self.deposit.inventory())
            .unwrap()
            .checked_add(&self.gas)
            .unwrap()
    }
    pub fn atmosphere(&self) -> Option<&GasAtmosphere> {
        self.atmosphere.as_ref()
    }
    /// The isothermal column's temperature, imposed or greenhouse-warmed.
    pub fn surface_temperature_k(&self) -> f64 {
        self.surface_temperature_k
    }
    /// Grey infrared optical depth, present only for a greenhouse request.
    pub fn greenhouse_optical_depth(&self) -> Option<f64> {
        self.optical_depth
    }
    /// Atmospheric gravity ignores the shell's own mass, consistently with the
    /// central-field hydrostatic prescription and its explicit <=1% mass mask.
    pub fn column_gm_m3_s2(&self) -> f64 {
        GRAVITATIONAL_CONSTANT * (self.condensed.mass_kg() + self.deposit.mass_kg())
    }
    pub fn column(&self) -> Result<Option<Column>, AtmosphereError> {
        self.atmosphere
            .map(|a| a.column(self.column_gm_m3_s2(), self.surface_radius_m()))
            .transpose()
    }
    /// Absent when one bar lies below the condensed boundary or there is no gas.
    pub fn one_bar_radius_m(&self) -> Result<Option<f64>, AtmosphereError> {
        self.atmosphere
            .map(|a| {
                a.pressure_level_altitude_m(
                    ONE_BAR_PA,
                    self.column_gm_m3_s2(),
                    self.surface_radius_m(),
                )
            })
            .transpose()
            .map(|h| h.flatten().map(|h| self.surface_radius_m() + h))
    }
    pub fn diagnostics(&self) -> Diagnostics {
        self.diagnostics
    }
    pub fn mass_kg(&self) -> f64 {
        self.retained_inventory().total_mass_kg()
    }
    /// Includes the deposit and finite gas shells, not just the condensed body.
    pub fn inertia_kg_m2(&self) -> f64 {
        self.condensed.inertia_kg_m2()
            + self.deposit.inertia_kg_m2(self.condensed.radius_m())
            + column::inertia_kg_m2(self)
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
            "Water vapour above 273.16 K is capped at IF97 saturation, ice by the cold-trap fit",
            "Below 50 K cold-trap caps are extrapolated from their liquid-vapour anchors",
            "O2 and H/He have no cold-trap cap, so they stay gas at any temperature",
            "Frost is one even global layer of fixed solid density, on top by convention",
            "Frost phase (ice or liquid) and dissolution in water are not modelled",
            "Entire gas inventory normalised to an operational finite shell",
            "Water and refractory thermal/phase limitations remain those of the radial model",
            "Greenhouse: grey Eddington fit to Earth, Mars and Venus, not radiative transfer",
            "Greenhouse: no lapse rate, clouds, ice-albedo feedback or collision-induced absorption",
            "Greenhouse: the coolest balance warming up from equilibrium; hot starts not explored",
            "Hot steam: ideal approximation at 1000..=2000 K, at least 90% water by mass, density at most 50 kg/m3",
            "Hot steam neglects nonideality and dissociation; no physical accuracy bound is claimed",
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
    /// Greenhouse warming left the column domain (2000 K or 1% gas mass)
    /// before balancing: a runaway this thin column cannot represent.
    WarmingOutsideDomain,
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
    solve_protected(request, &Inventory::default(), controls)
}

/// Protected secondaries participate in every interior/column iteration, not
/// in a second, mechanically inconsistent solve after convergence.
pub(super) fn solve_protected(
    request: &Request,
    protected: &Inventory,
    controls: Controls,
) -> Result<CoupledStructure, Error> {
    controls.radial.validate().map_err(Error::Radial)?;
    if controls.max_iterations > 32 || controls.max_total_eos_evaluations > 16_000_000 {
        return Err(Error::InvalidInput("coupling controls"));
    }
    let column_k = match request.temperature {
        Temperature::Prescribed { column_k, water_k } => {
            if let Some(t) = water_k
                && (!t.is_finite() || t <= 0.0)
            {
                return Err(Error::InvalidInput("water temperature"));
            }
            column_k
        }
        Temperature::Greenhouse { equilibrium_k } => equilibrium_k,
    };
    if !column_k.is_finite()
        || !(MIN_COLUMN_TEMPERATURE_K..=MAX_COLUMN_TEMPERATURE_K).contains(&column_k)
    {
        return Err(Error::InvalidInput("column temperature (20..=2000 K)"));
    }
    for material in Material::ALL {
        let offered = request.exchangeable.mass_kg(material);
        let shielded = protected.mass_kg(material);
        if shielded > 0.0
            && !matches!(
                material,
                Material::CarbonDioxide
                    | Material::Nitrogen
                    | Material::Methane
                    | Material::Oxygen
                    | Material::Argon
            )
        {
            return Err(Error::UnsupportedCondensate(material));
        }
        if shielded > request.retained.mass_kg(material)
            || offered > request.retained.mass_kg(material) - shielded
        {
            return Err(Error::InvalidInput("exchangeable exceeds retained"));
        }
        if offered > 0.0 && material.properties().gas.is_none() {
            return Err(Error::InvalidInput("exchangeable refractory"));
        }
        if !matches!(
            material,
            Material::Metal | Material::Silicate | Material::Water
        ) && offered != request.retained.mass_kg(material) - shielded
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
    // Mass and radius under the gas column: condensed body plus deposit.
    let mut base = (total, radius);
    let mut temperature = warming::temperatures(request, base)?;
    let mut work = 0;
    let mut central_pressure_pa = None;
    for iteration in 1..=controls.max_iterations {
        let (column_k, water_k) = temperature;
        let gas = column::partition(request, column_k, base.0, base.1)?;
        let exposed = Inventory::new(
            &Material::ALL.map(|m| (m, request.retained.mass_kg(m) - protected.mass_kg(m))),
        )
        .map_err(Error::Composition)?;
        let (inventory, deposit) = split(&exposed, &gas)?;
        let host_mass = inventory.total_mass_kg() + protected.total_mass_kg();
        let gm = GRAVITATIONAL_CONSTANT * (host_mass + deposit.mass_kg());
        // The load uses the geometry the gas was partitioned on, so a column
        // saturated in water loads its liquid at saturation, not just below it.
        let air = column::atmosphere(&gas, column_k, GRAVITATIONAL_CONSTANT * base.0, base.1)?;
        let pressure = air.map_or(0.0, |a| a.pressure_pa());
        let load = pressure + deposit.load_pa(host_mass, radius);
        let mut radial_controls = controls.radial;
        radial_controls.max_eos_evaluations = radial_controls
            .max_eos_evaluations
            .min(controls.max_total_eos_evaluations - work);
        if radial_controls.max_eos_evaluations == 0 {
            return Err(Error::WorkBudgetExhausted);
        }
        let condensed = if protected.total_mass_kg() > 0.0 {
            if inventory.mass_kg(Material::Water) > 0.0 {
                let t = water_k.ok_or(Error::InvalidInput("missing water temperature"))?;
                CondensedStructure::Water(
                    water::protected::solve_seeded(
                        &inventory,
                        protected,
                        t,
                        load,
                        radial_controls,
                        central_pressure_pa,
                    )
                    .map_err(Error::Radial)?,
                )
            } else {
                CondensedStructure::Dry(
                    dry::protected::solve_loaded_seeded(
                        &inventory,
                        protected,
                        load,
                        radial_controls,
                        central_pressure_pa,
                    )
                    .map_err(Error::Radial)?,
                )
            }
        } else if inventory.mass_kg(Material::Water) > 0.0 {
            let t = water_k.ok_or(Error::InvalidInput("missing water temperature"))?;
            CondensedStructure::Water(
                water::solve_seeded(&inventory, t, load, radial_controls, central_pressure_pa)
                    .map_err(Error::Radial)?,
            )
        } else {
            CondensedStructure::Dry(
                if central_pressure_pa.is_some() {
                    dry::solve_loaded_seeded(&inventory, load, radial_controls, central_pressure_pa)
                } else {
                    dry::solve_loaded(&inventory, load, radial_controls)
                }
                .map_err(Error::Radial)?,
            )
        };
        work += condensed.diagnostics().work.eos_evaluations;
        central_pressure_pa = Some(condensed.diagnostics().central_pressure_pa);
        radius = condensed.radius_m();
        let top = deposit.top_radius_m(radius);
        let actual_air = column::atmosphere(&gas, column_k, gm, top)?;
        base = (host_mass + deposit.mass_kg(), top);
        temperature = warming::temperatures(request, base)?;
        let next_gas = column::partition(request, temperature.0, base.0, base.1)?;
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
        let actual_load = actual_pressure + deposit.load_pa(host_mass, radius);
        if (pressure - actual_pressure).abs() <= PRESSURE_TOLERANCE_PA
            && (load - actual_load).abs() <= PRESSURE_TOLERANCE_PA
            && partition_residual <= ALLOCATION_RELATIVE_TOLERANCE
            && (temperature.0 - column_k).abs() <= TEMPERATURE_RELATIVE_TOLERANCE * column_k
        {
            column::validate(&gas, actual_air.as_ref(), gm, top)?;
            let interface_residual = condensed.outer().pressure_pa - actual_load;
            if !interface_residual.is_finite()
                || interface_residual.abs() > 2.0 * PRESSURE_TOLERANCE_PA
            {
                return Err(Error::NonConvergence);
            }
            let result = CoupledStructure {
                condensed,
                deposit,
                gas,
                atmosphere: actual_air,
                surface_temperature_k: column_k,
                optical_depth: matches!(request.temperature, Temperature::Greenhouse { .. })
                    .then(|| actual_air.map_or(0.0, |a| greenhouse::optical_depth(&a))),
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

/// Condensed remainder: metal, rock and water to the radial host, cold-trapped
/// secondaries to the surface deposit.
fn split(total: &Inventory, gas: &Inventory) -> Result<(Inventory, deposit::Deposit), Error> {
    let inventory = Inventory::new(&Material::ALL.map(|m| (m, total.mass_kg(m) - gas.mass_kg(m))))
        .map_err(Error::Composition)?;
    let host = |m: Material| matches!(m, Material::Metal | Material::Silicate | Material::Water);
    for m in Material::ALL {
        // A transfer below one ulp of its own species' total cannot change that
        // total in f64. Accept it: the sum still closes exactly. Larger lost
        // transfers stay errors.
        if gas.mass_kg(m) > total.mass_kg(m) * f64::EPSILON
            && inventory.mass_kg(m) == total.mass_kg(m)
        {
            return Err(Error::InvalidClosure(m));
        }
    }
    let part = |keep: bool| {
        Inventory::new(&Material::ALL.map(|m| {
            let mass = if host(m) == keep {
                inventory.mass_kg(m)
            } else {
                0.0
            };
            (m, mass)
        }))
        .map_err(Error::Composition)
    };
    Ok((part(true)?, deposit::Deposit::new(part(false)?)?))
}
