//! Labelled cold-water engineering approximation, not resolved polymorphs.
//! Rounded engineering boundaries inspired by the water phase diagram:
//! Ih/dense ice near 220 MPa at 250 K, deeper ice near 2.1 GPa. They are
//! constant across this cold mask, not measured equilibrium curves. Below
//! 250 K they are held constant down to 20 K, where the real Ih/II boundary
//! moves only slightly.
//! Intermediate compression uses a Murnaghan law anchored to the measured
//! H2O VI density at 1.5 GPa/293 K, with explicitly assumed elastic constants.

use super::{Unsupported, WaterError, error, ice, melting, subfreezing_liquid, surface};

pub const INTERMEDIATE_MODEL_ID: &str = "reference_anchored_cold_intermediate_ice_v1";
pub const IH_MODEL_ID: &str = "iapws_ih_extended_cold_pressure_proxy_v1";
pub const IH_BOUNDARY_PA: f64 = 220e6;
pub const DEEP_BOUNDARY_PA: f64 = 2.1e9;
pub const REFERENCE_DENSITY_KG_M3: f64 = 1386.12517;
const REFERENCE_PRESSURE_PA: f64 = 1.5e9;
// Engineering assumptions, not fitted or measured constants from that sample.
const REFERENCE_BULK_MODULUS_PA: f64 = 14e9;
const BULK_MODULUS_DERIVATIVE: f64 = 4.0;

pub const APPROXIMATION_NOTES: &[&str] = &[
    "Rounded cold ice boundaries; intermediate polymorphs unresolved",
    "Intermediate compression uses assumed elastic constants",
    "293 K lattice-density reference used without a thermal correction",
    "Liquid slivers up to 10 kPa near the rounded Ih/III triple point are unresolved",
    "Below 250 K the 250 K boundaries and compression are held fixed down to 20 K",
];

pub fn supports_temperature(temperature_k: f64) -> bool {
    // Below the minimum melting temperature there is no intervening liquid.
    if !temperature_k.is_finite()
        || !(surface::MIN_TEMPERATURE_K..=256.164).contains(&temperature_k)
    {
        return false;
    }
    if temperature_k <= 251.165 {
        return true;
    }
    // The published Ih/III endpoints differ by about 566 Pa. Treat the tiny
    // uncertain liquid wedge as unresolved solid instead of creating inverted
    // events or an arbitrarily small shell at a rounded triple point. This is
    // a declared physical-model resolution, not a numerical pressure repair.
    let low = melting::pressure_pa(melting::Branch::Ih, temperature_k).unwrap();
    let high = subfreezing_liquid::high_boundary_pa(temperature_k).unwrap();
    high - low <= 10_000.0
}

pub fn density_kg_m3(
    pressure_pa: f64,
    temperature_k: f64,
    model_id: &str,
) -> Result<f64, WaterError> {
    if !temperature_k.is_finite() || !(surface::MIN_TEMPERATURE_K..273.15).contains(&temperature_k)
    {
        return Err(error(
            pressure_pa,
            temperature_k,
            Unsupported::TemperatureDomain,
        ));
    }
    let cold = supports_temperature(temperature_k);
    let domain = match model_id {
        IH_MODEL_ID => {
            100e6..=if cold {
                IH_BOUNDARY_PA
            } else {
                melting::pressure_pa(melting::Branch::Ih, temperature_k)?
            }
        }
        INTERMEDIATE_MODEL_ID => {
            (if cold {
                IH_BOUNDARY_PA
            } else {
                subfreezing_liquid::high_boundary_pa(temperature_k)?
            })..=DEEP_BOUNDARY_PA
        }
        _ => {
            return Err(error(
                pressure_pa,
                temperature_k,
                Unsupported::PhaseMismatch,
            ));
        }
    };
    if !pressure_pa.is_finite() || !domain.contains(&pressure_pa) {
        return Err(error(
            pressure_pa,
            temperature_k,
            Unsupported::PressureDomain,
        ));
    }
    if model_id == IH_MODEL_ID {
        // Explicit small extension beyond the original 100 MPa API mask.
        // The rounded 220 MPa transition is not a precision stability claim.
        return Ok(ice::density_kg_m3(pressure_pa, temperature_k));
    }
    Ok(REFERENCE_DENSITY_KG_M3
        * (1.0
            + BULK_MODULUS_DERIVATIVE * (pressure_pa - REFERENCE_PRESSURE_PA)
                / REFERENCE_BULK_MODULUS_PA)
            .powf(1.0 / BULK_MODULUS_DERIVATIVE))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn anchored_compression_is_positive_bounded_and_explicitly_approximate() {
        assert_eq!(
            density_kg_m3(REFERENCE_PRESSURE_PA, 250.0, INTERMEDIATE_MODEL_ID).unwrap(),
            REFERENCE_DENSITY_KG_M3
        );
        for t in [20.0, 100.0, 250.0, 250.5, 251.165] {
            let mut previous = 0.0;
            for p in [
                IH_BOUNDARY_PA,
                500e6,
                1e9,
                REFERENCE_PRESSURE_PA,
                DEEP_BOUNDARY_PA,
            ] {
                let rho = density_kg_m3(p, t, INTERMEDIATE_MODEL_ID).unwrap();
                assert!(rho.is_finite() && rho > previous);
                // Independent forward Murnaghan pressure closure.
                let recovered = REFERENCE_PRESSURE_PA
                    + REFERENCE_BULK_MODULUS_PA / BULK_MODULUS_DERIVATIVE
                        * ((rho / REFERENCE_DENSITY_KG_M3).powi(4) - 1.0);
                assert!((recovered / p - 1.0).abs() < 1e-13);
                previous = rho;
            }
            let below = if t < super::super::MIN_TEMPERATURE_K {
                surface::density_kg_m3(100e6, t).unwrap()
            } else {
                super::super::state(100e6, t).unwrap().density_kg_m3
            };
            assert_eq!(density_kg_m3(100e6, t, IH_MODEL_ID).unwrap(), below);
            assert!(
                density_kg_m3(IH_BOUNDARY_PA, t, INTERMEDIATE_MODEL_ID).unwrap()
                    > density_kg_m3(IH_BOUNDARY_PA, t, IH_MODEL_ID).unwrap()
            );
        }
        for p in [f64::NAN, -1.0, IH_BOUNDARY_PA - 1.0, DEEP_BOUNDARY_PA + 1.0] {
            assert!(density_kg_m3(p, 250.0, INTERMEDIATE_MODEL_ID).is_err());
        }
        for t in [19.0, 273.15, 300.0, f64::NAN] {
            assert!(density_kg_m3(1e9, t, INTERMEDIATE_MODEL_ID).is_err());
        }
        for p in [0.0, 100e6 - 1.0, IH_BOUNDARY_PA + 1.0, f64::NAN] {
            assert!(density_kg_m3(p, 250.0, IH_MODEL_ID).is_err());
        }
    }
}
