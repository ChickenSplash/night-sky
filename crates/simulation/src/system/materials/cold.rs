//! Seager et al. (2007), equation 11 / table 3, SI density EOS.
//! Cold Fe and MgSiO3 proxies, not a mineral phase or melting prescription.

use super::Material;
use std::fmt;

pub const MAX_PRESSURE_PA: f64 = 1e13;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ColdEosError {
    UnsupportedMaterial(Material),
    OutsidePressureDomain {
        material: Material,
        pressure_pa: f64,
    },
}

impl fmt::Display for ColdEosError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedMaterial(m) => write!(f, "no cold density EOS for {}", m.id()),
            Self::OutsidePressureDomain {
                material,
                pressure_pa,
            } => write!(
                f,
                "cold {} EOS pressure outside [0, 1e13] Pa: {pressure_pa}",
                material.id()
            ),
        }
    }
}

impl std::error::Error for ColdEosError {}

pub fn density_kg_m3(material: Material, pressure_pa: f64) -> Result<f64, ColdEosError> {
    let (rho0, c, n) = match material {
        Material::Metal => (8300.0, 0.00349, 0.528),
        Material::Silicate => (4100.0, 0.00161, 0.541),
        _ => return Err(ColdEosError::UnsupportedMaterial(material)),
    };
    if !pressure_pa.is_finite() || !(0.0..=MAX_PRESSURE_PA).contains(&pressure_pa) {
        return Err(ColdEosError::OutsidePressureDomain {
            material,
            pressure_pa,
        });
    }
    Ok(rho0 + c * pressure_pa.powf(n))
}
