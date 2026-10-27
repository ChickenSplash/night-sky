use super::*;

fn relative(actual: f64, expected: f64, tolerance: f64) {
    assert!(
        (actual / expected - 1.0).abs() <= tolerance,
        "{actual} != {expected}"
    );
}

#[test]
fn liquid_matches_iapws_release_verification_values() {
    // R7-97(2012), table 5. Rounded published values, not values generated
    // by this implementation. Relative tolerance follows their printed digits.
    for (temperature, pressure, volume) in [
        (300.0, 3e6, 0.100215168e-2),
        (300.0, 80e6, 0.971180894e-3),
        (500.0, 3e6, 0.120241800e-2),
    ] {
        let water = state(pressure, temperature).unwrap();
        assert_eq!(water.phase, Phase::Liquid);
        relative(1.0 / water.density_kg_m3, volume, 5e-9);
    }
}

#[test]
fn ice_matches_iapws_release_verification_values() {
    // R10-06(2009), table 6. Two reference points lie outside our public
    // application mask; testing the private formula does not enable them.
    for (temperature, pressure, density) in [
        (273.16, 611.657, 916.709492200),
        (273.152519, 101325.0, 916.721463419),
        (100.0, 100e6, 941.678203297),
    ] {
        relative(ice::density_kg_m3(pressure, temperature), density, 5e-12);
    }
}

#[test]
fn phase_curves_match_published_values_and_domains() {
    // R14-08(2011), table 3, and normal triple point.
    relative(melting_pressure_ih_pa(260.0).unwrap(), 138.268e6, 4e-6);
    assert_eq!(melting_pressure_ih_pa(273.16).unwrap(), 611.657);
    relative(melting_pressure_ih_pa(251.165).unwrap(), 208.566e6, 4e-6);
    // R7-97(2012), table 35.
    for (temperature, pressure) in [
        (300.0, 0.353658941e-2 * 1e6),
        (500.0, 0.263889776e1 * 1e6),
        (600.0, 0.123443146e2 * 1e6),
    ] {
        relative(liquid::saturation_pressure_pa(temperature), pressure, 5e-9);
    }
    for t in [251.164, 273.161, f64::NAN, f64::INFINITY] {
        assert!(melting_pressure_ih_pa(t).is_err());
    }
}

#[test]
fn pressure_dependent_freezing_and_coexistence_are_not_density_blends() {
    assert_eq!(state(1e5, 250.0).unwrap().phase, Phase::IceIh);
    assert_eq!(state(1e5, 300.0).unwrap().phase, Phase::Liquid);
    assert_eq!(phase(1e5, 270.0).unwrap(), Phase::IceIh);
    assert_eq!(phase(80e6, 270.0).unwrap(), Phase::Liquid);
    assert_eq!(
        state(80e6, 270.0).unwrap_err().reason,
        Unsupported::SubfreezingLiquidDensity
    );
    let melting = melting_pressure_ih_pa(270.0).unwrap();
    assert_eq!(
        phase(melting, 270.0).unwrap(),
        Phase::IceIhLiquidCoexistence
    );
    assert_eq!(
        state(melting, 270.0).unwrap_err().reason,
        Unsupported::CoexistenceDensity
    );
    assert_eq!(phase(melting - 1.0, 270.0).unwrap(), Phase::IceIh);
    assert_eq!(phase(melting + 1.0, 270.0).unwrap(), Phase::Liquid);
    let saturation = liquid::saturation_pressure_pa(300.0);
    assert_eq!(
        phase(saturation, 300.0).unwrap(),
        Phase::LiquidVapourCoexistence
    );
    assert_eq!(
        state(saturation, 300.0).unwrap_err().reason,
        Unsupported::CoexistenceDensity
    );
    let liquid_side = condensed_state(saturation, 300.0, Phase::Liquid).unwrap();
    assert_eq!(
        liquid_side.density_kg_m3,
        liquid::density_kg_m3(saturation, 300.0)
    );
    assert_eq!(
        condensed_state(saturation, 300.0, Phase::Vapour)
            .unwrap_err()
            .reason,
        Unsupported::PhaseMismatch
    );
    assert_eq!(saturation_pressure_pa(300.0).unwrap(), saturation);
    for t in [273.149, 647.097, f64::NAN] {
        assert!(saturation_pressure_pa(t).is_err());
    }
    assert!(
        (saturation_pressure_pa(CRITICAL_TEMPERATURE_K).unwrap() / 22.064e6 - 1.0).abs() < 1e-4
    );
    assert_eq!(phase(saturation - 1.0, 300.0).unwrap(), Phase::Vapour);
    assert_eq!(
        state(saturation - 1.0, 300.0).unwrap_err().reason,
        Unsupported::VapourDensity
    );
    assert_eq!(state(saturation + 1.0, 300.0).unwrap().phase, Phase::Liquid);
}

