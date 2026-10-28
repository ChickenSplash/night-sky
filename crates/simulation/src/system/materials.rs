//! Shared material identities and local constitutive physics, independent of gas columns.

pub mod cold;
pub mod hhe;
pub mod water;

/// Stable IDs, not ordinal values, are the external identity of constituents.
/// Molecular budgets deliberately do not permit reactions between species.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Material {
    Metal,
    Silicate,
    Water,
    Hydrogen,
    Helium,
    Nitrogen,
    CarbonDioxide,
    Methane,
    Oxygen,
    Argon,
}

impl Material {
    pub const ALL: [Self; 10] = [
        Self::Metal,
        Self::Silicate,
        Self::Water,
        Self::Hydrogen,
        Self::Helium,
        Self::Nitrogen,
        Self::CarbonDioxide,
        Self::Methane,
        Self::Oxygen,
        Self::Argon,
    ];

    pub fn id(self) -> &'static str {
        match self {
            Self::Metal => "iron_nickel",
            Self::Silicate => "silicate",
            Self::Water => "water",
            Self::Hydrogen => "hydrogen",
            Self::Helium => "helium",
            Self::Nitrogen => "nitrogen",
            Self::CarbonDioxide => "carbon_dioxide",
            Self::Methane => "methane",
            Self::Oxygen => "oxygen",
            Self::Argon => "argon",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|m| m.id() == id)
    }

    pub(super) fn index(self) -> usize {
        Self::ALL.iter().position(|m| *m == self).unwrap()
    }
}
