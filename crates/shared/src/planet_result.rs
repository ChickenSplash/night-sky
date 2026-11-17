//! Authoritative-result contract. Simulation validates it. Generation's result
//! is the planet's for its whole lifetime: no later solve replaces it.
use crate::LayeredPlanet;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PhysicalRevision {
    /// Caller-owned identity of immutable supply and forcing, not a timestamp.
    pub inputs: String,
    /// Caller-owned prescription/resource revision, including data availability.
    pub model: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResultProvenance {
    Layered,
    Fallback,
}

/// Knowledge, not landing permission. Even a resolved solid phase says nothing
/// about terrain, strength or safe temperature. Cold rock mechanics leave phase
/// unresolved, including hot rock; there is no invented melting cutoff.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SurfaceKnowledge {
    GasReference,
    Liquid,
    SolidPhase,
    Unresolved,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PlanetResult {
    pub revision: PhysicalRevision,
    pub provenance: ResultProvenance,
    pub surface_knowledge: SurfaceKnowledge,
    /// Sole source for physical projections. No atmosphere appearance or craft
    /// state is inferred from this record.
    pub record: LayeredPlanet,
}
