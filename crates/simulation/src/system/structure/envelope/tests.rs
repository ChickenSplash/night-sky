use super::*;

mod condensation;
mod enriched;
mod trends;

#[test]
fn prescribed_water_trace_conserves_origins_and_obeys_its_mass_bound() {
    let base = inventory(5.0, 0.01);
    let light = base.mass_kg(Material::Hydrogen) + base.mass_kg(Material::Helium);
    for fraction in [0.001, 0.004] {
        let water = light * fraction / (1.0 - fraction);
        let retained = base
            .checked_add(&Inventory::new(&[(Material::Water, water)]).unwrap())
            .unwrap();
        let gas = Inventory::new(&[
            (Material::Hydrogen, base.mass_kg(Material::Hydrogen)),
            (Material::Helium, base.mass_kg(Material::Helium)),
            (Material::Water, water),
        ])
        .unwrap();
        let result = solve_allocated_with_density(
            &retained,
            &Inventory::default(),
            250.0,
            100.0,
            Default::default(),
            &|p, t, _| Ok(p / (3600.0 * t)),
            Some(Boundary {
                gas,
                gravity_m_s2: 12.0,
            }),
        );
        if fraction > dry::protected::MAX_TRACE_FRACTION {
            assert_eq!(result.unwrap_err(), Error::TraceDomain);
            continue;
        }
        let (interior, envelope) = result.unwrap();
        assert_eq!(envelope.thermal.gravity_m_s2, 12.0);
        assert_eq!(envelope.inventory, gas);
        assert_eq!(interior.retained_inventory().mass_kg(Material::Water), 0.0);
        let restored = interior
            .retained_inventory()
            .checked_add(&envelope.inventory)
            .unwrap();
        for m in Material::ALL {
            assert!(
                (restored.mass_kg(m) - retained.mass_kg(m)).abs() <= 1e-10 * retained.mass_kg(m)
            );
        }
    }
}

#[test]
fn radius_reference_distinguishes_sub_bar_shells_from_missed_events() {
    let shell = |inner, outer| Region {
        inventory: Inventory::new(&[(Material::Hydrogen, 1.0)]).unwrap(),
        samples: vec![
            dry::Sample {
                radius_m: 1e7,
                enclosed_mass_kg: 1e25,
                pressure_pa: inner,
                density_kg_m3: 1.0,
            },
            dry::Sample {
                radius_m: 2e7,
                enclosed_mass_kg: 2e25,
                pressure_pa: outer,
                density_kg_m3: 0.1,
            },
        ],
    };
    for pressure in [1001.0, 5e4, 1e5] {
        let region = shell(pressure, OUTER_PRESSURE_PA);
        assert_eq!(
            reference_sample(std::slice::from_ref(&region)).unwrap(),
            region.inner()
        );
    }
    assert_eq!(
        reference_sample(&[shell(2e5, OUTER_PRESSURE_PA)]),
        Err(Error::MissingOneBarBoundary)
    );
    let deep = shell(2e5, 1e5);
    assert_eq!(
        reference_sample(&[deep.clone(), shell(1e5, OUTER_PRESSURE_PA)]).unwrap(),
        deep.outer()
    );
    assert_eq!(reference_sample(&[]), Err(Error::MissingOneBarBoundary));
}

#[test]
fn sub_bar_envelope_reports_condensed_radius_and_actual_boundary_facts() {
    use crate::system::{physical, structure::prototype};
    let request = inventory(5.0, 1e-7);
    let (interior, envelope) = solve_with_density(
        &request,
        &Inventory::default(),
        250.0,
        100.0,
        Default::default(),
        &|p, t, _| Ok(p / (3600.0 * t)),
    )
    .unwrap();
    let radius = interior.radius_m();
    let pressure = interior.regions().last().unwrap().outer().pressure_pa;
    assert!((OUTER_PRESSURE_PA..1e5).contains(&pressure));
    assert_eq!(envelope.one_bar_radius_m(), None);
    let temperature = envelope.thermal.temperature_k(pressure).unwrap();
    let expected_gravity = GRAVITATIONAL_CONSTANT * interior.mass_kg() / radius.powi(2);
    assert!((envelope.reference_gravity_m_s2() / expected_gravity - 1.0).abs() < 1e-14);
    let solved = prototype::Solved {
        structure: coupled::CoupledStructure::from_envelope(interior, envelope),
        source: request,
        escaped: Inventory::default(),
        protected: Inventory::default(),
    };
    let record = physical::layered(&solved);
    physical::validate_record(&record).unwrap();
    assert_eq!(record.physical.reference_radius_m, radius);
    assert!((record.physical.reference_gravity_m_s2 / expected_gravity - 1.0).abs() < 1e-14);
    assert_eq!(
        record.physical.radius_reference,
        space_shared::RadiusReference::SolidSurface
    );
    assert_eq!(record.one_bar_radius_m, None);
    assert_eq!(record.surface, "rock");
    assert_eq!(record.surface_pressure_pa, pressure);
    assert_eq!(record.surface_temperature_k, temperature);
    assert!(
        record
            .model_limitations
            .iter()
            .any(|s| s.contains("below one bar"))
    );
    assert_eq!(record.physical.retained_mass_kg, request.total_mass_kg());
}

