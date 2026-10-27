use super::*;
use crate::system::atmosphere::{Gas, condensation::vapour_pressure};

fn request(water: f64, temperature: f64) -> Request {
    let retained = Inventory::from_mass_fractions(
        EARTH_MASS_KG,
        &[
            (Material::Metal, 0.325),
            (Material::Silicate, 0.674999 - water),
            (Material::Water, water),
            (Material::Nitrogen, 1e-6),
        ],
    )
    .unwrap();
    Request {
        retained,
        exchangeable: Inventory::new(&[
            (Material::Nitrogen, retained.mass_kg(Material::Nitrogen)),
            (Material::Water, retained.mass_kg(Material::Water)),
        ])
        .unwrap(),
        column_temperature_k: temperature,
        water_temperature_k: Some(temperature),
    }
}

fn close(a: f64, b: f64, tolerance: f64) {
    assert!((a / b - 1.0).abs() < tolerance, "{a} != {b}");
}

fn check(s: &CoupledStructure, r: &Request) {
    for m in Material::ALL {
        let expected = r.retained.mass_kg(m);
        let actual = s.retained_inventory().mass_kg(m);
        assert!((expected - actual).abs() <= expected * 1e-10);
    }
    assert!(s.diagnostics().interface_residual_pa.abs() <= 0.020001);
    assert!(s.diagnostics().partition_relative_residual <= ALLOCATION_RELATIVE_TOLERANCE);
    let Some(air) = s.atmosphere() else {
        return;
    };
    let c = s.column().unwrap().unwrap();
    assert!(c.profile.spherical);
    assert!(c.density_kg_m3 <= MAX_GAS_DENSITY_KG_M3);
    // Independent radial Simpson integration, not profile::mass_factor.
    let n = 8192;
    let dr = c.extent_m / n as f64;
    let radius = s.condensed().radius_m();
    let density = |h: f64| {
        air.pressure_pa() * air.composition().mean_molecular_mass_kg()
            / (crate::units::BOLTZMANN_J_K * air.temperature_k())
            * (-s.column_gm_m3_s2() * air.composition().mean_molecular_mass_kg()
                / (crate::units::BOLTZMANN_J_K * air.temperature_k())
                * h
                / (radius * (radius + h)))
                .exp()
    };
    let at = |h: f64| 4.0 * PI * (radius + h).powi(2) * density(h);
    let mut sum = at(0.0) + at(c.extent_m);
    for i in 1..n {
        sum += if i % 2 == 0 { 2.0 } else { 4.0 } * at(i as f64 * dr);
    }
    close(sum * dr / 3.0, s.gas_inventory().total_mass_kg(), 1e-7);
    let integrate = |f: &dyn Fn(f64) -> f64| {
        let mut sum = f(0.0) + f(c.extent_m);
        for i in 1..n {
            sum += if i % 2 == 0 { 2.0 } else { 4.0 } * f(i as f64 * dr);
        }
        sum * dr / 3.0
    };
    let inertia = integrate(&|h| (2.0 / 3.0) * at(h) * (radius + h).powi(2));
    close(inertia, column::inertia_kg_m2(s), 1e-7);
    assert!(s.inertia_kg_m2() > s.condensed().inertia_kg_m2());
    let drop = integrate(&|h| s.column_gm_m3_s2() * density(h) / (radius + h).powi(2));
    close(
        drop,
        air.pressure_pa()
            * (1.0
                - (-crate::system::atmosphere::profile::Profile::cutoff_drop(air.pressure_pa()))
                    .exp()),
        1e-9,
    );
    for g in Gas::ALL {
        assert!(
            air.pressure_pa() * air.composition().fraction(g)
                <= vapour_pressure(g, air.temperature_k()) * (1.0 + 1e-8)
        );
    }
}

#[test]
fn thin_nitrogen_matches_inventory_load_radius_and_independent_shell_mass() {
    let r = request(0.0, 250.0);
    let s = solve(&r, Controls::default()).unwrap();
    eprintln!(
        "dry coupling: {:?}, radius={} km, pressure={} Pa",
        s.diagnostics(),
        s.condensed().radius_m() / 1000.0,
        s.atmosphere().unwrap().pressure_pa()
    );
    check(&s, &r);
    assert!(s.diagnostics().iterations > 1);
    assert!(s.one_bar_radius_m().unwrap().unwrap() > s.condensed().radius_m());
    let inventory = s.condensed().retained_inventory();
    let direct = dry::solve_loaded(
        &inventory,
        s.atmosphere().unwrap().pressure_pa(),
        dry::Controls::default(),
    )
    .unwrap();
    close(direct.radius_m(), s.condensed().radius_m(), 1e-9);
    assert_eq!(s, solve(&r, Controls::default()).unwrap());
}

