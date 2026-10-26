//! Approximate pressure-melted liquid where the original density APIs stop.
//! Phase boundaries use IAPWS; density is a separately labelled engineering
//! Murnaghan compression proxy with a freezing-point IF97 reference.

use super::{Unsupported, WaterError, error, melting};

pub const MODEL_ID: &str = "approximate_subfreezing_compressed_liquid_v1";
// Direct IF97 evaluation at 100 MPa/273.15 K, regression-checked below.
// A constant avoids hiding a nested reference-EOS call in every dispatch.
const REFERENCE_DENSITY_KG_M3: f64 = 1045.2740171091934;
pub const APPROXIMATION_NOTES: &[&str] = &[
    "Subfreezing liquid density approximated from a 273.15 K reference",
    "Liquid compression uses assumed bulk modulus and pressure derivative",
];

pub fn high_boundary_pa(temperature_k: f64) -> Result<f64, WaterError> {
    let branch = if temperature_k <= 256.164 {
        melting::Branch::III
    } else if temperature_k <= 273.31 {
        melting::Branch::V
    } else {
        melting::Branch::VI
    };
    melting::pressure_pa(branch, temperature_k)
}

pub fn density_kg_m3(pressure_pa: f64, temperature_k: f64) -> Result<f64, WaterError> {
    if !temperature_k.is_finite() || !(251.165..273.16).contains(&temperature_k) {
        return Err(error(
            pressure_pa,
            temperature_k,
            Unsupported::TemperatureDomain,
        ));
    }
    let low = melting::pressure_pa(melting::Branch::Ih, temperature_k)?;
    let high = high_boundary_pa(temperature_k)?;
    if !pressure_pa.is_finite() || !(low..=high).contains(&pressure_pa) {
        return Err(error(
            pressure_pa,
            temperature_k,
            Unsupported::PressureDomain,
        ));
    }
    // Reference is a real supported IF97 state. Elastic constants and neglect
    // of thermal density change are explicit assumptions, not an IAPWS fit.
    Ok(REFERENCE_DENSITY_KG_M3 * (1.0 + 5.0 * (pressure_pa - 100e6) / 2.8e9).powf(0.2))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn approximate_liquid_is_bounded_compressive_and_uses_known_phase_curves() {
        assert_eq!(
            REFERENCE_DENSITY_KG_M3,
            super::super::liquid::density_kg_m3(100e6, 273.15)
        );
        for t in [251.2, 255.0, 256.164, 260.0, 270.0, 273.15, 273.159] {
            let low = melting::pressure_pa(melting::Branch::Ih, t).unwrap();
            let high = high_boundary_pa(t).unwrap();
            let a = density_kg_m3(low, t).unwrap();
            let b = density_kg_m3(high, t).unwrap();
            assert!(a.is_finite() && b > a && a > 900.0 && b < 1400.0);
            assert!(density_kg_m3(low - 1.0, t).is_err());
            assert!(density_kg_m3(high + 1.0, t).is_err());
            assert!(density_kg_m3(f64::NAN, t).is_err());
        }
        for t in [250.0, 273.16, 300.0, f64::NAN] {
            assert!(density_kg_m3(100e6, t).is_err());
        }
    }
}
