use super::*;

mod condensation;
mod enriched;

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
fn effective_component_handoff_is_continuous_and_preparation_is_budgeted() {
    let gas = Inventory::new(&[
        (Material::Hydrogen, 0.375),
        (Material::Helium, 0.125),
        (Material::CarbonDioxide, 0.25),
        (Material::Nitrogen, 0.25),
    ])
    .unwrap();
    let mix = hhe_mixture::Mixture::new_enriched(&gas).unwrap();
    for thermal in [
        Thermal::new(350.0, 100.0, 10.0).unwrap(),
        Thermal::new(624.0, 100.0, 0.1).unwrap(),
    ] {
        let route = mixture::ComponentRoute::new(thermal, gas).unwrap();
        let p = route.boundary_pa;
        let t = thermal.temperature_k(p).unwrap();
        assert!(t <= crate::system::materials::molecular::MAX_TEMPERATURE_K);
        let mut work = dry::Work::default();
        let molecular = mix.density_kg_m3(p, t, 1.0).unwrap();
        let effective = route.density(mix, p, t, 1.0, &mut work, 4).unwrap();
        assert!((effective / molecular - 1.0).abs() < 1e-14);
        assert_eq!(work.eos_evaluations, 4);
        let near = p * (1.0 + 1e-7);
        let effective = route
            .density(
                mix,
                near,
                thermal.temperature_k(near).unwrap(),
                1.0,
                &mut work,
                4,
            )
            .unwrap();
        assert!((effective / molecular - 1.0).abs() < 1e-5);
        assert_eq!(work.eos_evaluations, 4); // cached, no hidden repeated anchor work
        let interfaces = route.interfaces();
        assert!(
            interfaces
                .windows(2)
                .all(|p| p[0].pressure_pa > p[1].pressure_pa)
        );
        for (i, event) in interfaces.iter().enumerate() {
            if event.pressure_pa == p {
                assert!(route.effective(p, radial::DensitySide::Branch(i)));
                assert!(!route.effective(p, radial::DensitySide::Branch(i + 1)));
            }
        }
        for budget in 0..4 {
            let route = mixture::ComponentRoute::new(thermal, gas).unwrap();
            let mut work = dry::Work::default();
            assert!(matches!(
                route.density(mix, p, t, 1.0, &mut work, budget),
                Err(SolveError::WorkBudgetExhausted(_))
            ));
            assert_eq!(work.eos_evaluations, budget);
        }
    }
    let thermal = Thermal::new(1000.0, 100.0, 10.0).unwrap();
    let route = mixture::ComponentRoute::new(thermal, gas).unwrap();
    assert_eq!(route.boundary_pa, OUTER_PRESSURE_PA);
    assert_eq!(route.interfaces().len(), 1);
    assert!(route.effective(1e4, radial::DensitySide::Branch(1)));
    assert!(
        route
            .density(mix, 1e4, 1000.0, 1.0, &mut dry::Work::default(), 0)
            .is_ok()
    );
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

fn rich_molecular_inventory(water_mass: f64) -> Inventory {
    let base = inventory(1.0, 1e-5);
    let secondary = 1e-5 * EARTH_MASS_KG * 0.02 / 0.98;
    base.checked_add(
        &Inventory::new(&[
            (Material::CarbonDioxide, 0.5 * secondary),
            (Material::Nitrogen, 0.48 * secondary),
            (Material::Methane, 0.01 * secondary),
            (Material::Argon, 0.01 * secondary),
            (Material::Water, water_mass),
        ])
        .unwrap(),
    )
    .unwrap()
}

#[test]
fn rich_molecular_envelope_conserves_and_refines_without_dropping_water() {
    let request = rich_molecular_inventory(0.0);
    let density = |p: f64, t: f64, _: f64| {
        Ok(
            p / (crate::units::BOLTZMANN_J_K / crate::units::ATOMIC_MASS_KG
                * t
                * (0.75 / crate::system::atmosphere::Gas::Hydrogen.molecular_mass_u()
                    + 0.25 / crate::system::atmosphere::Gas::Helium.molecular_mass_u())),
        )
    };
    let (interior, e) = solve_with_density(
        &request,
        &Inventory::default(),
        350.0,
        100.0,
        Default::default(),
        &density,
    )
    .unwrap();
    assert!(e.uses_molecular_mixture());
    assert_eq!(e.density_model_id(), hhe_mixture::MODEL_ID);
    let allocated = interior
        .retained_inventory()
        .checked_add(&e.inventory)
        .unwrap();
    for m in Material::ALL {
        assert!((allocated.mass_kg(m) - request.mass_kg(m)).abs() <= 1e-10 * request.mass_kg(m));
    }
    let fine_controls = coupled::Controls {
        radial: dry::Controls {
            relative_tolerance: 2e-11,
            max_step: 1.0 / 256.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let (_, fine) = solve_with_density(
        &request,
        &Inventory::default(),
        350.0,
        100.0,
        fine_controls,
        &density,
    )
    .unwrap();
    assert!((fine.one_bar_radius_m / e.one_bar_radius_m - 1.0).abs() < 1e-7);
    assert!((fine.inertia_kg_m2 / e.inertia_kg_m2 - 1.0).abs() < 1e-7);
    let (wet, _) = solve_with_density(
        &rich_molecular_inventory(1e-6 * EARTH_MASS_KG),
        &Inventory::default(),
        350.0,
        100.0,
        Default::default(),
        &density,
    )
    .unwrap();
    let water = wet
        .regions()
        .iter()
        .find(|r| r.inventory().mass_kg(Material::Water) > 0.0)
        .unwrap();
    assert_eq!(water.density_model_id(), loaded::MODEL_ID);
    assert_eq!(water.phase(), water::PhaseKnowledge::UnresolvedCompression);
    assert!(
        (wet.retained_inventory().mass_kg(Material::Water) / (1e-6 * EARTH_MASS_KG) - 1.0).abs()
            < 1e-10
    );
}

#[test]
#[ignore = "local pinned source resource; run in release"]
fn rich_molecular_reference_and_refinement() {
    let request = rich_molecular_inventory(0.0);
    let (interior, e) = solve(
        &request,
        &Inventory::default(),
        350.0,
        100.0,
        Default::default(),
    )
    .unwrap();
    let fine_controls = coupled::Controls {
        radial: dry::Controls {
            relative_tolerance: 2e-11,
            max_step: 1.0 / 256.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let (_, fine) = solve(&request, &Inventory::default(), 350.0, 100.0, fine_controls).unwrap();
    assert!((fine.one_bar_radius_m / e.one_bar_radius_m - 1.0).abs() < 1e-7);
    assert!((fine.inertia_kg_m2 / e.inertia_kg_m2 - 1.0).abs() < 1e-7);
    eprintln!(
        "mixed R={} km base={} bar I={} residual={}",
        e.one_bar_radius_m / 1000.0,
        interior.regions().last().unwrap().outer().pressure_pa / 1e5,
        e.inertia_kg_m2 / (request.total_mass_kg() * e.one_bar_radius_m.powi(2)),
        e.gravity_residual
    );
    let solved = super::super::prototype::Solved {
        structure: coupled::CoupledStructure::from_envelope(interior, e),
        source: request,
        escaped: Inventory::default(),
        blowoff: Inventory::default(),
        protected: Inventory::default(),
    };
    let record = crate::system::physical::layered(&solved);
    assert!(
        record
            .physical
            .approximations
            .iter()
            .any(|m| m == hhe_mixture::MODEL_ID)
    );
    assert!(
        record
            .physical
            .approximations
            .iter()
            .any(|m| m == crate::system::materials::molecular::MODEL_ID)
    );
    assert!(
        !record
            .physical
            .approximations
            .iter()
            .any(|m| m == TRACE_MODEL_ID)
    );
    assert_eq!(record.layers[0].model, hhe_mixture::MODEL_ID);
    assert_eq!(
        record.physical.radius_reference,
        space_shared::RadiusReference::OneBar
    );
}

#[test]
fn shallow_loaded_water_conserves_locates_model_seam_and_refines() {
    shallow_water_checks(false);
}

#[test]
#[ignore = "local pinned source resource; run in release"]
fn shallow_loaded_water_source_backed() {
    shallow_water_checks(true);
}

fn shallow_water_checks(source_backed: bool) {
    let ideal = |p: f64, t: f64, _: f64| Ok(p / (3600.0 * t));
    let table = source_backed.then(|| hhe::Table::local().unwrap());
    let density = |p, t, y| match &table {
        Some(table) => table.density_kg_m3(p, t, y),
        None => ideal(p, t, y),
    };
    for water_mass in [1e-6, 0.03] {
        let request = inventory(1.0, 1e-5)
            .checked_add(
                &Inventory::new(&[
                    (Material::Water, water_mass * EARTH_MASS_KG),
                    (Material::CarbonDioxide, 1e-8 * EARTH_MASS_KG),
                ])
                .unwrap(),
            )
            .unwrap();
        let protected = Inventory::new(&[(Material::CarbonDioxide, 1e-8 * EARTH_MASS_KG)]).unwrap();
        let solve = |controls| {
            solve_with_density(&request, &protected, 350.0, 100.0, controls, &density)
                .unwrap_or_else(|e| panic!("source={source_backed} water={water_mass}: {e:?}"))
        };
        let (interior, envelope) = solve(Default::default());
        let (fine_interior, fine) = solve(coupled::Controls {
            radial: dry::Controls {
                relative_tolerance: 2e-11,
                max_step: 1.0 / 256.0,
                ..Default::default()
            },
            ..Default::default()
        });
        assert!((fine.one_bar_radius_m / envelope.one_bar_radius_m - 1.0).abs() < 1e-7);
        assert!((fine.inertia_kg_m2 / envelope.inertia_kg_m2 - 1.0).abs() < 1e-7);
        assert!((fine_interior.radius_m() / interior.radius_m() - 1.0).abs() < 1e-7);
        assert!(envelope.outer_pressure_residual_pa() <= PRESSURE_TOLERANCE_PA);
        let retained = interior
            .retained_inventory()
            .checked_add(&envelope.inventory)
            .unwrap();
        for m in Material::ALL {
            assert!((retained.mass_kg(m) - request.mass_kg(m)).abs() <= 1e-10 * request.mass_kg(m));
        }
        assert_eq!(envelope.inventory.mass_kg(Material::Water), 0.0);
        assert_eq!(envelope.inventory.mass_kg(Material::CarbonDioxide), 0.0);
        let waters: Vec<_> = interior
            .regions()
            .iter()
            .filter(|r| r.inventory().mass_kg(Material::Water) > 0.0)
            .collect();
        assert_eq!(waters.len(), if water_mass > 0.01 { 2 } else { 1 });
        let shallow = waters.last().unwrap();
        assert_eq!(shallow.density_model_id(), loaded::MODEL_ID);
        assert!(!shallow.approximation_notes().is_empty());
        if waters.len() == 2 {
            let deep = waters[0];
            assert_eq!(deep.outer().pressure_pa, loaded::BOUNDARY_PA);
            assert_eq!(shallow.inner().pressure_pa, loaded::BOUNDARY_PA);
            assert_eq!(deep.outer().radius_m, shallow.inner().radius_m);
            assert_eq!(
                deep.outer().enclosed_mass_kg,
                shallow.inner().enclosed_mass_kg
            );
            assert!(deep.outer().density_kg_m3 > shallow.inner().density_kg_m3);
        }
        for r in &waters {
            assert_eq!(r.phase(), water::PhaseKnowledge::UnresolvedCompression);
            let (mass, _, dp) = water_quadrature(r);
            assert!((mass / r.inventory().total_mass_kg() - 1.0).abs() < 1e-7);
            assert!((dp / (r.inner().pressure_pa - r.outer().pressure_pa) - 1.0).abs() < 2e-4);
        }
        let solved = super::super::prototype::Solved {
            structure: coupled::CoupledStructure::from_envelope(interior, envelope),
            source: request,
            escaped: Inventory::default(),
            blowoff: Inventory::default(),
            protected,
        };
        let record = crate::system::physical::layered(&solved);
        assert_eq!(
            record.model_fidelity,
            space_shared::ModelFidelity::Approximate
        );
        let shallow = record
            .layers
            .iter()
            .find(|l| l.model == loaded::MODEL_ID)
            .unwrap();
        assert_eq!(shallow.kind, "water_compression");
        assert_eq!(
            shallow.model_fidelity,
            space_shared::ModelFidelity::Approximate
        );
        assert!(
            shallow
                .model_limitations
                .iter()
                .any(|n| n.contains("vapour"))
        );
        assert!(
            record
                .model_limitations
                .iter()
                .any(|n| n.contains("one bar") || n.contains("One bar"))
        );
        let json = serde_json::to_string(&record).unwrap();
        assert_eq!(
            serde_json::from_str::<space_shared::LayeredPlanet>(&json).unwrap(),
            record
        );
    }
}

// Independent radial Gaussian quadrature, using EOS density at new radii and
// endpoint hydrostatic gradients, not the integrated mass/inertia increments.
fn water_quadrature(region: &water::Region) -> (f64, f64, f64) {
    profile_quadrature(region.samples(), |p| {
        if region.density_model_id() == loaded::MODEL_ID {
            loaded::density_kg_m3(p).unwrap()
        } else {
            1460.0 + 0.00311 * p.powf(0.513)
        }
    })
}

fn profile_quadrature(samples: &[dry::Sample], density: impl Fn(f64) -> f64) -> (f64, f64, f64) {
    use std::f64::consts::PI;
    let mut result = [0.0; 3];
    for pair in samples.windows(2) {
        let [a, b] = [pair[0], pair[1]];
        let dr = b.radius_m - a.radius_m;
        let gradient = |s: dry::Sample| {
            if s.radius_m == 0.0 {
                0.0
            } else {
                -GRAVITATIONAL_CONSTANT * s.enclosed_mass_kg * s.density_kg_m3 / s.radius_m.powi(2)
            }
        };
        for (node, w) in [
            (0.1834346424956498, 0.362683783378362),
            (0.525532409916329, 0.3137066458778873),
            (0.7966664774136267, 0.2223810344533745),
            (0.9602898564975363, 0.1012285362903763),
        ] {
            for sign in [-1.0, 1.0] {
                let t: f64 = (1.0 + sign * node) * 0.5;
                let r = a.radius_m + t * dr;
                let hermite = |x, y, dx, dy| {
                    (2.0 * t.powi(3) - 3.0 * t * t + 1.0) * x
                        + (t.powi(3) - 2.0 * t * t + t) * dr * dx
                        + (-2.0 * t.powi(3) + 3.0 * t * t) * y
                        + (t.powi(3) - t * t) * dr * dy
                };
                let p = hermite(a.pressure_pa, b.pressure_pa, gradient(a), gradient(b));
                let m = hermite(
                    a.enclosed_mass_kg,
                    b.enclosed_mass_kg,
                    4.0 * PI * a.radius_m.powi(2) * a.density_kg_m3,
                    4.0 * PI * b.radius_m.powi(2) * b.density_kg_m3,
                );
                let rho = density(p);
                let weight = w * dr * 0.5;
                result[0] += weight * 4.0 * PI * r * r * rho;
                result[1] += weight * 8.0 * PI / 3.0 * r.powi(4) * rho;
                result[2] += weight * GRAVITATIONAL_CONSTANT * m * rho / (r * r);
            }
        }
    }
    (result[0], result[1], result[2])
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
