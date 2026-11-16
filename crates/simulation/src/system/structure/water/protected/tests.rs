use super::super::{PhaseKnowledge, WorkLimit, eos};
use super::*;
use crate::units::GRAVITATIONAL_CONSTANT;

const SECONDARY: [Material; 5] = [
    Material::CarbonDioxide,
    Material::Nitrogen,
    Material::Methane,
    Material::Oxygen,
    Material::Argon,
];
const LOAD: f64 = 1e5;

fn host() -> Inventory {
    Inventory::from_mass_fractions(
        EARTH_MASS_KG,
        &[
            (Material::Metal, 0.2),
            (Material::Silicate, 0.5),
            (Material::Water, 0.3),
        ],
    )
    .unwrap()
}

fn mixed(host: &Inventory, reverse: bool) -> Inventory {
    let mass = MAX_TRACE_FRACTION * host.total_mass_kg() / (1.0 - MAX_TRACE_FRACTION);
    let mut entries = vec![
        (Material::CarbonDioxide, 0.49 * mass),
        (Material::Nitrogen, 0.49 * mass),
        (Material::Methane, 0.01 * mass),
        (Material::Argon, 0.01 * mass),
    ];
    if reverse {
        entries.reverse();
    }
    Inventory::new(&entries).unwrap()
}

fn error(actual: f64, expected: f64) -> f64 {
    if expected == 0.0 {
        assert_eq!(actual, 0.0);
        0.0
    } else {
        (actual / expected - 1.0).abs()
    }
}

fn close(actual: f64, expected: f64, tolerance: f64) {
    let residual = error(actual, expected);
    assert!(
        residual <= tolerance,
        "{actual:e} != {expected:e}: {residual:e}"
    );
}

// Independently evaluates the host EOS at Gaussian radial nodes, using Hermite
// pressure/mass interpolation, not the integrator's mass/inertia identities.
fn check_profile(
    body: &WaterStructure,
    host: &Inventory,
    traces: &Inventory,
    lambda: f64,
) -> [f64; 3] {
    assert_eq!(body.model_id(), MODEL_ID);
    assert_eq!(body.host_model_id(), super::super::MODEL_ID);
    let requested = host.checked_add(traces).unwrap();
    let allocated = body.retained_inventory();
    for origin in Material::ALL {
        close(allocated.mass_kg(origin), requested.mass_kg(origin), 1e-12);
    }
    let f = traces.total_mass_kg() / requested.total_mass_kg();
    let volume_factor = 1.0 - f + f / lambda;
    let mut residuals = [0.0_f64; 3];
    let (mut total_mass, mut total_inertia) = (0.0, 0.0);
    for region in body.regions() {
        let host_mass: f64 = [Material::Metal, Material::Silicate, Material::Water]
            .into_iter()
            .map(|m| region.inventory().mass_kg(m))
            .sum();
        let weight = host_mass / host.total_mass_kg();
        for origin in SECONDARY {
            close(
                region.inventory().mass_kg(origin),
                traces.mass_kg(origin) * weight,
                1e-12,
            );
            if traces.mass_kg(origin) > 0.0 {
                assert!(region.inventory().mass_kg(origin) > 0.0);
                assert_eq!(
                    region.origin_phase(origin),
                    Some(PhaseKnowledge::Unresolved)
                );
            } else {
                assert_eq!(region.origin_phase(origin), None);
            }
        }
        if region.inventory().mass_kg(Material::Water) > 0.0 {
            assert_eq!(region.origin_phase(Material::Water), Some(region.phase()));
            assert!(region.temperature_k().is_some());
        } else {
            assert_eq!(region.phase(), PhaseKnowledge::Unresolved);
            assert_eq!(region.temperature_k(), None);
        }
        let (mass, inertia, dp) =
            super::super::tests::quadrature_with_volume_factor(region, volume_factor);
        let drop = region.inner().pressure_pa - region.outer().pressure_pa;
        close(mass, region.inventory().total_mass_kg(), 1e-7);
        close(dp, drop, 2e-4);
        residuals[0] = residuals[0].max(error(mass, region.inventory().total_mass_kg()));
        residuals[1] = residuals[1].max(error(dp, drop));
        for origin in Material::ALL {
            close(
                mass * region.inventory().mass_fraction(origin).unwrap(),
                region.inventory().mass_kg(origin),
                1e-7,
            );
        }
        total_mass += mass;
        total_inertia += inertia;
    }
    close(total_mass, requested.total_mass_kg(), 1e-7);
    close(total_inertia, body.inertia_kg_m2(), 1e-7);
    close(
        body.reference_gravity_m_s2(),
        GRAVITATIONAL_CONSTANT * total_mass / body.radius_m().powi(2),
        1e-7,
    );
    residuals[2] = error(total_inertia, body.inertia_kg_m2());
    for pair in body.regions().windows(2) {
        let (a, b) = (pair[0].outer(), pair[1].inner());
        assert_eq!(
            (a.radius_m, a.enclosed_mass_kg, a.pressure_pa),
            (b.radius_m, b.enclosed_mass_kg, b.pressure_pa)
        );
        // A model seam need not have a density jump of either particular sign.
    }
    assert!(
        (0.0..=super::super::BOUNDARY_TOLERANCE_PA)
            .contains(&body.diagnostics().boundary_residual_pa)
    );
    residuals
}

