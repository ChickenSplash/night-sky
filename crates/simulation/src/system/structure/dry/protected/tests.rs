use super::super::{PhaseKnowledge, WorkLimit};
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
    Inventory::new(&[
        (Material::Metal, 0.3 * EARTH_MASS_KG),
        (Material::Silicate, 0.7 * EARTH_MASS_KG),
    ])
    .unwrap()
}

fn mixed(reverse: bool) -> Inventory {
    let trace_mass = MAX_TRACE_FRACTION * host().total_mass_kg() / (1.0 - MAX_TRACE_FRACTION);
    let mut parts = vec![
        (Material::CarbonDioxide, 0.49 * trace_mass),
        (Material::Nitrogen, 0.49 * trace_mass),
        (Material::Methane, 0.01 * trace_mass),
        (Material::Argon, 0.01 * trace_mass),
    ];
    if reverse {
        parts.reverse();
    }
    Inventory::new(&parts).unwrap()
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

// Uses the independent Gaussian/Hermite radial quadrature of the dry suite,
// evaluating the parcel EOS anew, not summing solver mass coordinates or inertia.
fn check_profile(
    body: &DryStructure,
    host: &Inventory,
    traces: &Inventory,
    lambda: f64,
) -> [f64; 3] {
    assert_eq!(body.model_id(), MODEL_ID);
    assert_eq!(body.host_density_model_id(), cold::MODEL_ID);
    let requested = host.checked_add(traces).unwrap();
    close(body.mass_kg(), requested.total_mass_kg(), 1e-14);
    let allocated = body.retained_inventory();
    for origin in Material::ALL {
        close(allocated.mass_kg(origin), requested.mass_kg(origin), 1e-12);
    }
    let f = traces.total_mass_kg() / requested.total_mass_kg();
    let volume_factor = 1.0 - f + f / lambda;
    let mut total_mass = 0.0;
    let mut total_inertia = 0.0;
    let mut residuals = [0.0_f64; 3];
    for region in body.regions() {
        let material = if region.inventory().mass_kg(Material::Metal) > 0.0 {
            Material::Metal
        } else {
            Material::Silicate
        };
        let weight = host.mass_kg(material) / host.total_mass_kg();
        close(
            region.inventory().mass_kg(material),
            host.mass_kg(material),
            1e-14,
        );
        for origin in SECONDARY {
            close(
                region.inventory().mass_kg(origin),
                traces.mass_kg(origin) * weight,
                1e-12,
            );
            if traces.mass_kg(origin) > 0.0 {
                assert!(region.inventory().mass_kg(origin) > 0.0);
            }
        }
        let (mass, inertia, dp) = super::super::tests::quadrature(region, |m, p| {
            cold::density_kg_m3(m, p).map(|rho| rho / volume_factor)
        });
        close(mass, region.inventory().total_mass_kg(), 1e-8);
        close(
            dp,
            region.inner().pressure_pa - region.outer().pressure_pa,
            2e-4,
        );
        residuals[0] = residuals[0].max(error(mass, region.inventory().total_mass_kg()));
        residuals[1] = residuals[1].max(error(
            dp,
            region.inner().pressure_pa - region.outer().pressure_pa,
        ));
        for origin in Material::ALL {
            let fraction = region.inventory().mass_fraction(origin).unwrap();
            close(mass * fraction, region.inventory().mass_kg(origin), 1e-8);
        }
        total_mass += mass;
        total_inertia += inertia;
        assert_eq!(region.phase(), PhaseKnowledge::Unresolved);
        assert_eq!(region.temperature_k(), None);
    }
    close(total_mass, body.mass_kg(), 1e-8);
    close(total_inertia, body.inertia_kg_m2(), 1e-8);
    residuals[2] = error(total_inertia, body.inertia_kg_m2());
    close(
        body.reference_gravity_m_s2(),
        GRAVITATIONAL_CONSTANT * total_mass / body.radius_m().powi(2),
        1e-8,
    );
    for pair in body.regions().windows(2) {
        let (a, b) = (pair[0].outer(), pair[1].inner());
        assert_eq!(a.radius_m, b.radius_m);
        assert_eq!(a.enclosed_mass_kg, b.enclosed_mass_kg);
        assert_eq!(a.pressure_pa, b.pressure_pa);
        assert!(a.density_kg_m3 > b.density_kg_m3);
    }
    let diagnostics = body.diagnostics();
    assert!(diagnostics.boundary_residual_pa >= 0.0);
    assert!(
        diagnostics.boundary_residual_pa <= 1.0_f64.max(1e-8 * diagnostics.central_pressure_pa)
    );
    residuals
}

#[test]
fn pure_host_recovers_dry_mechanics_and_origins() {
    let host = host();
    let body = solve(&host, &Inventory::default(), LOAD, Controls::default()).unwrap();
    let dry = super::super::solve(&host, LOAD, Controls::default()).unwrap();
    assert_eq!(body.regions(), dry.regions());
    assert_eq!(body.mass_kg(), dry.mass_kg());
    assert_eq!(body.inertia_kg_m2(), dry.inertia_kg_m2());
    assert_eq!(body.diagnostics(), dry.diagnostics());
    check_profile(&body, &host, &Inventory::default(), 1.0);
}

#[test]
fn fixed_trace_grid_closes_and_refines_against_scaled_dry_reference() {
    let host = host();
    let mut fixtures = vec![Inventory::default()];
    for material in SECONDARY {
        for ratio in [1e-12, 1e-7, 0.003] {
            fixtures.push(Inventory::new(&[(material, ratio * host.total_mass_kg())]).unwrap());
        }
    }
    fixtures.push(mixed(false));
    let mut max_radius_error = 0.0_f64;
    let mut max_inertia_error = 0.0_f64;
    let mut max_eos = 0;
    let mut quadrature_residuals = [0.0_f64; 3];
    for traces in fixtures {
        let body = solve(&host, &traces, LOAD, Controls::default()).unwrap();
        for (max, residual) in quadrature_residuals
            .iter_mut()
            .zip(check_profile(&body, &host, &traces, 1.0))
        {
            *max = max.max(residual);
        }
        let fine = solve(
            &host,
            &traces,
            LOAD,
            Controls {
                relative_tolerance: 2.5e-11,
                max_step: 1.0 / 512.0,
                ..Controls::default()
            },
        )
        .unwrap();
        check_profile(&fine, &host, &traces, 1.0);
        max_radius_error = max_radius_error.max(error(fine.radius_m(), body.radius_m()));
        max_inertia_error =
            max_inertia_error.max(error(fine.inertia_factor(), body.inertia_factor()));
        close(fine.radius_m(), body.radius_m(), 1e-5);
        close(fine.inertia_factor(), body.inertia_factor(), 1e-5);
        let parcels = host
            .scaled((host.total_mass_kg() + traces.total_mass_kg()) / host.total_mass_kg())
            .unwrap();
        let dry = super::super::solve(&parcels, LOAD, Controls::default()).unwrap();
        // Summing origins versus host+trace totals can differ by one ulp.
        close(body.radius_m(), dry.radius_m(), 1e-12);
        close(body.inertia_kg_m2(), dry.inertia_kg_m2(), 1e-12);
        close(
            body.diagnostics().central_pressure_pa,
            dry.diagnostics().central_pressure_pa,
            1e-12,
        );
        for (a, b) in body.regions().iter().zip(dry.regions()) {
            close(a.outer().radius_m, b.outer().radius_m, 1e-12);
            close(
                a.outer().enclosed_mass_kg,
                b.outer().enclosed_mass_kg,
                1e-14,
            );
        }
        max_eos = max_eos.max(fine.diagnostics().work.eos_evaluations);
    }
    println!(
        "6a 17 fixtures: max refined R residual={max_radius_error:e}, I/MR² residual={max_inertia_error:e}, fine EOS calls={max_eos}"
    );
    println!(
        "6a maximum independent quadrature residuals: region mass={:.3e}, pressure drop={:.3e}, total inertia={:.3e}",
        quadrature_residuals[0], quadrature_residuals[1], quadrature_residuals[2]
    );
}

#[test]
fn mixed_endpoint_is_order_independent_and_repeatable() {
    let a = solve(&host(), &mixed(false), LOAD, Controls::default()).unwrap();
    let b = solve(&host(), &mixed(true), LOAD, Controls::default()).unwrap();
    assert_eq!(a, b);
    assert_eq!(
        a,
        solve(&host(), &mixed(false), LOAD, Controls::default()).unwrap()
    );
    assert!(error(a.mass_kg(), host().total_mass_kg()) > 0.003);
}

#[test]
fn density_ratio_sensitivity_stays_within_frozen_gates() {
    let nominal = solve(&host(), &mixed(false), LOAD, Controls::default()).unwrap();
    for lambda in [0.5, 1.0, 2.0] {
        let body =
            solve_with_density_ratio(&host(), &mixed(false), LOAD, Controls::default(), lambda)
                .unwrap();
        check_profile(&body, &host(), &mixed(false), lambda);
        let dr = error(body.radius_m(), nominal.radius_m());
        let di = error(body.inertia_factor(), nominal.inertia_factor());
        assert!(dr <= 0.01);
        assert!(di <= 0.02);
        println!(
            "6a lambda={lambda}: R={:.6} km, I/MR²={:.10}, relative R change={dr:e}, inertia-factor change={di:e}",
            body.radius_m() / 1000.0,
            body.inertia_factor()
        );
    }
}

#[test]
fn unsupported_invalid_and_pressure_exiting_requests_fail_without_results() {
    let host = host();
    let empty = Inventory::default();
    assert!(matches!(
        solve(&empty, &mixed(false), LOAD, Controls::default()),
        Err(SolveError::InvalidInput(_))
    ));
    let excess = mixed(false).scaled(1.000001).unwrap();
    assert!(matches!(
        solve(&host, &excess, LOAD, Controls::default()),
        Err(SolveError::InvalidInput(_))
    ));
    for material in Material::ALL {
        let tiny = Inventory::new(&[(material, 1e-100)]).unwrap();
        if !SECONDARY.contains(&material) {
            assert_eq!(
                solve(&host, &tiny, LOAD, Controls::default()),
                Err(SolveError::UnsupportedMaterial(material))
            );
        }
        if !matches!(material, Material::Metal | Material::Silicate) {
            let invalid_host = host.checked_add(&tiny).unwrap();
            assert_eq!(
                solve(&invalid_host, &empty, LOAD, Controls::default()),
                Err(SolveError::UnsupportedMaterial(material))
            );
        }
    }
    for pressure in [-1.0, f64::NAN, f64::INFINITY, 1e13 + 1.0] {
        assert!(matches!(
            solve(&host, &empty, pressure, Controls::default()),
            Err(SolveError::InvalidInput(_))
        ));
    }
    assert!(matches!(
        solve(&host, &mixed(false), 1e13, Controls::default()),
        Err(SolveError::Unbracketed { .. })
    ));
    for mass in [0.000099, 20.001] {
        assert!(matches!(
            solve(
                &host.scaled(mass).unwrap(),
                &empty,
                LOAD,
                Controls::default()
            ),
            Err(SolveError::OutsideMassDomain(_))
        ));
    }
    assert!(matches!(
        solve(
            &host,
            &empty,
            LOAD,
            Controls {
                max_step: f64::NAN,
                ..Controls::default()
            }
        ),
        Err(SolveError::InvalidInput(_))
    ));
    assert!(Inventory::new(&[(Material::Oxygen, -1.0)]).is_err());
    assert!(Inventory::new(&[(Material::Oxygen, f64::NAN)]).is_err());
}

#[test]
fn trace_path_preserves_all_work_refusals() {
    let host = Inventory::new(&[(Material::Metal, EARTH_MASS_KG)]).unwrap();
    for (controls, limit) in [
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
        let first = solve(&host, &mixed(false), LOAD, controls);
        assert_eq!(first, Err(SolveError::WorkBudgetExhausted(limit)));
        assert_eq!(first, solve(&host, &mixed(false), LOAD, controls));
    }
}

#[test]
fn pure_hosts_mass_edges_and_unrepresentable_allocations_are_explicit() {
    for material in [Material::Metal, Material::Silicate] {
        let host = Inventory::new(&[(material, EARTH_MASS_KG)]).unwrap();
        let traces = mixed(false);
        let body = solve(&host, &traces, LOAD, Controls::default()).unwrap();
        assert_eq!(body.regions().len(), 1);
        check_profile(&body, &host, &traces, 1.0);
    }
    for total in [0.0001, 20.0] {
        let traces = mixed(false)
            .scaled(total * (1.0 - MAX_TRACE_FRACTION))
            .unwrap();
        let host = host().scaled(total * (1.0 - MAX_TRACE_FRACTION)).unwrap();
        let body = solve(&host, &traces, LOAD, Controls::default()).unwrap();
        check_profile(&body, &host, &traces, 1.0);
    }
    let tiny = Inventory::new(&[(Material::Oxygen, f64::from_bits(1))]).unwrap();
    assert!(matches!(
        solve(&host(), &tiny, LOAD, Controls::default()),
        Err(SolveError::InvalidClosure {
            material: Material::Oxygen,
            quantity: "unrepresentable trace allocation"
        })
    ));
    let unrepresentable_host =
        Inventory::new(&[(Material::Metal, EARTH_MASS_KG), (Material::Silicate, 1.0)]).unwrap();
    assert!(matches!(
        solve(
            &unrepresentable_host,
            &mixed(false),
            LOAD,
            Controls::default()
        ),
        Err(SolveError::InvalidClosure { .. })
    ));
}
