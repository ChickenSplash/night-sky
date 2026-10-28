use super::*;
use crate::star::{formation, galactic_formation};
use crate::system::{page_capacity, star_id};
use std::collections::BTreeMap;

#[test]
fn formation_starts_with_validated_fallback_and_only_projects_compatibility_fields() {
    for seed in 0..8 {
        let key = format!("formation-initial-result:{seed}");
        let (formed, _, _) = census(&key);
        for (index, body) in generate_system(&key, &formed, false, f64::INFINITY)
            .iter()
            .enumerate()
        {
            crate::system::physical::validate_result(&body.initial_result).unwrap();
            let facts = &body.initial_result.record.physical;
            assert_eq!(
                body.structure.approximations(),
                &[structure::Approximation::ProvisionalFallback]
            );
            assert_eq!(body.structure.radius_m(), facts.reference_radius_m);
            assert_eq!(body.structure.inertia_factor(), facts.inertia_factor);
            assert_eq!(body.radius_km, facts.reference_radius_m / 1000.0);
            assert_eq!(body.class, materials::classify(facts));
            assert!(
                body.initial_result
                    .revision
                    .inputs
                    .starts_with(&format!("{key}/{index}:"))
            );
            assert_eq!(body.formation_mass_earth, body.mass_earth);
            assert_eq!(body.budget.source(), body.budget.retained());
        }
    }
}

#[test]
fn systems_are_keyed_by_star() {
    let star = star_id("list", 3);
    assert_eq!(star, "list:page-3");
    assert_eq!(
        planetary_system(&star).len() as u32,
        page_capacity("list", 3)
    );
    assert_eq!(crate::star(&star).id, star);
    assert_ne!(
        planetary_system("elsewhere")[0].orbit,
        planetary_system(&star)[0].orbit
    );
}

fn systems() -> impl Iterator<Item = Vec<Body>> {
    (0..400).map(|page| planetary_system(&star_id("list", page)))
}

/// Each host's planets on the first 400 legacy pages: the page, the
/// host's mass, whether it is the primary, and its planets.
fn hosted() -> impl Iterator<Item = (u32, f64, bool, Vec<Body>)> {
    (0..400).flat_map(|page| {
        let bodies = bodies(&star_id("list", page));
        let primary = bodies.stars[0].id.as_str();
        bodies
            .by_host()
            .into_iter()
            .map(|(host, planets)| {
                (
                    page,
                    bodies.mass(host),
                    host == primary,
                    planets.into_iter().cloned().collect(),
                )
            })
            .collect::<Vec<_>>()
    })
}

#[test]
fn planet_counts_vary_within_bounds() {
    let (mut single, mut multiple) = (Vec::new(), Vec::new());
    for page in 0..400 {
        let bodies = bodies(&star_id("list", page));
        let count = bodies.planets.len() as u32;
        if bodies.stars.len() == 1 {
            single.push(count);
        } else {
            multiple.push(count);
        }
    }
    assert!(
        single
            .iter()
            .all(|c| (MIN_PLANETS..=MAX_PLANETS).contains(c))
    );
    for n in MIN_PLANETS..=MAX_PLANETS {
        assert!(single.contains(&n));
    }
    // Companions cut some systems short, and give others planets around
    // more than one star.
    assert!(multiple.iter().any(|&c| c < MIN_PLANETS));
    assert!(multiple.iter().any(|&c| c > MAX_PLANETS));
}

#[test]
fn sizes_run_from_pluto_to_just_past_jupiter() {
    let radii: Vec<_> = systems().flatten().map(|b| b.radius_km).collect();
    let (low, high) = radii
        .iter()
        .fold((f64::MAX, 0.0f64), |(l, h), r| (l.min(*r), h.max(*r)));
    // Metal-rich small bodies can be smaller than icy Pluto at the same mass.
    assert!((700.0..1_400.0).contains(&low), "smallest {low} km");
    assert!((70_000.0..84_000.0).contains(&high), "largest {high} km");
}

#[test]
fn neighbours_are_hill_stable_and_never_cross() {
    for (_, star, _, bodies) in hosted() {
        for pair in bodies.windows(2) {
            let (inner, outer) = (&pair[0], &pair[1]);
            assert!(outer.orbit.semi_major_au > inner.orbit.semi_major_au);
            assert!(stable(
                inner,
                outer.orbit.semi_major_au,
                outer.orbit.eccentricity,
                outer.mass_earth,
                star
            ));
        }
    }
}

