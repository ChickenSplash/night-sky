//! IAPWS R14-08(2011), equations 1–5, attributed to IAPWS.
//! Standalone solid/liquid coexistence curves, NOT a high-pressure phase
//! selector or density EOS. In particular these give no solid/solid boundaries.

use super::{TRIPLE_PRESSURE_PA, TRIPLE_TEMPERATURE_K, Unsupported, WaterError};

pub const MODEL_ID: &str = "iapws_r14_08_2011_melting_v1";

/// Names the solid on a solid/liquid melting curve, not a stable phase at an
/// arbitrary P–T point. Multiple branches can exist at the same temperature.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Branch {
    Ih,
    III,
    V,
    VI,
    VII,
}

impl Branch {
    /// Inclusive source validity interval in kelvin, not the density API mask.
    pub const fn temperature_bounds_k(self) -> (f64, f64) {
        match self {
            Self::Ih => (251.165, TRIPLE_TEMPERATURE_K),
            Self::III => (251.165, 256.164),
            Self::V => (256.164, 273.31),
            Self::VI => (273.31, 355.0),
            Self::VII => (355.0, 715.0),
        }
    }

    /// Published relative pressure uncertainty (table 2), not a numerical
    /// root tolerance, probability distribution or solid/solid uncertainty.
    pub const fn relative_pressure_uncertainty(self) -> f64 {
        match self {
            Self::Ih => 0.02,
            Self::III | Self::V | Self::VI => 0.03,
            Self::VII => 0.07,
        }
    }
}

/// Absolute coexistence pressure in Pa for the explicitly selected branch.
/// Refuses extrapolation. Rounded source coefficients leave small endpoint
/// mismatches at adjoining branches; no offset or smoothing repairs are made.
pub fn pressure_pa(branch: Branch, temperature_k: f64) -> Result<f64, WaterError> {
    let (min, max) = branch.temperature_bounds_k();
    if !temperature_k.is_finite() || !(min..=max).contains(&temperature_k) {
        return Err(WaterError {
            pressure_pa: None,
            temperature_k,
            reason: Unsupported::TemperatureDomain,
        });
    }
    Ok(match branch {
        Branch::Ih => {
            let theta = temperature_k / TRIPLE_TEMPERATURE_K;
            let sum = [
                (1.19539337e6, 3.0),
                (8.08183159e4, 25.75),
                (3.33826860e3, 103.75),
            ]
            .into_iter()
            .map(|(a, b)| a * (1.0 - theta.powf(b)))
            .sum::<f64>();
            TRIPLE_PRESSURE_PA * (1.0 + sum)
        }
        Branch::III => power_curve(temperature_k, 251.165, 208.566e6, 0.299948, 60.0),
        Branch::V => power_curve(temperature_k, 256.164, 350.1e6, 1.18721, 8.0),
        Branch::VI => power_curve(temperature_k, 273.31, 632.4e6, 1.07476, 4.6),
        Branch::VII => {
            let theta = temperature_k / 355.0;
            let log_pi = 1.73683 * (1.0 - theta.recip()) - 0.0544606 * (1.0 - theta.powi(5))
                + 8.06106e-8 * (1.0 - theta.powi(22));
            2216e6 * log_pi.exp()
        }
    })
}

fn power_curve(temperature: f64, anchor: f64, pressure: f64, a: f64, b: f64) -> f64 {
    pressure * (1.0 - a * (1.0 - (temperature / anchor).powf(b)))
}

#[cfg(test)]
mod tests;
