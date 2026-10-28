//! Uniform additive-specific-volume H/He-rich molecular mixture. H/He density
//! remains supplied by the screened CMS19/HG23 reference, including its masks.
use super::{Material, molecular};
use crate::system::composition::Inventory;

pub const MODEL_ID: &str = "hhe_rich_additive_molecular_volume_v1";
/// Perturbative engineering scope, not a classification or a phase boundary.
pub const MAX_SECONDARY_MASS_FRACTION: f64 = 0.05;

const SECONDARIES: [Material; 5] = [
    Material::CarbonDioxide,
    Material::Nitrogen,
    Material::Methane,
    Material::Oxygen,
    Material::Argon,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    CompositionDomain,
    Component(molecular::Error),
    InvalidDensity,
}

impl Error {
    pub fn label(self) -> &'static str {
        match self {
            Self::CompositionDomain => "H/He-rich mixture composition domain",
            Self::Component(e) => e.label(),
            Self::InvalidDensity => "H/He-rich mixture density stability",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mixture {
    fractions: Inventory,
    light_fraction: f64,
    secondary_mass: f64,
}

impl Mixture {
    pub fn new(gas: &Inventory) -> Result<Self, Error> {
        let mass = gas.total_mass_kg();
        let light = gas.mass_kg(Material::Hydrogen) + gas.mass_kg(Material::Helium);
        let secondary_mass: f64 = SECONDARIES.into_iter().map(|m| gas.mass_kg(m)).sum();
        if mass <= 0.0
            || light <= 0.0
            || secondary_mass / mass > MAX_SECONDARY_MASS_FRACTION * (1.0 + 8.0 * f64::EPSILON)
            || [Material::Metal, Material::Silicate, Material::Water]
                .into_iter()
                .any(|m| gas.mass_kg(m) > 0.0)
        {
            return Err(Error::CompositionDomain);
        }
        Ok(Self {
            fractions: gas
                .scaled(1.0 / mass)
                .map_err(|_| Error::CompositionDomain)?,
            light_fraction: light / mass,
            secondary_mass,
        })
    }

    pub fn density_kg_m3(
        self,
        pressure_pa: f64,
        temperature_k: f64,
        hhe_density_kg_m3: f64,
    ) -> Result<f64, Error> {
        if !hhe_density_kg_m3.is_finite() || hhe_density_kg_m3 <= 0.0 {
            return Err(Error::InvalidDensity);
        }
        // Preserve the H/He endpoint exactly; no absent species is evaluated.
        if self.secondary_mass == 0.0 {
            return Ok(hhe_density_kg_m3);
        }
        let mut volume = self.light_fraction / hhe_density_kg_m3;
        for m in SECONDARIES {
            let fraction = self.fractions.mass_kg(m);
            if fraction > 0.0 {
                volume += fraction
                    / molecular::density_kg_m3(m, pressure_pa, temperature_k)
                        .map_err(Error::Component)?;
            }
        }
        if !volume.is_finite() || volume <= 0.0 {
            return Err(Error::InvalidDensity);
        }
        Ok(1.0 / volume)
    }
}

#[cfg(test)]
mod tests;
