//! Presentation only: no physical records or configurable schema.
use serde::Serialize;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Section {
    #[default]
    Overview,
    /// Experimental layered solve, beside the legacy facts it may disagree with.
    Layered,
    Composition,
    Atmosphere,
    Orbit,
    ModelDetails,
}

impl Section {
    pub const ALL: [Self; 6] = [
        Self::Overview,
        Self::Layered,
        Self::Composition,
        Self::Atmosphere,
        Self::Orbit,
        Self::ModelDetails,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::Overview => "Overview",
            Self::Layered => "Layered model (experimental)",
            Self::Composition => "Composition",
            Self::Orbit => "Orbit and rotation",
            Self::ModelDetails => "Model details",
            Self::Atmosphere => "Atmosphere",
        }
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "state", content = "text", rename_all = "snake_case")]
pub enum Value {
    Known(String),
    Unknown(String),
}

impl Value {
    pub fn text(&self) -> &str {
        match self {
            Self::Known(v) | Self::Unknown(v) => v,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct Fact {
    pub id: String,
    pub section: Section,
    pub label: String,
    pub unit: Option<&'static str>,
    pub value: Value,
}
