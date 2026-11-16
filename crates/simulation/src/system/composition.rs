//! Conserved molecular/material inventories, independent of generation and labels.
//! Empty inventories are useful for reservoirs; a planet must retain positive mass.
//! See docs/simulation/planet-composition.md for bookkeeping and physical limits.

mod catalogue;
mod reservoirs;
pub use super::materials::Material;
pub use catalogue::MaterialProperties;
pub use reservoirs::{Budget, Reservoir};

use super::atmosphere::{Composition as GasComposition, Gas};
use std::fmt;

/// Private, finite, nonnegative SI masses. No implicit missing material.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Inventory {
    masses_kg: [f64; Material::ALL.len()],
}

impl Inventory {
    pub fn new(parts: &[(Material, f64)]) -> Result<Self, CompositionError> {
        let mut result = Self::default();
        let mut seen = [false; Material::ALL.len()];
        for &(material, mass_kg) in parts {
            nonnegative("material mass", mass_kg)?;
            if seen[material.index()] {
                return Err(CompositionError::DuplicateMaterial(material));
            }
            seen[material.index()] = true;
            result.masses_kg[material.index()] = mass_kg;
        }
        nonnegative("inventory total", result.total_mass_kg())?;
        Ok(result)
    }

    /// Fractions must sum to one within 1e-9; only rounding error is normalised.
    pub fn from_mass_fractions(
        total_mass_kg: f64,
        parts: &[(Material, f64)],
    ) -> Result<Self, CompositionError> {
        positive("total mass", total_mass_kg)?;
        for &(_, fraction) in parts {
            if !fraction.is_finite() || !(0.0..=1.0).contains(&fraction) {
                return Err(CompositionError::InvalidValue("mass fraction"));
            }
        }
        let weights = Self::new(parts)?;
        let sum = weights.total_mass_kg();
        if (sum - 1.0).abs() > 1e-9 {
            return Err(CompositionError::FractionSum);
        }
        Self::new(
            &parts
                .iter()
                .map(|&(material, fraction)| (material, fraction / sum * total_mass_kg))
                .collect::<Vec<_>>(),
        )
    }

    pub fn mass_kg(&self, material: Material) -> f64 {
        self.masses_kg[material.index()]
    }

    pub fn total_mass_kg(&self) -> f64 {
        self.masses_kg.iter().sum()
    }

    /// None for an empty reservoir, not an invented zero-composition planet.
    pub fn mass_fraction(&self, material: Material) -> Option<f64> {
        let total = self.total_mass_kg();
        (total > 0.0).then(|| self.mass_kg(material) / total)
    }

    pub fn scaled(&self, factor: f64) -> Result<Self, CompositionError> {
        nonnegative("inventory scale", factor)?;
        Self::new(&Material::ALL.map(|material| (material, self.mass_kg(material) * factor)))
    }

    pub fn checked_add(&self, other: &Self) -> Result<Self, CompositionError> {
        Self::new(&Material::ALL.map(|m| (m, self.mass_kg(m) + other.mass_kg(m))))
    }

    /// Molecular identity is preserved. No photochemistry or element conversion.
    pub fn from_gas_mass(mixture: &GasComposition, mass_kg: f64) -> Result<Self, CompositionError> {
        nonnegative("gas mass", mass_kg)?;
        let mean_u: f64 = Gas::ALL
            .iter()
            .map(|&gas| mixture.fraction(gas) * gas.molecular_mass_u())
            .sum();
        Self::new(&Gas::ALL.map(|gas| {
            (
                Material::from_gas(gas),
                mass_kg * (mixture.fraction(gas) * gas.molecular_mass_u() / mean_u),
            )
        }))
    }

    /// Convert a gas-only inventory to mole fractions. Empty means no gas.
    /// Condensable species are allowed here; their phase is the caller's input.
    pub fn gas_composition(&self) -> Result<Option<GasComposition>, CompositionError> {
        let mut weights = Vec::new();
        for material in Material::ALL {
            let mass = self.mass_kg(material);
            if mass == 0.0 {
                continue;
            }
            let gas = material
                .properties()
                .gas
                .ok_or(CompositionError::NotGas(material))?;
            // A common kg/u factor cancels. Scale by total first to avoid overflow.
            weights.push((gas, mass / self.total_mass_kg() / gas.molecular_mass_u()));
        }
        if weights.is_empty() {
            return Ok(None);
        }
        let sum: f64 = weights.iter().map(|(_, weight)| weight).sum();
        for (_, weight) in &mut weights {
            *weight /= sum;
        }
        GasComposition::new(&weights)
            .map(Some)
            .map_err(|_| CompositionError::InvalidValue("derived gas fractions"))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompositionError {
    InvalidValue(&'static str),
    DuplicateMaterial(Material),
    FractionSum,
    NotGas(Material),
    BudgetExceeded(Material),
    InvalidTransfer,
    TransferPrecision,
}

impl fmt::Display for CompositionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidValue(name) => write!(f, "invalid composition {name}"),
            Self::DuplicateMaterial(m) => write!(f, "duplicate material: {}", m.id()),
            Self::FractionSum => write!(f, "material mass fractions must sum to one (within 1e-9)"),
            Self::NotGas(m) => write!(f, "material has no molecular gas prescription: {}", m.id()),
            Self::BudgetExceeded(m) => write!(f, "allocation exceeds source inventory: {}", m.id()),
            Self::InvalidTransfer => write!(f, "transfer requires distinct retained reservoirs"),
            Self::TransferPrecision => write!(f, "transfer is below reservoir mass precision"),
        }
    }
}

impl std::error::Error for CompositionError {}

pub(crate) fn nonnegative(name: &'static str, value: f64) -> Result<(), CompositionError> {
    if value.is_finite() && value >= 0.0 {
        Ok(())
    } else {
        Err(CompositionError::InvalidValue(name))
    }
}

pub(crate) fn positive(name: &'static str, value: f64) -> Result<(), CompositionError> {
    nonnegative(name, value)?;
    if value > 0.0 {
        Ok(())
    } else {
        Err(CompositionError::InvalidValue(name))
    }
}

#[cfg(test)]
mod tests;
