use super::*;
use crate::AU_METRES;
use space_shared::{Barycentre, Orbit, Star};

fn star(letter: &str, mass_solar: f64) -> Star {
    Star {
        id: format!("t#{letter}"),
        page: 0,
        spectral: "G2 V".into(),
        kind: "Yellow dwarf".into(),
        mass_solar,
        radius_solar: 1.0,
        luminosity_solar: 1.0,
        temperature_k: 5_772.0,
        colour: [255; 3],
        spin: Default::default(),
        component: letter.into(),
    }
}

fn pair(letters: &str, members: [&str; 2], semi_major_au: f64) -> Barycentre {
    Barycentre {
        id: format!("t#{letters}"),
        page: 0,
        component: letters.into(),
        members: members.map(|m| format!("t#{m}")),
        orbit: Orbit {
            semi_major_au,
            ..Orbit::default()
        },
    }
}

/// A hierarchical triple of Sun-like stars, (AB)C: A and B 20 AU apart,
/// C 400 AU from their barycentre, with an Earth at 1 AU around A. At time
/// zero every orbit lies along +X: AB at -133 AU, C at +267 AU, and A and B
/// 10 AU either side of AB.
fn triple() -> Field {
    let planet = Planet {
        id: "t#A/0".into(),
        host: "t#A".into(),
        orbit: Orbit {
            semi_major_au: 1.0,
            ..Orbit::default()
        },
        radius_km: 6_371.0,
        mass_earth: 1.0,
        ..Planet::default()
    };
    let system = StarSystem {
        id: "t".into(),
        name: "Test".into(),
        position_ly: [0.0; 3],
        planets: vec![planet],
        stars: vec![star("A", 1.0), star("B", 1.0), star("C", 1.0)],
        barycentres: vec![
            pair("AB", ["A", "B"], 20.0),
            pair("ABC", ["AB", "C"], 400.0),
        ],
        page_stars: Vec::new(),
    };
    Field::new(system, 0)
}

fn at(field: &Field, id: &str, offset_au: [f64; 3]) -> [f64; 3] {
    let centre = field.reference_by_id(id, 0.0).unwrap().state.position;
    std::array::from_fn(|i| centre[i] + offset_au[i] * AU_METRES)
}

fn chosen(field: &Field, current: Option<&str>, position: [f64; 3]) -> String {
    field.select_reference(current, position, 0.0).unwrap().id
}

