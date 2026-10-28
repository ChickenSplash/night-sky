use super::*;

fn gas(co2: f64, n2: f64) -> Inventory {
    Inventory::new(&[
        (Material::Hydrogen, 0.75),
        (Material::Helium, 0.25),
        (Material::CarbonDioxide, co2),
        (Material::Nitrogen, n2),
    ])
    .unwrap()
}

#[test]
fn suspended_partial_pressure_distinguishes_dilution_from_condensation() {
    let p = 1e5;
    let t = 180.0;
    let dilute = gas(0.001, 0.0);
    assert!(molecular::density_kg_m3(Material::CarbonDioxide, p, t).is_err());
    assert_eq!(
        State::new(&dilute, p, t, 0.2)
            .unwrap()
            .condensate
            .total_mass_kg(),
        0.0
    );
    let rich = gas(100.0, 0.0);
    let s = State::new(&rich, p, t, 0.2).unwrap();
    assert!(s.condensate.mass_kg(Material::CarbonDioxide) > 0.0);
    let n = Material::ALL
        .into_iter()
        .filter_map(|m| {
            m.properties()
                .gas
                .map(|g| s.vapour.mass_kg(m) / g.molecular_mass_u())
        })
        .sum::<f64>();
    let partial = p * s.vapour.mass_kg(Material::CarbonDioxide) / 44.01 / n;
    assert!((partial / saturation_pa(Material::CarbonDioxide, t).unwrap() - 1.0).abs() < 1e-4);
    let normal = rich.scaled(1.0 / rich.total_mass_kg()).unwrap();
    for m in Material::ALL {
        assert!(
            (s.vapour.mass_kg(m) + s.condensate.mass_kg(m) - normal.mass_kg(m)).abs()
                <= 1e-12 * normal.mass_kg(m)
        );
    }
    let warm = State::new(&rich, p, 350.0, 0.2).unwrap();
    assert_eq!(warm.condensate.total_mass_kg(), 0.0);
    assert!(s.density_kg_m3 > warm.density_kg_m3);
}

#[test]
fn suspended_multiple_saturated_species_close_simultaneously() {
    let request = Inventory::new(&[
        (Material::Hydrogen, 1e-5),
        (Material::Helium, 1e-5),
        (Material::CarbonDioxide, 1.0),
        (Material::Nitrogen, 1.0),
        (Material::Methane, 1.0),
        (Material::Oxygen, 1.0),
        (Material::Argon, 1.0),
    ])
    .unwrap();
    for t in [80.0, 85.0, 100.0, 180.0, 250.0, 350.0, 625.0] {
        for p in [1e3, 1e4, 1e5, 1e6] {
            let s = State::new(&request, p, t, 1.0).unwrap();
            let n = Material::ALL
                .into_iter()
                .filter_map(|m| {
                    m.properties()
                        .gas
                        .map(|g| s.vapour.mass_kg(m) / g.molecular_mass_u())
                })
                .sum::<f64>();
            for m in SECONDARIES {
                let partial =
                    p * s.vapour.mass_kg(m) / m.properties().gas.unwrap().molecular_mass_u() / n;
                let cap = saturation_pa(m, t).unwrap();
                assert!(partial <= cap * (1.0 + 1e-12));
                if s.condensate.mass_kg(m) > 0.0 {
                    assert!((partial / cap - 1.0).abs() < 1e-12);
                }
            }
        }
    }
}

