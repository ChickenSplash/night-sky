//! Presentation only: no physical records or configurable schema.
use serde::Serialize;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Section {
    #[default]
    Overview,
    Composition,
    Orbit,
    Rotation,
    Atmosphere,
}

impl Section {
    pub const ALL: [Self; 5] = [
        Self::Overview,
        Self::Composition,
        Self::Orbit,
        Self::Rotation,
        Self::Atmosphere,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::Overview => "Overview",
            Self::Composition => "Composition",
            Self::Orbit => "Orbit",
            Self::Rotation => "Rotation",
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
