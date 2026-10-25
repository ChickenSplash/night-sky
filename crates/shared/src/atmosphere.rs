//! Atmosphere snapshots for transport and inspection. Physical validation and
//! generation belong to the simulation, not these serialisable records.
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PlanetEnvironment {
    /// Approximate phase-averaged bolometric flux from every system star.
    pub irradiation_w_m2: f64,
    pub assumed_bond_albedo: f64,
    /// Full-redistribution, unit-emissivity estimate, not surface climate.
    pub equilibrium_temperature_k: f64,
    /// System age used by the atmospheric-loss heuristic.
    pub age_gyr: f64,
    /// None means airless; absent Planet::environment means not yet generated.
    pub atmosphere: Option<AtmosphereData>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AtmosphereData {
    pub gases: Vec<GasFraction>,
    pub reference_level: AtmosphereReference,
    pub pressure_pa: f64,
    /// Representative column temperature, not a solved surface temperature.
    pub temperature_k: f64,
    pub aerosol: Option<AerosolData>,
    pub mean_molecular_mass_kg: f64,
    pub reference_density_kg_m3: f64,
    pub scale_height_m: f64,
    #[serde(default)]
    pub reference_radius_m: f64,
    pub extent_m: f64,
    /// Geometric extent exceeds 10% of the reference radius, not a validity warning.
    pub extended: bool,
    /// A static spherical column cannot reach its cutoff below its sonic radius.
    /// An explicitly approximate exponential fallback is used, not a wind solution.
    #[serde(default)]
    pub hydrostatic_unresolved: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GasFraction {
    /// Chemical formula, for example "N2", "CO2" or "H2".
    pub gas: String,
    pub mole_fraction: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AtmosphereReference {
    Surface,
    OneBar,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AerosolKind {
    WaterDroplets,
    WaterIce,
    SulfuricAcid,
    AmmoniaIce,
    MineralDust,
    OrganicHaze,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AerosolData {
    pub material: AerosolKind,
    pub reference_density_kg_m3: f64,
    pub scale_height_m: f64,
    pub particle_radius_m: f64,
}