#[test]
fn water_rich_warm_and_cold_columns_partition_without_adding_water() {
    for t in [250.0, 300.0] {
        let r = request(0.3, t);
        let s = solve(&r, Controls::default()).unwrap_or_else(|e| panic!("{t} K: {e:?}"));
        eprintln!(
            "water coupling {t}: {:?}, radius={} km, pressure={} Pa",
            s.diagnostics(),
            s.condensed().radius_m() / 1000.0,
            s.atmosphere().unwrap().pressure_pa()
        );
        check(&s, &r);
        assert!(matches!(s.condensed(), CondensedStructure::Water(_)));
        assert!(s.gas_inventory().mass_kg(Material::Water) > 0.0);
        assert!(
            s.gas_inventory().mass_kg(Material::Water) < r.exchangeable.mass_kg(Material::Water)
        );
        let air = s.atmosphere().unwrap();
        close(
            air.pressure_pa() * air.composition().fraction(Gas::Water),
            vapour_pressure(Gas::Water, t),
            1e-8,
        );
    }
}

#[test]
fn refinement_preserves_matching_and_water_partition() {
    let r = request(0.01, 300.0);
    let s = solve(&r, Controls::default()).unwrap();
    let fine = solve(
        &r,
        Controls {
            radial: dry::Controls {
                relative_tolerance: 2.5e-11,
                max_step: 1.0 / 512.0,
                ..dry::Controls::default()
            },
            ..Controls::default()
        },
    )
    .unwrap();
    check(&fine, &r);
    close(s.condensed().radius_m(), fine.condensed().radius_m(), 1e-7);
    close(
        s.atmosphere().unwrap().pressure_pa(),
        fine.atmosphere().unwrap().pressure_pa(),
        1e-7,
    );
    close(
        s.gas_inventory().mass_kg(Material::Water),
        fine.gas_inventory().mass_kg(Material::Water),
        1e-7,
    );
}

#[test]
fn airless_trace_and_protected_supply_have_explicit_reference_semantics() {
    let mut r = request(0.0, 250.0);
    r.retained = Inventory::from_mass_fractions(
        EARTH_MASS_KG,
        &[(Material::Metal, 0.325), (Material::Silicate, 0.675)],
    )
    .unwrap();
    r.exchangeable = Inventory::default();
    let s = solve(&r, Controls::default()).unwrap();
    check(&s, &r);
    assert!(s.atmosphere().is_none());
    assert!(s.one_bar_radius_m().unwrap().is_none());
    r.retained = r
        .retained
        .checked_add(&Inventory::new(&[(Material::Nitrogen, 1e10)]).unwrap())
        .unwrap();
    r.exchangeable = Inventory::new(&[(Material::Nitrogen, 1e10)]).unwrap();
    let trace = solve(&r, Controls::default()).unwrap();
    check(&trace, &r);
    assert!(trace.atmosphere().unwrap().pressure_pa() < 0.1);
    assert!(trace.one_bar_radius_m().unwrap().is_none());
    let mut wet = request(0.01, 300.0);
    wet.exchangeable =
        Inventory::new(&[(Material::Nitrogen, wet.retained.mass_kg(Material::Nitrogen))]).unwrap();
    let protected = solve(&wet, Controls::default()).unwrap();
    assert_eq!(protected.gas_inventory().mass_kg(Material::Water), 0.0);
    check(&protected, &wet);
}

#[test]
fn thin_h_he_is_a_column_not_a_giant_interior() {
    let mut r = request(0.0, 250.0);
    let light = Inventory::new(&[
        (Material::Hydrogen, EARTH_MASS_KG * 7.5e-9),
        (Material::Helium, EARTH_MASS_KG * 2.5e-9),
    ])
    .unwrap();
    r.retained = r.retained.checked_add(&light).unwrap();
    r.exchangeable = r.exchangeable.checked_add(&light).unwrap();
    let s = solve(&r, Controls::default()).unwrap();
    check(&s, &r);
    assert_eq!(
        s.gas_inventory().mass_kg(Material::Hydrogen),
        light.mass_kg(Material::Hydrogen)
    );
}

