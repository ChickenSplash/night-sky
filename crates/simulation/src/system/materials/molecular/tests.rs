use super::*;

#[test]
fn peng_robinson_matches_independent_nist_density_points() {
    // NIST Chemistry WebBook SRD 69 fluid service, retrieved 2026-10-28.
    // Sparse vapour/supercritical references, not a validation of the full mask.
    // The cubic approximation is allowed 4% here; numerical roots use 1e-12.
    for (m, t, one_bar, hundred_bar) in [
        (Material::Nitrogen, 300.0, 1.1232786, 111.72541),
        (Material::CarbonDioxide, 400.0, 1.3256858, 161.52729),
        (Material::Methane, 300.0, 0.64425426, 75.175486),
        (Material::Oxygen, 300.0, 1.2836705, 134.57394),
        (Material::Argon, 300.0, 1.6025107, 167.60263),
    ] {
        for (p, reference) in [(1e5, one_bar), (1e7, hundred_bar)] {
            let density = density_kg_m3(m, p, t).unwrap();
            assert!(
                (density / reference - 1.0).abs() < 0.04,
                "{m:?} {p} {t}: {density} vs {reference}"
            );
        }
    }
}

#[test]
fn cubic_roots_and_low_pressure_ideal_limit_are_correct() {
    let mut roots = cubic_roots(-6.0, 11.0, -6.0);
    roots.sort_by(f64::total_cmp);
    for (actual, expected) in roots.into_iter().zip([1.0, 2.0, 3.0]) {
        assert!((actual - expected).abs() < 1e-12);
    }
    for m in [
        Material::CarbonDioxide,
        Material::Nitrogen,
        Material::Methane,
        Material::Oxygen,
        Material::Argon,
    ] {
        let gas = parameters(m).unwrap().0;
        let ideal = gas.molecular_mass_u() * ATOMIC_MASS_KG / (BOLTZMANN_J_K * 400.0);
        assert!((density_kg_m3(m, 1.0, 400.0).unwrap() / ideal - 1.0).abs() < 1e-6);
    }
}

#[test]
fn unsupported_states_and_condensation_are_not_returned_as_fluids() {
    for (m, p, t, reason) in [
        (Material::Water, 1e5, 300.0, Reason::UnsupportedMaterial),
        (
            Material::CarbonDioxide,
            1e5,
            100.0,
            Reason::PureCondensationScreen,
        ),
        (
            Material::Nitrogen,
            1e7,
            90.0,
            Reason::PureCondensationScreen,
        ),
        (
            Material::Oxygen,
            1000.0,
            100.0,
            Reason::PureCondensationScreen,
        ),
        (
            Material::CarbonDioxide,
            7.4e6,
            305.0,
            Reason::CriticalRegion,
        ),
        (
            Material::CarbonDioxide,
            4e6,
            260.0,
            Reason::PureCondensationScreen,
        ),
        (
            Material::CarbonDioxide,
            2.6e6,
            260.0,
            Reason::PureCondensationScreen,
        ),
        (Material::Nitrogen, 1e9, 300.0, Reason::StateDomain),
        (Material::Methane, 1e5, 1000.0, Reason::StateDomain),
        (Material::Argon, 0.0, 300.0, Reason::StateDomain),
        (Material::Argon, 1e5, f64::NAN, Reason::StateDomain),
    ] {
        assert_eq!(density_kg_m3(m, p, t).unwrap_err().reason, reason);
    }
}

#[test]
fn accepted_molecular_density_is_continuous_and_compressible() {
    for m in [
        Material::CarbonDioxide,
        Material::Nitrogen,
        Material::Methane,
        Material::Oxygen,
        Material::Argon,
    ] {
        for t in [350.0, 400.0, 600.0] {
            let mut previous = 0.0;
            for i in 0..81 {
                let p = 1000.0 * 10.0_f64.powf(i as f64 / 16.0);
                let rho = density_kg_m3(m, p, t).unwrap();
                assert!(rho > previous);
                previous = rho;
                let nearby = density_kg_m3(m, p * (1.0 - 1e-7), t).unwrap();
                assert!((rho / nearby - 1.0).abs() < 1e-5);
            }
        }
    }
}
