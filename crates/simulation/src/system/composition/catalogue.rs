use super::super::atmosphere::Gas;
use super::Material;

impl Material {
    pub fn from_gas(gas: Gas) -> Self {
        match gas {
            Gas::Nitrogen => Self::Nitrogen,
            Gas::CarbonDioxide => Self::CarbonDioxide,
            Gas::Hydrogen => Self::Hydrogen,
            Gas::Helium => Self::Helium,
            Gas::Water => Self::Water,
            Gas::Methane => Self::Methane,
            Gas::Oxygen => Self::Oxygen,
            Gas::Argon => Self::Argon,
        }
    }

    pub fn properties(self) -> MaterialProperties {
        let gas = Gas::ALL
            .into_iter()
            .find(|&gas| Self::from_gas(gas) == self);
        MaterialProperties { gas }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MaterialProperties {
    pub gas: Option<Gas>,
}
