//! Uniform additive-specific-volume H/He-rich molecular mixture. H/He density
//! remains supplied by the screened CMS19/HG23 reference, including its masks.
use super::{Material, effective_secondary, molecular};
use crate::system::composition::Inventory;

pub const MODEL_ID: &str = "hhe_rich_additive_molecular_volume_v1";
pub const ENRICHED_MODEL_ID: &str = "secondary_enriched_additive_molecular_volume_v1";
pub const EFFECTIVE_MODEL_ID: &str = "additive_effective_secondary_volume_v1";
pub const COMPOSITE_MODEL_ID: &str = "molecular_to_effective_secondary_envelope_v1";
/// Perturbative engineering scope, not a classification or a phase boundary.
pub const MAX_SECONDARY_MASS_FRACTION: f64 = 0.05;

pub(crate) const SECONDARIES: [Material; 5] = [
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
    Effective(effective_secondary::Error),
    InvalidDensity,
}

impl Error {
    pub fn label(self) -> &'static str {
        match self {
            Self::CompositionDomain => "H/He-rich mixture composition domain",
            Self::Component(e) => e.label(),
            Self::Effective(e) => e.label(),
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
        let mixture = Self::new_enriched(gas)?;
        let secondary_mass: f64 = SECONDARIES.into_iter().map(|m| gas.mass_kg(m)).sum();
        if secondary_mass / gas.total_mass_kg()
            > MAX_SECONDARY_MASS_FRACTION * (1.0 + 8.0 * f64::EPSILON)
        {
            return Err(Error::CompositionDomain);
        }
        Ok(mixture)
    }

    /// Explicit enriched structural approximation, not a widened validation claim
    /// for the original H/He-rich model. Positive H/He is still required.
    pub fn new_enriched(gas: &Inventory) -> Result<Self, Error> {
        let mass = gas.total_mass_kg();
        let light = gas.mass_kg(Material::Hydrogen) + gas.mass_kg(Material::Helium);
        let secondary_mass: f64 = SECONDARIES.into_iter().map(|m| gas.mass_kg(m)).sum();
        if mass <= 0.0
            || light <= 0.0
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
        self.density_with_components(hhe_density_kg_m3, |m| {
            molecular::density_kg_m3(m, pressure_pa, temperature_k).map_err(Error::Component)
        })
    }

    pub(crate) fn density_with_components(
        self,
        hhe_density_kg_m3: f64,
        mut component: impl FnMut(Material) -> Result<f64, Error>,
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
                let density = component(m)?;
                if !density.is_finite() || density <= 0.0 {
                    return Err(Error::InvalidDensity);
                }
                volume += fraction / density;
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
