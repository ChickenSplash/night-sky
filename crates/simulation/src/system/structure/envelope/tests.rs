use super::*;

fn inventory(core: f64, gas: f64) -> Inventory {
    Inventory::new(&[
        (Material::Metal, core * 0.3 * EARTH_MASS_KG),
        (Material::Silicate, core * 0.7 * EARTH_MASS_KG),
        (Material::Hydrogen, gas * 0.75 * EARTH_MASS_KG),
        (Material::Helium, gas * 0.25 * EARTH_MASS_KG),
    ])
    .unwrap()
}

#[test]
fn thermal_profile_matches_decision_probe_and_rejects_invalid_inputs() {
    let t = Thermal::new(110.0, 50.0, 25.0).unwrap();
    assert!((t.boundary_pressure_pa() / 1e5 - 4.611637453226349).abs() < 1e-10);
    assert!((t.temperature_k(1e11).unwrap() - 4573.963790220213).abs() < 1e-8);
    assert!(Thermal::new(f64::NAN, 50.0, 10.0).is_err());
    assert!(t.temperature_k(0.0).is_err());
}

#[test]
fn zero_budget_and_invalid_allocations_are_explicit() {
    let request = inventory(5.0, 1.0);
    let eos = |_: f64, _: f64, _: f64| Ok(100.0);
    for temperature in [f64::NAN, f64::INFINITY, 0.0, -1.0] {
        assert_eq!(
            solve_with_density(
                &request,
                &Inventory::default(),
                temperature,
                100.0,
                Default::default(),
                &eos
            )
            .unwrap_err(),
            Error::InvalidInput("equilibrium temperature")
        );
    }
    let controls = coupled::Controls {
        max_total_eos_evaluations: 0,
        ..Default::default()
    };
    assert_eq!(
        solve_with_density(
            &request,
            &Inventory::default(),
            250.0,
            100.0,
            controls,
            &eos
        )
        .unwrap_err(),
        Error::WorkBudget
    );
    assert!(
        solve_with_density(&request, &request, 250.0, 100.0, Default::default(), &eos).is_err()
    );
}

