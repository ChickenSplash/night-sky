use super::*;
use crate::system::physical;

#[test]
fn sub_resolution_condensed_shell_keeps_mass_and_finite_shell_limits() {
    let mass = 300_000.0;
    let b = Budget::new(
        Inventory::new(&[(Material::Silicate, EARTH_MASS_KG), (Material::Water, mass)]).unwrap(),
        Inventory::new(&[(Material::Silicate, EARTH_MASS_KG)]).unwrap(),
        Inventory::new(&[(Material::Water, mass)]).unwrap(),
        Inventory::default(),
    )
    .unwrap();
    let f = initial(&b, &environment(1e8, 4000.0, 4.5));
    let water = f.regions.last().unwrap();
    assert_eq!(water.inner_radius_m, water.outer_radius_m);
    assert_eq!(water.inventory.mass_kg(Material::Water), mass);
    let shell = Shell::new(water, EARTH_MASS_KG);
    assert_eq!(
        shell.inertia_kg_m2(),
        (2.0 / 3.0) * mass * water.outer_radius_m.powi(2)
    );
    assert!(shell.pressure_drop_pa().is_finite() && shell.pressure_drop_pa() > 0.0);
    physical::validate_record(&physical::fallback(&f)).unwrap();
}

#[test]
fn zero_irradiation_uses_the_cold_fit_bound_not_earth_flux() {
    let b = budget(&[
        (Material::Silicate, 5.0),
        (Material::Hydrogen, 0.75),
        (Material::Helium, 0.25),
    ]);
    let cold = initial(&b, &environment(0.0, 0.0, 4.5));
    let tiny = initial(&b, &environment(1e-12, 0.0, 4.5));
    let warm = initial(&b, &environment(1361.0, 250.0, 4.5));
    assert_eq!(cold.radius_m, tiny.radius_m);
    assert!(cold.radius_m < warm.radius_m);
    // A one-bar level carries the envelope's intrinsic heat above the floor.
    assert!(cold.surface_temperature_k > MIN_TEMPERATURE_K);
    assert!(cold.surface_temperature_k < 100.0);
    assert!(cold.extrapolated);
    physical::validate_record(&physical::fallback(&cold)).unwrap();
}

fn environment(
    irradiation_w_m2: f64,
    equilibrium_temperature_k: f64,
    age_gyr: f64,
) -> PlanetEnvironment {
    PlanetEnvironment {
        irradiation_w_m2,
        assumed_bond_albedo: 0.3,
        equilibrium_temperature_k,
        age_gyr,
        atmosphere: None,
    }
}

fn budget(parts: &[(Material, f64)]) -> Budget {
    let retained = Inventory::new(
        &parts
            .iter()
            .map(|&(m, earth)| (m, earth * EARTH_MASS_KG))
            .collect::<Vec<_>>(),
    )
    .unwrap();
    Budget::new(
        retained,
        retained,
        Inventory::default(),
        Inventory::default(),
    )
    .unwrap()
}

fn solve_checked(budget: &Budget, environment: &PlanetEnvironment) -> Fallback {
    let f = solve(budget, environment, Error::DenseGasUnsupported);
    let retained = budget.retained();
    for m in Material::ALL {
        let allocated: f64 = f.regions.iter().map(|r| r.inventory.mass_kg(m)).sum();
        assert_eq!(allocated, retained.mass_kg(m), "{m:?} closure");
    }
    assert_eq!(f.regions[0].inner_radius_m, 0.0);
    for pair in f.regions.windows(2) {
        assert_eq!(pair[0].outer_radius_m, pair[1].inner_radius_m);
        assert!(pair[0].inner_pressure_pa >= pair[1].inner_pressure_pa);
    }
    for r in &f.regions {
        // A trace layer may add less than one radius ulp: see the fallback note.
        assert!(r.outer_radius_m >= r.inner_radius_m, "{r:?}");
        assert!(r.outer_radius_m.is_finite());
        assert!(r.inner_pressure_pa.is_finite() && r.inner_pressure_pa >= r.outer_pressure_pa);
        assert!(r.outer_pressure_pa >= 0.0);
    }
    let l = physical::fallback(&f);
    let p = &l.physical;
    for value in [
        p.reference_radius_m,
        p.mean_density_kg_m3,
        p.reference_gravity_m_s2,
        p.inertia_factor,
        l.surface_temperature_k,
    ] {
        assert!(value.is_finite() && value > 0.0, "{l:?}");
    }
    // The thin-shell limit, reachable only when all mass lies within the reference.
    assert!(p.inertia_factor <= 2.0 / 3.0, "{}", p.inertia_factor);
    assert!(f.regions.last().unwrap().outer_radius_m <= p.reference_radius_m * 2.0 + 1.0);
    assert!(l.surface_pressure_pa.is_finite() && l.surface_pressure_pa >= 0.0);
    assert!(l.fallback_reason.is_some());
    for b in &p.materials {
        assert_eq!(
            b.interior_kg + b.condensed_kg + b.gas_kg,
            b.retained_kg,
            "{}",
            b.material
        );
    }
    f
}

