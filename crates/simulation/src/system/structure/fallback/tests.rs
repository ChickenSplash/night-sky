use super::*;
use crate::system::physical;

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
    assert_eq!(cold.surface_temperature_k, MIN_TEMPERATURE_K);
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
        assert!(r.outer_radius_m > r.inner_radius_m, "{r:?}");
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
