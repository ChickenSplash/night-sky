use super::*;
use crate::craft::warp::GravityPolicy;
use crate::craft::{CraftKind, Frame, Order, WarpOrder};
use space_shared::{Barycentre, Orbit, Planet, Star, StarSystem};

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

/// A hierarchical triple, (AB)C: A and B `ab_au` apart, C 400 AU from
/// their barycentre, with an Earth at 1 AU around A.
fn triple(ab_au: f64) -> Field {
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
    Field::new(
        StarSystem {
            id: "t".into(),
            name: "Test".into(),
            position_ly: [0.0; 3],
            planets: vec![planet],
            stars: vec![star("A", 1.0), star("B", 1.0), star("C", 1.0)],
            barycentres: vec![
                pair("AB", ["A", "B"], ab_au),
                pair("ABC", ["AB", "C"], 400.0),
            ],
            page_stars: Vec::new(),
        },
        0,
    )
}

/// One Sun-like star, `t`, keyed by its own ID.
fn single() -> Field {
    let mut sun = star("A", 1.0);
    sun.id = "t".into();
    Field::new(
        StarSystem {
            id: "t".into(),
            name: "Single".into(),
            position_ly: [0.0; 3],
            planets: Vec::new(),
            stars: vec![sun],
            barycentres: Vec::new(),
            page_stars: Vec::new(),
        },
        0,
    )
}

/// A ship at rest `away` from a target's centre at time zero, nose along
/// `nose`, not yet in warp.
fn ship(field: &Field, target: &str, away: DVec3, nose: DVec3) -> Craft {
    let centre = field.target(target, 0.0).unwrap().state.position;
    let mut craft = Craft::new(
        "ship".into(),
        "Ship".into(),
        CraftKind::Ship,
        Frame::System { star: "t".into() },
        0,
    );
    craft.place(
        State {
            position: (DVec3::from_array(centre) + away).to_array(),
            velocity: [0.0; 3],
        },
        0,
    );
    craft.orientation = DQuat::from_rotation_arc(-DVec3::Z, nose.normalize()).to_array();
    craft
}

/// Step until the route ends, returning the seconds flown.
fn fly(craft: &mut Craft, field: &Field) -> f64 {
    let start = craft.seconds();
    for _ in 0..(2.0 * MAX_TRIP_S / STEP_SECONDS) as usize {
        if !craft.route.as_ref().unwrap().active() {
            break;
        }
        craft.step(field);
    }
    craft.seconds() - start
}

fn order(craft: &mut Craft, field: &Field, command: WarpOrder) -> Result<(), String> {
    Order::Warp { command }.apply(craft, Some(field))
}

fn to(target: &str, matched: bool, orbit: bool, time: Option<f64>) -> WarpOrder {
    WarpOrder::To {
        target: target.into(),
        matched,
        orbit,
        time,
    }
}