#[test]
fn earth_twin_is_close_to_earth() {
    let f = solve_checked(
        &budget(&[(Material::Metal, 0.325), (Material::Silicate, 0.675)]),
        &environment(1361.0, 255.0, 4.5),
    );
    let l = physical::fallback(&f);
    assert!((l.physical.reference_radius_m / 6.371e6 - 1.0).abs() < 0.05);
    assert!((0.30..0.36).contains(&l.physical.inertia_factor));
    assert_eq!(l.surface, "rock");
    assert_eq!(l.physical.radius_reference, RadiusReference::SolidSurface);
    // Earth's central pressure is about 3.6 Mbar.
    assert!((2e11..6e11).contains(&f.regions[0].inner_pressure_pa));
}

#[test]
fn prescribed_reference_worlds_keep_broad_radius_and_density_ranges() {
    // Prescribed budgets, not reconstructed formation histories or accuracy bounds.
    for (mass, metal, water, gas, flux, radii, densities) in [
        (
            0.0553,
            0.7,
            0.0,
            0.0,
            1361.0 / 0.387_f64.powi(2),
            0.323..0.443,
            4000.0..8000.0,
        ),
        (1.0, 0.325, 0.0, 0.0, 1361.0, 0.9..1.1, 4500.0..6500.0),
        (1.0, 0.325, 0.5, 0.0, 1361.0, 1.1..1.6, 1500.0..4000.0),
        (17.0, 0.325, 0.6, 0.15, 1.5, 3.0..6.0, 400.0..3500.0),
        (95.0, 0.325, 0.0, 0.75, 15.0, 8.0..12.0, 300.0..1500.0),
        (318.0, 0.325, 0.0, 0.9, 50.0, 10.0..12.0, 900.0..1800.0),
    ] {
        let refractory = 1.0 - water - gas;
        let f = solve_checked(
            &budget(&[
                (Material::Metal, mass * refractory * metal),
                (Material::Silicate, mass * refractory * (1.0 - metal)),
                (Material::Water, mass * water),
                (Material::Hydrogen, mass * gas * 0.75),
                (Material::Helium, mass * gas * 0.25),
            ]),
            &environment(flux, 255.0, 4.5),
        );
        let p = physical::fallback(&f).physical;
        eprintln!(
            "{mass} ME: {:.3} RE, {:.0} kg/m3",
            f.radius_m / 6.371e6,
            p.mean_density_kg_m3
        );
        assert!(radii.contains(&(f.radius_m / 6.371e6)), "{mass}: {f:?}");
        assert!(densities.contains(&p.mean_density_kg_m3), "{mass}: {p:?}");
    }
}

#[test]
fn jupiter_twin_has_a_giant_radius_and_low_inertia() {
    let f = solve_checked(
        &budget(&[
            (Material::Silicate, 10.0),
            (Material::Hydrogen, 0.75 * 308.0),
            (Material::Helium, 0.25 * 308.0),
        ]),
        &environment(50.0, 110.0, 4.5),
    );
    let l = physical::fallback(&f);
    let radius_earth = l.physical.reference_radius_m / 6.371e6;
    assert!((9.0..13.0).contains(&radius_earth), "{radius_earth}");
    assert!((0.2..0.3).contains(&l.physical.inertia_factor));
    assert_eq!(l.surface, "envelope");
    assert_eq!(l.surface_pressure_pa, 1e5);
}