fn dropout_ship(field: &Field, body: &str) -> Craft {
    let mut craft = Craft::new(
        "ship".into(),
        "Ship".into(),
        CraftKind::Ship,
        Frame::System { star: "t#A".into() },
        1234,
    );
    let reference = field.reference_by_id(body, craft.seconds()).unwrap();
    craft.place(
        State {
            position: (DVec3::from_array(reference.state.position) + DVec3::X * 7_000_000.0)
                .to_array(),
            velocity: (DVec3::from_array(reference.state.velocity)
                + DVec3::new(200.0, 30.0, -400.0))
            .to_array(),
        },
        1234,
    );
    craft.engage_warp().unwrap();
    craft
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn matched_dropout_uses_current_analytic_body_velocity_without_moving_or_turning() {
    use crate::craft::{Order, WarpOrder, warp::GravityPolicy};
    let field = triple();
    for body in ["t#A/0", "t#A", "t#AB", "t#ABC"] {
        for policy in [GravityPolicy::Active, GravityPolicy::Suspended] {
            let mut craft = dropout_ship(&field, body);
            craft.warp.gravity = policy;
            let before = craft.clone();
            Order::Warp {
                command: WarpOrder::Match {
                    body: body.into(),
                    orbit: false,
                },
            }
            .apply(&mut craft, Some(&field))
            .unwrap();
            assert_eq!(craft.state().position, before.state().position);
            assert_eq!(craft.orientation, before.orientation);
            assert_eq!(craft.tick, before.tick);
            assert_eq!(
                craft.state().velocity,
                field
                    .reference_by_id(body, craft.seconds())
                    .unwrap()
                    .state
                    .velocity
            );
            assert!(!craft.warp.engaged);
            assert_eq!(craft.warp.actual_ms, 0.0);
            let exit = craft.warp.last_match.as_ref().unwrap();
            assert_eq!(exit.body, body);
            assert_eq!(exit.velocity_ms, craft.state().velocity);
            let restored: Craft =
                serde_json::from_str(&serde_json::to_string(&craft).unwrap()).unwrap();
            assert_eq!(restored, craft);
        }
    }
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn rough_orbit_exit_is_tangential_and_circular_in_two_body_estimate() {
    let field = triple();
    let body = "t#A/0";
    for initial in [
        DVec3::new(200.0, 30.0, -400.0),
        DVec3::ZERO,
        DVec3::X * 100.0,
    ] {
        let mut craft = dropout_ship(&field, body);
        let reference = field.reference_by_id(body, craft.seconds()).unwrap();
        craft.motion = Motion::Free(State {
            position: craft.state().position,
            velocity: (DVec3::from_array(reference.state.velocity) + initial).to_array(),
        });
        // Nose parallel to the radial direction exercises the up fallback.
        craft.orientation = DQuat::from_rotation_y(-std::f64::consts::FRAC_PI_2).to_array();
        let before = craft.clone();
        craft.matched_dropout(Some(&field), body, true).unwrap();
        let r =
            DVec3::from_array(craft.state().position) - DVec3::from_array(reference.state.position);
        let v =
            DVec3::from_array(craft.state().velocity) - DVec3::from_array(reference.state.velocity);
        assert!(r.normalize().dot(v).abs() < 1e-9);
        assert!((v.length() - (reference.gm / r.length()).sqrt()).abs() < 1e-9);
        let relative = State {
            position: r.to_array(),
            velocity: v.to_array(),
        };
        assert!(Elements::of(relative, reference.gm).eccentricity < 1e-12);
        assert_eq!(craft.state().position, before.state().position);
        assert_eq!(craft.orientation, before.orientation);
        assert!(craft.warp.last_match.as_ref().unwrap().orbit);
        // Coasting resumes ordinary gravity, never bubble motion.
        craft.step(&field);
        assert!(craft.state().velocity.iter().all(|v| v.is_finite()));
        assert_eq!(craft.tick, before.tick + 1);
    }
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn refused_matched_exits_are_atomic_and_do_not_stop_warp() {
    let field = triple();
    let mut craft = dropout_ship(&field, "t#A/0");
    for (body, orbit, missing) in [("remote/0", false, false), ("t#A/0", true, true)] {
        let before = craft.clone();
        assert!(
            craft
                .matched_dropout(if missing { None } else { Some(&field) }, body, orbit)
                .is_err()
        );
        assert_eq!(craft, before);
    }
    let mut state = craft.state();
    state.position = field
        .reference_by_id("t#A/0", craft.seconds())
        .unwrap()
        .state
        .position;
    craft.motion = Motion::Free(state);
    let before = craft.clone();
    assert!(craft.matched_dropout(Some(&field), "t#A/0", true).is_err());
    assert_eq!(craft, before);
    craft.frame = Frame::Galaxy {
        origin_pc: [0.0; 3],
    };
    let before = craft.clone();
    assert!(craft.matched_dropout(Some(&field), "t#A/0", false).is_err());
    assert_eq!(craft, before);
    craft.frame = Frame::System { star: "t#A".into() };
    craft.warp.stop("test");
    let before = craft.clone();
    assert!(craft.matched_dropout(Some(&field), "t#A/0", false).is_err());
    assert_eq!(craft, before);
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn queued_matched_exit_resolves_at_execution_tick_and_replays() {
    use crate::craft::{Order, WarpOrder, voyage::Voyage};
    let field = triple();
    let craft = dropout_ship(&field, "t#A/0");
    let tick = craft.tick + 5;
    let mut voyage = Voyage::new(craft, field.clone(), [0.0; 3]);
    voyage
        .order(
            tick,
            Order::Warp {
                command: WarpOrder::Match {
                    body: "t#A/0".into(),
                    orbit: false,
                },
            },
        )
        .unwrap();
    let snapshot: Craft =
        serde_json::from_str(&serde_json::to_string(&voyage.craft).unwrap()).unwrap();
    let mut restored = Voyage::new(snapshot, field.clone(), [0.0; 3]);
    let inputs: Vec<crate::craft::voyage::ScheduledOrder> =
        serde_json::from_str(&serde_json::to_string(&voyage.pending_orders()).unwrap()).unwrap();
    for input in inputs {
        restored.order(input.tick, input.order).unwrap();
    }
    while voyage.craft.tick < tick {
        assert!(voyage.craft.warp.engaged);
        assert!(voyage.advance().is_empty());
        assert!(restored.advance().is_empty());
        assert_eq!(voyage.craft, restored.craft);
    }
    assert!(!voyage.craft.warp.engaged);
    assert_eq!(
        voyage.craft.state().velocity,
        field
            .reference_by_id("t#A/0", tick_seconds(tick))
            .unwrap()
            .state
            .velocity
    );
    assert_eq!(voyage.craft.warp.last_match.as_ref().unwrap().tick, tick);
    assert!(voyage.pending_orders().is_empty());
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn the_reference_walks_down_the_stellar_hierarchy() {
    let field = triple();
    let ab = at(&field, "t#AB", [0.0; 3]);
    assert!((ab[0] / AU_METRES + 400.0 / 3.0).abs() < 1e-9, "{ab:?}");
    // Circumstellar, around one star of the close pair.
    assert_eq!(
        chosen(&field, None, at(&field, "t#A", [0.0, 0.0, 2.0])),
        "t#A"
    );
    // Circumbinary: outside both members' zones, inside the pair's.
    let around_pair = field
        .select_reference(None, at(&field, "t#AB", [0.0, 0.0, 30.0]), 0.0)
        .unwrap();
    assert_eq!(around_pair.id, "t#AB");
    assert_eq!(around_pair.kind, ReferenceKind::Barycentre);
    assert_eq!(around_pair.radius_m, 0.0);
    assert!((around_pair.gm / (2.0 * SUN_GM) - 1.0).abs() < 1e-12);
    // The distant third, alone.
    assert_eq!(
        chosen(&field, None, at(&field, "t#C", [0.0, 0.0, 20.0])),
        "t#C"
    );
    // Far outside everything, the whole system.
    let far = field.select_reference(None, [0.0, 0.0, 3_000.0 * AU_METRES], 0.0);
    let far = far.unwrap();
    assert_eq!(far.id, "t#ABC");
    assert_eq!(far.reach_m, f64::INFINITY);
    assert!((far.gm / (3.0 * SUN_GM) - 1.0).abs() < 1e-12);
    // Inside a planet's Hill sphere, the planet, with the velocity it orbits at.
    let planet = field
        .select_reference(None, at(&field, "t#A/0", [1e-4, 0.0, 0.0]), 0.0)
        .unwrap();
    assert_eq!(planet.kind, ReferenceKind::Planet);
    let host = field.reference_by_id("t#A", 0.0).unwrap().state.velocity;
    let speed = distance(planet.state.velocity, host);
    assert!((speed / 29_785.0 - 1.0).abs() < 1e-3, "{speed}");
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn a_reference_is_kept_past_its_boundary_by_the_margin() {
    let field = triple();
    let zone = field.reference_by_id("t#A", 0.0).unwrap().reach_m / AU_METRES;
    // Holman & Wiegert for an equal-mass circular pair, 20 AU apart.
    assert!((zone - (0.464 - 0.380 * 0.5) * 20.0).abs() < 1e-9, "{zone}");
    let just_out = at(&field, "t#A", [0.0, 0.0, zone * 1.05]);
    assert_eq!(chosen(&field, None, just_out), "t#AB");
    assert_eq!(chosen(&field, Some("t#A"), just_out), "t#A");
    // Held through a planet inside it, too.
    assert_eq!(chosen(&field, Some("t#A/0"), just_out), "t#A");
    // Past the margin it switches out, and back in only inside the zone.
    let beyond = at(&field, "t#A", [0.0, 0.0, zone * 1.15]);
    assert_eq!(chosen(&field, Some("t#A"), beyond), "t#AB");
    assert_eq!(chosen(&field, Some("t#AB"), just_out), "t#AB");

    let hill = field.reference_by_id("t#A/0", 0.0).unwrap().reach_m / AU_METRES;
    let edge = at(&field, "t#A/0", [hill * 1.05, 0.0, 0.0]);
    assert_eq!(chosen(&field, None, edge), "t#A");
    assert_eq!(chosen(&field, Some("t#A/0"), edge), "t#A/0");
    let gone = at(&field, "t#A/0", [hill * 1.15, 0.0, 0.0]);
    assert_eq!(chosen(&field, Some("t#A/0"), gone), "t#A");
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn a_craft_keeps_its_reference_and_points_relative_to_it() {
    let field = triple();
    let star = field.reference_by_id("t#A", 0.0).unwrap();
    // On the edge of A's zone, where with no history the pair would rule.
    let zone = star.reach_m;
    let offset = [0.0, 0.0, zone * 1.05];
    let state = State {
        position: std::array::from_fn(|i| star.state.position[i] + offset[i]),
        velocity: star.state.velocity,
    };
    let mut craft = Craft::new("c".into(), "C".into(), CraftKind::Ship, field_frame(), 0);
    craft.place(state, 0);
    craft.reference = Some("t#A".into());
    craft.point(PointTarget::RadialOut, None).unwrap();
    // Past a reference choice, a second in.
    craft.advance(&field, 1.2, usize::MAX);
    assert_eq!(craft.reference.as_deref(), Some("t#A"));
    // Radial out from A, not from the pair's barycentre 10 AU away.
    let direction = PointTarget::RadialOut
        .direction(
            craft.state(),
            craft.seconds(),
            &field,
            craft.reference.as_deref(),
        )
        .unwrap();
    assert!(direction.z > 0.999, "{direction:?}");
    // A saved craft keeps it.
    let copy: Craft = serde_json::from_str(&serde_json::to_string(&craft).unwrap()).unwrap();
    assert_eq!(copy.reference, craft.reference);
}

fn field_frame() -> Frame {
    Frame::System { star: "t".into() }
}
