//! Physical atmospheric states, independent of generation, climate and optics.
//! An ideal-gas, well-mixed, isothermal column uses constant spherical gravity
//! at a supplied reference radius. Derived extent is an operational cutoff,
//! not an exobase. See docs/simulation/atmospheres.md for conventions and limits.

use crate::units::{ATOMIC_MASS_KG, BOLTZMANN_J_K};
use std::fmt;

pub mod generation;
mod wire;

/// One bar in SI pascals.
pub const ONE_BAR_PA: f64 = 100_000.0;
/// Match the renderer's current exponential-column cutoff.
pub const EXTENT_SCALE_HEIGHTS: f64 = 12.5;
/// Operational warning threshold, not a physical atmospheric boundary.
/// At this height/radius, spherical gravity has fallen by about 17%.
pub const MAX_THIN_EXTENT_RATIO: f64 = 0.1;

/// Supported constituents. Fractions are by molecule, not by mass.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Gas {
    Nitrogen,
    CarbonDioxide,
    Hydrogen,
    Helium,
    Water,
    Methane,
    Oxygen,
    Argon,
}

impl Gas {
    pub fn formula(self) -> &'static str {
        match self {
            Self::Nitrogen => "N2",
            Self::CarbonDioxide => "CO2",
            Self::Hydrogen => "H2",
            Self::Helium => "He",
            Self::Water => "H2O",
            Self::Methane => "CH4",
            Self::Oxygen => "O2",
            Self::Argon => "Ar",
        }
    }

    pub fn from_formula(formula: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|gas| gas.formula() == formula)
    }
    pub const ALL: [Self; 8] = [
        Self::Nitrogen,
        Self::CarbonDioxide,
        Self::Hydrogen,
        Self::Helium,
        Self::Water,
        Self::Methane,
        Self::Oxygen,
        Self::Argon,
    ];

    fn index(self) -> usize {
        Self::ALL.iter().position(|gas| *gas == self).unwrap()
    }

    /// Representative molecular mass, in atomic mass units; fixed isotope mix.
    pub fn molecular_mass_u(self) -> f64 {
        match self {
            Self::Nitrogen => 28.014,
            Self::CarbonDioxide => 44.009,
            Self::Hydrogen => 2.016,
            Self::Helium => 4.002_602,
            Self::Water => 18.015,
            Self::Methane => 16.043,
            Self::Oxygen => 31.998,
            Self::Argon => 39.948,
        }
    }
}

/// Validated and normalised mole fractions. No implicit missing constituent.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Composition {
    fractions: [f64; 8],
}

impl Composition {
    /// Supply each constituent at most once. The sum must be within 1e-9 of
    /// one; only rounding error is normalised, not arbitrary relative weights.
    pub fn new(parts: &[(Gas, f64)]) -> Result<Self, AtmosphereError> {
        let mut fractions = [0.0; 8];
        let mut seen = [false; 8];
        for &(gas, fraction) in parts {
            if !fraction.is_finite() || !(0.0..=1.0).contains(&fraction) {
                return Err(AtmosphereError::InvalidValue("mole fraction"));
            }
            let index = gas.index();
            if seen[index] {
                return Err(AtmosphereError::DuplicateGas(gas));
            }
            seen[index] = true;
            fractions[index] = fraction;
        }
        let sum: f64 = fractions.iter().sum();
        if (sum - 1.0).abs() > 1e-9 {
            return Err(AtmosphereError::FractionSum);
        }
        Ok(Self {
            fractions: fractions.map(|fraction| fraction / sum),
        })
    }

    pub fn fraction(&self, gas: Gas) -> f64 {
        self.fractions[gas.index()]
    }

    /// Mean mass per molecule, kg, not molar mass in kg/mol.
    pub fn mean_molecular_mass_kg(&self) -> f64 {
        Gas::ALL
            .iter()
            .map(|gas| self.fraction(*gas) * gas.molecular_mass_u() * ATOMIC_MASS_KG)
            .sum()
    }
}

/// The physical meaning of the radius at which pressure is specified.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReferenceLevel {
    /// Reference ground spheroid; no terrain correction or rotation here yet.
    Surface,
    /// Envelope world's chosen 1 bar radius, not a solid surface/cloud top.
    OneBar,
}

/// Prescribed aerosol material, not a gas or a solved condensation product.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AerosolMaterial {
    WaterDroplets,
    WaterIce,
    SulfuricAcid,
    AmmoniaIce,
    MineralDust,
    OrganicHaze,
}

/// One exponential aerosol profile based at the same reference level as gas.
/// This is a simple physical placeholder, not resolved cloud layers or optics.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AerosolLayer {
    material: AerosolMaterial,
    reference_density_kg_m3: f64,
    scale_height_m: f64,
    particle_radius_m: f64,
}

impl AerosolLayer {
    pub fn new(
        material: AerosolMaterial,
        reference_density_kg_m3: f64,
        scale_height_m: f64,
        particle_radius_m: f64,
    ) -> Result<Self, AtmosphereError> {
        positive("aerosol density", reference_density_kg_m3)?;
        positive("aerosol scale height", scale_height_m)?;
        positive("aerosol particle radius", particle_radius_m)?;
        Ok(Self {
            material,
            reference_density_kg_m3,
            scale_height_m,
            particle_radius_m,
        })
    }

