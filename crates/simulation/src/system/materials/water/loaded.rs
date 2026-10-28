//! Barotropic shallow water under an envelope, not a thermal/phase EOS.
//! Engineering Murnaghan compression with ambient liquid-like density/stiffness.
//! Kept separate from the checked local IAPWS APIs and the deep Seager proxy.
pub const MODEL_ID: &str = "shallow_loaded_water_murnaghan_proxy_v1";
pub const BOUNDARY_PA: f64 = 1e9;
pub const APPROXIMATION_NOTES: &[&str] = &[
    "Approximate condensed-water compression: reference density 1000 kg/m3, bulk modulus 2.2 GPa, pressure derivative 7",
    "Engineering constants, not a fitted thermal EOS; no uniform physical error bound",
    "Temperature, thermal expansion, vapour, ice polymorphs and H/He-water mixing unresolved",
    "A separate water parcel is imposed even where real water could mix or become steam; not an ocean or solid surface claim",
    "At 10000 bar the deep compression proxy takes over with an unblended model density jump, not a phase transition",
];

pub fn density_kg_m3(pressure_pa: f64) -> Option<f64> {
    if !pressure_pa.is_finite() || !(0.0..=BOUNDARY_PA).contains(&pressure_pa) {
        return None;
    }
    Some(1000.0 * (1.0 + 7.0 * pressure_pa / 2.2e9).powf(1.0 / 7.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compression_inverse_and_domain() {
        let mut previous = 0.0;
        for i in 0..=100 {
            let p = i as f64 * BOUNDARY_PA / 100.0;
            let rho = density_kg_m3(p).unwrap();
            assert!(rho > previous);
            previous = rho;
            let inverse = 2.2e9 / 7.0 * ((rho / 1000.0).powi(7) - 1.0);
            assert!((inverse - p).abs() < 1e-5);
        }
        assert_eq!(density_kg_m3(0.0), Some(1000.0));
        for p in [-1.0, f64::NAN, f64::INFINITY, BOUNDARY_PA + 1.0] {
            assert!(density_kg_m3(p).is_none());
        }
    }

    #[test]
    fn room_temperature_liquid_overlap_is_plausible_not_a_domain_guarantee() {
        for p in [1e5, 1e7, 1e8] {
            let reference = super::super::condensed_state(p, 300.0, super::super::Phase::Liquid)
                .unwrap()
                .density_kg_m3;
            assert!((density_kg_m3(p).unwrap() / reference - 1.0).abs() < 0.01);
        }
        // The original local model still refuses pressures outside its mask.
        assert!(super::super::state(BOUNDARY_PA, 300.0).is_err());
    }
}