#[test]
#[ignore = "local pinned source resource; run in release"]
fn sub_bar_water_references_close_and_refine() {
    use crate::{
        galaxy::Component,
        system::{galactic_bodies, physical, structure::prototype},
    };
    for (component, index, suffix) in [
        (Component::Nucleus, 6, "/2"),
        (Component::ThinDisc, 9, "/4"),
    ] {
        let key = format!("composition-calibration:{component:?}:{index}");
        let system = galactic_bodies(&key, component);
        let planet = system
            .planets
            .iter()
            .find(|p| p.id().ends_with(suffix))
            .unwrap();
        let solve = |controls| {
            let solved = prototype::solve(
                &planet.body.budget,
                planet.environment.as_ref().unwrap(),
                controls,
            )
            .unwrap();
            physical::layered(&solved)
        };
        let record = solve(coupled::Controls::default());
        let fine = solve(coupled::Controls {
            radial: dry::Controls {
                relative_tolerance: 2e-11,
                max_step: 1.0 / 256.0,
                ..Default::default()
            },
            ..Default::default()
        });
        assert_eq!(record.one_bar_radius_m, None);
        assert_eq!(
            record.physical.reference_radius_m,
            record.physical.condensed_radius_m
        );
        assert_eq!(
            record.physical.radius_reference,
            space_shared::RadiusReference::UnresolvedVolatileBoundary
        );
        assert_eq!(record.surface, "unresolved compressed water");
        assert!((OUTER_PRESSURE_PA..1e5).contains(&record.surface_pressure_pa));
        assert!(
            (fine.physical.reference_radius_m / record.physical.reference_radius_m - 1.0).abs()
                < 1e-7
        );
        assert!((fine.physical.inertia_factor / record.physical.inertia_factor - 1.0).abs() < 1e-7);
        assert_eq!(record.physical.materials, fine.physical.materials);
        assert!(
            record
                .layers
                .windows(2)
                .all(|p| p[0].inner_radius_m == p[1].outer_radius_m)
        );
        let mass: f64 = record.layers.iter().map(|r| r.mass_kg).sum();
        assert!((mass / record.physical.retained_mass_kg - 1.0).abs() < 1e-10);
    }
}

