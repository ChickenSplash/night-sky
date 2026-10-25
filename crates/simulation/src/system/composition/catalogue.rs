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
        let condensed_fit = match self {
            Self::Metal => Some(CondensedFit {
                mass_scale_earth: 5.80,
                radius_scale_earth: 2.52,
                k1: -0.209490,
                k2: 0.0804,
                k3: 0.394,
            }),
            Self::Silicate => Some(CondensedFit {
                mass_scale_earth: 10.55,
                radius_scale_earth: 3.90,
                k1: -0.209594,
                k2: 0.0799,
                k3: 0.413,
            }),
            Self::Water => Some(CondensedFit {
                mass_scale_earth: 5.52,
                radius_scale_earth: 4.43,
                k1: -0.209396,
                k2: 0.0807,
                k3: 0.375,
            }),
            _ => None,
        };
        let gas = Gas::ALL
            .into_iter()
            .find(|&gas| Self::from_gas(gas) == self);
        MaterialProperties { condensed_fit, gas }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MaterialProperties {
    /// None explicitly means no supported pure condensed EOS/radius model.
    pub condensed_fit: Option<CondensedFit>,
    pub gas: Option<Gas>,
}

/// Seager et al. (2007), equation 23 / table 4, modified-polytropic rows.
/// Cold Fe, MgSiO3 and water-ice proxies, not alloy/mineral/phase solvers.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CondensedFit {
    pub mass_scale_earth: f64,
    pub radius_scale_earth: f64,
    pub k1: f64,
    pub k2: f64,
    pub k3: f64,
}
