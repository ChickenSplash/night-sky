use super::parameters::*;
use super::*;
use crate::system::composition::Inventory;

fn budget(mass_earth: f64, parts: &[(Material, f64)]) -> Budget {
    let source = Inventory::from_mass_fractions(mass_earth * EARTH_MASS_KG, parts).unwrap();
    let interior = Inventory::new(&Material::ALL.map(|m| {
        (
            m,
            if matches!(m, Material::Hydrogen | Material::Helium) {
                0.0
            } else {
                source.mass_kg(m)
            },
        )
    }))
    .unwrap();
    let gas = Inventory::new(&[
        (Material::Hydrogen, source.mass_kg(Material::Hydrogen)),
        (Material::Helium, source.mass_kg(Material::Helium)),
    ])
    .unwrap();
    Budget::new(source, interior, Inventory::default(), gas).unwrap()
}

fn world(mass: f64, water: f64, h_he: f64) -> Budget {
    let rock = 1.0 - water - h_he;
    budget(
        mass,
        &[
            (Material::Metal, rock * 0.325),
            (Material::Silicate, rock * 0.675),
            (Material::Water, water),
            (Material::Hydrogen, h_he * 0.75),
            (Material::Helium, h_he * 0.25),
        ],
    )
}

fn radius(budget: &Budget) -> f64 {
    derive(budget, EARTH_REFERENCE_FLUX_W_M2, 5.0)
        .unwrap()
        .radius_m()
        / EARTH_REFERENCE_RADIUS_M
}

#[test]
fn prescribed_reference_worlds_have_broad_declared_structure_ranges() {
    // These are prescribed budgets, not reconstructions of Solar System formation
    // or measurements of bulk water/envelope composition. Radius ranges in Earth
    // units accommodate mixture/reference-level errors; they are not confidence intervals.
    let mercury = budget(0.0553, &[(Material::Metal, 0.7), (Material::Silicate, 0.3)]);
    let cases = [
        (
            "Mercury-like",
            mercury,
            1361.0 / 0.387_f64.powi(2),
            0.323..0.443,
            4000.0..8000.0,
        ),
        (
            "Earth-like",
            world(1.0, 0.0, 0.0),
            1361.0,
            0.9..1.1,
            4500.0..6500.0,
        ),
        (
            "Water-rich",
            world(1.0, 0.5, 0.0),
            1361.0,
            1.1..1.6,
            1500.0..4000.0,
        ),
        (
            "Neptune-like",
            world(17.0, 0.6, 0.15),
            1.5,
            3.0..6.0,
            400.0..3500.0,
        ),
        (
            "Saturn-like",
            world(95.0, 0.0, 0.75),
            15.0,
            8.0..12.0,
            300.0..1500.0,
        ),
        (
            "Jupiter-like",
            world(318.0, 0.0, 0.9),
            50.0,
            10.0..12.0,
            900.0..1800.0,
        ),
    ];
    eprintln!("reference | mass_E | radius_E | density_kg_m3 | gravity_m_s2 | reference");
    for (name, input, flux, radii, densities) in cases {
        let s = derive(&input, flux, 4.5).unwrap();
        let r = s.radius_m() / EARTH_REFERENCE_RADIUS_M;
        eprintln!(
            "{name} | {:.5} | {r:.5} | {:.2} | {:.3} | {:?}",
            s.mass_kg() / EARTH_MASS_KG,
            s.mean_density_kg_m3(),
            s.reference_gravity_m_s2(),
            s.reference()
        );
        assert!(radii.contains(&r), "{name}: {r}");
        assert!(densities.contains(&s.mean_density_kg_m3()), "{name}: {s:?}");
    }
    assert!(radius(&world(1.0, 0.5, 0.0)) > radius(&world(1.0, 0.0, 0.0)));
    // At fixed giant composition, self-compression must turn over, not allow
    // indefinite M^(1/3) growth or a discontinuity at a display-class threshold.
    assert!(radius(&world(318.0, 0.0, 0.9)) > radius(&world(95.0, 0.0, 0.9)));
    assert!(radius(&world(4000.0, 0.0, 0.9)) < radius(&world(1000.0, 0.0, 0.9)));
}

#[test]
fn pure_materials_and_mixtures_follow_compression_trends() {
    for mass in [0.002, 0.055, 1.0, 10.0, 20.0, 100.0] {
        let metal = radius(&budget(mass, &[(Material::Metal, 1.0)]));
        let rock = radius(&budget(mass, &[(Material::Silicate, 1.0)]));
        let water = radius(&budget(mass, &[(Material::Water, 1.0)]));
        let mixture = radius(&world(mass, 0.0, 0.0));
        assert!(metal < mixture && mixture < rock && rock < water);
        let compressed = radius(&budget(mass * 2.0, &[(Material::Silicate, 1.0)]));
        assert!(compressed > rock && compressed / rock < 2_f64.cbrt());
    }
    assert!((radius(&world(1.0, 0.0, 0.0)) - 1.0).abs() < 0.1);
    // Broad checks, not precision claims for our mixture surrogate.
    let mercury = radius(&budget(
        0.0553,
        &[(Material::Metal, 0.7), (Material::Silicate, 0.3)],
    ));
    assert!((mercury - 0.383).abs() < 0.06);
}