fn compare_reference(
    body: &WaterStructure,
    host: &Inventory,
    traces: &Inventory,
    t: f64,
    load: f64,
) {
    let parcels = host
        .scaled(host.checked_add(traces).unwrap().total_mass_kg() / host.total_mass_kg())
        .unwrap();
    let reference = super::super::solve(&parcels, t, load, Controls::default()).unwrap();
    close(body.radius_m(), reference.radius_m(), 1e-12);
    close(body.inertia_kg_m2(), reference.inertia_kg_m2(), 1e-12);
    close(
        body.diagnostics().central_pressure_pa,
        reference.diagnostics().central_pressure_pa,
        1e-12,
    );
    assert_eq!(body.regions().len(), reference.regions().len());
    for (a, b) in body.regions().iter().zip(reference.regions()) {
        assert_eq!(a.samples(), b.samples());
        assert_eq!(a.phase(), b.phase());
        assert_eq!(a.temperature_k(), b.temperature_k());
        assert_eq!(a.density_model_id(), b.density_model_id());
        assert_eq!(a.approximation_notes(), b.approximation_notes());
    }
}

#[test]
fn zero_traces_recover_water_profiles_without_api_changes() {
    for t in [250.0, 260.0, 300.0] {
        let body = solve(&host(), &Inventory::default(), t, LOAD, Controls::default()).unwrap();
        let reference = super::super::solve(&host(), t, LOAD, Controls::default()).unwrap();
        assert_eq!(body.regions(), reference.regions());
        assert_eq!(body.diagnostics(), reference.diagnostics());
        assert_eq!(body.inertia_kg_m2(), reference.inertia_kg_m2());
        assert_eq!(reference.model_id(), super::super::MODEL_ID);
        check_profile(&body, &host(), &Inventory::default(), 1.0);
    }
}

#[test]
fn every_secondary_origin_survives_all_warm_host_segments() {
    let host = host();
    let mut maxima = [0.0_f64; 3];
    for origin in SECONDARY {
        for ratio in [1e-12, 1e-7, 0.003] {
            let traces = Inventory::new(&[(origin, ratio * host.total_mass_kg())]).unwrap();
            let body = solve(&host, &traces, 300.0, LOAD, Controls::default()).unwrap();
            for (max, residual) in maxima
                .iter_mut()
                .zip(check_profile(&body, &host, &traces, 1.0))
            {
                *max = max.max(residual);
            }
            compare_reference(&body, &host, &traces, 300.0, LOAD);
        }
    }
    println!("6b 15 warm single-origin fixtures: quadrature maxima={maxima:?}");
}

