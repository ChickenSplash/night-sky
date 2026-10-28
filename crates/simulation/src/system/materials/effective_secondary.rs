//! Engineering hot/deep secondary-fluid specific volume. NOT a chemical EOS,
//! metallic mixture, condensed-phase prescription or extension of Peng-Robinson.
use super::{Material, molecular};
use crate::units::{ATOMIC_MASS_KG, BOLTZMANN_J_K};

pub const MODEL_ID: &str = "thermal_plus_water_compression_secondary_proxy_v1";
pub const MAX_PRESSURE_PA: f64 = 1e13;
pub const MAX_TEMPERATURE_K: f64 = 1e5;
pub const APPROXIMATION_NOTES: &[&str] = &[
    "Hot/deep secondary specific volume: molecular ideal thermal volume plus Seager water-like compression volume",
    "Water-like compression is a structural surrogate, not conversion of any constituent into water",
    "Where a molecular handoff exists, each component is explicitly normalised to its molecular endpoint density for continuity",
    "No binary excess volume, phase equilibrium, dissociation, ionisation or metallic-depth chemistry; no quantified physical error bound",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    StateDomain,
    Component(molecular::Error),
}

impl Error {
    pub fn label(self) -> &'static str {
        match self {
            Self::StateDomain => "effective secondary pressure/temperature domain",
            Self::Component(e) => e.label(),
        }
    }
}

pub fn density_kg_m3(material: Material, p: f64, t: f64) -> Result<f64, Error> {
    if !p.is_finite()
        || !(1.0..=MAX_PRESSURE_PA).contains(&p)
        || !t.is_finite()
        || !(molecular::MIN_TEMPERATURE_K..=MAX_TEMPERATURE_K).contains(&t)
    {
        return Err(Error::StateDomain);
    }
    // This slice does not turn a failed condensation/critical screen into an
    // effective fluid. There is still no liquid/solid secondary allocation.
    molecular::screen_pure_endpoint(material, p, t).map_err(Error::Component)?;
    let gas = material.properties().gas.ok_or(Error::StateDomain)?;
    let r = BOLTZMANN_J_K / (ATOMIC_MASS_KG * gas.molecular_mass_u());
    let compressed = 1460.0 + 0.00311 * p.powf(0.513);
    let rho = 1.0 / (r * t / p + 1.0 / compressed);
    if !rho.is_finite() || rho <= 0.0 {
        return Err(Error::StateDomain);
    }
    Ok(rho)
}

#[cfg(test)]
mod tests;
