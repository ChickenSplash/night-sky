use super::*;

#[test]
fn effective_secondary_has_dilute_limit_and_positive_compression() {
    for m in super::super::hhe_mixture::SECONDARIES {
        let gas = m.properties().gas.unwrap();
        let r = BOLTZMANN_J_K / (ATOMIC_MASS_KG * gas.molecular_mass_u());
        assert!((density_kg_m3(m, 1.0, 1000.0).unwrap() / (1.0 / (r * 1000.0)) - 1.0).abs() < 1e-8);
        let mut previous = 0.0;
        for i in 0..=120 {
            let p = 1e3 * 10.0_f64.powf(i as f64 / 12.0);
            let t = 1000.0 * (p / 1e7).powf(2.0 / 7.0).max(1.0);
            let rho = density_kg_m3(m, p, t).unwrap();
            assert!(rho > previous);
            previous = rho;
            let compression = 1460.0 + 0.00311 * p.powf(0.513);
            assert!(rho < compression);
            let reconstructed = 1.0 / (1.0 / rho - 1.0 / compression);
            assert!((reconstructed / (p / (r * t)) - 1.0).abs() < 1e-10);
        }
    }
}

#[test]
fn effective_secondary_never_converts_bad_states_or_condensation_into_success() {
    for (p, t) in [
        (0.0, 1000.0),
        (1e14, 1000.0),
        (1e8, 1e6),
        (1e8, 79.0),
        (f64::NAN, 1000.0),
    ] {
        assert_eq!(
            density_kg_m3(Material::Nitrogen, p, t),
            Err(Error::StateDomain)
        );
    }
    for m in [
        Material::Water,
        Material::Hydrogen,
        Material::Helium,
        Material::Metal,
    ] {
        assert!(density_kg_m3(m, 1e9, 1000.0).is_err());
    }
    assert!(matches!(
        density_kg_m3(Material::Nitrogen, 1e9, 90.0),
        Err(Error::Component(molecular::Error {
            reason: molecular::Reason::PureCondensationScreen,
            ..
        }))
    ));
    assert!(matches!(
        density_kg_m3(Material::CarbonDioxide, 1e8, 305.0),
        Err(Error::Component(molecular::Error {
            reason: molecular::Reason::CriticalRegion,
            ..
        }))
    ));
    // The original PR API has not been widened or given a fallback.
    assert!(molecular::density_kg_m3(Material::Nitrogen, 1e10, 2000.0).is_err());
    assert!(density_kg_m3(Material::Nitrogen, 1e10, 2000.0).is_ok());
}