#[test]
fn suspended_saturation_onset_is_continuous_and_invalid_states_stay_errors() {
    let gas = gas(10.0, 1.0);
    let mut previous: Option<(f64, f64)> = None;
    for i in 0..=2000 {
        let t = 150.0 + i as f64 * 0.1;
        let s = State::new(&gas, 1e5, t, 1.0).unwrap();
        if let Some((rho, c)) = previous {
            assert!((s.density_kg_m3 / rho - 1.0_f64).abs() < 0.03);
            assert!(s.condensate.total_mass_kg() <= c + 1e-14);
        }
        previous = Some((s.density_kg_m3, s.condensate.total_mass_kg()));
    }
    for (p, t, rho) in [
        (0.0, 100.0, 1.0),
        (1e9, 100.0, 1.0),
        (1e5, 79.0, 1.0),
        (1e5, 626.0, 1.0),
        (1e5, f64::NAN, 1.0),
        (1e5, 100.0, -1.0),
    ] {
        assert!(State::new(&gas, p, t, rho).is_err());
    }
    assert!(matches!(
        State::new(&gas, 1e7, 305.0, 1.0),
        Err(Error::CriticalRegion)
    ));
    let dry = State::new(&gas, 1.0, 180.0, 1.0).unwrap();
    let n = Material::ALL
        .into_iter()
        .filter_map(|m| {
            m.properties()
                .gas
                .map(|g| dry.vapour.mass_kg(m) / g.molecular_mass_u())
        })
        .sum::<f64>();
    let y = dry.vapour.mass_kg(Material::CarbonDioxide)
        / Material::CarbonDioxide
            .properties()
            .gas
            .unwrap()
            .molecular_mass_u()
        / n;
    let dew = saturation_pa(Material::CarbonDioxide, 180.0).unwrap() / y;
    let before = State::new(&gas, dew * (1.0 - 1e-8), 180.0, 1.0).unwrap();
    let after = State::new(&gas, dew * (1.0 + 1e-8), 180.0, 1.0).unwrap();
    assert_eq!(before.condensate.total_mass_kg(), 0.0);
    assert!(after.condensate.total_mass_kg() > 0.0);
    assert!((after.density_kg_m3 / before.density_kg_m3 - 1.0).abs() < 1e-7);
    let wet = gas
        .checked_add(&Inventory::new(&[(Material::Water, 1e-10)]).unwrap())
        .unwrap();
    assert!(State::new(&wet, 1e5, 180.0, 1.0).is_err());
}

#[test]
fn suspended_saturation_has_independent_nist_phase_point_checks() {
    // NIST WebBook SRD69, CO2 frost 194.65 K near 1 atm and triple
    // 216.58 K / 5.185 bar; N2 boiling 77.34 K below our 80 K domain,
    // so use its independent Antoine fit at 90/110 K instead.
    assert!(
        (saturation_pa(Material::CarbonDioxide, 194.65).unwrap() / 101325.0 - 1.0).abs() < 0.05
    );
    assert!(
        (saturation_pa(Material::CarbonDioxide, 216.58).unwrap() / 518500.0 - 1.0).abs() < 1e-12
    );
    for t in [90.0, 110.0] {
        let nist = 1e5 * 10.0_f64.powf(3.7362 - 264.651 / (t - 6.788));
        assert!((saturation_pa(Material::Nitrogen, t).unwrap() / nist - 1.0).abs() < 0.1);
    }
    for (m, t, a, b, c) in [
        (Material::Methane, 110.0, 3.9895, 443.028, -0.49),
        (Material::Oxygen, 90.0, 3.9523, 340.024, -4.144),
        (Material::Argon, 100.0, 3.29555, 215.24, -22.233),
    ] {
        let nist = 1e5 * 10.0_f64.powf(a - b / (t + c));
        assert!(
            (saturation_pa(m, t).unwrap() / nist - 1.0).abs() < 0.1,
            "{m:?}"
        );
    }
    for (m, t) in [
        (Material::CarbonDioxide, 216.58),
        (Material::Methane, 90.694),
        (Material::Argon, 83.806),
        (Material::Oxygen, 90.188),
    ] {
        let a = saturation_pa(m, t * (1.0 - 1e-9)).unwrap();
        let b = saturation_pa(m, t * (1.0 + 1e-9)).unwrap();
        assert!((b / a - 1.0).abs() < 1e-6);
    }
    // Independent dilute gas density NIST at 300 K, 1 bar, N2. A tiny
    // required H/He carrier does not alter the molecular endpoint measurably.
    let gas = gas(0.0, 1e10);
    let s = State::new(&gas, 1e5, 300.0, 0.1).unwrap();
    assert!((s.density_kg_m3 / 1.1232786 - 1.0).abs() < 0.01);
    // Independent NIST fluid service: liquid N2 at 80 K, 2 bar.
    assert!(
        (condensate_density_kg_m3(Material::Nitrogen, 2e5).unwrap() / 794.11547 - 1.0).abs() < 0.03
    );
    for m in SECONDARIES {
        let mut old = 0.0;
        for p in [1.0, 1e3, 1e5, 1e7, 1e8] {
            let rho = condensate_density_kg_m3(m, p).unwrap();
            assert!(rho > old);
            old = rho;
        }
    }
}