#[test]
fn unsupported_dense_weakly_bound_and_condensed_species_are_not_fallback_planets() {
    let mut r = request(0.0, 250.0);
    let light = Inventory::new(&[(Material::Hydrogen, EARTH_MASS_KG * 0.1)]).unwrap();
    r.retained = r.retained.checked_add(&light).unwrap();
    r.exchangeable = r.exchangeable.checked_add(&light).unwrap();
    assert_eq!(
        solve(&r, Controls::default()),
        Err(Error::GasMassOutsideDomain)
    );
    let mut dense = request(0.0, 250.0);
    dense.exchangeable = dense.exchangeable.scaled(100.0).unwrap();
    dense.retained = dense
        .retained
        .checked_add(&dense.exchangeable.scaled(0.99).unwrap())
        .unwrap();
    assert_eq!(
        solve(&dense, Controls::default()),
        Err(Error::DenseGasUnsupported)
    );
    let mut cold = request(0.0, 50.0);
    assert_eq!(
        solve(&cold, Controls::default()),
        Err(Error::UnsupportedCondensate(Material::Nitrogen))
    );
    cold = request(0.0, 1000.0);
    cold.retained = cold.retained.scaled(0.0002).unwrap();
    cold.exchangeable = cold.exchangeable.scaled(0.0002).unwrap();
    assert_eq!(
        solve(&cold, Controls::default()),
        Err(Error::WeakBindingUnsupported)
    );
}

#[test]
fn iteration_and_aggregate_work_limits_fail_and_exact_work_replays() {
    let r = request(0.0, 250.0);
    assert_eq!(
        solve(
            &r,
            Controls {
                max_iterations: 1,
                ..Controls::default()
            }
        ),
        Err(Error::NonConvergence)
    );
    assert_eq!(
        solve(
            &r,
            Controls {
                max_total_eos_evaluations: 0,
                ..Controls::default()
            }
        ),
        Err(Error::WorkBudgetExhausted)
    );
    let s = solve(&r, Controls::default()).unwrap();
    assert_eq!(
        s,
        solve(
            &r,
            Controls {
                max_total_eos_evaluations: s.diagnostics().total_eos_evaluations,
                ..Controls::default()
            }
        )
        .unwrap()
    );
    assert!(matches!(
        solve(
            &r,
            Controls {
                max_total_eos_evaluations: s.diagnostics().total_eos_evaluations - 1,
                ..Controls::default()
            }
        ),
        Err(Error::Radial(dry::SolveError::WorkBudgetExhausted(_)))
    ));
}

#[test]
fn invalid_supply_thermal_inputs_and_missing_water_anchors_fail_explicitly() {
    let mut r = request(0.01, 300.0);
    r.water_temperature_k = None;
    assert_eq!(
        solve(&r, Controls::default()),
        Err(Error::InvalidInput("missing water temperature"))
    );
    r = request(0.0, f64::NAN);
    assert!(matches!(
        solve(&r, Controls::default()),
        Err(Error::InvalidInput(_))
    ));
    r = request(0.0, 250.0);
    r.exchangeable = r.exchangeable.scaled(2.0).unwrap();
    assert_eq!(
        solve(&r, Controls::default()),
        Err(Error::InvalidInput("exchangeable exceeds retained"))
    );
    r = request(0.0, 250.0);
    assert!(matches!(
        solve(
            &r,
            Controls {
                max_iterations: 33,
                ..Controls::default()
            }
        ),
        Err(Error::InvalidInput(_))
    ));
}

#[test]
fn gas_stripping_and_mixed_secondary_gases_keep_inventory_and_radius_continuous() {
    let original = request(0.0, 300.0);
    let mut previous = None;
    for scale in [1.0, 0.1, 0.001, 0.0] {
        let gas = Inventory::new(&[
            (Material::Nitrogen, EARTH_MASS_KG * 7e-7 * scale),
            (Material::CarbonDioxide, EARTH_MASS_KG * 3e-7 * scale),
        ])
        .unwrap();
        let mut r = original;
        r.retained = Inventory::new(&[
            (Material::Metal, original.retained.mass_kg(Material::Metal)),
            (
                Material::Silicate,
                original.retained.mass_kg(Material::Silicate),
            ),
        ])
        .unwrap()
        .checked_add(&gas)
        .unwrap();
        r.exchangeable = gas;
        let s = solve(&r, Controls::default()).unwrap();
        check(&s, &r);
        if let Some(radius) = previous {
            assert!(s.condensed().radius_m() >= radius);
            close(s.condensed().radius_m(), radius, 1e-5);
        }
        previous = Some(s.condensed().radius_m());
    }
}

#[test]
fn sub_ulp_water_exchange_keeps_vapour_and_closes_exactly() {
    let mut r = request(0.3, 300.0);
    r.exchangeable = Inventory::new(&[
        (Material::Water, 1.0),
        (Material::Nitrogen, r.retained.mass_kg(Material::Nitrogen)),
    ])
    .unwrap();
    let water = r.retained.mass_kg(Material::Water);
    assert!(1.0 <= water * f64::EPSILON);
    let s = solve(&r, Controls::default()).unwrap();
    check(&s, &r);
    assert_eq!(s.gas_inventory().mass_kg(Material::Water), 1.0);
    assert_eq!(
        s.condensed().retained_inventory().mass_kg(Material::Water),
        water
    );
    assert_eq!(s.retained_inventory().mass_kg(Material::Water), water);
}