#[test]
fn density_gravity_and_radius_semantics_are_explicit() {
    let dry = derive(&world(1.0, 0.0, 0.0), 1361.0, 4.5).unwrap();
    assert_eq!(dry.reference(), RadiusReference::SolidSurface);
    assert!((4500.0..6500.0).contains(&dry.mean_density_kg_m3()));
    assert!((8.0..12.0).contains(&dry.reference_gravity_m_s2()));
    let wet = derive(&world(1.0, 0.5, 0.0), 1361.0, 4.5).unwrap();
    assert_eq!(wet.reference(), RadiusReference::UnresolvedVolatileBoundary);
    assert!(
        wet.approximations()
            .contains(&Approximation::ColdWaterProxy)
    );
    let envelope = derive(&world(17.0, 0.6, 0.15), 1.5, 4.5).unwrap();
    assert_eq!(envelope.reference(), RadiusReference::OneBar);
    assert!(envelope.radius_m() > envelope.condensed_radius_m());
    assert!((3.0..6.0).contains(&(envelope.radius_m() / EARTH_REFERENCE_RADIUS_M)));
    for (mass, gas, range) in [(95.0, 0.75, 8.0..12.0), (318.0, 0.9, 10.0..12.0)] {
        assert!(range.contains(&radius(&world(mass, 0.0, gas))));
    }
}

#[test]
fn stripping_and_blend_boundaries_are_continuous() {
    let bare = radius(&world(5.0, 0.0, 0.0));
    let stripped = radius(&world(5.0, 0.0, 1e-12));
    assert!((stripped - bare).abs() < 1e-5);
    for mass in [1.0, 20.0, 95.0, 318.0] {
        for fraction in [0.1, 0.2, 0.5] {
            let left = radius(&world(mass, 0.0, fraction - 1e-8));
            let right = radius(&world(mass, 0.0, fraction + 1e-8));
            assert!(
                (left - right).abs() < 1e-4,
                "{mass}, {fraction}: {left}, {right}"
            );
        }
    }
    let left = radius(&world(20.0 - 1e-8, 0.0, 0.0));
    let right = radius(&world(20.0 + 1e-8, 0.0, 0.0));
    assert!((left - right).abs() < 1e-6);
}

#[test]
fn pure_h_he_and_grid_are_finite_deterministic_and_bounded() {
    let pure = budget(
        318.0,
        &[(Material::Hydrogen, 0.75), (Material::Helium, 0.25)],
    );
    assert!((radius(&pure) - GIANT_RADIUS_EARTH).abs() < 0.01);
    for mass in [0.1, 1.0, 5.0, 20.0, 100.0, 318.0, 4000.0] {
        for fraction in [0.0, 1e-8, 0.001, 0.05, 0.1, 0.2, 0.5, 0.9, 0.99] {
            let input = world(mass, 0.0, fraction);
            // Massive bare heavy bodies intentionally exceed the model domain.
            if mass * (1.0 - fraction) > MAX_CONDENSED_MASS_EARTH {
                continue;
            }
            for (flux, age) in [(0.0, 0.001), (1361.0, 5.0), (1e10, 100.0)] {
                let a = derive(&input, flux, age).unwrap();
                let b = derive(&input, flux, age).unwrap();
                assert_eq!(a, b);
                assert!(a.radius_m() >= a.condensed_radius_m());
                assert!(a.radius_m() / EARTH_REFERENCE_RADIUS_M < 20.0);
            }
        }
    }
}

#[test]
fn invalid_and_unsupported_physics_return_errors() {
    let earth = world(1.0, 0.0, 0.0);
    for bad in [f64::NAN, f64::INFINITY, -1.0] {
        assert!(derive(&earth, bad, 1.0).is_err());
        assert!(derive(&earth, 1361.0, bad).is_err());
    }
    assert!(derive(&earth, 1361.0, 0.0).is_err());
    assert!(derive(&world(1e-5, 0.0, 0.0), 0.0, 1.0).is_err());
    assert!(derive(&world(5000.0, 0.0, 0.9), 0.0, 1.0).is_err());
    assert!(derive(&budget(1.0, &[(Material::Nitrogen, 1.0)]), 0.0, 1.0).is_err());
    assert!(derive(&budget(1.0, &[(Material::Hydrogen, 1.0)]), 0.0, 1.0).is_err());
    assert!(derive(&world(0.01, 0.0, 0.1), 0.0, 1.0).is_err());
    let trace = budget(
        1.0,
        &[(Material::Silicate, 0.999), (Material::Nitrogen, 0.001)],
    );
    assert!(
        derive(&trace, 1361.0, 1.0)
            .unwrap()
            .approximations()
            .contains(&Approximation::TraceVolatilesAsWater)
    );
    let source = Inventory::from_mass_fractions(
        EARTH_MASS_KG,
        &[
            (Material::Silicate, 0.9),
            (Material::Hydrogen, 0.075),
            (Material::Helium, 0.025),
        ],
    )
    .unwrap();
    let dissolved =
        Budget::new(source, source, Inventory::default(), Inventory::default()).unwrap();
    assert!(derive(&dissolved, 1361.0, 1.0).is_err());
}

