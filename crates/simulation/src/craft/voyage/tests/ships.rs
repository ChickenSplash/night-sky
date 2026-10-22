//! Other craft as route and matching targets.
use super::*;
use crate::craft::WarpOrder;

/// Two ships at rest a little over an AU apart in a real system.
fn pair() -> Vec<Voyage> {
    let star = star();
    let chaser = in_system(&star, [AU, 0.0, 0.0]);
    let mut target = in_system(&star, [0.0, 0.0, AU]);
    target.craft.id = "craft-2".into();
    let mut fleet = vec![chaser, target];
    sight(&mut fleet);
    fleet
}

/// Show every craft the others as they are now.
fn sight(fleet: &mut [Voyage]) {
    let seen: Vec<_> = fleet.iter().map(Voyage::sighting).collect();
    for voyage in fleet {
        voyage.traffic.clone_from(&seen);
    }
}

/// One step of each craft, in the given order, each seeing the others as
/// they were as the step began.
fn step_all(fleet: &mut [Voyage], order: &[usize], library: &mut Library) {
    let seen: Vec<_> = fleet.iter().map(Voyage::sighting).collect();
    for &i in order {
        fleet[i].traffic.clone_from(&seen);
        library.step(&mut fleet[i]);
    }
}

fn warp(command: WarpOrder) -> Order {
    Order::Warp { command }
}

fn to_ship(target: &str, matched: bool, orbit: bool) -> Order {
    warp(WarpOrder::To {
        target: target.into(),
        matched,
        orbit,
        time: None,
    })
}

/// Step until the first craft's route ends.
fn chase(fleet: &mut [Voyage], library: &mut Library) {
    for _ in 0..4_000 {
        step_all(fleet, &[0, 1], library);
        if !fleet[0].craft.route.as_ref().unwrap().active() {
            return;
        }
    }
    panic!("never arrived: {:?}", fleet[0].craft.route);
}

/// Speed of the first craft relative to the second, in m/s.
fn relative_speed(fleet: &[Voyage]) -> f64 {
    let velocity = |v: &Voyage| DVec3::from_array(v.craft.state().velocity);
    (velocity(&fleet[0]) - velocity(&fleet[1])).length()
}

#[test]
fn a_coasting_ship_is_flown_to_and_matched_in_formation() {
    let mut fleet = pair();
    fleet[0].order(0, to_ship("craft-2", true, false)).unwrap();
    let route = fleet[0].craft.route.clone().unwrap();
    assert_eq!((route.stage, route.standoff_m), (Stage::Align, 1_000.0));
    assert_eq!(route.system, "");
    chase(&mut fleet, &mut Library::default());
    let route = fleet[0].craft.route.as_ref().unwrap();
    assert_eq!(route.stage, Stage::Arrived, "{route:?}");
    assert!(
        (route.distance_m / route.standoff_m - 1.0).abs() < 0.1,
        "{route:?}"
    );
    assert!(
        (fleet[0].craft.seconds() - route.trip_s).abs() < 0.5,
        "{route:?}"
    );
    // Falling together, both pulled the same way.
    assert!(relative_speed(&fleet) < 0.01, "{}", relative_speed(&fleet));
}

#[test]
fn a_thrusting_ship_is_chased_and_matched() {
    let mut fleet = pair();
    fleet[1].order(0, Order::Throttle { value: 1.0 }).unwrap();
    fleet[0].order(0, to_ship("craft-2", true, false)).unwrap();
    let mut library = Library::default();
    chase(&mut fleet, &mut library);
    let route = fleet[0].craft.route.as_ref().unwrap();
    assert_eq!(route.stage, Stage::Arrived, "{route:?}");
    assert!(
        (route.distance_m / route.standoff_m - 1.0).abs() < 0.1,
        "{route:?}"
    );
    // Matched as it was at the step's start; it has thrust for one step since.
    let speed = DVec3::from_array(fleet[1].craft.state().velocity).length();
    assert!(speed > 100.0, "{speed}");
    assert!(
        relative_speed(&fleet) < 0.01 * speed,
        "{}",
        relative_speed(&fleet)
    );
}

#[test]
fn a_warping_ship_is_caught_and_its_ordinary_velocity_matched() {
    let mut fleet = pair();
    fleet[1].order(0, warp(WarpOrder::Engage)).unwrap();
    fleet[1]
        .order(0, warp(WarpOrder::Set { speed_ms: 1e6 }))
        .unwrap();
    // Up to speed before the chase begins.
    let mut library = Library::default();
    for _ in 0..40 {
        step_all(&mut fleet, &[0, 1], &mut library);
    }
    let tick = fleet[0].craft.tick;
    fleet[0]
        .order(tick, to_ship("craft-2", true, false))
        .unwrap();
    chase(&mut fleet, &mut library);
    let route = fleet[0].craft.route.as_ref().unwrap();
    assert_eq!(route.stage, Stage::Arrived, "{route:?}");
    // Still closing fast, it drops out a step early rather than pass inside.
    let standoffs = route.distance_m / route.standoff_m;
    assert!((1.0..1.5).contains(&standoffs), "{route:?}");
    // The bubble is the target's alone: the chaser drops out at rest with it,
    // as it was, while it warps on.
    assert!(fleet[1].craft.warp.engaged);
    assert!(relative_speed(&fleet) < 0.01, "{}", relative_speed(&fleet));
}

