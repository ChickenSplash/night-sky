//! Authoritative-result contract. Simulation validates it; adoption is a separate
//! operation. An active result remains usable while refinement runs or fails.
use crate::LayeredPlanet;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PhysicalRevision {
    /// Caller-owned identity of immutable supply and forcing, not a timestamp.
    pub inputs: String,
    /// Caller-owned prescription/resource revision, including data availability.
    pub model: String,
    /// Monotone adoption sequence within one planet's lifetime. Zero is initial.
    /// Equal sequences must never identify different adopted facts.
    pub sequence: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResultProvenance {
    Layered,
    Fallback,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum RefinementStatus {
    Pending,
    Running,
    Complete,
    Refused { reason: String },
    Unavailable { reason: String },
    Failed { reason: String },
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
    pub refinement: RefinementStatus,
    pub surface_knowledge: SurfaceKnowledge,
    /// Sole source for physical projections. No atmosphere appearance, craft
    /// state or adoption permission is inferred from this record.
    pub record: LayeredPlanet,
}