/// A ship lined up on `target` from `standoffs` standoffs away.
fn lined_up(field: &Field, target: &str, standoffs: f64) -> Craft {
    let standoff = field.target(target, 0.0).unwrap().standoff().unwrap();
    let away = DVec3::new(1.0, 0.3, -0.2).normalize() * standoff * standoffs;
    ship(field, target, away, -away)
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn every_kind_of_body_has_a_standoff_between_its_surface_and_reach() {
    let field = triple(20.0);
    let radius = crate::SUN_RADIUS_METRES;
    let planet = field.target("t#A/0", 0.0).unwrap();
    assert_eq!(planet.standoff().unwrap(), 4.0 * 6_371_000.0);
    let star = field.target("t#B", 0.0).unwrap();
    assert_eq!(star.standoff().unwrap(), 4.0 * radius);
    // A barycentre's size is its pair's separation; it keeps clear of its
    // stars, not of an empty centre.
    let pair = field.target("t#AB", 0.0).unwrap();
    assert!((pair.size_m / AU_METRES - 20.0).abs() < 1e-9);
    assert!((pair.clear_m - (10.0 * AU_METRES + KEEP_CLEAR * radius)).abs() < 1.0);
    let standoff = pair.standoff().unwrap();
    assert!(standoff > pair.clear_m && standoff < pair.reach_m.unwrap());
    // A system's key names its outermost barycentre.
    let system = field.target("t", 0.0).unwrap();
    assert_eq!(system.id, "t#ABC");
    assert_eq!(system.standoff().unwrap(), 4.0 * 400.0 * AU_METRES);
    assert_eq!(system.system, voyage::page_star_id(field.system(), 0));
    // A single star has no stable zone to stay inside.
    let alone = single().target("t", 0.0).unwrap();
    assert_eq!(alone.reach_m, Some(f64::INFINITY));
    assert_eq!(alone.standoff().unwrap(), 4.0 * radius);
    // In a tight pair a star's stable zone barely clears its surface.
    let tight = triple(0.02);
    let refused = tight.target("t#B", 0.0).unwrap().standoff();
    assert!(refused.unwrap_err().contains("no safe standoff"));
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn flies_to_each_kind_of_target_and_drops_out_at_its_standoff_on_time() {
    let (triple, single) = (triple(20.0), single());
    let targets = [
        (&triple, "t#A/0"),
        (&triple, "t#B"),
        (&triple, "t#AB"),
        (&triple, "t"),
        (&single, "t"),
    ];
    for (field, target) in targets {
        let mut craft = lined_up(field, target, 1e3);
        order(&mut craft, field, to(target, false, false, None)).unwrap();
        assert!(craft.warp.engaged);
        let route = craft.route.clone().unwrap();
        assert_eq!(route.trip_s, default_trip_s(route.distance_m));
        let flown = fly(&mut craft, field);
        let route = craft.route.as_ref().unwrap();
        assert_eq!(route.stage, Stage::Arrived, "{target}: {route:?}");
        assert!(!craft.warp.engaged);
        assert!(
            route.distance_m <= route.standoff_m && route.distance_m > 0.9 * route.standoff_m,
            "{target}: {route:?}"
        );
        assert!((flown - route.trip_s).abs() < 0.5, "{target}: {flown} s");
        assert_eq!(craft.autopilot, AttitudePilot::Manual);
        assert_eq!(craft.spin, [0.0; 3]);
    }
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn arrival_keeps_velocity_matches_or_drops_into_a_rough_orbit() {
    let field = triple(20.0);
    let planet = "t#A/0";
    for (matched, orbit) in [(false, false), (true, false), (false, true)] {
        let mut craft = lined_up(&field, planet, 1e4);
        let before = craft.state().velocity;
        order(&mut craft, &field, to(planet, matched, orbit, None)).unwrap();
        fly(&mut craft, &field);
        let target = field.target(planet, craft.seconds()).unwrap();
        let relative =
            DVec3::from_array(craft.state().velocity) - DVec3::from_array(target.state.velocity);
        let r = distance(craft.state().position, target.state.position);
        match (matched, orbit) {
            (false, false) => {
                // Only gravity during the trip changed it.
                let change = DVec3::from_array(craft.state().velocity) - DVec3::from_array(before);
                assert!(change.length() < 10.0, "{change}");
                assert!(craft.warp.last_match.is_none());
            }
            (true, false) => {
                // Gravity acts on the rest of the step after dropout.
                assert!(relative.length() < 0.1, "{relative}");
            }
            _ => {
                let circular = circular_speed(target.gm, r);
                assert!((relative.length() / circular - 1.0).abs() < 1e-3);
                let radial = (DVec3::from_array(craft.state().position)
                    - DVec3::from_array(target.state.position))
                    / r;
                assert!(relative.dot(radial).abs() < 1e-4 * circular);
                assert!(craft.warp.last_match.as_ref().unwrap().orbit);
            }
        }
    }
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn trip_times_from_ten_seconds_to_an_hour_are_kept() {
    let field = triple(20.0);
    for time in [MIN_TRIP_S, 60.0, 600.0, MAX_TRIP_S] {
        let mut craft = lined_up(&field, "t#A/0", 1e4);
        order(&mut craft, &field, to("t#A/0", true, false, Some(time))).unwrap();
        let flown = fly(&mut craft, &field);
        let route = craft.route.as_ref().unwrap();
        assert_eq!(route.stage, Stage::Arrived);
        assert!((flown - time).abs() < 0.5, "{time} s trip took {flown} s");
        assert!(route.late_s.abs() < 0.5);
    }
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn the_trip_time_includes_turning_round_first() {
    let field = triple(20.0);
    let target = "t#A/0";
    let standoff = field.target(target, 0.0).unwrap().standoff().unwrap();
    let away = DVec3::X * standoff * 1e4;
    // Facing away, the turn alone at 30 °/s takes most of ten seconds, so
    // the trip runs late and says so from the start.
    let mut craft = ship(&field, target, away, away);
    order(
        &mut craft,
        &field,
        to(target, false, false, Some(MIN_TRIP_S)),
    )
    .unwrap();
    let late = craft.route.as_ref().unwrap().late_s;
    assert!(late > 0.0);
    assert_eq!(craft.route.as_ref().unwrap().stage, Stage::Align);
    craft.step(&field);
    assert_eq!(craft.warp.requested_ms, MIN_SPEED, "held low while turning");
    let flown = fly(&mut craft, &field);
    assert!(
        (flown - MIN_TRIP_S - late).abs() < 1.5,
        "{flown} s, {late} s late"
    );
    // With time to spare, the same turn still arrives on time.
    let mut craft = ship(&field, target, away, away);
    order(&mut craft, &field, to(target, false, false, Some(30.0))).unwrap();
    assert!(craft.route.as_ref().unwrap().late_s.abs() < 1e-9);
    let flown = fly(&mut craft, &field);
    assert!((flown - 30.0).abs() < 0.5, "{flown} s");
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn both_gravity_policies_arrive() {
    let field = triple(20.0);
    for policy in [GravityPolicy::Active, GravityPolicy::Suspended] {
        let mut craft = lined_up(&field, "t#B", 100.0);
        order(&mut craft, &field, WarpOrder::Gravity { policy }).unwrap();
        order(&mut craft, &field, to("t#B", true, false, Some(120.0))).unwrap();
        let flown = fly(&mut craft, &field);
        assert_eq!(craft.route.as_ref().unwrap().stage, Stage::Arrived);
        assert!((flown - 120.0).abs() < 0.5, "{policy:?}: {flown} s");
    }
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn a_moving_target_is_met_despite_the_ships_own_drift() {
    let field = triple(20.0);
    // A slow trip, so the planet moves far further than its standoff, from
    // a ship already drifting fast across the line.
    let mut craft = lined_up(&field, "t#A/0", 1e3);
    let mut state = craft.state();
    state.velocity = [0.0, 0.0, 80_000.0];
    craft.place(state, 0);
    order(&mut craft, &field, to("t#A/0", true, false, Some(1_800.0))).unwrap();
    fly(&mut craft, &field);
    let route = craft.route.as_ref().unwrap();
    assert_eq!(route.stage, Stage::Arrived);
    assert!(route.distance_m > 0.9 * route.standoff_m);
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn cancelling_or_taking_over_leaves_warp_as_it_was() {
    let field = triple(20.0);
    let takeovers = [
        Order::Rotate {
            pitch: 0.0,
            yaw: 0.5,
            roll: 0.0,
        },
        Order::Point {
            target: PointTarget::Vector([1.0, 0.0, 0.0]),
            up: None,
        },
        Order::Stop,
        Order::Release,
        Order::Cut,
        Order::Warp {
            command: WarpOrder::Up,
        },
        Order::Warp {
            command: WarpOrder::Set { speed_ms: 1e6 },
        },
        Order::Warp {
            command: WarpOrder::Cancel,
        },
    ];
    for takeover in takeovers {
        let mut craft = lined_up(&field, "t#B", 1e3);
        order(&mut craft, &field, to("t#B", false, false, Some(60.0))).unwrap();
        for _ in 0..200 {
            craft.step(&field);
        }
        assert_eq!(craft.route.as_ref().unwrap().stage, Stage::Cruise);
        let (speed, orientation) = (craft.warp.actual_ms, craft.orientation);
        takeover.apply(&mut craft, Some(&field)).unwrap();
        let route = craft.route.as_ref().unwrap();
        assert_eq!(route.stage, Stage::Released, "{takeover:?}");
        assert!(craft.warp.engaged);
        assert_eq!(
            (craft.warp.actual_ms, craft.orientation),
            (speed, orientation)
        );
        if matches!(takeover, Order::Warp { .. }) {
            assert_eq!(craft.autopilot, AttitudePilot::Manual);
        }
        let requested = craft.warp.requested_ms;
        craft.step(&field);
        assert_eq!(craft.warp.requested_ms, requested, "{takeover:?}");
    }
    let mut craft = lined_up(&field, "t#B", 1e3);
    let cancel = WarpOrder::Cancel;
    assert!(order(&mut craft, &field, cancel.clone()).is_err());
    order(&mut craft, &field, to("t#B", false, false, None)).unwrap();
    order(&mut craft, &field, WarpOrder::Disengage).unwrap();
    assert_eq!(craft.route.as_ref().unwrap().end.as_deref(), Some(TAKEOVER));
    assert!(order(&mut craft, &field, cancel).is_err());
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn refused_routes_leave_the_ship_unchanged() {
    let field = triple(20.0);
    let tight = triple(0.02);
    let craft = lined_up(&field, "t#B", 1e3);
    let refusals: [(&Field, WarpOrder, &str); 6] = [
        (&field, to("nowhere", false, false, None), "No body"),
        (&field, to("ship", false, false, None), "itself"),
        (&tight, to("t#B", false, false, None), "no safe standoff"),
        (&field, to("t#B", false, false, Some(9.0)), "Trip time"),
        (&field, to("t#B", false, false, Some(3_601.0)), "Trip time"),
        (&field, to("t#B", false, false, Some(f64::NAN)), "Trip time"),
    ];
    for (field, command, reason) in refusals {
        let mut tried = craft.clone();
        let err = order(&mut tried, field, command).unwrap_err();
        assert!(err.contains(reason), "{err}");
        assert_eq!(tried, craft);
    }
    let mut tried = craft.clone();
    let err = Order::Warp {
        command: to("t#B", false, false, None),
    }
    .apply(&mut tried, None)
    .unwrap_err();
    assert!(err.contains("galactic"), "{err}");
    // Already there.
    let mut near = lined_up(&field, "t#B", 0.5);
    let before = near.clone();
    let err = order(&mut near, &field, to("t#B", false, false, None)).unwrap_err();
    assert!(err.contains("Already within"), "{err}");
    assert_eq!(near, before);
    // Sublight thrust must be cut before warp engages.
    let mut thrusting = craft.clone();
    thrusting.input.controls.throttle = 0.5;
    let before = thrusting.clone();
    assert!(order(&mut thrusting, &field, to("t#B", false, false, None)).is_err());
    assert_eq!(thrusting, before);
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn a_route_replays_exactly_from_a_snapshot() {
    let field = triple(20.0);
    let mut craft = lined_up(&field, "t#A/0", 1e4);
    order(&mut craft, &field, to("t#A/0", false, true, Some(40.0))).unwrap();
    for _ in 0..300 {
        craft.step(&field);
    }
    let mut restored: Craft =
        serde_json::from_str(&serde_json::to_string(&craft).unwrap()).unwrap();
    fly(&mut craft, &field);
    fly(&mut restored, &field);
    assert_eq!(restored, craft);
    assert_eq!(craft.route.as_ref().unwrap().stage, Stage::Arrived);
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn the_profile_fits_its_time_and_scales_its_default() {
    let (distance, standoff) = (1e12, 1e7);
    let tau = fit_tau(distance, standoff, 30.0);
    assert!((profile_seconds(distance, standoff, tau) - 30.0).abs() < 1e-9);
    assert_eq!(fit_tau(distance, standoff, 0.0), MIN_TAU_S);
    // At the top speed the profile is cut off and the trip stretches.
    let far = 1e5 * crate::LY_METRES;
    assert!(profile_seconds(far, standoff, 1.0) > (far / standoff).ln());
    assert_eq!(default_trip_s(0.01 * AU_METRES), SHORT_TRIP_S);
    assert!((default_trip_s(100.0 * AU_METRES) - 40.0).abs() < 1e-9);
    assert_eq!(default_trip_s(crate::LY_METRES), LONG_TRIP_S);
    assert_eq!(align_seconds(0.01, 0.5), 0.0);
    // Facing away at 30 °/s: about six seconds flat out, then easing in.
    let turn = align_seconds(std::f64::consts::PI, 30f64.to_radians());
    assert!((7.0..10.0).contains(&turn), "{turn}");
}