#[test]
fn mixed_endpoints_preserve_events_refine_and_repeat() {
    let host = host();
    let traces = mixed(&host, false);
    for t in [250.0, 260.0, 300.0] {
        let body = solve(&host, &traces, t, LOAD, Controls::default()).unwrap();
        let residuals = check_profile(&body, &host, &traces, 1.0);
        compare_reference(&body, &host, &traces, t, LOAD);
        assert_eq!(
            body,
            solve(&host, &mixed(&host, true), t, LOAD, Controls::default()).unwrap()
        );
        assert_eq!(
            body,
            solve(&host, &traces, t, LOAD, Controls::default()).unwrap()
        );
        let fine = solve(
            &host,
            &traces,
            t,
            LOAD,
            Controls {
                relative_tolerance: 2.5e-11,
                max_step: 1.0 / 512.0,
                ..Controls::default()
            },
        )
        .unwrap();
        check_profile(&fine, &host, &traces, 1.0);
        close(body.radius_m(), fine.radius_m(), 1e-7);
        close(body.inertia_factor(), fine.inertia_factor(), 1e-7);
        close(
            body.diagnostics().central_pressure_pa,
            fine.diagnostics().central_pressure_pa,
            1e-7,
        );
        assert_eq!(body.regions().len(), fine.regions().len());
        let mut allocation_error = 0.0_f64;
        for (a, b) in body.regions().iter().zip(fine.regions()) {
            assert_eq!(a.phase(), b.phase());
            assert_eq!(a.density_model_id(), b.density_model_id());
            for origin in Material::ALL {
                close(
                    a.inventory().mass_kg(origin),
                    b.inventory().mass_kg(origin),
                    1e-5,
                );
                allocation_error = allocation_error.max(error(
                    a.inventory().mass_kg(origin),
                    b.inventory().mass_kg(origin),
                ));
            }
            close(
                a.outer().radius_m - a.inner().radius_m,
                b.outer().radius_m - b.inner().radius_m,
                1e-5,
            );
            close(
                a.inner().pressure_pa - a.outer().pressure_pa,
                b.inner().pressure_pa - b.outer().pressure_pa,
                1e-5,
            );
        }
        println!(
            "6b T={t}: R={} km, I/MR²={}, quadrature={residuals:?}, refined R={:e}, I/MR²={:e}, allocation={allocation_error:e}, fine EOS={}",
            body.radius_m() / 1000.0,
            body.inertia_factor(),
            error(body.radius_m(), fine.radius_m()),
            error(body.inertia_factor(), fine.inertia_factor()),
            fine.diagnostics().work.eos_evaluations
        );
    }
}

#[test]
fn density_sensitivity_is_bounded_without_reassigning_trace_phase() {
    let host = host();
    let traces = mixed(&host, false);
    for t in [250.0, 260.0, 300.0] {
        let nominal = solve(&host, &traces, t, LOAD, Controls::default()).unwrap();
        for lambda in [0.5, 1.0, 2.0] {
            let body =
                solve_with_density_ratio(&host, &traces, t, LOAD, Controls::default(), lambda)
                    .unwrap();
            check_profile(&body, &host, &traces, lambda);
            let dr = error(body.radius_m(), nominal.radius_m());
            let di = error(body.inertia_factor(), nominal.inertia_factor());
            assert!(dr <= 0.01 && di <= 0.02);
            println!("6b T={t} lambda={lambda}: relative R={dr:e}, I/MR²={di:e}");
        }
    }
}

#[test]
fn pure_thin_and_event_loaded_water_hosts_keep_one_sided_allocations() {
    for (mass, water_fraction) in [(0.0001, 1.0), (1.0, 1e-6)] {
        let host = super::super::tests::inventory(mass, water_fraction);
        let traces = mixed(&host, false);
        for t in [250.0, 300.0, 273.155] {
            let load = if t == 273.155 { 1000.0 } else { LOAD };
            let body = solve(&host, &traces, t, load, Controls::default()).unwrap();
            check_profile(&body, &host, &traces, 1.0);
            compare_reference(&body, &host, &traces, t, load);
        }
    }
    for (t, load) in [
        (250.0, 100e6),
        (250.0, 220e6),
        (250.0, 2.1e9),
        (300.0, 100e6),
        (300.0, eos::deep_ice::boundary_pressure_pa(300.0).unwrap()),
    ] {
        let host = host();
        let traces = mixed(&host, false);
        let body = solve(&host, &traces, t, load, Controls::default()).unwrap();
        check_profile(&body, &host, &traces, 1.0);
        compare_reference(&body, &host, &traces, t, load);
        assert_eq!(
            body.regions().last().unwrap().outer().pressure_pa - load,
            body.diagnostics().boundary_residual_pa
        );
    }
    let host = super::super::tests::inventory(0.0001, 1.0);
    let load = eos::melting_pressure_ih_pa(273.155).unwrap();
    let body = solve(
        &host,
        &mixed(&host, false),
        273.155,
        load,
        Controls::default(),
    )
    .unwrap();
    assert_eq!(body.regions().len(), 1);
    assert_eq!(
        body.regions()[0].phase(),
        PhaseKnowledge::Water(eos::Phase::Liquid)
    );
    check_profile(&body, &host, &mixed(&host, false), 1.0);
}