#[test]
fn declared_domain_edges_are_inclusive_but_not_silent_extrapolations() {
    for mass in [MIN_MASS_EARTH, MAX_CONDENSED_MASS_EARTH] {
        assert!(derive(&world(mass, 0.0, 0.0), 0.0, 1.0).is_ok());
    }
    for mass in [MIN_MASS_EARTH * 0.99, MAX_CONDENSED_MASS_EARTH * 1.01] {
        assert!(derive(&world(mass, 0.0, 0.0), 0.0, 1.0).is_err());
    }
    for mass in [MIN_ENVELOPE_MASS_EARTH, MAX_MASS_EARTH] {
        for helium in [MIN_HELIUM_SHARE, MAX_HELIUM_SHARE] {
            let input = budget(
                mass,
                &[
                    (Material::Hydrogen, 1.0 - helium),
                    (Material::Helium, helium),
                ],
            );
            assert!(derive(&input, 0.0, 1.0).is_ok());
        }
    }
    for helium in [MIN_HELIUM_SHARE - 1e-6, MAX_HELIUM_SHARE + 1e-6] {
        let input = budget(
            1.0,
            &[
                (Material::Hydrogen, 1.0 - helium),
                (Material::Helium, helium),
            ],
        );
        assert!(derive(&input, 0.0, 1.0).is_err());
    }
    let limit = budget(
        1.0,
        &[(Material::Silicate, 0.99), (Material::Nitrogen, 0.01)],
    );
    assert!(derive(&limit, 0.0, 1.0).is_ok());
    let excessive = budget(
        1.0,
        &[(Material::Silicate, 0.989), (Material::Nitrogen, 0.011)],
    );
    assert!(derive(&excessive, 0.0, 1.0).is_err());
}

#[test]
fn thermal_trends_and_outside_fit_warnings_are_visible() {
    let input = world(5.0, 0.0, 0.05);
    let old = derive(&input, 1361.0, 10.0).unwrap();
    let young = derive(&input, 1361.0, 0.1).unwrap();
    let hot = derive(&input, 136100.0, 10.0).unwrap();
    assert!(young.radius_m() > old.radius_m());
    assert!(hot.radius_m() > old.radius_m());
    let cold = derive(&input, 0.0, 100.0).unwrap();
    assert!(
        cold.approximations()
            .contains(&Approximation::EnvelopeFitInputsBounded)
    );
    let massive = derive(&world(100.0, 0.0, 0.0), 0.0, 100.0).unwrap();
    assert!(
        massive
            .approximations()
            .contains(&Approximation::CondensedMassContinuation)
    );
}

#[test]
fn sub_bar_envelopes_do_not_invent_one_bar_or_a_radius_jump() {
    let bare = world(1.0, 0.0, 0.0);
    let gas = world(1.0, 0.0, 1e-12);
    let result = derive(&gas, 1361.0, 5.0).unwrap();
    assert_eq!(result.reference(), RadiusReference::SolidSurface);
    assert!(
        result
            .approximations()
            .contains(&Approximation::SubBarEnvelope)
    );
    assert_eq!(result.radius_m(), result.condensed_radius_m());
    assert!((radius(&gas) - radius(&bare)).abs() < 1e-9);
    let mut previous = radius(&bare);
    for i in 0..1000 {
        let fraction = 10_f64.powf(-12.0 + i as f64 / 1000.0 * 9.0);
        let next = radius(&world(1.0, 0.0, fraction));
        assert!(
            (next - previous).abs() < 0.01,
            "{fraction}: {previous} -> {next}"
        );
        previous = next;
    }
}

#[test]
fn escaped_mass_never_enters_structure_and_condensation_does_not_change_bulk() {
    let retained =
        Inventory::new(&[(Material::Silicate, EARTH_MASS_KG), (Material::Water, 1e18)]).unwrap();
    let lost = Inventory::new(&[(Material::Hydrogen, EARTH_MASS_KG)]).unwrap();
    let source = retained.checked_add(&lost).unwrap();
    let mut escaped =
        Budget::new(source, retained, Inventory::default(), Inventory::default()).unwrap();
    let original = derive(&escaped, 1361.0, 5.0).unwrap();
    let never_acquired = Budget::new(
        retained,
        retained,
        Inventory::default(),
        Inventory::default(),
    )
    .unwrap();
    assert_eq!(original, derive(&never_acquired, 1361.0, 5.0).unwrap());
    escaped
        .transfer(
            Reservoir::Interior,
            Reservoir::CondensedVolatiles,
            Material::Water,
            1e18,
        )
        .unwrap();
    assert_eq!(original, derive(&escaped, 1361.0, 5.0).unwrap());
}