#[test]
fn crowded_pages_stay_within_the_planetary_system_limit() {
    let mut compact_count = 0;
    for list in ["list", "demo-flight", "demo-research"] {
        for page in 0..1_000 {
            let star = star_id(list, page);
            let formed = formation(&star);
            let ordinary = generate_system(&star, &formed, false, f64::INFINITY);
            if ordinary.last().is_some_and(|body| {
                body.orbit.semi_major_au * (1.0 + body.orbit.eccentricity) > MAX_SYSTEM_APOAPSIS_AU
            }) {
                compact_count += 1;
            }
            let generated = bodies(&star);
            assert_eq!(generated.planets.len(), page_capacity(list, page) as usize);
            for (host, planets) in generated.by_host() {
                for body in &planets {
                    let apoapsis = body.orbit.semi_major_au * (1.0 + body.orbit.eccentricity);
                    assert!(
                        apoapsis <= MAX_SYSTEM_APOAPSIS_AU,
                        "{list} page {page}: {apoapsis} AU"
                    );
                    if host == generated.stars[0].id {
                        assert!(body.orbit.semi_major_au >= formed.inner_au);
                    }
                }
                for pair in planets.windows(2) {
                    assert!(stable(
                        pair[0],
                        pair[1].orbit.semi_major_au,
                        pair[1].orbit.eccentricity,
                        pair[1].mass_earth,
                        generated.mass(host)
                    ));
                }
            }
        }
    }
    assert!(compact_count > 0, "compact generation was never exercised");
}

#[test]
fn galactic_stars_of_every_kind_have_stable_bounded_systems() {
    // A sample of every component, and the rare massive stars of the arms.
    let rare = (0..300_000)
        .map(|i| format!("Arms:{i}"))
        .filter(|star| {
            let kind = galactic_formation(star, Component::Arms).star.kind;
            kind.starts_with("Blue") || kind.contains("supergiant")
        })
        .map(|star| (star, Component::Arms));
    let sample = Component::ALL
        .into_iter()
        .flat_map(|c| (0..2_000).map(move |i| (format!("{c:?}:{i}"), c)));
    let mut kinds = std::collections::HashSet::new();
    for (star, component) in sample.chain(rare) {
        let formed = galactic_formation(&star, component);
        kinds.insert(formed.star.kind.clone());
        let generated = galactic_bodies(&star, component);
        for (host, planets) in generated.by_host() {
            for body in &planets {
                let apoapsis = body.orbit.semi_major_au * (1.0 + body.orbit.eccentricity);
                assert!(apoapsis <= MAX_SYSTEM_APOAPSIS_AU, "{star}: {apoapsis} AU");
                if host == generated.stars[0].id {
                    assert!(body.orbit.semi_major_au >= formed.inner_au);
                }
            }
            for pair in planets.windows(2) {
                assert!(stable(
                    pair[0],
                    pair[1].orbit.semi_major_au,
                    pair[1].orbit.eccentricity,
                    pair[1].mass_earth,
                    generated.mass(host)
                ));
            }
        }
    }
    for kind in [
        "Blue main-sequence star",
        "Blue supergiant",
        "Red supergiant",
        "White dwarf",
    ] {
        assert!(kinds.contains(kind), "no {kind} among {kinds:?}");
    }
}

#[test]
fn resonant_pairs_have_exact_periods_and_protected_conjunctions() {
    let mut found = 0;
    for (_, _, _, bodies) in hosted() {
        for pair in bodies.windows(2) {
            let Some([p, q]) = pair[1].resonance else {
                continue;
            };
            found += 1;
            let (inner, outer) = (&pair[0].orbit, &pair[1].orbit);
            let ratio = period_seconds(outer, 1.0) / period_seconds(inner, 1.0);
            assert!((ratio - f64::from(p) / f64::from(q)).abs() < 1e-9);
            let (p, q) = (f64::from(p), f64::from(q));
            let angle = p * (outer.ascending_node_rad + outer.phase_rad)
                - q * (inner.ascending_node_rad + inner.phase_rad)
                - (p - q) * outer.ascending_node_rad;
            assert!((angle.rem_euclid(TAU) - std::f64::consts::PI).abs() < 1e-6);
        }
    }
    assert!(found > 100, "only {found} resonant pairs");
}

#[test]
fn formation_water_follows_the_proposed_snow_line_not_relocated_orbit() {
    for (page, _, primary, bodies) in hosted() {
        if !primary {
            continue;
        }
        let formed = formation(&star_id("list", page));
        for body in bodies {
            use crate::system::composition::Material;
            let source = body.budget.source();
            let heavy = source.total_mass_kg()
                - source.mass_kg(Material::Hydrogen)
                - source.mass_kg(Material::Helium);
            let water_share = source.mass_kg(Material::Water) / heavy;
            if body.formation_distance_au < 0.3 * formed.snow_au {
                assert!(water_share < 0.005);
            }
            if body.formation_distance_au > 2.0 * formed.snow_au {
                assert!(water_share > 0.04);
            }
            assert!(body.orbit.semi_major_au >= formed.inner_au);
        }
    }
}