#[test]
fn single_constituents_and_invalid_environment_still_resolve() {
    let invalid = environment(f64::NAN, f64::NAN, -1.0);
    for m in Material::ALL {
        for earth in [1e-6, 1.0, 3000.0] {
            solve_checked(&budget(&[(m, earth)]), &invalid);
        }
    }
    let f = solve_checked(
        &budget(&[(Material::Hydrogen, 100.0), (Material::Helium, 30.0)]),
        &invalid,
    );
    assert!(f.extrapolated && f.notes.contains(&CLAMPED_NOTE));
}

#[test]
fn volatiles_without_h_he_are_condensed_and_labelled() {
    let f = solve_checked(
        &budget(&[(Material::Silicate, 0.05), (Material::CarbonDioxide, 0.01)]),
        &environment(1361.0, 200.0, 4.5),
    );
    assert_eq!(f.regions.last().unwrap().kind, Kind::Volatiles);
    assert_eq!(f.surface(), "unresolved volatiles");
    assert!(f.notes.contains(&VOLATILE_PROXY_NOTE));
}

/// Far beyond anything generation produces: log-uniform masses from 1e-6 to
/// 1e5 Earth masses, random mixtures with missing constituents and hostile
/// environments, including NaN and zero.
#[test]
fn randomised_extreme_inventories_always_resolve() {
    let mut state = 0x5eed_u64;
    let mut next = || {
        state = state.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = state;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        ((z ^ (z >> 31)) >> 11) as f64 / (1u64 << 53) as f64
    };
    let hostile = [f64::NAN, 0.0, -5.0, f64::INFINITY];
    for case in 0..20_000 {
        let total = 10f64.powf(-6.0 + 11.0 * next());
        let weights: Vec<f64> = Material::ALL
            .iter()
            .map(|_| if next() < 0.3 { 0.0 } else { next().powi(4) })
            .collect();
        let sum: f64 = weights.iter().sum();
        if sum == 0.0 {
            continue;
        }
        let parts: Vec<_> = Material::ALL
            .into_iter()
            .zip(&weights)
            .map(|(m, w)| (m, total * w / sum))
            .collect();
        let mut pick = |range: f64| {
            if next() < 0.05 {
                hostile[(next() * 4.0) as usize % 4]
            } else {
                range * next()
            }
        };
        let env = environment(pick(1e6), pick(3000.0), pick(13.0));
        let b = budget(&parts);
        let f = std::panic::catch_unwind(|| solve_checked(&b, &env));
        assert!(f.is_ok(), "case {case}: {parts:?} {env:?}");
    }
}

fn wet(earth: f64, water_share: f64) -> Inventory {
    let mass = earth * EARTH_MASS_KG;
    Inventory::new(&[
        (Material::Metal, mass * (1.0 - water_share) * 0.3),
        (Material::Silicate, mass * (1.0 - water_share) * 0.7),
        (Material::Water, mass * water_share),
    ])
    .unwrap()
}

