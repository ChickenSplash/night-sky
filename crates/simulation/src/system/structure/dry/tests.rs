use super::*;

fn inventory(mass_earth: f64, metal: f64) -> Inventory {
    Inventory::from_mass_fractions(
        mass_earth * EARTH_MASS_KG,
        &[(Material::Metal, metal), (Material::Silicate, 1.0 - metal)],
    )
    .unwrap()
}

fn close(actual: f64, expected: f64, tolerance: f64) {
    assert!(
        (actual / expected - 1.0).abs() <= tolerance,
        "{actual:e} != {expected:e}, relative error {:e}",
        (actual / expected - 1.0).abs()
    );
}

fn constant_density(material: Material, _: f64) -> Result<f64, cold::ColdEosError> {
    match material {
        Material::Metal => Ok(8300.0),
        Material::Silicate => Ok(4100.0),
        _ => Err(cold::ColdEosError::UnsupportedMaterial(material)),
    }
}

#[test]
fn uniform_spheres_match_analytic_radius_pressure_and_inertia() {
    for mass in [0.0001, 1.0, 20.0] {
        for metal in [0.0, 1.0] {
            for load in [0.0, 1e8] {
                let inv = inventory(mass, metal);
                let body =
                    solve_with_eos(&inv, load, Controls::default(), constant_density).unwrap();
                let rho = if metal == 1.0 { 8300.0 } else { 4100.0 };
                let m = inv.total_mass_kg();
                let r = (3.0 * m / (4.0 * PI * rho)).cbrt();
                let pressure_increment =
                    3.0 * GRAVITATIONAL_CONSTANT * m.powi(2) / (8.0 * PI * r.powi(4));
                close(body.radius_m(), r, 1e-6);
                close(
                    body.diagnostics.central_pressure_pa - load,
                    pressure_increment,
                    1e-6,
                );
                close(body.inertia_factor(), 0.4, 1e-6);
                close(
                    body.reference_gravity_m_s2(),
                    GRAVITATIONAL_CONSTANT * m / r.powi(2),
                    1e-6,
                );
                assert_eq!(body.regions.len(), 1);
            }
        }
    }
}

#[test]
fn differentiated_constant_density_sphere_matches_independent_solution() {
    let inv = inventory(1.0, 0.325);
    let load = 1e7;
    let body = solve_with_eos(&inv, load, Controls::default(), constant_density).unwrap();
    let rho_c: f64 = 8300.0;
    let rho_m: f64 = 4100.0;
    let core_mass = inv.mass_kg(Material::Metal);
    let a = (3.0 * core_mass / (4.0 * PI * rho_c)).cbrt();
    let r = (a.powi(3) + 3.0 * inv.mass_kg(Material::Silicate) / (4.0 * PI * rho_m)).cbrt();
    let excess = core_mass - 4.0 * PI / 3.0 * rho_m * a.powi(3);
    let interface_p = load
        + GRAVITATIONAL_CONSTANT
            * rho_m
            * (excess * (1.0 / a - 1.0 / r) + 2.0 * PI / 3.0 * rho_m * (r.powi(2) - a.powi(2)));
    let central_p =
        interface_p + 2.0 * PI / 3.0 * GRAVITATIONAL_CONSTANT * rho_c.powi(2) * a.powi(2);
    let inertia = 8.0 * PI / 15.0 * (rho_c * a.powi(5) + rho_m * (r.powi(5) - a.powi(5)));
    close(body.radius_m(), r, 1e-6);
    close(body.regions[0].outer().radius_m, a, 1e-6);
    close(body.regions[0].outer().pressure_pa, interface_p, 1e-6);
    close(body.diagnostics.central_pressure_pa, central_p, 1e-6);
    close(body.inertia_kg_m2, inertia, 1e-6);
}

