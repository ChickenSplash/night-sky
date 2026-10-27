//! Transport records only. Simulation owns validation and physical prescriptions.
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RadiusReference {
    SolidSurface,
    UnresolvedVolatileBoundary,
    OneBar,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MaterialBudget {
    /// Stable catalogue identifier, never an array index or display label.
    pub material: String,
    pub source_kg: f64,
    pub retained_kg: f64,
    pub escaped_kg: f64,
    pub interior_kg: f64,
    pub condensed_kg: f64,
    /// Entire gas reservoir, not additional atmospheric-column mass.
    pub gas_kg: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PlanetPhysical {
    pub materials: Vec<MaterialBudget>,
    pub retained_mass_kg: f64,
    pub reference_radius_m: f64,
    /// Cold heavy-material proxy. `thin_boundary_column` or
    /// `extended_boundary_column` promotes it to an approximate atmospheric
    /// boundary; never an unconditionally resolved core.
    pub condensed_radius_m: f64,
    pub mean_density_kg_m3: f64,
    pub reference_gravity_m_s2: f64,
    pub h_he_mass_fraction: f64,
    pub inertia_factor: f64,
    pub radius_reference: RadiusReference,
    /// Stable model-caveat IDs, not statistical uncertainties.
    pub approximations: Vec<String>,
}

/// Experimental layered-model result, alongside and never replacing
/// `Planet::physical`. Live rendering and navigation do not read it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LayeredPlanet {
    pub model: String,
    /// Same transport shape as the legacy record. `approximations` lists the
    /// model IDs used, and `reference_radius_m` is the top of the condensed
    /// surface (frost included), not an atmospheric level.
    pub physical: PlanetPhysical,
    /// `rock`, `ocean`, `ice`, `frost`, `deep ice`, `water coexistence` or
    /// `unresolved hot water`.
    pub surface: String,
    pub surface_temperature_k: f64,
    /// Gas pressure on the surface, zero only when there is no gas at all.
    pub surface_pressure_pa: f64,
    pub greenhouse_optical_depth: Option<f64>,
    /// Absent when one bar lies below the surface or there is no gas.
    pub one_bar_radius_m: Option<f64>,
    /// Mass removed by this model's blow-off, beyond generation's own escape.
    pub blowoff_kg: f64,
    /// Outermost first.
    pub layers: Vec<Layer>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Layer {
    /// `atmosphere`, `frost`, `ocean`, `ice`, `deep_ice`, `hot_water`,
    /// `water_coexistence`, `mantle` or `core`.
    pub kind: String,
    /// Material catalogue ID holding most of the layer's mass.
    pub material: String,
    pub inner_radius_m: f64,
    pub outer_radius_m: f64,
    pub mass_kg: f64,
    pub inner_pressure_pa: f64,
    /// Absent for the atmosphere, whose finite shell has no resolved top pressure.
    pub outer_pressure_pa: Option<f64>,
    /// Absent where the model establishes no temperature (cold refractories).
    pub temperature_k: Option<f64>,
    /// Density or column model ID.
    pub model: String,
}
