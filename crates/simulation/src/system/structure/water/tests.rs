use super::*;

fn acceptance_inventory() -> Inventory {
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

#[test]
fn warm_water_rich_acceptance_conserves_interfaces_and_independent_closure() {
    let inv = acceptance_inventory();
    let body = solve(&inv, 300.0, 1e5, Controls::default()).unwrap();
    eprintln!(
        "warm water-rich: radius={} km, Pc={} GPa, inertia={}, work={:?}",
        body.radius_m() / 1000.0,
        body.diagnostics().central_pressure_pa / 1e9,
        body.inertia_factor(),
        body.diagnostics().work
    );
    close(
        body.retained_inventory().mass_kg(Material::Water),
        inv.mass_kg(Material::Water),
        1e-15,
    );
    assert_eq!(body.regions().len(), 5);
    assert_eq!(
        body.regions()[2].phase(),
        PhaseKnowledge::EffectiveHighPressureIce
    );
    assert_eq!(
        body.regions()[2].density_model_id(),
        eos::deep_ice::MODEL_ID
    );
    assert_eq!(
        body.regions()[3].density_model_id(),
        eos::dense_liquid::MODEL_ID
    );
    assert_eq!(
        body.regions()[4].phase(),
        PhaseKnowledge::Water(eos::Phase::Liquid)
    );
    assert_eq!(
        body.regions()[2].outer().pressure_pa,
        eos::deep_ice::boundary_pressure_pa(300.0).unwrap()
    );
    assert!(body.regions()[2].outer().density_kg_m3 > body.regions()[3].inner().density_kg_m3);
    let mut inertia = 0.0;
    for r in body.regions() {
        let (m, i, dp) = quadrature(r);
        close(m, r.inventory().total_mass_kg(), 1e-8);
        close(dp, r.inner().pressure_pa - r.outer().pressure_pa, 2e-4);
        inertia += i;
    }
    close(inertia, body.inertia_kg_m2(), 1e-8);
    assert_eq!(body, solve(&inv, 300.0, 1e5, Controls::default()).unwrap());
}

#[test]
fn deep_water_refines_loads_and_accounts_for_exact_work() {
    let inv = acceptance_inventory();
    let body = solve(&inv, 300.0, 1e5, Controls::default()).unwrap();
    let fine = solve(
        &inv,
        300.0,
        1e5,
        Controls {
            relative_tolerance: 2.5e-11,
            max_step: 1.0 / 512.0,
            ..Controls::default()
        },
    )
    .unwrap();
    close(body.radius_m(), fine.radius_m(), 1e-7);
    close(body.inertia_factor(), fine.inertia_factor(), 1e-7);
    close(
        body.diagnostics().central_pressure_pa,
        fine.diagnostics().central_pressure_pa,
        1e-7,
    );
    for (a, b) in body.regions().iter().zip(fine.regions()) {
        close(
            a.inventory().total_mass_kg(),
            b.inventory().total_mass_kg(),
            1e-5,
        );
        close(
            a.outer().radius_m - a.inner().radius_m,
            b.outer().radius_m - b.inner().radius_m,
            1e-5,
        );
    }
    let melting = eos::deep_ice::boundary_pressure_pa(300.0).unwrap();
    for load in [1e7, 100e6, melting, 2e9] {
        let loaded = solve(&inv, 300.0, load, Controls::default()).unwrap();
        assert!(loaded.radius_m() < body.radius_m());
        assert!(loaded.diagnostics().central_pressure_pa > body.diagnostics().central_pressure_pa);
        close(
            loaded.retained_inventory().mass_kg(Material::Water),
            inv.mass_kg(Material::Water),
            1e-15,
        );
        if load >= melting {
            assert_eq!(loaded.regions().len(), 3);
            assert_eq!(
                loaded.regions().last().unwrap().phase(),
                PhaseKnowledge::EffectiveHighPressureIce
            );
        }
    }
    let count = body.diagnostics().work.eos_evaluations;
    assert_eq!(
        body,
        solve(
            &inv,
            300.0,
            1e5,
            Controls {
                max_eos_evaluations: count,
                ..Controls::default()
            }
        )
        .unwrap()
    );
    assert_eq!(
        solve(
            &inv,
            300.0,
            1e5,
            Controls {
                max_eos_evaluations: count - 1,
                ..Controls::default()
            }
        ),
        Err(SolveError::WorkBudgetExhausted(WorkLimit::EosEvaluations))
    );
}

#[test]
fn deep_water_inventory_and_mass_trends_are_plausible() {
    for t in [273.32, 280.0, 290.0] {
        let body = solve(&acceptance_inventory(), t, 1e5, Controls::default()).unwrap();
        assert_eq!(
            body.regions()[2].phase(),
            PhaseKnowledge::EffectiveHighPressureIce
        );
        assert_eq!(
            body.regions()[2].outer().pressure_pa,
            eos::deep_ice::boundary_pressure_pa(t).unwrap()
        );
    }
    let mut radius = 0.0;
    for fraction in [0.1, 0.3, 0.6, 1.0] {
        let body = solve(&inventory(1.0, fraction), 300.0, 1e5, Controls::default()).unwrap();
        assert!(body.radius_m() > radius);
        radius = body.radius_m();
    }
    let mut density = 0.0;
    for mass in [0.1, 1.0, 5.0] {
        let body = solve(&inventory(mass, 0.3), 300.0, 1e5, Controls::default()).unwrap();
        assert!(body.mean_density_kg_m3() > density);
        density = body.mean_density_kg_m3();
    }
}

#[test]
fn cold_water_rich_acceptance_has_labelled_approximations_and_independent_closure() {
    let inv = acceptance_inventory();
    let body = solve(&inv, 250.0, 1e5, Controls::default()).unwrap();
    eprintln!(
        "cold water-rich: radius={} km, Pc={} GPa, inertia={}, work={:?}",
        body.radius_m() / 1000.0,
        body.diagnostics().central_pressure_pa / 1e9,
        body.inertia_factor(),
        body.diagnostics().work
    );
    assert_eq!(body.regions().len(), 6);
    assert_eq!(
        body.regions()[2].density_model_id(),
        eos::deep_ice::MODEL_ID
    );
    assert_eq!(
        body.regions()[3].density_model_id(),
        eos::cold_ice::INTERMEDIATE_MODEL_ID
    );
    assert_eq!(
        body.regions()[4].density_model_id(),
        eos::cold_ice::IH_MODEL_ID
    );
    assert_eq!(
        body.regions()[5].phase(),
        PhaseKnowledge::Water(eos::Phase::IceIh)
    );
    for r in &body.regions()[2..5] {
        assert!(!r.approximation_notes().is_empty());
        assert_eq!(r.temperature_k(), Some(250.0));
    }
    for m in Material::ALL {
        assert!(
            (body.retained_inventory().mass_kg(m) - inv.mass_kg(m)).abs()
                <= 1e-15 * inv.total_mass_kg()
        );
    }
    for pair in body.regions().windows(2) {
        let a = pair[0].outer();
        let b = pair[1].inner();
        assert_eq!(
            (a.radius_m, a.enclosed_mass_kg, a.pressure_pa),
            (b.radius_m, b.enclosed_mass_kg, b.pressure_pa)
        );
    }
    for index in [2, 3] {
        assert!(
            body.regions()[index].outer().density_kg_m3
                > body.regions()[index + 1].inner().density_kg_m3
        );
    }
    let mut inertia = 0.0;
    for r in body.regions() {
        let (m, i, dp) = quadrature(r);
        close(m, r.inventory().total_mass_kg(), 1e-8);
        close(dp, r.inner().pressure_pa - r.outer().pressure_pa, 2e-4);
        inertia += i;
    }
    close(inertia, body.inertia_kg_m2(), 1e-8);
    assert_eq!(body, solve(&inv, 250.0, 1e5, Controls::default()).unwrap());
    // Original local IAPWS domains remain unchanged.
    assert!(eos::state(220e6, 250.0).is_err());
}

#[test]
fn cold_deep_water_refines_loads_and_preserves_work_limits() {
    let inv = acceptance_inventory();
    let coarse = solve(&inv, 250.0, 1e5, Controls::default()).unwrap();
    let fine = solve(
        &inv,
        250.0,
        1e5,
        Controls {
            relative_tolerance: 2.5e-11,
            max_step: 1.0 / 512.0,
            ..Controls::default()
        },
    )
    .unwrap();
    close(coarse.radius_m(), fine.radius_m(), 1e-7);
    close(coarse.inertia_factor(), fine.inertia_factor(), 1e-7);
    close(
        coarse.diagnostics().central_pressure_pa,
        fine.diagnostics().central_pressure_pa,
        1e-7,
    );
    for (a, b) in coarse.regions().iter().zip(fine.regions()) {
        close(
            a.inventory().total_mass_kg(),
            b.inventory().total_mass_kg(),
            1e-5,
        );
        close(
            a.outer().radius_m - a.inner().radius_m,
            b.outer().radius_m - b.inner().radius_m,
            1e-5,
        );
    }
    for load in [
        1e7,
        100e6,
        eos::cold_ice::IH_BOUNDARY_PA,
        eos::cold_ice::DEEP_BOUNDARY_PA,
    ] {
        let loaded = solve(&inv, 250.0, load, Controls::default()).unwrap();
        assert!(loaded.radius_m() < coarse.radius_m());
        assert!(
            loaded.diagnostics().central_pressure_pa > coarse.diagnostics().central_pressure_pa
        );
        if load == eos::cold_ice::IH_BOUNDARY_PA {
            assert_eq!(loaded.regions().len(), 4);
        }
        if load == eos::cold_ice::DEEP_BOUNDARY_PA {
            assert_eq!(loaded.regions().len(), 3);
        }
    }
    let count = coarse.diagnostics().work.eos_evaluations;
    assert_eq!(
        coarse,
        solve(
            &inv,
            250.0,
            1e5,
            Controls {
                max_eos_evaluations: count,
                ..Controls::default()
            }
        )
        .unwrap()
    );
    assert_eq!(
        solve(
            &inv,
            250.0,
            1e5,
            Controls {
                max_eos_evaluations: count - 1,
                ..Controls::default()
            }
        ),
        Err(SolveError::WorkBudgetExhausted(WorkLimit::EosEvaluations))
    );
    for temperature in [250.5, 251.165] {
        assert!(solve(&inv, temperature, 1e5, Controls::default()).is_ok());
    }
    for fraction in [0.1, 0.6, 1.0] {
        assert!(solve(&inventory(1.0, fraction), 250.0, 1e5, Controls::default()).is_ok());
    }
}

#[test]
fn cold_to_warm_coverage_includes_pressure_melted_liquid_and_model_seams() {
    let inv = acceptance_inventory();
    for t in [
        251.165000001,
        251.1651,
        251.166,
        251.2,
        255.0,
        256.164,
        256.165,
        260.0,
        265.0,
        270.0,
        273.14,
        273.15,
        273.155,
        273.16,
        273.31,
        273.31001,
        273.32,
    ] {
        let body =
            solve(&inv, t, 1e5, Controls::default()).unwrap_or_else(|e| panic!("{t} K: {e}"));
        let mut inertia = 0.0;
        for r in body.regions() {
            let (m, i, dp) = quadrature(r);
            close(m, r.inventory().total_mass_kg(), 1e-8);
            close(dp, r.inner().pressure_pa - r.outer().pressure_pa, 2e-4);
            inertia += i;
            if r.density_model_id() == eos::subfreezing_liquid::MODEL_ID {
                assert!(!r.approximation_notes().is_empty());
                assert_eq!(r.phase(), PhaseKnowledge::Water(eos::Phase::Liquid));
            }
        }
        close(inertia, body.inertia_kg_m2(), 1e-8);
        assert_eq!(
            body.regions()
                .iter()
                .any(|r| r.phase() == PhaseKnowledge::Water(eos::Phase::Liquid)),
            !eos::cold_ice::supports_temperature(t)
        );
        close(
            body.retained_inventory().mass_kg(Material::Water),
            inv.mass_kg(Material::Water),
            1e-15,
        );
    }
    let body = solve(&inv, 260.0, 1e5, Controls::default()).unwrap();
    let fine = solve(
        &inv,
        260.0,
        1e5,
        Controls {
            relative_tolerance: 2.5e-11,
            max_step: 1.0 / 512.0,
            ..Controls::default()
        },
    )
    .unwrap();
    close(body.radius_m(), fine.radius_m(), 1e-7);
    close(body.inertia_factor(), fine.inertia_factor(), 1e-7);
    for (a, b) in body.regions().iter().zip(fine.regions()) {
        close(
            a.inventory().total_mass_kg(),
            b.inventory().total_mass_kg(),
            1e-5,
        );
        close(
            a.outer().radius_m - a.inner().radius_m,
            b.outer().radius_m - b.inner().radius_m,
            1e-5,
        );
    }
    // Local research-grade API still refuses unavailable subfreezing density;
    // only the named approximate radial prescription fills that coverage.
    assert!(eos::state(90e6, 270.0).is_err());
}

#[test]
fn approximate_liquid_boundary_loads_and_work_remain_explicit() {
    let inv = acceptance_inventory();
    for t in [260.0, 270.0, 273.15, 273.155] {
        let low = eos::melting_pressure_ih_pa(t).unwrap();
        let high = eos::subfreezing_liquid::high_boundary_pa(t).unwrap();
        for load in [low, 100e6, high] {
            let body = solve(&inv, t, load, Controls::default()).unwrap();
            let outer = body.regions().last().unwrap();
            assert!(outer.inventory().total_mass_kg() > 0.0);
            if load == high && t < 273.15 {
                assert_eq!(outer.phase(), PhaseKnowledge::EffectiveHighPressureIce);
            } else if load == low {
                assert_eq!(outer.phase(), PhaseKnowledge::Water(eos::Phase::Liquid));
            }
        }
    }
    let body = solve(&inv, 260.0, 1e5, Controls::default()).unwrap();
    let count = body.diagnostics().work.eos_evaluations;
    assert_eq!(
        body,
        solve(
            &inv,
            260.0,
            1e5,
            Controls {
                max_eos_evaluations: count,
                ..Controls::default()
            }
        )
        .unwrap()
    );
    assert_eq!(
        solve(
            &inv,
            260.0,
            1e5,
            Controls {
                max_eos_evaluations: count - 1,
                ..Controls::default()
            }
        ),
        Err(SolveError::WorkBudgetExhausted(WorkLimit::EosEvaluations))
    );
}

fn inventory(mass: f64, water: f64) -> Inventory {
    Inventory::from_mass_fractions(
        mass * EARTH_MASS_KG,
        &[
            (Material::Metal, (1.0 - water) * 0.325),
            (Material::Silicate, (1.0 - water) * 0.675),
            (Material::Water, water),
        ],
    )
    .unwrap()
}

fn close(actual: f64, expected: f64, tolerance: f64) {
    assert!(
        (actual / expected - 1.0).abs() <= tolerance,
        "{actual:e} != {expected:e}"
    );
}

#[test]
fn shared_kernel_matches_analytic_three_layer_constant_density_body() {
    // Synthetic EOS only checks numerical mechanics, not water-model accuracy.
    let inv = inventory(0.0001, 0.1);
    let mut radius: f64 = 0.0;
    let mut enclosed = 0.0;
    let mut pressure = 1e5;
    let mut inertia = 0.0;
    let mut interfaces = Vec::new();
    for (material, rho) in [
        (Material::Metal, 8300.0),
        (Material::Silicate, 4100.0),
        (Material::Water, 1000.0),
    ] {
        let next = (radius.powi(3) + 3.0 * inv.mass_kg(material) / (4.0 * PI * rho)).cbrt();
        let excess = enclosed - 4.0 * PI / 3.0 * rho * radius.powi(3);
        pressure +=
            2.0 * PI / 3.0 * GRAVITATIONAL_CONSTANT * rho * rho * (next * next - radius * radius)
                + if radius == 0.0 {
                    0.0
                } else {
                    GRAVITATIONAL_CONSTANT * rho * excess * (1.0 / radius - 1.0 / next)
                };
        inertia += 8.0 * PI / 15.0 * rho * (next.powi(5) - radius.powi(5));
        interfaces.push(next);
        enclosed += inv.mass_kg(material);
        radius = next;
    }
    let density = |material, _, _, _: &mut Work, _| {
        Ok(match material {
            Material::Metal => 8300.0,
            Material::Silicate => 4100.0,
            Material::Water => 1000.0,
            _ => unreachable!(),
        })
    };
    let mut kernel = radial::Solver::new(
        &inv,
        1e5,
        Controls::default(),
        &density,
        radial::Policy::default(),
    );
    let trial = kernel.trial(pressure + 1.0).unwrap().unwrap();
    for (region, expected) in trial.regions.iter().zip(interfaces) {
        close(region.outer().radius_m, expected, 1e-7);
    }
    close(trial.inertia_kg_m2, inertia, 1e-7);
    assert!((trial.regions.last().unwrap().outer().pressure_pa - 1e5 - 1.0).abs() < 0.1);
}

#[test]
fn liquid_and_ice_shells_conserve_inventory_and_one_sided_interfaces() {
    for (mass, fraction) in [(0.0001, 1.0), (0.0001, 0.01), (1.0, 1e-6)] {
        for temperature in [250.0, 300.0] {
            let inv = inventory(mass, fraction);
            let body = solve(&inv, temperature, 1e5, Controls::default())
                .unwrap_or_else(|e| panic!("{mass} {fraction} {temperature}: {e}"));
            assert_eq!(body.retained_inventory(), inv);
            assert_eq!(
                body,
                solve(&inv, temperature, 1e5, Controls::default()).unwrap()
            );
            assert!(
                (0.0..=BOUNDARY_TOLERANCE_PA).contains(&body.diagnostics().boundary_residual_pa)
            );
            assert_eq!(body.regions().len(), if fraction == 1.0 { 1 } else { 3 });
            for region in body.regions() {
                if region.inventory().mass_kg(Material::Water) > 0.0 {
                    assert_eq!(region.temperature_k(), Some(temperature));
                    let expected = if temperature == 250.0 {
                        eos::Phase::IceIh
                    } else {
                        eos::Phase::Liquid
                    };
                    assert_eq!(region.phase(), PhaseKnowledge::Water(expected));
                    for sample in region.samples() {
                        assert_eq!(
                            eos::state(sample.pressure_pa, temperature).unwrap().phase,
                            expected
                        );
                    }
                } else {
                    assert_eq!(region.temperature_k(), None);
                    assert_eq!(region.phase(), PhaseKnowledge::Unresolved);
                }
            }
            for pair in body.regions().windows(2) {
                let a = pair[0].outer();
                let b = pair[1].inner();
                assert_eq!(a.radius_m, b.radius_m);
                assert_eq!(a.enclosed_mass_kg, b.enclosed_mass_kg);
                assert_eq!(a.pressure_pa, b.pressure_pa);
                assert_ne!(a.density_kg_m3, b.density_kg_m3);
            }
        }
    }
}

// Independent Gaussian radial integrals using Hermite-interpolated pressure
// and mass, not summation of allocated masses or the kernel's inertia state.
fn quadrature(region: &Region) -> (f64, f64, f64) {
    let material = Material::ALL
        .into_iter()
        .find(|m| region.inventory().mass_kg(*m) > 0.0)
        .unwrap();
    let mut mass = 0.0;
    let mut inertia = 0.0;
    let mut pressure_drop = 0.0;
    for pair in region.samples().windows(2) {
        let [a, b] = [pair[0], pair[1]];
        let dr = b.radius_m - a.radius_m;
        let gradient = |s: Sample| {
            if s.radius_m == 0.0 {
                0.0
            } else {
                -GRAVITATIONAL_CONSTANT * s.enclosed_mass_kg * s.density_kg_m3 / s.radius_m.powi(2)
            }
        };
        for (node, weight) in [
            (0.1834346424956498, 0.362683783378362),
            (0.525532409916329, 0.3137066458778873),
            (0.7966664774136267, 0.2223810344533745),
            (0.9602898564975363, 0.1012285362903763),
        ] {
            for sign in [-1.0, 1.0] {
                let t: f64 = (1.0 + sign * node) * 0.5;
                let r = a.radius_m + t * dr;
                let h = [
                    2.0 * t.powi(3) - 3.0 * t.powi(2) + 1.0,
                    t.powi(3) - 2.0 * t.powi(2) + t,
                    -2.0 * t.powi(3) + 3.0 * t.powi(2),
                    t.powi(3) - t.powi(2),
                ];
                let p = h[0] * a.pressure_pa
                    + h[1] * dr * gradient(a)
                    + h[2] * b.pressure_pa
                    + h[3] * dr * gradient(b);
                let rho = if let Some(temperature) = region.temperature_k() {
                    if region.density_model_id() == eos::subfreezing_liquid::MODEL_ID {
                        eos::subfreezing_liquid::density_kg_m3(p, temperature).unwrap()
                    } else if matches!(
                        region.density_model_id(),
                        eos::cold_ice::INTERMEDIATE_MODEL_ID | eos::cold_ice::IH_MODEL_ID
                    ) {
                        eos::cold_ice::density_kg_m3(p, temperature, region.density_model_id())
                            .unwrap()
                    } else if region.density_model_id() == eos::deep_ice::MODEL_ID {
                        eos::deep_ice::density_kg_m3(p, temperature).unwrap()
                    } else if region.density_model_id() == eos::dense_liquid::MODEL_ID {
                        eos::dense_liquid::state(p, temperature)
                            .unwrap()
                            .density_kg_m3
                    } else {
                        eos::state(p, temperature).unwrap().density_kg_m3
                    }
                } else {
                    cold::density_kg_m3(material, p).unwrap()
                };
                let m = h[0] * a.enclosed_mass_kg
                    + h[1] * dr * 4.0 * PI * a.radius_m.powi(2) * a.density_kg_m3
                    + h[2] * b.enclosed_mass_kg
                    + h[3] * dr * 4.0 * PI * b.radius_m.powi(2) * b.density_kg_m3;
                let w = weight * dr * 0.5;
                mass += w * 4.0 * PI * r * r * rho;
                inertia += w * 8.0 * PI / 3.0 * r.powi(4) * rho;
                pressure_drop += w * GRAVITATIONAL_CONSTANT * m * rho / (r * r);
            }
        }
    }
    (mass, inertia, pressure_drop)
}

#[test]
fn independent_radial_integrals_close_shell_mass_inertia_and_hydrostatics() {
    for (mass, fraction) in [(0.0001, 1.0), (0.0001, 0.01), (1.0, 1e-6)] {
        for temperature in [250.0, 300.0] {
            let body = solve(
                &inventory(mass, fraction),
                temperature,
                1e5,
                Controls::default(),
            )
            .unwrap();
            let mut inertia = 0.0;
            for region in body.regions() {
                let (m, i, dp) = quadrature(region);
                close(m, region.inventory().total_mass_kg(), 1e-8);
                close(
                    dp,
                    region.inner().pressure_pa - region.outer().pressure_pa,
                    2e-4,
                );
                inertia += i;
            }
            close(inertia, body.inertia_kg_m2(), 1e-8);
        }
    }
}

#[test]
fn loading_and_refinement_preserve_shallow_shells() {
    for (mass, fraction) in [(0.0001, 1.0), (0.0001, 0.01), (1.0, 1e-6)] {
        for temperature in [250.0, 300.0] {
            let inv = inventory(mass, fraction);
            let coarse = solve(&inv, temperature, 1e5, Controls::default()).unwrap();
            let fine = solve(
                &inv,
                temperature,
                1e5,
                Controls {
                    relative_tolerance: 2.5e-11,
                    max_step: 1.0 / 512.0,
                    ..Controls::default()
                },
            )
            .unwrap();
            close(coarse.radius_m(), fine.radius_m(), 1e-7);
            close(coarse.inertia_factor(), fine.inertia_factor(), 1e-7);
            close(
                coarse.diagnostics().central_pressure_pa,
                fine.diagnostics().central_pressure_pa,
                1e-7,
            );
            let a = coarse.regions().last().unwrap();
            let b = fine.regions().last().unwrap();
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
            let loaded = solve(&inv, temperature, 1e7, Controls::default()).unwrap();
            assert!(loaded.radius_m() < coarse.radius_m());
            assert!(
                loaded.diagnostics().central_pressure_pa > coarse.diagnostics().central_pressure_pa
            );
            assert_eq!(loaded.retained_inventory(), inv);
        }
    }
}

#[test]
fn unsupported_deep_water_phase_crossings_and_surface_states_are_explicit_failures() {
    for (temperature, pressure) in [
        (300.0, 0.0),
        (300.0, 1000.0),
        (249.0, 1e5),
        (300.0, 1e13 + 1.0),
        (f64::NAN, 1e5),
    ] {
        assert!(matches!(
            solve(
                &inventory(0.0001, 0.01),
                temperature,
                pressure,
                Controls::default()
            ),
            Err(SolveError::Water(_) | SolveError::DenseWater(_))
        ));
    }
    assert!(matches!(
        solve(&inventory(1.0, 0.0), 300.0, 1e5, Controls::default()),
        Err(SolveError::InvalidInput(_))
    ));
    let inv = inventory(1.0, 1e-6)
        .checked_add(&Inventory::new(&[(Material::Hydrogen, 1.0)]).unwrap())
        .unwrap();
    assert!(matches!(
        solve(&inv, 300.0, 1e5, Controls::default()),
        Err(SolveError::UnsupportedMaterial(Material::Hydrogen))
    ));
    let unrepresentable = inventory(1.0, 0.0)
        .checked_add(&Inventory::new(&[(Material::Water, 1.0)]).unwrap())
        .unwrap();
    assert!(matches!(
        solve(&unrepresentable, 300.0, 1e5, Controls::default()),
        Err(SolveError::InvalidClosure {
            material: Material::Water,
            ..
        })
    ));
}

#[test]
fn work_limits_propagate_instead_of_becoming_physical_brackets() {
    let inv = inventory(0.0001, 0.01);
    for (controls, expected) in [
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
    ] {
        assert_eq!(
            solve(&inv, 300.0, 1e5, controls),
            Err(SolveError::WorkBudgetExhausted(expected))
        );
    }
    let body = solve(&inv, 300.0, 1e5, Controls::default()).unwrap();
    let work = body.diagnostics().work;
    assert!(work.attempted_steps > 0 && work.eos_evaluations > 7 * work.attempted_steps);
    assert_eq!(
        solve(
            &inv,
            300.0,
            1e5,
            Controls {
                max_eos_evaluations: work.eos_evaluations - 1,
                ..Controls::default()
            }
        ),
        Err(SolveError::WorkBudgetExhausted(WorkLimit::EosEvaluations))
    );
    assert_eq!(
        body,
        solve(
            &inv,
            300.0,
            1e5,
            Controls {
                max_eos_evaluations: work.eos_evaluations,
                ..Controls::default()
            }
        )
        .unwrap()
    );
}

#[test]
fn located_ice_liquid_interfaces_conserve_and_close_independent_integrals() {
    for (mass, fraction) in [(0.0001, 1.0), (0.0001, 0.01), (1.0, 1e-6)] {
        for temperature in [273.15, 273.155, 273.159] {
            let inv = inventory(mass, fraction);
            let body = solve(&inv, temperature, 1000.0, Controls::default())
                .unwrap_or_else(|e| panic!("{mass} {fraction} {temperature}: {e}"));
            let water: Vec<_> = body
                .regions()
                .iter()
                .filter(|r| r.temperature_k().is_some())
                .collect();
            assert_eq!(water.len(), 2, "{mass} {fraction} {temperature}");
            assert_eq!(water[0].phase(), PhaseKnowledge::Water(eos::Phase::Liquid));
            assert_eq!(water[1].phase(), PhaseKnowledge::Water(eos::Phase::IceIh));
            let a = water[0].outer();
            let b = water[1].inner();
            assert_eq!(
                a.pressure_pa,
                eos::melting_pressure_ih_pa(temperature).unwrap()
            );
            assert_eq!(a.pressure_pa, b.pressure_pa);
            assert_eq!(a.radius_m, b.radius_m);
            assert_eq!(a.enclosed_mass_kg, b.enclosed_mass_kg);
            assert!(a.density_kg_m3 > b.density_kg_m3);
            close(
                body.retained_inventory().mass_kg(Material::Water),
                inv.mass_kg(Material::Water),
                1e-15,
            );
            let mut inertia = 0.0;
            for r in body.regions() {
                if let PhaseKnowledge::Water(phase) = r.phase() {
                    for s in r.samples() {
                        assert_eq!(
                            s.density_kg_m3,
                            eos::condensed_state(s.pressure_pa, temperature, phase)
                                .unwrap()
                                .density_kg_m3
                        );
                    }
                }
                let (m, i, dp) = quadrature(r);
                close(m, r.inventory().total_mass_kg(), 1e-8);
                close(dp, r.inner().pressure_pa - r.outer().pressure_pa, 2e-4);
                inertia += i;
            }
            close(inertia, body.inertia_kg_m2(), 1e-8);
            assert_eq!(
                body,
                solve(&inv, temperature, 1000.0, Controls::default()).unwrap()
            );
        }
    }
}

#[test]
fn phase_interface_radius_and_allocations_converge_under_refinement() {
    for (mass, fraction) in [(0.0001, 1.0), (0.0001, 0.01), (1.0, 1e-6)] {
        let inv = inventory(mass, fraction);
        let coarse = solve(&inv, 273.155, 1000.0, Controls::default()).unwrap();
        let fine = solve(
            &inv,
            273.155,
            1000.0,
            Controls {
                relative_tolerance: 2.5e-11,
                max_step: 1.0 / 512.0,
                ..Controls::default()
            },
        )
        .unwrap();
        close(coarse.radius_m(), fine.radius_m(), 1e-7);
        close(coarse.inertia_factor(), fine.inertia_factor(), 1e-7);
        for (a, b) in coarse.regions().iter().zip(fine.regions()) {
            if a.temperature_k().is_some() {
                close(
                    a.inventory().mass_kg(Material::Water),
                    b.inventory().mass_kg(Material::Water),
                    1e-5,
                );
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
        }
        let work = coarse.diagnostics().work;
        assert_eq!(
            coarse,
            solve(
                &inv,
                273.155,
                1000.0,
                Controls {
                    max_eos_evaluations: work.eos_evaluations,
                    ..Controls::default()
                }
            )
            .unwrap()
        );
        assert_eq!(
            solve(
                &inv,
                273.155,
                1000.0,
                Controls {
                    max_eos_evaluations: work.eos_evaluations - 1,
                    ..Controls::default()
                }
            ),
            Err(SolveError::WorkBudgetExhausted(WorkLimit::EosEvaluations))
        );
    }
}

#[test]
fn coexistence_outer_endpoint_has_only_the_inward_liquid_limit() {
    let inv = inventory(0.0001, 1.0);
    let temperature = 273.155;
    let melting = eos::melting_pressure_ih_pa(temperature).unwrap();
    for pressure in [melting, melting + 1.0] {
        let body = solve(&inv, temperature, pressure, Controls::default()).unwrap();
        assert_eq!(body.regions().len(), 1);
        assert_eq!(
            body.regions()[0].phase(),
            PhaseKnowledge::Water(eos::Phase::Liquid)
        );
        assert_eq!(body.retained_inventory(), inv);
    }
    // A water shell entirely below the melting pressure needs no liquid region.
    let body = solve(
        &inventory(0.0001, 1e-4),
        temperature,
        1000.0,
        Controls::default(),
    )
    .unwrap();
    let water: Vec<_> = body
        .regions()
        .iter()
        .filter(|r| r.temperature_k().is_some())
        .collect();
    assert_eq!(water.len(), 1);
    assert_eq!(water[0].phase(), PhaseKnowledge::Water(eos::Phase::IceIh));
    // This thinner trace ice interval cannot meet the existing mass-resolution
    // contract in total enclosed-mass coordinates. It must not be discarded.
    assert!(matches!(
        solve(
            &inventory(1.0, 1e-6),
            273.15995,
            1000.0,
            Controls::default()
        ),
        Err(SolveError::InvalidClosure {
            material: Material::Water,
            ..
        })
    ));
}

#[test]
fn located_pressure_event_matches_analytic_two_density_sphere() {
    let inv = inventory(0.0001, 1.0);
    let mass = inv.total_mass_kg();
    let high_rho = 1000.0;
    let low_rho = 900.0;
    let inner_mass = mass * 0.4;
    let interface_radius = (3.0 * inner_mass / (4.0 * PI * high_rho)).cbrt();
    let radius =
        (interface_radius.powi(3) + 3.0 * (mass - inner_mass) / (4.0 * PI * low_rho)).cbrt();
    let excess = inner_mass - 4.0 * PI / 3.0 * low_rho * interface_radius.powi(3);
    let boundary_pressure = 1e5
        + 2.0 * PI / 3.0
            * GRAVITATIONAL_CONSTANT
            * low_rho.powi(2)
            * (radius.powi(2) - interface_radius.powi(2))
        + GRAVITATIONAL_CONSTANT * low_rho * excess * (1.0 / interface_radius - 1.0 / radius);
    let central = boundary_pressure
        + 2.0 * PI / 3.0 * GRAVITATIONAL_CONSTANT * high_rho.powi(2) * interface_radius.powi(2);
    let density = |_, p, side, _: &mut Work, _| {
        assert!(match side {
            radial::DensitySide::Branch(0) => p >= boundary_pressure - 1e-8,
            radial::DensitySide::Branch(_) => p <= boundary_pressure + 1e-8,
            radial::DensitySide::Equilibrium => false,
        });
        Ok(if side == radial::DensitySide::Branch(0) {
            high_rho
        } else {
            low_rho
        })
    };
    let mut kernel = radial::Solver::new(
        &inv,
        1e5,
        Controls::default(),
        &density,
        radial::Policy {
            pressure_floor_pa: 1000.0,
            boundary_tolerance_pa: Some(BOUNDARY_TOLERANCE_PA),
            pressure_error_scale_pa: Some(eos::MAX_PRESSURE_PA),
            interfaces: vec![radial::PressureInterface {
                material: Material::Water,
                pressure_pa: boundary_pressure,
            }],
        },
    );
    let trial = kernel.trial(central + 0.01).unwrap().unwrap();
    assert_eq!(trial.regions.len(), 2);
    close(trial.regions[0].outer().radius_m, interface_radius, 1e-7);
    close(
        trial.regions[0].inventory().mass_kg(Material::Water),
        inner_mass,
        1e-7,
    );
    close(trial.regions[1].outer().radius_m, radius, 1e-7);
    let expected_i = 8.0 * PI / 15.0
        * (high_rho * interface_radius.powi(5)
            + low_rho * (radius.powi(5) - interface_radius.powi(5)));
    close(trial.inertia_kg_m2, expected_i, 1e-7);
}

#[test]
fn compressed_liquid_regions_preserve_models_conservation_and_independent_closure() {
    for (mass, fraction, temperature, load) in [
        (0.0001, 0.01, 500.0, 990e6),
        (0.0001, 0.01, 500.0, 700e6),
        (1.0, 0.005, 300.0, 1e5),
        (0.0001, 0.01, 500.0, 100e6),
        (0.0001, 0.01, 273.16, 100e6),
    ] {
        let inv = inventory(mass, fraction);
        let body = solve(&inv, temperature, load, Controls::default())
            .unwrap_or_else(|e| panic!("{mass} {fraction} {temperature} {load}: {e}"));
        close(
            body.retained_inventory().mass_kg(Material::Water),
            inv.mass_kg(Material::Water),
            1e-15,
        );
        assert!((0.0..=BOUNDARY_TOLERANCE_PA).contains(&body.diagnostics().boundary_residual_pa));
        let water: Vec<_> = body
            .regions()
            .iter()
            .filter(|r| r.temperature_k().is_some())
            .collect();
        assert_eq!(water[0].density_model_id(), eos::dense_liquid::MODEL_ID);
        assert_eq!(water.len(), if load < 100e6 { 2 } else { 1 });
        if water.len() == 2 {
            let a = water[0].outer();
            let b = water[1].inner();
            assert_eq!(water[1].density_model_id(), eos::MODEL_ID);
            assert_eq!(a.pressure_pa, 100e6);
            assert_eq!(a.pressure_pa, b.pressure_pa);
            assert_eq!(a.radius_m, b.radius_m);
            assert_eq!(a.enclosed_mass_kg, b.enclosed_mass_kg);
            assert_ne!(a.density_kg_m3, b.density_kg_m3);
        }
        for r in &water {
            assert_eq!(r.phase(), PhaseKnowledge::Water(eos::Phase::Liquid));
            for s in r.samples() {
                let expected = if r.density_model_id() == eos::dense_liquid::MODEL_ID {
                    eos::dense_liquid::state(s.pressure_pa, temperature).unwrap()
                } else {
                    eos::state(s.pressure_pa, temperature).unwrap()
                };
                assert_eq!(s.density_kg_m3, expected.density_kg_m3);
            }
        }
        let mut inertia = 0.0;
        for r in body.regions() {
            let (m, i, dp) = quadrature(r);
            close(m, r.inventory().total_mass_kg(), 1e-8);
            close(dp, r.inner().pressure_pa - r.outer().pressure_pa, 2e-4);
            inertia += i;
        }
        close(inertia, body.inertia_kg_m2(), 1e-8);
    }
}

#[test]
fn compressed_liquid_model_interfaces_refine_and_nested_work_limits_are_exact() {
    let inv = inventory(1.0, 0.005);
    let body = solve(&inv, 300.0, 1e5, Controls::default()).unwrap();
    let fine = solve(
        &inv,
        300.0,
        1e5,
        Controls {
            relative_tolerance: 2.5e-11,
            max_step: 1.0 / 512.0,
            ..Controls::default()
        },
    )
    .unwrap();
    close(body.radius_m(), fine.radius_m(), 1e-7);
    close(body.inertia_factor(), fine.inertia_factor(), 1e-7);
    close(
        body.diagnostics().central_pressure_pa,
        fine.diagnostics().central_pressure_pa,
        1e-7,
    );
    for (a, b) in body.regions().iter().zip(fine.regions()) {
        close(
            a.inventory().total_mass_kg(),
            b.inventory().total_mass_kg(),
            1e-5,
        );
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
    let count = body.diagnostics().work.eos_evaluations;
    for budget in [0, 1, 2, 10, count - 1, count] {
        let result = solve(
            &inv,
            300.0,
            1e5,
            Controls {
                max_eos_evaluations: budget,
                ..Controls::default()
            },
        );
        if budget == count {
            assert_eq!(result.unwrap(), body);
        } else {
            assert_eq!(
                result,
                Err(SolveError::WorkBudgetExhausted(WorkLimit::EosEvaluations))
            );
        }
    }
    // Exhaust inside the initial dense surface inversion, not just a dispatch.
    for budget in [1, 2, 3, 10] {
        assert_eq!(
            solve(
                &inventory(0.0001, 0.01),
                500.0,
                700e6,
                Controls {
                    max_eos_evaluations: budget,
                    ..Controls::default()
                }
            ),
            Err(SolveError::WorkBudgetExhausted(WorkLimit::EosEvaluations))
        );
    }
}

#[test]
fn unavailable_dense_profiles_and_numerical_failures_are_not_repaired() {
    assert_eq!(
        solve(&inventory(0.0001, 1.0), 300.0, 80e6, Controls::default()),
        Err(SolveError::WorkBudgetExhausted(WorkLimit::EosEvaluations))
    );
    assert_eq!(
        solve(&inventory(0.0001, 0.01), 500.0, 1e9, Controls::default()),
        Err(SolveError::UnsupportedWaterProfile)
    );
    for reason in [
        eos::dense_liquid::Failure::DensityBracket,
        eos::dense_liquid::Failure::DensityNonConvergence,
    ] {
        let error = SolveError::DenseWater(eos::dense_liquid::Error {
            pressure_pa: 700e6,
            temperature_k: 300.0,
            reason,
            eos_evaluations: 17,
        });
        assert!(matches!(classify_upper(Err(error)), Err(e) if e == error));
    }
}