#[test]
fn ships_chasing_each_other_fly_the_same_whatever_the_update_order() {
    let mut forward = pair();
    forward[0]
        .order(0, to_ship("craft-2", false, false))
        .unwrap();
    forward[1]
        .order(0, to_ship("craft-1", false, false))
        .unwrap();
    let mut backward = forward.clone();
    let mut library = Library::default();
    for _ in 0..1_000 {
        step_all(&mut forward, &[0, 1], &mut library);
        step_all(&mut backward, &[1, 0], &mut library);
        assert_eq!(forward[0].craft, backward[0].craft);
        assert_eq!(forward[1].craft, backward[1].craft);
    }
    for voyage in &forward {
        let route = voyage.craft.route.as_ref().unwrap();
        assert_eq!(route.stage, Stage::Arrived, "{route:?}");
    }
}

#[test]
fn a_despawned_target_releases_the_route_with_warp_left_on() {
    let mut fleet = pair();
    fleet[0].order(0, to_ship("craft-2", true, false)).unwrap();
    let mut library = Library::default();
    for _ in 0..20 {
        step_all(&mut fleet, &[0, 1], &mut library);
    }
    let mut alone = vec![fleet.remove(0)];
    step_all(&mut alone, &[0], &mut library);
    let craft = &alone[0].craft;
    let route = craft.route.as_ref().unwrap();
    assert_eq!(route.stage, Stage::Released);
    assert_eq!(route.end.as_deref(), Some("`craft-2` is gone"));
    assert!(craft.warp.engaged);
    assert_eq!(craft.autopilot, crate::craft::AttitudePilot::Manual);
}

#[test]
fn ship_targets_refuse_orbits_themselves_and_missing_craft() {
    let mut fleet = pair();
    let unchanged = fleet[0].craft.clone();
    for (order, reason) in [
        (to_ship("craft-2", false, true), "cannot be orbited"),
        (to_ship("craft-1", false, false), "itself"),
        (to_ship("craft-9", false, false), "No craft"),
    ] {
        let err = fleet[0].order(0, order).unwrap_err();
        assert!(err.contains(reason), "{err}");
        assert_eq!(fleet[0].craft, unchanged);
    }
    fleet[0].order(0, warp(WarpOrder::Engage)).unwrap();
    let matched = |body: &str, orbit| {
        warp(WarpOrder::Match {
            body: body.into(),
            orbit,
        })
    };
    for (order, reason) in [
        (matched("craft-2", true), "cannot be orbited"),
        (matched("craft-1", false), "itself"),
        (matched("craft-9", false), "No craft"),
    ] {
        let err = fleet[0].order(0, order).unwrap_err();
        assert!(err.contains(reason), "{err}");
    }
    // Formation by hand: its velocity, from wherever the ship is.
    fleet[1].craft.place(
        State {
            position: [0.0, 0.0, AU],
            velocity: [0.0, 30_000.0, 0.0],
        },
        0,
    );
    sight(&mut fleet);
    fleet[0].order(0, matched("craft-2", false)).unwrap();
    assert!(!fleet[0].craft.warp.engaged);
    assert!(relative_speed(&fleet) < 1e-9);
}

#[test]
fn a_ship_in_galactic_space_is_reached_from_a_system() {
    let star = star();
    let chaser = in_system(&star, [AU, 0.0, 0.0]);
    let mut target = galactic(&star, [0.3 * LY_METRES, 0.0, 0.0], [0.0; 3]);
    target.craft.id = "craft-2".into();
    let mut fleet = vec![chaser, target];
    sight(&mut fleet);
    fleet[0].order(0, to_ship("craft-2", true, false)).unwrap();
    chase(&mut fleet, &mut Library::default());
    let route = fleet[0].craft.route.as_ref().unwrap();
    assert_eq!(route.stage, Stage::Arrived, "{route:?}");
    assert!(
        (route.distance_m / route.standoff_m - 1.0).abs() < 0.1,
        "{route:?}"
    );
    assert!(fleet[0].in_galaxy());
    // Both in galactic frames, anchored apart: compare in the chaser's.
    sight(&mut fleet);
    let ships = fleet[0].ships();
    let gap = DVec3::from_array(ships[0].state.position)
        - DVec3::from_array(fleet[0].craft.state().position);
    assert!(
        (900.0..1_100.0).contains(&gap.length()),
        "{} m",
        gap.length()
    );
    let relative = DVec3::from_array(fleet[0].craft.state().velocity)
        - DVec3::from_array(ships[0].state.velocity);
    assert!(relative.length() < 0.01, "{relative}");
}