/// The walk is the solver's own water integration, started from the radius
/// the solver ends on: it must find the same phases and the same rock.
#[test]
fn phase_walk_from_the_solved_radius_finds_the_solver_s_layers() {
    use crate::system::structure::water;
    for (earth, water_share, temperature_k, load_pa) in [
        (0.02, 0.5, 122.0, 141.0),
        (0.16, 0.26, 260.3, 7.96e5),
        (0.8, 0.035, 302.9, 64.1e5),
        (1.0, 0.0003, 288.0, 1e5),
    ] {
        let inventory = wet(earth, water_share);
        let solved = water::solve(&inventory, temperature_k, load_pa, Default::default())
            .unwrap_or_else(|e| panic!("{earth} {water_share} {temperature_k}: {e:?}"));
        let water_regions: Vec<_> = solved
            .regions()
            .iter()
            .filter(|r| r.inventory().mass_kg(Material::Water) > 0.0)
            .collect();
        let rock = water_regions[0].inner();
        let mut kinds: Vec<Kind> = water_regions.iter().map(|r| Kind::of(r.phase())).collect();
        kinds.dedup();
        // The estimate only sizes the steps: a poor one must not matter.
        for estimate in [0.5, 1.0, 3.0] {
            let walk = phases::walk(
                temperature_k,
                load_pa,
                inventory.total_mass_kg(),
                solved.radius_m(),
                inventory.mass_kg(Material::Water),
                estimate * (rock.pressure_pa - load_pa),
            )
            .unwrap();
            let case = format!("{earth} {water_share} {temperature_k} {estimate}");
            assert_eq!(
                walk.layers.iter().map(|l| l.kind).collect::<Vec<_>>(),
                kinds,
                "{case}"
            );
            let thickness = solved.radius_m() - rock.radius_m;
            assert!(
                (walk.rock_radius_m - rock.radius_m).abs() < 2e-3 * thickness,
                "{case}: {} against {}",
                walk.rock_radius_m,
                rock.radius_m
            );
            assert!(
                (walk.rock_pa + load_pa - rock.pressure_pa).abs() < 2e-3 * rock.pressure_pa,
                "{case}"
            );
            assert_eq!(
                walk.layers.iter().map(|l| l.mass_kg).sum::<f64>(),
                inventory.mass_kg(Material::Water),
                "{case}"
            );
            assert_eq!(
                walk.layers.last().unwrap().outer_radius_m,
                solved.radius_m()
            );
            assert_eq!(walk.layers.last().unwrap().outer_pa, 0.0);
        }
    }
}

#[test]
fn a_decided_surface_states_its_water_phases_and_tabulated_pressures() {
    let retained = wet(0.02, 0.5);
    let budget = Budget::new(
        retained,
        retained,
        Inventory::default(),
        Inventory::default(),
    )
    .unwrap();
    let env = environment(20.0, 110.0, 4.5);
    let decided = decide(&initial(&budget, &env), &env).unwrap();
    let f = &decided.fallback;
    assert_eq!(f.surface(), "ice");
    assert_eq!(f.reference, RadiusReference::SolidSurface);
    assert!(f.notes.contains(&decide::PHASES_NOTE));
    let kinds: Vec<Kind> = f.regions.iter().map(|r| r.kind).collect();
    assert_eq!(
        kinds[..4],
        [Kind::Core, Kind::Mantle, Kind::DeepIce, Kind::Ice]
    );
    // Every condensed pressure is the table's or the walk's, and they join.
    for pair in f.regions.windows(2) {
        assert_eq!(pair[0].outer_radius_m, pair[1].inner_radius_m);
        if pair[1].kind != Kind::Atmosphere {
            assert_eq!(pair[0].outer_pressure_pa, pair[1].inner_pressure_pa);
        }
    }
    for region in &f.regions[..4] {
        assert!(region.tabulated_pa.is_some());
        assert!(region.inner_pressure_pa > region.outer_pressure_pa);
    }
    let water: f64 = f
        .regions
        .iter()
        .filter(|r| r.kind.is_water())
        .map(|r| r.inventory.mass_kg(Material::Water))
        .sum();
    assert!((water / decided.budget.retained().mass_kg(Material::Water) - 1.0).abs() < 1e-12);
    let record = physical::fallback(f);
    physical::validate_record(&record).unwrap();
    assert_eq!(record.surface, "ice");
    for layer in &record.layers {
        let phase = ["ice", "deep_ice"].contains(&layer.kind.as_str());
        if phase {
            assert_eq!(layer.temperature_k, Some(f.surface_temperature_k));
        }
        assert_eq!(
            layer.temperature_k.is_none(),
            ["core", "mantle"].contains(&layer.kind.as_str())
        );
    }
}

#[test]
fn layers_off_the_table_say_so() {
    let inside = physical::fallback(&initial(
        &budget(&[(Material::Silicate, 1.0)]),
        &environment(1361.0, 255.0, 4.5),
    ));
    let outside = physical::fallback(&initial(
        &budget(&[(Material::Silicate, 30.0)]),
        &environment(1361.0, 255.0, 4.5),
    ));
    let labelled = |record: &space_shared::LayeredPlanet| {
        record
            .model_limitations
            .iter()
            .any(|l| l.starts_with("Structure table:"))
    };
    assert!(!labelled(&inside));
    assert!(labelled(&outside));
    assert_eq!(
        outside.model_fidelity,
        space_shared::ModelFidelity::Extrapolated
    );
}
