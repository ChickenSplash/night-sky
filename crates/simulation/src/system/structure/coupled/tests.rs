use super::*;
use crate::system::atmosphere::{Gas, condensation::vapour_pressure, greenhouse};

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
        temperature: Temperature::Prescribed {
            column_k: temperature,
            water_k: Some(temperature),
        },
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
    assert!(
        c.density_kg_m3
            <= if column::hot_steam(air) {
                MAX_HOT_STEAM_DENSITY_KG_M3
            } else {
                MAX_GAS_DENSITY_KG_M3
            }
    );
    // Independent radial Simpson integration, not profile::mass_factor.
    let n = 16384;
    let dr = c.extent_m / n as f64;
    let radius = s.surface_radius_m();
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
                <= column::cap_pa(g, air.temperature_k()) * (1.0 + 1e-8)
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
            column::cap_pa(Gas::Water, t),
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
    let mut protected = request(0.0, 50.0);
    protected.exchangeable = Inventory::default();
    assert_eq!(
        solve(&protected, Controls::default()),
        Err(Error::UnsupportedCondensate(Material::Nitrogen))
    );
    let mut hot = request(0.0, 1000.0);
    hot.retained = hot.retained.scaled(0.0002).unwrap();
    hot.exchangeable = hot.exchangeable.scaled(0.0002).unwrap();
    assert_eq!(
        solve(&hot, Controls::default()),
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
    r.temperature = Temperature::Prescribed {
        column_k: 300.0,
        water_k: None,
    };
    assert_eq!(
        solve(&r, Controls::default()),
        Err(Error::InvalidInput("missing water temperature"))
    );
    for t in [f64::NAN, 19.0, 2001.0] {
        r = request(0.0, t);
        assert!(matches!(
            solve(&r, Controls::default()),
            Err(Error::InvalidInput(_))
        ));
    }
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

#[test]
fn frost_settles_as_a_weighted_surface_deposit_under_its_vapour_cap() {
    let r = request(0.0, 50.0);
    let s = solve(&r, Controls::default()).unwrap();
    check(&s, &r);
    let nitrogen = r.retained.mass_kg(Material::Nitrogen);
    let frost = s.deposit_inventory().mass_kg(Material::Nitrogen);
    assert!(frost > 0.9 * nitrogen);
    assert_eq!(
        frost + s.gas_inventory().mass_kg(Material::Nitrogen),
        nitrogen
    );
    let air = s.atmosphere().unwrap();
    close(
        air.pressure_pa(),
        vapour_pressure(Gas::Nitrogen, 50.0),
        1e-8,
    );
    let radius = s.condensed().radius_m();
    close(
        s.deposit_thickness_m(),
        frost / (1030.0 * 4.0 * PI * radius.powi(2)),
        1e-5,
    );
    close(s.surface_radius_m() - radius, s.deposit_thickness_m(), 1e-6);
    let weight =
        GRAVITATIONAL_CONSTANT * s.condensed().mass_kg() * frost / (4.0 * PI * radius.powi(4));
    close(s.condensed_load_pa() - air.pressure_pa(), weight, 1e-5);
    assert!(
        (s.condensed().outer().pressure_pa - s.condensed_load_pa()).abs()
            <= 2.0 * PRESSURE_TOLERANCE_PA
    );
    let direct = dry::solve_loaded(
        &s.condensed().retained_inventory(),
        s.condensed_load_pa(),
        dry::Controls::default(),
    )
    .unwrap();
    close(direct.radius_m(), radius, 1e-9);
    let shell = s.inertia_kg_m2() - s.condensed().inertia_kg_m2() - column::inertia_kg_m2(&s);
    close(shell, (2.0 / 3.0) * frost * radius.powi(2), 1e-5);
}

#[test]
fn frost_load_reaches_a_warmer_water_host() {
    let mut r = request(0.01, 150.0);
    let carbon = Inventory::new(&[(Material::CarbonDioxide, EARTH_MASS_KG * 1e-6)]).unwrap();
    r.retained = r.retained.checked_add(&carbon).unwrap();
    r.exchangeable = r.exchangeable.checked_add(&carbon).unwrap();
    r.temperature = Temperature::Prescribed {
        column_k: 150.0,
        water_k: Some(260.0),
    };
    let s = solve(&r, Controls::default()).unwrap();
    check(&s, &r);
    assert!(matches!(s.condensed(), CondensedStructure::Water(_)));
    let frost = s.deposit_inventory().mass_kg(Material::CarbonDioxide);
    assert!(frost > 0.0);
    assert_eq!(s.deposit_inventory().total_mass_kg(), frost);
    let radius = s.condensed().radius_m();
    close(
        s.condensed_load_pa() - s.atmosphere().unwrap().pressure_pa(),
        GRAVITATIONAL_CONSTANT * s.condensed().mass_kg() * frost / (4.0 * PI * radius.powi(4)),
        1e-5,
    );
    let direct = water::solve(
        &s.condensed().retained_inventory(),
        260.0,
        s.condensed_load_pa(),
        dry::Controls::default(),
    )
    .unwrap();
    close(direct.radius_m(), s.condensed().radius_m(), 1e-9);
}

#[test]
fn cold_icy_body_below_fifty_kelvin_holds_frost_on_airless_ice() {
    let r = request(0.3, 30.0);
    let s = solve(&r, Controls::default()).unwrap();
    check(&s, &r);
    let CondensedStructure::Water(body) = s.condensed() else {
        panic!("water host expected");
    };
    let top = body.regions().last().unwrap();
    assert_eq!(
        top.density_model_id(),
        crate::system::materials::water::surface::MODEL_ID
    );
    assert_eq!(
        top.phase(),
        water::PhaseKnowledge::Water(crate::system::materials::water::Phase::IceIh)
    );
    let nitrogen = r.retained.mass_kg(Material::Nitrogen);
    assert!(s.deposit_inventory().mass_kg(Material::Nitrogen) > 0.999 * nitrogen);
    let air = s.atmosphere().unwrap();
    assert!(air.pressure_pa() < 1.0);
    close(
        air.pressure_pa(),
        vapour_pressure(Gas::Nitrogen, 30.0),
        1e-6,
    );
    assert!(s.approximation_notes().iter().any(|n| n.contains("50 K")));
}

fn water_only(water: f64, temperature: f64) -> Request {
    let retained = Inventory::from_mass_fractions(
        EARTH_MASS_KG,
        &[
            (Material::Metal, 0.325),
            (Material::Silicate, 0.675 - water),
            (Material::Water, water),
        ],
    )
    .unwrap();
    Request {
        retained,
        exchangeable: Inventory::new(&[(Material::Water, retained.mass_kg(Material::Water))])
            .unwrap(),
        temperature: Temperature::Prescribed {
            column_k: temperature,
            water_k: Some(temperature),
        },
    }
}

#[test]
fn exposed_water_saturates_its_column_or_evaporates_entirely() {
    // A pure steam column over its own ocean sits exactly on the saturation
    // curve; the liquid-side limit is taken, not refused as vapour.
    for t in [280.0, 350.0, 450.0] {
        let r = water_only(0.001, t);
        let s = solve(&r, Controls::default()).unwrap_or_else(|e| panic!("{t} K: {e:?}"));
        check(&s, &r);
        let CondensedStructure::Water(body) = s.condensed() else {
            panic!("ocean expected");
        };
        assert_eq!(
            body.regions().last().unwrap().phase(),
            water::PhaseKnowledge::Water(crate::system::materials::water::Phase::Liquid)
        );
        let saturation = crate::system::materials::water::saturation_pressure_pa(t).unwrap();
        close(s.atmosphere().unwrap().pressure_pa(), saturation, 1e-8);
        assert!(s.condensed().outer().pressure_pa >= saturation * (1.0 - 1e-14));
    }
    // Too little water to saturate: it all becomes steam over dry rock.
    let r = water_only(1e-7, 400.0);
    let s = solve(&r, Controls::default()).unwrap();
    check(&s, &r);
    assert!(matches!(s.condensed(), CondensedStructure::Dry(_)));
    assert_eq!(
        s.gas_inventory().mass_kg(Material::Water),
        r.retained.mass_kg(Material::Water)
    );
    assert!(s.atmosphere().unwrap().pressure_pa() < column::cap_pa(Gas::Water, 400.0));
}

fn greenhouse(mut r: Request, equilibrium_k: f64) -> Request {
    r.temperature = Temperature::Greenhouse { equilibrium_k };
    r
}

#[test]
fn greenhouse_warms_an_earth_twin_into_a_liquid_ocean() {
    // Earth's air by mass, about 1 bar with 420 ppm CO₂, over a 0.1% ocean.
    let mut r = water_only(0.001, 255.0);
    let air = Inventory::new(&[
        (Material::Nitrogen, EARTH_MASS_KG * 6.5e-7),
        (Material::Oxygen, EARTH_MASS_KG * 2.0e-7),
        (Material::Argon, EARTH_MASS_KG * 1.1e-8),
        (Material::CarbonDioxide, EARTH_MASS_KG * 5.4e-10),
    ])
    .unwrap();
    r.retained = r.retained.checked_add(&air).unwrap();
    r.exchangeable = r.exchangeable.checked_add(&air).unwrap();
    let cold = solve(&r, Controls::default()).unwrap();
    let CondensedStructure::Water(body) = cold.condensed() else {
        panic!("water expected");
    };
    assert_eq!(
        body.regions().last().unwrap().phase(),
        water::PhaseKnowledge::Water(crate::system::materials::water::Phase::IceIh)
    );
    assert_eq!(cold.greenhouse_optical_depth(), None);
    let r = greenhouse(r, 255.0);
    let s = solve(&r, Controls::default()).unwrap();
    check(&s, &r);
    let t = s.surface_temperature_k();
    eprintln!(
        "earth twin {t} K, {} bar",
        s.atmosphere().unwrap().pressure_pa() / 1e5
    );
    assert!((283.0..293.0).contains(&t), "{t}");
    let CondensedStructure::Water(body) = s.condensed() else {
        panic!("water expected");
    };
    assert_eq!(
        body.regions().last().unwrap().phase(),
        water::PhaseKnowledge::Water(crate::system::materials::water::Phase::Liquid)
    );
    let air = s.atmosphere().unwrap();
    assert_eq!(air.temperature_k(), t);
    let tau = s.greenhouse_optical_depth().unwrap();
    close(tau, greenhouse::optical_depth(air), 1e-12);
    close(greenhouse::surface_temperature_k(255.0, tau), t, 1e-8);
    close(
        air.pressure_pa() * air.composition().fraction(Gas::Water),
        column::cap_pa(Gas::Water, t),
        1e-8,
    );
}

#[test]
fn transparent_or_absent_air_keeps_the_equilibrium_temperature() {
    let r = greenhouse(request(0.0, 250.0), 250.0);
    let s = solve(&r, Controls::default()).unwrap();
    assert_eq!(s.surface_temperature_k(), 250.0);
    assert_eq!(s.greenhouse_optical_depth(), Some(0.0));
    let mut dry = request(0.0, 250.0);
    dry.exchangeable = Inventory::default();
    dry.retained = Inventory::from_mass_fractions(
        EARTH_MASS_KG,
        &[(Material::Metal, 0.325), (Material::Silicate, 0.675)],
    )
    .unwrap();
    let s = solve(&greenhouse(dry, 250.0), Controls::default()).unwrap();
    assert!(s.atmosphere().is_none());
    assert_eq!(s.surface_temperature_k(), 250.0);
}

#[test]
fn a_steam_runaway_beyond_the_column_is_refused_not_capped() {
    // Warming can now balance above 1000 K, but this supply is still too dense.
    let r = water_only(0.001, 300.0);
    assert!(solve(&r, Controls::default()).is_ok());
    assert_eq!(
        solve(&greenhouse(r, 300.0), Controls::default()).unwrap_err(),
        Error::DenseGasUnsupported
    );
    assert_eq!(
        solve(
            &greenhouse(water_only(0.0001, 1900.0), 1900.0),
            Controls::default()
        )
        .unwrap_err(),
        Error::WarmingOutsideDomain
    );
}

#[test]
fn hot_steam_extension_is_labelled_conserved_and_bounded() {
    let r = water_only(0.0001, 1200.0);
    let s = solve(&r, Controls::default()).unwrap();
    check(&s, &r);
    assert_eq!(s.column_model_id(), HOT_STEAM_MODEL_ID);
    assert!(s.column().unwrap().unwrap().density_kg_m3 > MAX_GAS_DENSITY_KG_M3);
    assert!(matches!(s.condensed(), CondensedStructure::Dry(_)));
    assert_eq!(
        s.gas_inventory().mass_kg(Material::Water),
        r.retained.mass_kg(Material::Water)
    );
    assert_eq!(s, solve(&r, Controls::default()).unwrap());
    let refined = solve(
        &r,
        Controls {
            radial: dry::Controls {
                relative_tolerance: dry::Controls::default().relative_tolerance / 4.0,
                max_step: dry::Controls::default().max_step / 4.0,
                ..dry::Controls::default()
            },
            ..Controls::default()
        },
    )
    .unwrap();
    close(
        s.condensed().radius_m(),
        refined.condensed().radius_m(),
        1e-7,
    );
    close(s.inertia_kg_m2(), refined.inertia_kg_m2(), 1e-7);
    assert_eq!(
        solve(&water_only(0.001, 1200.0), Controls::default()).unwrap_err(),
        Error::DenseGasUnsupported
    );
    // Neither dense non-water gas nor cold steam receives the wider mask.
    let nitrogen = GasAtmosphere::new(
        crate::system::atmosphere::Composition::new(&[(Gas::Nitrogen, 1.0)]).unwrap(),
        crate::system::atmosphere::ReferenceLevel::Surface,
        1e7,
        1200.0,
        None,
    )
    .unwrap();
    assert!(!column::hot_steam(&nitrogen));
    assert!(!column::hot_steam(
        solve(&water_only(1e-6, 999.0), Controls::default())
            .unwrap()
            .atmosphere()
            .unwrap()
    ));
    for temperature in [1000.0, 2000.0] {
        let r = water_only(0.0001, temperature);
        let s = solve(&r, Controls::default()).unwrap();
        check(&s, &r);
        assert_eq!(s.column_model_id(), HOT_STEAM_MODEL_ID);
    }
    for fraction in [0.89, 0.91] {
        let inventory = Inventory::from_mass_fractions(
            1.0,
            &[
                (Material::Water, fraction),
                (Material::Nitrogen, 1.0 - fraction),
            ],
        )
        .unwrap();
        let air = column::atmosphere(
            &inventory,
            1200.0,
            s.column_gm_m3_s2(),
            s.surface_radius_m(),
        )
        .unwrap()
        .unwrap();
        assert_eq!(column::hot_steam(&air), fraction > 0.9);
    }
    let count = s.diagnostics().total_eos_evaluations;
    assert_eq!(
        s,
        solve(
            &r,
            Controls {
                max_total_eos_evaluations: count,
                ..Controls::default()
            }
        )
        .unwrap()
    );
    assert!(
        solve(
            &r,
            Controls {
                max_total_eos_evaluations: count - 1,
                ..Controls::default()
            }
        )
        .is_err()
    );
}

fn escaping_closes(e: &escape::Escaping, r: &Request) {
    let total = e
        .structure
        .retained_inventory()
        .checked_add(&e.escaped)
        .unwrap();
    for m in Material::ALL {
        let expected = r.retained.mass_kg(m);
        assert!(
            (total.mass_kg(m) - expected).abs() <= 1e-10 * expected,
            "{m:?}"
        );
        assert!(e.escaped.mass_kg(m) <= r.exchangeable.mass_kg(m));
    }
}

#[test]
fn unbound_columns_lose_their_lightest_gases_until_the_rest_binds() {
    // Bound columns are untouched.
    let bound = request(0.001, 300.0);
    let e = escape::solve_escaping(&bound, Controls::default()).unwrap();
    assert_eq!(e.escaped.total_mass_kg(), 0.0);
    assert_eq!(e.structure, solve(&bound, Controls::default()).unwrap());
    // A small hot body cannot hold nitrogen at all.
    let mut hot = request(0.0, 1000.0);
    hot.retained = hot.retained.scaled(0.0002).unwrap();
    hot.exchangeable = hot.exchangeable.scaled(0.0002).unwrap();
    let e = escape::solve_escaping(&hot, Controls::default()).unwrap();
    escaping_closes(&e, &hot);
    assert!(e.structure.atmosphere().is_none());
    assert_eq!(e.escaped, hot.exchangeable);
    assert_eq!(
        e,
        escape::solve_escaping(&hot, Controls::default()).unwrap()
    );
    // Mixed supply: the light gas leaves first; heavier gas stays if then bound.
    for (mass, temperature, kept) in [(0.01, 400.0, true), (0.0002, 1000.0, false)] {
        let mut mixed = request(0.0, temperature);
        mixed.retained = Inventory::from_mass_fractions(
            mass * EARTH_MASS_KG,
            &[
                (Material::Metal, 0.3),
                (Material::Silicate, 0.7 - 2e-6),
                (Material::Hydrogen, 1e-6),
                (Material::CarbonDioxide, 1e-6),
            ],
        )
        .unwrap();
        mixed.exchangeable = Inventory::new(&[
            (
                Material::Hydrogen,
                mixed.retained.mass_kg(Material::Hydrogen),
            ),
            (
                Material::CarbonDioxide,
                mixed.retained.mass_kg(Material::CarbonDioxide),
            ),
        ])
        .unwrap();
        let e = escape::solve_escaping(&mixed, Controls::default()).unwrap();
        escaping_closes(&e, &mixed);
        assert_eq!(
            e.escaped.mass_kg(Material::Hydrogen),
            mixed.retained.mass_kg(Material::Hydrogen)
        );
        assert_eq!(
            e.structure.gas_inventory().mass_kg(Material::CarbonDioxide) > 0.0,
            kept,
            "T={temperature}"
        );
    }
    // Other refusals are never converted into escape.
    let mut dense = request(0.0, 250.0);
    dense.exchangeable = dense.exchangeable.scaled(100.0).unwrap();
    dense.retained = dense
        .retained
        .checked_add(&dense.exchangeable.scaled(0.99).unwrap())
        .unwrap();
    assert_eq!(
        escape::solve_escaping(&dense, Controls::default()).unwrap_err(),
        Error::DenseGasUnsupported
    );
}

#[test]
fn dry_interior_hints_preserve_accuracy_and_work_limits() {
    let inventory = Inventory::new(&[
        (Material::Metal, 0.3 * EARTH_MASS_KG),
        (Material::Silicate, 0.7 * EARTH_MASS_KG),
    ])
    .unwrap();
    let controls = dry::Controls::default();
    let original = dry::solve_loaded(&inventory, 1e5, controls).unwrap();
    let pressure = original.diagnostics().central_pressure_pa;
    for seed in [
        0.5 * pressure,
        pressure,
        1.5 * pressure,
        0.0,
        f64::NAN,
        f64::INFINITY,
    ] {
        let hinted = dry::solve_loaded_seeded(&inventory, 1e5, controls, Some(seed)).unwrap();
        assert!((hinted.radius_m() / original.radius_m() - 1.0).abs() < 1e-10);
        assert!(hinted.diagnostics().boundary_residual_pa <= water::BOUNDARY_TOLERANCE_PA);
    }
    let hinted = dry::solve_loaded_seeded(&inventory, 1e5, controls, Some(pressure)).unwrap();
    let needed = hinted.diagnostics().work.eos_evaluations;
    assert!(needed < original.diagnostics().work.eos_evaluations);
    assert!(
        dry::solve_loaded_seeded(
            &inventory,
            1e5,
            dry::Controls {
                max_eos_evaluations: needed,
                ..controls
            },
            Some(pressure)
        )
        .is_ok()
    );
    assert!(matches!(
        dry::solve_loaded_seeded(
            &inventory,
            1e5,
            dry::Controls {
                max_eos_evaluations: needed - 1,
                ..controls
            },
            Some(pressure)
        ),
        Err(dry::SolveError::WorkBudgetExhausted(_))
    ));
}

#[test]
fn water_interior_hints_preserve_accuracy_and_work_limits() {
    let inventory = Inventory::new(&[
        (Material::Metal, 0.3 * EARTH_MASS_KG),
        (Material::Silicate, 0.4 * EARTH_MASS_KG),
        (Material::Water, 0.3 * EARTH_MASS_KG),
    ])
    .unwrap();
    let controls = dry::Controls::default();
    for temperature in [250.0, 300.0] {
        let original = water::solve(&inventory, temperature, 1e5, controls).unwrap();
        let pressure = original.diagnostics().central_pressure_pa;
        for seed in [
            0.5 * pressure,
            pressure,
            1.5 * pressure,
            0.0,
            f64::NAN,
            f64::INFINITY,
        ] {
            let hinted =
                water::solve_seeded(&inventory, temperature, 1e5, controls, Some(seed)).unwrap();
            assert!((hinted.radius_m() / original.radius_m() - 1.0).abs() < 1e-10);
            assert!((hinted.inertia_kg_m2() / original.inertia_kg_m2() - 1.0).abs() < 1e-10);
            assert!(hinted.diagnostics().boundary_residual_pa <= water::BOUNDARY_TOLERANCE_PA);
        }
        let hinted =
            water::solve_seeded(&inventory, temperature, 1e5, controls, Some(pressure)).unwrap();
        let needed = hinted.diagnostics().work.eos_evaluations;
        assert!(needed < original.diagnostics().work.eos_evaluations);
        assert!(
            water::solve_seeded(
                &inventory,
                temperature,
                1e5,
                dry::Controls {
                    max_eos_evaluations: needed,
                    ..controls
                },
                Some(pressure)
            )
            .is_ok()
        );
        assert!(matches!(
            water::solve_seeded(
                &inventory,
                temperature,
                1e5,
                dry::Controls {
                    max_eos_evaluations: needed - 1,
                    ..controls
                },
                Some(pressure)
            ),
            Err(dry::SolveError::WorkBudgetExhausted(_))
        ));
    }
}