#[test]
fn nested_work_is_charged_and_all_budget_refusals_are_deterministic() {
    let host = host();
    let traces = mixed(&host, false);
    let body = solve(&host, &traces, 300.0, LOAD, Controls::default()).unwrap();
    let count = body.diagnostics().work.eos_evaluations;
    assert_eq!(
        body,
        solve(
            &host,
            &traces,
            300.0,
            LOAD,
            Controls {
                max_eos_evaluations: count,
                ..Controls::default()
            }
        )
        .unwrap()
    );
    for (controls, limit) in [
        (
            Controls {
                max_eos_evaluations: count - 1,
                ..Controls::default()
            },
            WorkLimit::EosEvaluations,
        ),
        (
            Controls {
                max_eos_evaluations: 0,
                ..Controls::default()
            },
            WorkLimit::EosEvaluations,
        ),
        (
            Controls {
                max_steps_per_trial: 0,
                ..Controls::default()
            },
            WorkLimit::Integration,
        ),
        (
            Controls {
                max_shooting_iterations: 0,
                ..Controls::default()
            },
            WorkLimit::Shooting,
        ),
        (
            Controls {
                max_bracket_expansions: 0,
                ..Controls::default()
            },
            WorkLimit::Bracketing,
        ),
    ] {
        let result = solve(&host, &traces, 300.0, LOAD, controls);
        assert_eq!(result, Err(SolveError::WorkBudgetExhausted(limit)));
        assert_eq!(result, solve(&host, &traces, 300.0, LOAD, controls));
    }
    // Initial compressed-water validation also debits nested Helmholtz probes.
    assert_eq!(
        solve(
            &host,
            &traces,
            300.0,
            100e6,
            Controls {
                max_eos_evaluations: 1,
                ..Controls::default()
            }
        ),
        Err(SolveError::WorkBudgetExhausted(WorkLimit::EosEvaluations))
    );
}

#[test]
fn invalid_unsupported_and_unrepresentable_requests_never_return_partial_bodies() {
    let host = host();
    let empty = Inventory::default();
    assert!(matches!(
        solve(&empty, &empty, 300.0, LOAD, Controls::default()),
        Err(SolveError::InvalidInput(_))
    ));
    let dry = Inventory::new(&[(Material::Metal, EARTH_MASS_KG)]).unwrap();
    assert!(matches!(
        solve(&dry, &empty, 300.0, LOAD, Controls::default()),
        Err(SolveError::InvalidInput(_))
    ));
    assert!(matches!(
        solve(
            &host,
            &mixed(&host, false).scaled(1.000001).unwrap(),
            300.0,
            LOAD,
            Controls::default()
        ),
        Err(SolveError::InvalidInput(_))
    ));
    for origin in Material::ALL {
        let tiny = Inventory::new(&[(origin, 1e-100)]).unwrap();
        if !SECONDARY.contains(&origin) {
            assert_eq!(
                solve(&host, &tiny, 300.0, LOAD, Controls::default()),
                Err(SolveError::UnsupportedMaterial(origin))
            );
        }
        if !matches!(
            origin,
            Material::Metal | Material::Silicate | Material::Water
        ) {
            assert_eq!(
                solve(
                    &host.checked_add(&tiny).unwrap(),
                    &empty,
                    300.0,
                    LOAD,
                    Controls::default()
                ),
                Err(SolveError::UnsupportedMaterial(origin))
            );
        }
    }
    for mass in [0.000099, 20.001] {
        assert!(matches!(
            solve(
                &host.scaled(mass).unwrap(),
                &empty,
                300.0,
                LOAD,
                Controls::default()
            ),
            Err(SolveError::OutsideMassDomain(_))
        ));
    }
    for (t, load) in [
        (f64::NAN, LOAD),
        (f64::INFINITY, LOAD),
        (19.0, LOAD),
        (300.0, -1.0),
        (300.0, 999.0),
        (300.0, f64::NAN),
        (300.0, f64::INFINITY),
        (300.0, 1e13 + 1.0),
        (2001.0, LOAD),
        (300.0, 1e13),
    ] {
        assert!(
            solve(&host, &empty, t, load, Controls::default()).is_err(),
            "T={t} P={load}"
        );
    }
    assert!(matches!(
        solve(
            &host,
            &empty,
            300.0,
            LOAD,
            Controls {
                max_step: f64::NAN,
                ..Controls::default()
            }
        ),
        Err(SolveError::InvalidInput(_))
    ));
    let tiny = Inventory::new(&[(Material::Oxygen, f64::from_bits(1))]).unwrap();
    assert!(matches!(
        solve(&host, &tiny, 250.0, LOAD, Controls::default()),
        Err(SolveError::InvalidClosure {
            material: Material::Oxygen,
            ..
        })
    ));
    let unresolved_host =
        Inventory::new(&[(Material::Metal, EARTH_MASS_KG), (Material::Water, 1.0)]).unwrap();
    assert!(matches!(
        solve(&unresolved_host, &empty, 250.0, LOAD, Controls::default()),
        Err(SolveError::InvalidClosure { .. })
    ));
}