#[test]
#[ignore = "local pinned source resource; run in release"]
fn recovered_envelope_steps_close_and_refine() {
    use crate::{
        galaxy::Component,
        system::{galactic_bodies, physical, structure::prototype},
    };
    for (component, index, suffix) in [
        (Component::Nucleus, 15, "/0"),
        (Component::NuclearDisc, 17, "/1"),
    ] {
        let key = format!("composition-calibration:{component:?}:{index}");
        let system = galactic_bodies(&key, component);
        let planet = system
            .planets
            .iter()
            .find(|p| p.id().ends_with(suffix))
            .unwrap();
        let solve = |controls| {
            let solved = prototype::solve(
                &planet.body.budget,
                planet.environment.as_ref().unwrap(),
                controls,
            )
            .unwrap();
            physical::layered(&solved)
        };
        let baseline = solve(coupled::Controls::default());
        let fine = solve(coupled::Controls {
            radial: dry::Controls {
                relative_tolerance: 2e-11,
                max_step: 1.0 / 256.0,
                ..Default::default()
            },
            ..Default::default()
        });
        let p = &baseline.physical;
        let q = &fine.physical;
        assert!(baseline.fallback_reason.is_none());
        assert!((q.reference_radius_m / p.reference_radius_m - 1.0).abs() < 1e-7);
        let inertia = |p: &space_shared::PlanetPhysical| {
            p.inertia_factor * p.retained_mass_kg * p.reference_radius_m.powi(2)
        };
        assert!((inertia(q) / inertia(p) - 1.0).abs() < 1e-7);
        assert_eq!(p.materials, q.materials);
        let layer_mass: f64 = baseline.layers.iter().map(|r| r.mass_kg).sum();
        assert!((layer_mass / p.retained_mass_kg - 1.0).abs() < 1e-10);
        assert!(
            baseline
                .layers
                .windows(2)
                .all(|r| r[0].inner_radius_m == r[1].outer_radius_m)
        );
    }
}

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
    // The core and the gas are integrated towards each other: their meeting
    // point closes to the recorded residuals, the mass coordinate exactly.
    let base = e.regions.first().unwrap().inner();
    let surface = core.regions().last().unwrap().outer();
    assert_eq!(surface.enclosed_mass_kg, base.enclosed_mass_kg);
    assert_eq!(
        e.interface_radius_residual_m(),
        base.radius_m - surface.radius_m
    );
    assert_eq!(
        e.interface_pressure_residual_pa(),
        surface.pressure_pa - base.pressure_pa
    );
    assert!(e.interface_radius_residual_m().abs() <= 1e-10 * surface.radius_m);
    assert!(e.interface_pressure_residual_pa().abs() <= interface_tolerance_pa(base.pressure_pa));
    assert_eq!(e.outer_pressure_residual_pa(), 0.0);
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
    assert!((fine.reference_radius_m / e.reference_radius_m - 1.0).abs() < 1e-7);
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
    assert!((fine.reference_radius_m / e.reference_radius_m - 1.0).abs() < 1e-7);
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
    assert!((fine.reference_radius_m / e.reference_radius_m - 1.0).abs() < 1e-7);
    assert!((fine.inertia_kg_m2 / e.inertia_kg_m2 - 1.0).abs() < 1e-7);
    eprintln!(
        "mixed R={} km base={} bar I={} residual={}",
        e.reference_radius_m / 1000.0,
        interior.regions().last().unwrap().outer().pressure_pa / 1e5,
        e.inertia_kg_m2 / (request.total_mass_kg() * e.reference_radius_m.powi(2)),
        e.gravity_residual
    );
    let solved = super::super::prototype::Solved {
        structure: coupled::CoupledStructure::from_envelope(interior, e),
        source: request,
        escaped: Inventory::default(),
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
        assert!((fine.reference_radius_m / envelope.reference_radius_m - 1.0).abs() < 1e-7);
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
    assert!((15000.0..18000.0).contains(&(baseline.reference_radius_m / 1000.0)));
    assert!(core.radius_m() < baseline.reference_radius_m);
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
    assert!((fine.reference_radius_m / baseline.reference_radius_m - 1.0).abs() < 1e-7);
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
                e.reference_radius_m / 1000.0,
                e.inertia_kg_m2 / ((core + gas) * EARTH_MASS_KG * e.reference_radius_m.powi(2)),
                e.iterations,
                e.evaluations,
                interior.radius_m() / 1000.0,
                e.gravity_residual
            ),
            Err(e) => eprintln!("core={core} gas={gas} Tint={tint}: {e:?}"),
        }
    }
}

#[test]
fn intrinsic_temperature_follows_the_giants_and_cools_with_age() {
    use crate::units::EARTH_MASS_KG;
    let at = |mass_earth: f64, age: f64| intrinsic_k(mass_earth * EARTH_MASS_KG, age);
    // Measured: Jupiter about 99 K, Saturn about 77 K, Neptune about 53 K.
    for (mass, measured) in [(317.83, 99.0), (95.16, 77.0), (17.15, 53.0)] {
        assert!(
            (at(mass, 4.57) - measured).abs() < 2.0,
            "{mass} Earth masses"
        );
    }
    assert!(at(9.5, 6.9) < 50.0);
    assert!(at(9.5, 0.5) > at(9.5, 5.0));
    assert!(at(100.0, 5.0) > at(10.0, 5.0));
    // Always inside the profile's accepted range, whatever the inputs.
    for (mass, age) in [(1e-4, 13.0), (1200.0, 1e-3), (0.0, 1.0), (1.0, 0.0)] {
        let t = at(mass, age);
        assert!((thermal::INTRINSIC_RANGE_K[0]..=thermal::INTRINSIC_RANGE_K[1]).contains(&t));
        Thermal::new(300.0, t, 10.0).unwrap();
    }
}
