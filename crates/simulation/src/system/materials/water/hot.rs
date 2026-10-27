//! Bounded engineering proxy, not an extension of the IAPWS local APIs.
use super::{Unsupported, WaterError, error};

pub const MODEL_ID: &str = "ideal_to_compressed_hot_water_proxy_v1";
pub const MIN_TEMPERATURE_K: f64 = 623.15;
pub const MAX_TEMPERATURE_K: f64 = 2000.0;
pub const DEEP_BOUNDARY_PA: f64 = 1e9;
pub const APPROXIMATION_NOTES: &[&str] = &[
    "Hot water uses additive ideal-gas and compressed-fluid specific volumes",
    "Compressed reference density 1000 kg/m3, bulk modulus 2 GPa, derivative 4 are engineering assumptions",
    "No latent heat, critical anomaly, dissociation or resolved superionic phases",
    "Effective deep ice begins at 1 GPa regardless of temperature; not a melting curve",
];

/// v = RT/P + 1/rho_compressed. Correct dilute ideal limit and positive
/// compression, but no claim of laboratory accuracy or a matched critical EOS.
/// Below critical T the caller must supply at least saturation pressure.
pub fn density_kg_m3(p: f64, t: f64) -> Result<f64, WaterError> {
    if !t.is_finite() || !(MIN_TEMPERATURE_K..=MAX_TEMPERATURE_K).contains(&t) {
        return Err(error(p, t, Unsupported::TemperatureDomain));
    }
    if !p.is_finite() || !(0.0..=DEEP_BOUNDARY_PA).contains(&p) || p == 0.0 {
        return Err(error(p, t, Unsupported::PressureDomain));
    }
    if t < super::CRITICAL_TEMPERATURE_K
        && let Ok(saturation) = super::saturation_pressure_pa(t)
        && p < saturation
    {
        return Err(error(p, t, Unsupported::VapourDensity));
    }
    let compressed = 1000.0 * (1.0 + 4.0 * p / 2e9).powf(0.25);
    Ok(1.0 / (461.526 * t / p + 1.0 / compressed))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compresses_with_a_dilute_limit_and_explicit_bounds() {
        for t in [647.096, 1000.0, 2000.0] {
            let mut previous = 0.0;
            for p in [1.0, 1e5, 1e7, 1e8, 1e9] {
                let rho = density_kg_m3(p, t).unwrap();
                assert!(rho.is_finite() && rho > previous);
                // Independent algebraic pressure closure of the specific-volume law.
                let compressed = 1000.0 * (1.0 + p / 5e8).sqrt().sqrt();
                let recovered = 461.526 * t / (rho.recip() - compressed.recip());
                assert!((recovered / p - 1.0).abs() < 1e-12);
                previous = rho;
            }
            assert!((density_kg_m3(1.0, t).unwrap() * 461.526 * t - 1.0).abs() < 1e-8);
        }
        for (p, t) in [
            (0.0, 1000.0),
            (1e9 + 1.0, 1000.0),
            (1e8, 623.0),
            (1e8, 2000.01),
            (f64::NAN, 1000.0),
            (1e8, f64::INFINITY),
            (1.0, 640.0),
        ] {
            assert!(density_kg_m3(p, t).is_err());
        }
        assert!(super::super::state(1e8, 640.0).is_err());
    }
}