    pub fn material(&self) -> AerosolMaterial {
        self.material
    }
    pub fn reference_density_kg_m3(&self) -> f64 {
        self.reference_density_kg_m3
    }
    pub fn scale_height_m(&self) -> f64 {
        self.scale_height_m
    }
    pub fn particle_radius_m(&self) -> f64 {
        self.particle_radius_m
    }
}

/// Absence is not a zero-pressure gas column or a rendering-slider minimum.
/// Not yet attached to generated planets or their wire models.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Atmosphere {
    #[default]
    Airless,
    Present(GasAtmosphere),
}

/// Validated physical input. Equilibrium and surface-climate temperatures are
/// deliberately not represented by this column's temperature.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GasAtmosphere {
    composition: Composition,
    reference_level: ReferenceLevel,
    pressure_pa: f64,
    temperature_k: f64,
    aerosol: Option<AerosolLayer>,
}

impl GasAtmosphere {
    pub fn new(
        composition: Composition,
        reference_level: ReferenceLevel,
        pressure_pa: f64,
        temperature_k: f64,
        aerosol: Option<AerosolLayer>,
    ) -> Result<Self, AtmosphereError> {
        positive("reference pressure", pressure_pa)?;
        positive("reference temperature", temperature_k)?;
        if reference_level == ReferenceLevel::OneBar && pressure_pa != ONE_BAR_PA {
            return Err(AtmosphereError::OneBarPressure);
        }
        Ok(Self {
            composition,
            reference_level,
            pressure_pa,
            temperature_k,
            aerosol,
        })
    }

    pub fn composition(&self) -> &Composition {
        &self.composition
    }
    pub fn reference_level(&self) -> ReferenceLevel {
        self.reference_level
    }
    pub fn pressure_pa(&self) -> f64 {
        self.pressure_pa
    }
    pub fn temperature_k(&self) -> f64 {
        self.temperature_k
    }
    pub fn aerosol(&self) -> Option<&AerosolLayer> {
        self.aerosol.as_ref()
    }

    /// GM in m³/s², reference radius in metres (use the volume-equivalent
    /// radius for a spheroid). No rotation or latitude dependence yet.
    /// Extended columns are returned with an explicit warning, not clamped.
    pub fn column(&self, gm_m3_s2: f64, radius_m: f64) -> Result<Column, AtmosphereError> {
        positive("gravitational parameter", gm_m3_s2)?;
        positive("reference radius", radius_m)?;
        let molecular_mass_kg = self.composition.mean_molecular_mass_kg();
        let gravity_m_s2 = gm_m3_s2 / radius_m / radius_m;
        let thermal_energy = BOLTZMANN_J_K * self.temperature_k;
        let density_kg_m3 = self.pressure_pa * molecular_mass_kg / thermal_energy;
        let scale_height_m = thermal_energy / molecular_mass_kg / gravity_m_s2;
        let aerosol_height = self.aerosol.map_or(0.0, |aerosol| aerosol.scale_height_m);
        let extent_m = EXTENT_SCALE_HEIGHTS * scale_height_m.max(aerosol_height);
        for value in [gravity_m_s2, density_kg_m3, scale_height_m, extent_m] {
            positive("derived column quantity", value)?;
        }
        let validity = if extent_m / radius_m <= MAX_THIN_EXTENT_RATIO {
            ColumnValidity::Thin
        } else {
            ColumnValidity::Extended
        };
        Ok(Column {
            molecular_mass_kg,
            gravity_m_s2,
            density_kg_m3,
            scale_height_m,
            extent_m,
            validity,
        })
    }
}

/// Approximation status, not an atmospheric-retention or climate verdict.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColumnValidity {
    Thin,
    /// Constant-gravity estimates only. A future consumer must choose how to
    /// handle this explicitly before using the extent for rendering/clearance.
    Extended,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Column {
    pub molecular_mass_kg: f64,
    pub gravity_m_s2: f64,
    pub density_kg_m3: f64,
    pub scale_height_m: f64,
    /// Height above the reference level, not radius from the planet's centre.
    pub extent_m: f64,
    pub validity: ColumnValidity,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AtmosphereError {
    InvalidValue(&'static str),
    DuplicateGas(Gas),
    FractionSum,
    OneBarPressure,
}

impl fmt::Display for AtmosphereError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidValue(name) => write!(
                f,
                "invalid {name}: must be finite and in its physical range"
            ),
            Self::DuplicateGas(gas) => write!(f, "duplicate gas: {gas:?}"),
            Self::FractionSum => write!(f, "gas mole fractions must sum to one (within 1e-9)"),
            Self::OneBarPressure => write!(f, "a one-bar reference level requires 100000 Pa"),
        }
    }
}

impl std::error::Error for AtmosphereError {}

fn positive(name: &'static str, value: f64) -> Result<(), AtmosphereError> {
    if value.is_finite() && value > 0.0 {
        Ok(())
    } else {
        Err(AtmosphereError::InvalidValue(name))
    }
}

#[cfg(test)]
mod tests;