#[test]
fn application_masks_never_extrapolate_or_fabricate_a_phase() {
    for p in [
        0.0,
        -1.0,
        999.999,
        100e6 + 1.0,
        1e12,
        f64::NAN,
        f64::INFINITY,
    ] {
        assert_eq!(
            state(p, 300.0).unwrap_err().reason,
            Unsupported::PressureDomain
        );
    }
    for t in [
        0.0,
        -1.0,
        249.999,
        623.151,
        647.096,
        1000.0,
        f64::NAN,
        f64::INFINITY,
    ] {
        assert_eq!(
            state(1e5, t).unwrap_err().reason,
            Unsupported::TemperatureDomain
        );
    }
    for t in [250.0, 260.0, 273.15, 273.16, 300.0, 400.0, 500.0, 623.15] {
        for p in [1000.0, 1e5, 1e6, 10e6, 50e6, 100e6] {
            if let Ok(water) = state(p, t) {
                assert!(water.density_kg_m3.is_finite() && water.density_kg_m3 > 0.0);
                assert!(matches!(water.phase, Phase::IceIh | Phase::Liquid));
                assert_eq!(water, state(p, t).unwrap());
            }
        }
    }
    // Check the narrow region where IF97's lower bound and the melting line
    // overlap, rather than replacing freezing by a universal 273.15 K rule.
    assert_eq!(state(1000.0, 273.15).unwrap().phase, Phase::IceIh);
    assert_eq!(state(1e6, 273.15).unwrap().phase, Phase::Liquid);
    assert_eq!(state(100e6, 623.15).unwrap().phase, Phase::Liquid);
}

#[test]
fn single_phase_compression_is_positive_and_continuous() {
    for t in [250.0, 260.0, 280.0, 300.0, 400.0, 500.0, 600.0] {
        for n in 1..100 {
            let p = f64::from(n) * 1e6;
            if let (Ok(a), Ok(b)) = (state(p, t), state(p + 1.0, t)) {
                assert_eq!(a.phase, b.phase);
                assert!(b.density_kg_m3 > a.density_kg_m3);
                assert!((b.density_kg_m3 / a.density_kg_m3 - 1.0) < 1e-7);
            }
        }
    }
}

#[test]
fn named_coexistence_endpoints_are_one_sided_not_mixture_densities() {
    for t in [273.15, 273.155] {
        let p = melting_pressure_ih_pa(t).unwrap();
        assert_eq!(
            state(p, t).unwrap_err().reason,
            Unsupported::CoexistenceDensity
        );
        let liquid = condensed_state(p, t, Phase::Liquid).unwrap();
        let ice = condensed_state(p, t, Phase::IceIh).unwrap();
        assert!(liquid.density_kg_m3 > ice.density_kg_m3);
        relative(
            liquid.density_kg_m3,
            state(p + 1.0, t).unwrap().density_kg_m3,
            1e-9,
        );
        relative(
            ice.density_kg_m3,
            state(p - 1.0, t).unwrap().density_kg_m3,
            1e-9,
        );
        assert_eq!(
            condensed_state(p + 1.0, t, Phase::IceIh)
                .unwrap_err()
                .reason,
            Unsupported::PhaseMismatch
        );
        assert_eq!(
            condensed_state(p - 1.0, t, Phase::Liquid)
                .unwrap_err()
                .reason,
            Unsupported::PhaseMismatch
        );
    }
    let p = melting_pressure_ih_pa(270.0).unwrap();
    assert!(condensed_state(p, 270.0, Phase::IceIh).is_ok());
    assert_eq!(
        condensed_state(p, 270.0, Phase::Liquid).unwrap_err().reason,
        Unsupported::SubfreezingLiquidDensity
    );
    assert_eq!(
        condensed_state(100e6 + 1.0, 300.0, Phase::Liquid)
            .unwrap_err()
            .reason,
        Unsupported::PressureDomain
    );
    // On the saturation curve only the liquid side is named; vapour has no EOS.
    let p = liquid::saturation_pressure_pa(300.0);
    relative(
        condensed_state(p, 300.0, Phase::Liquid)
            .unwrap()
            .density_kg_m3,
        state(p + 1.0, 300.0).unwrap().density_kg_m3,
        1e-9,
    );
    assert_eq!(
        condensed_state(p - 1.0, 300.0, Phase::Liquid)
            .unwrap_err()
            .reason,
        Unsupported::PhaseMismatch
    );
}
