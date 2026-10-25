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
    /// Cold heavy-material proxy, not an envelope bottom.
    pub condensed_radius_m: f64,
    pub mean_density_kg_m3: f64,
    pub reference_gravity_m_s2: f64,
    pub h_he_mass_fraction: f64,
    pub inertia_factor: f64,
    pub radius_reference: RadiusReference,
    /// Stable model-caveat IDs, not statistical uncertainties.
    pub approximations: Vec<String>,
}