// Independent radial quadrature, not a sum of solver mass coordinates or I.
// Cubic Hermite pressure interpolation uses physical dP/dr at saved endpoints;
// eight-point Gaussian integration evaluates the EOS at new radial locations.
pub(super) fn quadrature(
    region: &Region,
    eos: impl Fn(Material, f64) -> Result<f64, cold::ColdEosError>,
) -> (f64, f64, f64) {
    const NODES: [f64; 4] = [
        0.1834346424956498,
        0.525532409916329,
        0.7966664774136267,
        0.9602898564975363,
    ];
    const WEIGHTS: [f64; 4] = [
        0.362683783378362,
        0.3137066458778873,
        0.2223810344533745,
        0.1012285362903763,
    ];
    let material = if region.inventory.mass_kg(Material::Metal) > 0.0 {
        Material::Metal
    } else {
        Material::Silicate
    };
    let mut mass = 0.0;
    let mut inertia = 0.0;
    let mut pressure_drop = 0.0;
    for pair in region.samples.windows(2) {
        let [a, b] = [pair[0], pair[1]];
        let dr = b.radius_m - a.radius_m;
        let gradient = |s: Sample| {
            if s.radius_m == 0.0 {
                0.0
            } else {
                -GRAVITATIONAL_CONSTANT * s.enclosed_mass_kg * s.density_kg_m3 / s.radius_m.powi(2)
            }
        };
        for (node, weight) in NODES.into_iter().zip(WEIGHTS) {
            for sign in [-1.0, 1.0] {
                let t = (1.0 + sign * node) * 0.5;
                let r = a.radius_m + t * dr;
                let p = (2.0 * t.powi(3) - 3.0 * t.powi(2) + 1.0) * a.pressure_pa
                    + (t.powi(3) - 2.0 * t.powi(2) + t) * dr * gradient(a)
                    + (-2.0 * t.powi(3) + 3.0 * t.powi(2)) * b.pressure_pa
                    + (t.powi(3) - t.powi(2)) * dr * gradient(b);
                let rho = eos(material, p).unwrap();
                let w = weight * dr * 0.5;
                mass += w * 4.0 * PI * r * r * rho;
                inertia += w * 8.0 * PI / 3.0 * r.powi(4) * rho;
                let m = (2.0 * t.powi(3) - 3.0 * t.powi(2) + 1.0) * a.enclosed_mass_kg
                    + (t.powi(3) - 2.0 * t.powi(2) + t)
                        * dr
                        * 4.0
                        * PI
                        * a.radius_m.powi(2)
                        * a.density_kg_m3
                    + (-2.0 * t.powi(3) + 3.0 * t.powi(2)) * b.enclosed_mass_kg
                    + (t.powi(3) - t.powi(2))
                        * dr
                        * 4.0
                        * PI
                        * b.radius_m.powi(2)
                        * b.density_kg_m3;
                pressure_drop += w * GRAVITATIONAL_CONSTANT * m * rho / (r * r);
            }
        }
    }
    (mass, inertia, pressure_drop)
}

#[test]
fn cold_profiles_close_inventory_interfaces_and_independent_radial_integrals() {
    for mass in [0.0001, 0.0553, 1.0, 5.0, 20.0] {
        for metal in [0.0, 1e-6, 0.325, 0.7, 0.999999, 1.0] {
            let inv = inventory(mass, metal);
            if mass == 20.0 && metal >= 0.7 {
                assert!(matches!(
                    solve(&inv, 0.0, Controls::default()),
                    Err(SolveError::Unbracketed { .. })
                ));
                continue;
            }
            let body = solve(&inv, 0.0, Controls::default())
                .unwrap_or_else(|e| panic!("mass {mass} metal {metal}: {e}"));
            assert_eq!(body.retained_inventory(), inv);
            assert!(body.diagnostics.boundary_residual_pa >= 0.0);
            assert!(
                body.diagnostics.boundary_residual_pa
                    <= 1.0_f64.max(1e-8 * body.diagnostics.central_pressure_pa)
            );
            assert!(body.inertia_factor() > 0.0 && body.inertia_factor() < 0.401);
            let mut total_i = 0.0;
            for region in &body.regions {
                let (m, i, dp) = quadrature(region, cold::density_kg_m3);
                close(m, region.inventory.total_mass_kg(), 1e-7);
                close(
                    dp,
                    region.inner().pressure_pa - region.outer().pressure_pa,
                    2e-4,
                );
                total_i += i;
                assert_eq!(region.phase(), PhaseKnowledge::Unresolved);
                assert_eq!(region.temperature_k(), None);
            }
            close(total_i, body.inertia_kg_m2, 1e-7);
            if body.regions.len() == 2 {
                let a = body.regions[0].outer();
                let b = body.regions[1].inner();
                assert_eq!(a.radius_m, b.radius_m);
                assert_eq!(a.enclosed_mass_kg, b.enclosed_mass_kg);
                assert_eq!(a.pressure_pa, b.pressure_pa);
                assert!(a.density_kg_m3 > b.density_kg_m3);
            }
        }
    }
}

#[test]
fn cold_structure_converges_under_accuracy_and_step_refinement() {
    for mass in [0.0001, 1.0, 20.0] {
        for metal in [0.0, 0.325, 1.0] {
            if mass == 20.0 && metal == 1.0 {
                continue;
            }
            let inv = inventory(mass, metal);
            let coarse = solve(&inv, 0.0, Controls::default())
                .unwrap_or_else(|e| panic!("mass {mass} metal {metal}: {e}"));
            let fine = solve(
                &inv,
                0.0,
                Controls {
                    relative_tolerance: 2.5e-11,
                    max_step: 1.0 / 512.0,
                    ..Controls::default()
                },
            )
            .unwrap();
            close(fine.radius_m(), coarse.radius_m(), 1e-5);
            close(fine.inertia_factor(), coarse.inertia_factor(), 1e-5);
            close(
                fine.diagnostics.central_pressure_pa,
                coarse.diagnostics.central_pressure_pa,
                1e-5,
            );
        }
    }
}