#[test]
fn unified_kernel_closes_interfaces_origins_and_shell_inertia() {
    let requested = inventory(5.0, 0.1);
    let density = |_: f64, _: f64, _: f64| Ok(100.0);
    let (core, e) = solve_with_density(
        &requested,
        &Inventory::default(),
        250.0,
        100.0,
        Default::default(),
        &density,
    )
    .unwrap();
    let base = e.regions.first().unwrap().inner();
    assert_eq!(
        core.regions().last().unwrap().outer().radius_m,
        base.radius_m
    );
    assert_eq!(
        core.regions().last().unwrap().outer().pressure_pa,
        base.pressure_pa
    );
    assert!(e.gravity_residual.abs() < GRAVITY_TOLERANCE);
    let outer = e.regions.last().unwrap().outer();
    let analytic = 8.0 * std::f64::consts::PI / 15.0
        * 100.0
        * (outer.radius_m.powi(5) - base.radius_m.powi(5));
    assert!(((e.inertia_kg_m2 - core.inertia_kg_m2()) / analytic - 1.0).abs() < 1e-8);
    let refined = coupled::Controls {
        radial: dry::Controls {
            relative_tolerance: 2e-11,
            max_step: 1.0 / 256.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let (_, fine) = solve_with_density(
        &requested,
        &Inventory::default(),
        250.0,
        100.0,
        refined,
        &density,
    )
    .unwrap();
    assert!((fine.one_bar_radius_m / e.one_bar_radius_m - 1.0).abs() < 1e-7);
    assert!((fine.inertia_kg_m2 / e.inertia_kg_m2 - 1.0).abs() < 1e-7);
}

#[test]
fn passive_traces_keep_origins_and_do_not_gain_protection() {
    let trace = 1e-5 * EARTH_MASS_KG;
    let requested = inventory(5.0, 0.1)
        .checked_add(
            &Inventory::new(&[
                (Material::CarbonDioxide, trace),
                (Material::Nitrogen, trace),
            ])
            .unwrap(),
        )
        .unwrap();
    let protected = Inventory::new(&[(Material::CarbonDioxide, 0.9 * trace)]).unwrap();
    let density = |_: f64, _: f64, _: f64| Ok(100.0);
    let (core, e) = solve_with_density(
        &requested,
        &protected,
        250.0,
        100.0,
        Default::default(),
        &density,
    )
    .unwrap();
    assert!(
        (core.retained_inventory().mass_kg(Material::CarbonDioxide) / (0.9 * trace) - 1.0).abs()
            < 1e-12
    );
    assert_eq!(core.retained_inventory().mass_kg(Material::Nitrogen), 0.0);
    assert!((e.inventory.mass_kg(Material::CarbonDioxide) / (0.1 * trace) - 1.0).abs() < 1e-12);
    assert_eq!(e.inventory.mass_kg(Material::Nitrogen), trace);
}

#[test]
fn positive_water_gets_its_own_loaded_unresolved_region() {
    let water_mass = 0.5 * EARTH_MASS_KG;
    let requested = inventory(5.0, 0.1)
        .checked_add(&Inventory::new(&[(Material::Water, water_mass)]).unwrap())
        .unwrap();
    let density = |_: f64, _: f64, _: f64| Ok(100.0);
    let (core, _) = solve_with_density(
        &requested,
        &Inventory::default(),
        250.0,
        100.0,
        Default::default(),
        &density,
    )
    .unwrap();
    let water = core
        .regions()
        .iter()
        .find(|r| r.inventory().mass_kg(Material::Water) > 0.0)
        .unwrap();
    assert!((water.inventory().mass_kg(Material::Water) / water_mass - 1.0).abs() < 1e-12);
    assert_eq!(water.density_model_id(), WATER_MODEL_ID);
    assert_eq!(water.phase(), water::PhaseKnowledge::UnresolvedCompression);
    assert!(water.outer().pressure_pa >= 1e9);
}

#[test]
fn transport_is_conserved_and_one_bar_is_not_ground() {
    let requested = inventory(5.0, 0.1);
    let density = |_: f64, _: f64, _: f64| Ok(100.0);
    let (interior, envelope) = solve_with_density(
        &requested,
        &Inventory::default(),
        250.0,
        100.0,
        Default::default(),
        &density,
    )
    .unwrap();
    let solved = super::super::prototype::Solved {
        structure: coupled::CoupledStructure::from_envelope(interior, envelope),
        source: requested,
        escaped: Inventory::default(),
        blowoff: Inventory::default(),
        protected: Inventory::default(),
    };
    let record = crate::system::physical::layered(&solved);
    assert_eq!(record.surface, "envelope");
    assert_eq!(
        record.physical.radius_reference,
        space_shared::RadiusReference::OneBar
    );
    assert_eq!(
        record.one_bar_radius_m,
        Some(record.physical.reference_radius_m)
    );
    assert_eq!(record.surface_pressure_pa, 1e5);
    assert!(record.layers[0].outer_radius_m > record.physical.reference_radius_m);
    assert!(
        record
            .layers
            .windows(2)
            .all(|p| p[0].inner_radius_m == p[1].outer_radius_m)
    );
    assert!(
        (record.layers.iter().map(|l| l.mass_kg).sum::<f64>() / requested.total_mass_kg() - 1.0)
            .abs()
            < 1e-10
    );
    for m in record.physical.materials {
        assert!(
            (m.interior_kg + m.condensed_kg + m.gas_kg - m.retained_kg).abs()
                <= 1e-10 * m.retained_kg
        );
    }
}

#[test]
#[ignore = "local pinned source resource; run in release"]
fn envelope_reference_cases() {
    let (core, baseline) = solve(
        &inventory(5.0, 0.1),
        &Inventory::default(),
        250.0,
        100.0,
        Default::default(),
    )
    .unwrap();
    assert!((15000.0..18000.0).contains(&(baseline.one_bar_radius_m / 1000.0)));
    assert!(core.radius_m() < baseline.one_bar_radius_m);
    let refined = coupled::Controls {
        radial: dry::Controls {
            relative_tolerance: 2e-11,
            max_step: 1.0 / 256.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let (_, fine) = solve(
        &inventory(5.0, 0.1),
        &Inventory::default(),
        250.0,
        100.0,
        refined,
    )
    .unwrap();
    assert!((fine.one_bar_radius_m / baseline.one_bar_radius_m - 1.0).abs() < 1e-7);
    assert!((fine.inertia_kg_m2 / baseline.inertia_kg_m2 - 1.0).abs() < 1e-7);
    for (core, gas, teq, tint) in [
        (5.0, 0.1, 250.0, 100.0),
        (10.0, 30.0, 110.0, 100.0),
        (10.0, 308.0, 110.0, 100.0),
        (10.0, 308.0, 110.0, 200.0),
    ] {
        let result = solve(
            &inventory(core, gas),
            &Inventory::default(),
            teq,
            tint,
            Default::default(),
        );
        match result {
            Ok((interior, e)) => eprintln!(
                "core={core} gas={gas} Tint={tint}: R={} km I={} iterations={} work={} coreR={} km residual={}",
                e.one_bar_radius_m / 1000.0,
                e.inertia_kg_m2 / ((core + gas) * EARTH_MASS_KG * e.one_bar_radius_m.powi(2)),
                e.iterations,
                e.evaluations,
                interior.radius_m() / 1000.0,
                e.gravity_residual
            ),
            Err(e) => eprintln!("core={core} gas={gas} Tint={tint}: {e:?}"),
        }
    }
}
