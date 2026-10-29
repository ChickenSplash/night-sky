//! Transport records only. Simulation owns validation and physical prescriptions.
use serde::{Deserialize, Serialize};

/// Model provenance/scope, NOT a numerical tolerance or physical error bar.
/// ReferenceChecked does not certify a whole planet against observations.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelFidelity {
    ReferenceChecked,
    Approximate,
    Extrapolated,
}

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
    /// Includes suspended condensate for a layered envelope, not just ground.
    pub condensed_kg: f64,
    /// Gaseous/fluid allocation, not additional atmospheric-column mass.
    /// Layered records exclude their explicitly resolved suspended condensate.
    pub gas_kg: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PlanetPhysical {
    pub materials: Vec<MaterialBudget>,
    pub retained_mass_kg: f64,
    pub reference_radius_m: f64,
    /// Outer edge of the condensed layers: a labelled proxy in a fallback
    /// result, the solved, loaded condensed boundary in a layered result.
    /// Never an unconditionally resolved core.
    pub condensed_radius_m: f64,
    pub mean_density_kg_m3: f64,
    pub reference_gravity_m_s2: f64,
    pub h_he_mass_fraction: f64,
    pub inertia_factor: f64,
    pub radius_reference: RadiusReference,
    /// Stable model-caveat IDs, not statistical uncertainties.
    pub approximations: Vec<String>,
}

/// Region record owned by the active `PlanetResult` or a separate candidate.
/// The live `Planet::physical` is a projection, not another physical authority.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LayeredPlanet {
    pub model: String,
    /// Why a layered attempt refused. Initial/unavailable/worker-failed fallback
    /// has no solver refusal; authoritative provenance lives in `PlanetResult`.
    /// Fallback records never count as layered solves.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fallback_reason: Option<String>,
    pub model_fidelity: ModelFidelity,
    /// Global closure assumptions, in addition to each layer's limitations.
    pub model_limitations: Vec<String>,
    /// Conserved material and reference structure. `approximations` lists the
    /// model IDs used. `reference_radius_m` is the top of the condensed surface
    /// (frost included) for thin models, or the explicit one-bar envelope level
    /// when present. Sub-bar envelopes reference their condensed/gas boundary.
    pub physical: PlanetPhysical,
    /// `rock`, `ocean`, `ice`, `frost`, `deep ice`, `water coexistence` or
    /// `unresolved hot water`, or `envelope` (one-bar reference, not ground).
    /// The fallback adds `unresolved water` and `unresolved volatiles`.
    pub surface: String,
    pub surface_temperature_k: f64,
    /// Gas pressure on the surface, or one bar for a one-bar envelope reference.
    /// Sub-bar envelopes expose the actual condensed/gas boundary pressure.
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
    /// Full region inventory. Empty only for older records without a breakdown.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub materials: Vec<LayerMaterial>,
    /// Optional phase SUB-allocations of the existing layer mass, never added
    /// to it. Empty when the selected model does not resolve a local partition.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub phase_allocations: Vec<PhaseAllocation>,
    pub model_fidelity: ModelFidelity,
    /// Physical limitations of this region, not numerical solver diagnostics.
    pub model_limitations: Vec<String>,
    /// `atmosphere`, `frost`, `ocean`, `ice`, `deep_ice`, `hot_water`,
    /// `water_coexistence`, `water_compression`, `envelope`, `mantle` or `core`,
    /// or the fallback's `volatiles`.
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
    /// For envelope/compression layers this is the outer boundary value, not
    /// an assertion of uniform temperature through the layer.
    pub temperature_k: Option<f64>,
    /// Density or column model ID.
    pub model: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LayerMaterial {
    pub material: String,
    pub mass_kg: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PhaseAllocation {
    pub material: String,
    pub vapour_kg: f64,
    /// Locally suspended liquid/solid-like material, not rainout or ground.
    pub condensate_kg: f64,
}