#[test]
fn spins_follow_tides_and_physical_envelopes() {
    let (mut locked, mut retrograde, mut total) = (0, 0, 0);
    for bodies in systems() {
        for body in bodies {
            total += 1;
            let spin = body.spin;
            assert!(spin.period_hours > 0.0 && (0.0..=PI).contains(&spin.tilt_rad));
            if spin.locked {
                locked += 1;
                assert!(spin.tilt_rad < 0.1);
            } else if body.structure.h_he_mass_fraction() >= 0.5 {
                assert!((9.0..=15.0).contains(&spin.period_hours));
            }
            if spin.tilt_rad > PI / 2.0 {
                retrograde += 1;
            }
        }
    }
    assert!(
        locked > total / 20 && locked < total / 2,
        "{locked} of {total} locked"
    );
    assert!(retrograde > 0 && retrograde < total / 4);
}

#[test]
fn spins_keep_bodies_round_enough() {
    use crate::system::shape::{MAX_FLATTENING, fastest_planet_hours, planet_flattening};
    let (mut slowed, mut total) = (0, 0);
    let (mut flat, mut slowed_by) = (
        BTreeMap::<&str, usize>::new(),
        BTreeMap::<&str, usize>::new(),
    );
    for body in systems().flatten() {
        total += 1;
        let spin = body.spin;
        let f = planet_flattening(
            body.structure.inertia_factor(),
            body.mass_earth,
            body.radius_km,
            spin.period_hours,
        );
        let fastest = fastest_planet_hours(
            body.structure.inertia_factor(),
            body.mass_earth,
            body.radius_km,
        );
        if !spin.locked {
            assert!(spin.period_hours >= fastest * (1.0 - 1e-12), "{body:?}");
        }
        assert!(f <= MAX_FLATTENING, "{body:?}");
        if spin.period_hours == fastest {
            slowed += 1;
            *slowed_by.entry(body.class.name()).or_default() += 1;
        }
        if f > 0.05 {
            *flat.entry(body.class.name()).or_default() += 1;
        }
    }
    eprintln!("{slowed} of {total} slowed to the limit {slowed_by:?}; flatter than 0.05: {flat:?}");
    assert!(slowed < total / 10, "{slowed} of {total} slowed");
}

#[test]
fn inventories_match_final_structure_and_regenerate_independently_of_order() {
    use crate::system::composition::Material;
    for component in Component::ALL {
        let keys: Vec<_> = (0..40)
            .map(|i| format!("inventory:{component:?}:{i}"))
            .collect();
        let forward: Vec<_> = keys
            .iter()
            .map(|key| galactic_bodies(key, component))
            .collect();
        for (key, expected) in keys.iter().zip(&forward).rev() {
            let actual = galactic_bodies(key, component);
            for (a, b) in actual.planets.iter().zip(&expected.planets) {
                let body = &a.body;
                assert_eq!(a.id(), b.id());
                assert_eq!(body.budget, b.body.budget);
                assert_eq!(body.structure, b.body.structure);
                assert_eq!(body.spin, b.body.spin);
                assert_eq!(body.orbit, b.body.orbit);
                assert_eq!(body.mass_earth, body.structure.mass_kg() / EARTH_MASS_KG);
                assert_eq!(body.radius_km, body.structure.radius_m() / 1000.0);
                assert!(body.mass_earth <= body.formation_mass_earth);
                assert!(
                    body.formation_mass_earth
                        <= MAX_MASS_RATIO * actual.mass(&a.host) / EARTH_MASS_SOLAR * (1.0 + 1e-12)
                );
                assert_eq!(
                    body.structure.mass_kg(),
                    body.budget.retained().total_mass_kg()
                );
                for material in Material::ALL {
                    let original = body.budget.source().mass_kg(material);
                    let accounted = body.budget.retained().mass_kg(material)
                        + body.budget.escaped().mass_kg(material);
                    assert!((original - accounted).abs() <= original * 1e-14);
                }
                // Spin draws and structure must not use the compatibility label.
                let mut renamed = body.clone();
                renamed.class = Class::WaterRich;
                let spin =
                    |b: &Body| planet_spin(&mut Rng::new("same-spin"), b, actual.mass(&a.host));
                assert_eq!(spin(body), spin(&renamed));
            }
            for (host, planets) in actual.by_host() {
                for pair in planets.windows(2) {
                    let mut inner = pair[0].clone();
                    inner.mass_earth = inner.formation_mass_earth;
                    assert!(stable(
                        &inner,
                        pair[1].orbit.semi_major_au,
                        pair[1].orbit.eccentricity,
                        pair[1].formation_mass_earth,
                        actual.mass(host)
                    ));
                }
            }
        }
    }
}
