//! Effective compression, not resolved VI/VII/VIII thermodynamics.
//! Seager et al. (2007), equation 11/table 3, SI water row. The published
//! fit spans P<1e16 Pa; this application restricts it to <=1e13 Pa and
//! 20..=300 K, using an IAPWS V/VI melting boundary at >=273.15 K or a
//! rounded 2.1 GPa effective-ice boundary below it. Thermal expansion and individual
//! polymorphs are deliberately unresolved.

use super::{Unsupported, WaterError, cold_ice, error, subfreezing_liquid, surface};

pub const MODEL_ID: &str = "seager_2007_effective_high_pressure_ice_v1";
pub const MAX_PRESSURE_PA: f64 = 1e13;

pub fn boundary_pressure_pa(temperature_k: f64) -> Result<f64, WaterError> {
    if temperature_k.is_finite() && (surface::MIN_TEMPERATURE_K..273.15).contains(&temperature_k) {
        return Ok(cold_ice::DEEP_BOUNDARY_PA);
    }
    if !temperature_k.is_finite() || !(273.15..=300.0).contains(&temperature_k) {
        return Err(WaterError {
            pressure_pa: None,
            temperature_k,
            reason: Unsupported::TemperatureDomain,
        });
    }
    subfreezing_liquid::high_boundary_pa(temperature_k)
}

pub fn density_kg_m3(pressure_pa: f64, temperature_k: f64) -> Result<f64, WaterError> {
    let boundary = boundary_pressure_pa(temperature_k)?;
    if !pressure_pa.is_finite() || !(boundary..=MAX_PRESSURE_PA).contains(&pressure_pa) {
        return Err(error(
            pressure_pa,
            temperature_k,
            Unsupported::PressureDomain,
        ));
    }
    Ok(1460.0 + 0.00311 * pressure_pa.powf(0.513))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn published_si_parameters_compress_and_refuse_extrapolation() {
        // Independent 55-digit Decimal evaluations of the printed SI row.
        for (p, expected) in [
            (1e9, 1588.7538988689362),
            (1e10, 1879.5274564860043),
            (1e12, 5914.10436674072),
        ] {
            assert!((density_kg_m3(p, 300.0).unwrap() / expected - 1.0).abs() < 1e-14);
        }
        // At 1 Pa the algebraic fit is 1460.00311; public states instead
        // begin at the independently prescribed melting pressure.
        for t in [
            20.0, 100.0, 250.0, 251.165, 260.0, 273.15, 273.31, 273.32, 280.0, 300.0,
        ] {
            let boundary = boundary_pressure_pa(t).unwrap();
            let mut previous = 0.0;
            for p in [boundary, 3e9, 1e10, 1e11, 1e12, 1e13] {
                let rho = density_kg_m3(p, t).unwrap();
                assert!(rho > previous && rho.is_finite());
                let recovered = ((rho - 1460.0) / 0.00311).powf(1.0 / 0.513);
                assert!((recovered / p - 1.0).abs() < 1e-13);
                previous = rho;
            }
            assert!(density_kg_m3(boundary - 1.0, t).is_err());
        }
        for p in [f64::NAN, f64::INFINITY, -1.0, 1e13 + 1.0] {
            assert!(density_kg_m3(p, 300.0).is_err());
        }
        for t in [19.0, 300.01, f64::NAN] {
            assert!(density_kg_m3(1e10, t).is_err());
        }
    }
}