#[test]
fn external_load_compresses_without_adding_material_and_runs_are_deterministic() {
    let inv = inventory(1.0, 0.325);
    let bare = solve(&inv, 0.0, Controls::default()).unwrap();
    let loaded = solve(&inv, 1e10, Controls::default()).unwrap();
    assert!(loaded.radius_m() < bare.radius_m());
    assert_eq!(loaded.retained_inventory(), inv);
    assert_eq!(bare, solve(&inv, 0.0, Controls::default()).unwrap());
    let metal = solve(&inventory(1.0, 1.0), 0.0, Controls::default()).unwrap();
    let rock = solve(&inventory(1.0, 0.0), 0.0, Controls::default()).unwrap();
    assert!(metal.radius_m() < bare.radius_m() && bare.radius_m() < rock.radius_m());
    for region in loaded.regions() {
        let (mass, _, _) = quadrature(region, cold::density_kg_m3);
        close(mass, region.inventory().total_mass_kg(), 1e-7);
    }
}

#[test]
fn prescribed_dry_reference_profiles() {
    for (name, mass, metal) in [
        ("Mercury-like", 0.0553, 0.7),
        ("Earth-like", 1.0, 0.325),
        ("Super-Earth", 5.0, 0.325),
    ] {
        let body = solve(&inventory(mass, metal), 0.0, Controls::default()).unwrap();
        let work = body.diagnostics().work;
        println!(
            "{name}: R={:.3} km, Pc={:.3} bar, I/MR²={:.8}, EOS evaluations={}",
            body.radius_m() / 1000.0,
            body.diagnostics().central_pressure_pa / 1e5,
            body.inertia_factor(),
            work.eos_evaluations
        );
        assert!(work.bracket_expansions <= 64);
        assert!(work.shooting_iterations <= 128);
        assert!(work.eos_evaluations <= 2_000_000);
        assert_eq!(body.model_id(), MODEL_ID);
    }
}

#[test]
fn eos_coefficients_and_domains_are_explicit() {
    assert_eq!(cold::density_kg_m3(Material::Metal, 0.0).unwrap(), 8300.0);
    assert_eq!(
        cold::density_kg_m3(Material::Silicate, 0.0).unwrap(),
        4100.0
    );
    close(
        cold::density_kg_m3(Material::Metal, 1e12).unwrap(),
        8300.0 + 0.00349 * 10.0_f64.powf(12.0 * 0.528),
        1e-14,
    );
    close(
        cold::density_kg_m3(Material::Silicate, 1e12).unwrap(),
        4100.0 + 0.00161 * 10.0_f64.powf(12.0 * 0.541),
        1e-14,
    );
    for p in [-1.0, f64::NAN, f64::INFINITY, 1e13 + 1.0] {
        assert!(cold::density_kg_m3(Material::Metal, p).is_err());
    }
    assert!(cold::density_kg_m3(Material::Metal, 1e13).is_ok());
    for material in Material::ALL {
        assert_eq!(Material::from_id(material.id()), Some(material));
        if matches!(material, Material::Metal | Material::Silicate) {
            continue;
        }
        let inv =
            Inventory::new(&[(Material::Silicate, EARTH_MASS_KG), (material, 1e-100)]).unwrap();
        assert_eq!(
            solve(&inv, 0.0, Controls::default()),
            Err(SolveError::UnsupportedMaterial(material))
        );
    }
}

#[test]
fn invalid_inputs_and_unrepresentable_regions_never_become_partial_planets() {
    for mass in [0.0, 0.000099, 20.001] {
        assert!(matches!(
            solve(
                &Inventory::new(&[(Material::Metal, mass * EARTH_MASS_KG)]).unwrap(),
                0.0,
                Controls::default()
            ),
            Err(SolveError::OutsideMassDomain(_))
        ));
    }
    let inv = inventory(1.0, 0.325);
    for p in [-1.0, f64::NAN, f64::INFINITY, 1e13 + 1.0] {
        assert!(matches!(
            solve(&inv, p, Controls::default()),
            Err(SolveError::InvalidInput(_))
        ));
    }
    assert!(matches!(
        solve(&inv, 1e13, Controls::default()),
        Err(SolveError::Unbracketed { .. })
    ));
    for controls in [
        Controls {
            relative_tolerance: 0.0,
            ..Controls::default()
        },
        Controls {
            max_step: f64::NAN,
            ..Controls::default()
        },
        Controls {
            max_eos_evaluations: 2_000_001,
            ..Controls::default()
        },
    ] {
        assert!(matches!(
            solve(&inv, 0.0, controls),
            Err(SolveError::InvalidInput(_))
        ));
    }
    let trace =
        Inventory::new(&[(Material::Metal, EARTH_MASS_KG), (Material::Silicate, 1.0)]).unwrap();
    assert!(matches!(
        solve(&trace, 0.0, Controls::default()),
        Err(SolveError::InvalidClosure {
            material: Material::Silicate,
            ..
        })
    ));
}

#[test]
fn every_work_budget_fails_explicitly() {
    let inv = inventory(1.0, 1.0);
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
        assert_eq!(
            solve(&inv, 0.0, controls),
            Err(SolveError::WorkBudgetExhausted(limit))
        );
    }
}
