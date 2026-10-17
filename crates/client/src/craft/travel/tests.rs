use super::super::control::{Order, WarpOrder};
use super::*;

fn live() -> Live {
    let galaxy = space_simulation::demo_galaxy();
    let system = galaxy.systems[0].clone();
    let page = system.first_page();
    let body = system.on_page(page).next().unwrap().id.clone();
    let origin = crate::scene::page_origin_ly(&system, page) * (LY_METRES / PC_METRES)
        + DVec3::from_array(GalaxyModel::milky_way().home_pc);
    let mut fleet = Fleet::default();
    fleet
        .spawn(system, page, &body, 400_000.0, Launch::Circular, 0.0)
        .unwrap();
    let mut live = fleet.craft.remove(0);
    live.system_pc = origin.to_array();
    live
}

fn discover_current(live: &mut Live) {
    live.nearby.fields = vec![Known {
        field: live.field.clone(),
        origin_pc: live.system_pc,
    }];
    live.nearby.ready = true;
}

#[test]
fn unsafe_warp_crosses_unknown_space_at_ten_light_years_per_second() {
    let mut live = live();
    live.craft.warp.gravity = space_simulation::craft::warp::GravityPolicy::Suspended;
    live.craft.engage_warp().unwrap();
    let speed = 10.0 * LY_METRES;
    live.craft.warp.set_speed(speed).unwrap();
    let start = live.position_pc(0.0);
    for _ in 0..100 {
        live.previous = live.craft.state();
        live.travel_step();
        assert!(live.craft.warp.engaged);
        assert!(live.craft.warp.stop_reason.is_none());
    }
    assert!(distance(start, live.position_pc(live.craft.seconds)) * PC_METRES > 40.0 * LY_METRES);
    assert!((live.craft.warp.actual_ms / speed - 1.0).abs() < 1e-6);
    // Even an explicit discovery error is informational, not a drive interlock.
    live.nearby.error = Some("discovery failed".into());
    live.travel_step();
    assert!(live.craft.warp.engaged);
}

#[test]
fn unsafe_warp_crosses_a_planet_and_discards_the_forecast() {
    let mut live = live();
    let planet = live
        .field
        .bodies(0.0)
        .into_iter()
        .find(|b| b.kind == space_simulation::craft::BodyKind::Planet)
        .unwrap();
    let centre = DVec3::from_array(planet.state.position);
    let start = centre + DVec3::Z * (planet.radius_m + 400_000.0);
    live.craft.place(
        State {
            position: start.to_array(),
            velocity: [0.0; 3],
        },
        0.0,
    );
    live.previous = live.craft.state();
    discover_current(&mut live);
    live.craft.warp.set_speed(1e12).unwrap();
    live.craft.engage_warp().unwrap();
    live.travel_step();
    assert!(live.craft.warp.engaged);
    assert!(live.craft.warp.stop_reason.is_none());
    assert!(live.craft.state().position[2] < centre.z - planet.radius_m);
    assert!(live.forecast.shown.is_none());
}

#[test]
fn unsafe_warp_can_engage_inside_a_planet() {
    let mut live = live();
    let planet = live
        .field
        .bodies(0.0)
        .into_iter()
        .find(|b| b.kind == space_simulation::craft::BodyKind::Planet)
        .unwrap();
    live.craft.place(planet.state, 0.0);
    Order::Warp {
        command: WarpOrder::Engage,
    }
    .apply(&mut live.craft, Some(&live.field))
    .unwrap();
    assert!(live.craft.warp.engaged);
}

#[test]
fn maximum_warp_ignores_discovery_refresh_and_failures() {
    let mut live = live();
    let galaxy = space_simulation::demo_galaxy();
    live.craft.warp.gravity = space_simulation::craft::warp::GravityPolicy::Suspended;
    live.craft
        .warp
        .set_speed(space_simulation::craft::warp::MAX_SPEED)
        .unwrap();
    live.craft.engage_warp().unwrap();
    for _ in 0..100 {
        live.prepare_nearby(&galaxy);
        live.nearby.error = Some("discovery unavailable".into());
        live.previous = live.craft.state();
        live.travel_step();
        assert!(live.craft.warp.engaged);
        assert!(live.craft.warp.stop_reason.is_none());
        assert!(live.craft.state().position.iter().all(|v| v.is_finite()));
    }
    assert!(
        (live.craft.warp.actual_ms / space_simulation::craft::warp::MAX_SPEED - 1.0).abs() < 1e-6
    );
}

#[test]
fn current_frame_gravity_does_not_wait_for_discovery() {
    let mut live = live();
    live.craft.engage_warp().unwrap();
    let (expected, _, _) = live.craft.warp_candidate(&live.field);
    live.travel_step();
    assert!(distance(expected.position, live.craft.state().position) < 0.01);
    assert!(distance(expected.velocity, live.craft.state().velocity) < 1e-8);
}

#[test]
fn handoffs_preserve_position_heading_and_galactic_momentum() {
    let mut live = live();
    discover_current(&mut live);
    let state = State {
        position: [3.0 * range(&live.field), 0.0, 0.0],
        velocity: [123.0, 45.0, 67.0],
    };
    live.craft.place(state, 0.0);
    live.previous = state;
    live.displayed = Some(state);
    let origin = DVec3::from_array(live.origin_pc());
    let global_position = live.basis() * DVec3::from_array(state.position);
    let global_velocity = live.basis() * DVec3::from_array(state.velocity)
        + frame_velocity(&live.craft.frame, live.system_pc);
    let heading = live.basis() * DQuat::from_array(live.craft.orientation) * -DVec3::Z;
    live.handoff();
    assert!(matches!(live.craft.frame, Frame::Galaxy { .. }));
    assert_eq!(live.displayed.unwrap(), live.craft.state());
    assert!((DVec3::from_array(live.craft.state().position) - global_position).length() < 0.1);
    assert!((DVec3::from_array(live.craft.state().velocity) - global_velocity).length() < 1e-8);
    assert!(
        (live.basis() * DQuat::from_array(live.craft.orientation) * -DVec3::Z - heading).length()
            < 1e-12
    );
    // Enter a distinct destination, not just the departure frame again.
    let destination = origin + global_position / PC_METRES;
    let mut field = live.nearby.fields[0].field.clone();
    let mut system = field.system().clone();
    system.id = "destination".into();
    system.page_stars[0].id = "destination".into();
    field = Field::new(system, field.page());
    live.nearby.fields = vec![Known {
        field,
        origin_pc: destination.to_array(),
    }];
    let before = live.craft.state();
    live.handoff();
    assert!(matches!(live.craft.frame, Frame::System { .. }));
    assert_eq!(live.displayed.unwrap(), live.craft.state());
    let after = live.craft.state();
    let represented = (DVec3::from_array(live.origin_pc()) - origin) * PC_METRES
        + live.basis() * DVec3::from_array(after.position);
    assert!((represented - DVec3::from_array(before.position)).length() < 0.1);
    let velocity = live.basis() * DVec3::from_array(after.velocity)
        + frame_velocity(&live.craft.frame, live.system_pc);
    assert!((velocity - global_velocity).length() < 1e-8);
}

#[test]
fn warp_orders_reject_thrust_and_turning_and_validate_speed() {
    let mut live = live();
    let order = |command| Order::Warp { command };
    order(WarpOrder::Engage)
        .apply(&mut live.craft, Some(&live.field))
        .unwrap();
    assert!(
        Order::Throttle { value: 0.1 }
            .apply(&mut live.craft, Some(&live.field))
            .is_err()
    );
    assert!(
        Order::Rotate {
            pitch: 0.1,
            yaw: 0.0,
            roll: 0.0
        }
        .apply(&mut live.craft, Some(&live.field))
        .is_err()
    );
    assert!(
        Order::Stop
            .apply(&mut live.craft, Some(&live.field))
            .is_err()
    );
    assert!(
        order(WarpOrder::Set { speed_ms: f64::NAN })
            .apply(&mut live.craft, Some(&live.field))
            .is_err()
    );
    order(WarpOrder::Up)
        .apply(&mut live.craft, Some(&live.field))
        .unwrap();
    assert!(live.craft.warp.requested_ms > 1.0);
    order(WarpOrder::Disengage)
        .apply(&mut live.craft, Some(&live.field))
        .unwrap();
    Order::Throttle { value: 0.1 }
        .apply(&mut live.craft, Some(&live.field))
        .unwrap();
    assert!(
        order(WarpOrder::Engage)
            .apply(&mut live.craft, Some(&live.field))
            .is_err()
    );
}

#[test]
fn current_frame_does_not_search_or_generate_neighbouring_systems() {
    let mut live = live();
    let galaxy = space_simulation::demo_galaxy();
    live.prepare_nearby(&galaxy);
    assert!(live.nearby.ready, "{}", live.nearby_status());
    assert!(live.nearby.fields.is_empty());
    assert!(live.nearby.pending.is_none());
}

#[test]
fn empty_space_travel_never_starts_discovery_jobs_or_accumulates_systems() {
    let mut live = live();
    live.craft.frame = Frame::Galaxy {
        origin_pc: live.system_pc,
    };
    let empty = Galaxy {
        source: "test".into(),
        systems: vec![],
    };
    live.craft.warp.set_speed(10.0 * LY_METRES).unwrap();
    live.craft.engage_warp().unwrap();
    for _ in 0..200 {
        live.prepare_nearby(&empty);
        assert!(live.nearby.pending.is_none());
        assert!(live.nearby.fields.is_empty());
        live.previous = live.craft.state();
        let velocity = live.craft.state().velocity;
        live.travel_step();
        assert_eq!(live.craft.state().velocity, velocity);
        assert!(live.craft.warp.engaged);
    }
}

#[test]
fn loaded_list_proximity_supplies_arrival_without_body_generation() {
    let mut live = live();
    let galaxy = space_simulation::demo_galaxy();
    live.craft.frame = Frame::Galaxy {
        origin_pc: live.system_pc,
    };
    live.craft.place(
        State {
            position: [1e7, 0.0, 0.0],
            velocity: [0.0; 3],
        },
        0.0,
    );
    live.previous = live.craft.state();
    live.prepare_nearby(&galaxy);
    assert!(live.nearby.pending.is_none());
    assert_eq!(live.nearby.fields.len(), 1);
    live.handoff();
    assert!(!live.star().is_empty());
}

#[test]
fn background_result_is_discarded_after_the_ship_has_passed_it() {
    let mut live = live();
    let origin = live.system_pc;
    let id = live.star().to_owned();
    live.request_arrival(id.clone(), origin);
    // One in-flight request is retained instead of spawning a job every frame.
    live.request_arrival("another".into(), origin);
    assert_eq!(live.nearby.pending.as_ref().unwrap().id, id);
    live.craft.frame = Frame::Galaxy { origin_pc: origin };
    live.craft.place(
        State {
            position: [10.0 * LY_METRES, 0.0, 0.0],
            velocity: [0.0; 3],
        },
        0.0,
    );
    live.previous = live.craft.state();
    let empty = Galaxy {
        source: "test".into(),
        systems: vec![],
    };
    for _ in 0..1000 {
        live.prepare_nearby(&empty);
        if live.nearby.pending.is_none() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert!(live.nearby.pending.is_none());
    assert!(live.nearby.fields.is_empty());
    live.handoff();
    assert!(live.star().is_empty());
}

#[test]
fn continuous_warp_leaves_one_system_and_enters_another() {
    let mut live = live();
    let page = live.field.page();
    let mut source = live.field.system().clone();
    source.planets.clear();
    source.barycentres.clear();
    source.stars.retain(|s| s.page == page);
    source.stars.truncate(1);
    live.field = Field::new(source.clone(), page);
    let origin = live.system_pc;
    let start = [0.03 * LY_METRES, 0.0, 0.0];
    live.craft.place(
        State {
            position: start,
            velocity: [0.0; 3],
        },
        0.0,
    );
    live.previous = live.craft.state();
    live.craft.orientation = DQuat::from_rotation_y(-std::f64::consts::FRAC_PI_2).to_array();
    discover_current(&mut live);
    let destination_pc = (DVec3::from_array(origin)
        + live.basis() * DVec3::X * (0.12 * LY_METRES / PC_METRES))
        .to_array();
    source.id = "destination".into();
    source
        .page_stars
        .iter_mut()
        .filter(|s| s.page == page)
        .for_each(|s| {
            s.id = "destination".into();
            s.position_ly = ((DVec3::from_array(destination_pc)
                - DVec3::from_array(GalaxyModel::milky_way().home_pc))
                * (PC_METRES / LY_METRES))
                .to_array();
        });
    live.nearby.fields.push(Known {
        field: Field::new(source, page),
        origin_pc: destination_pc,
    });
    // Use a speed low enough to sample this small destination instead of
    // passing entirely through its frame between integration endpoints.
    let cruise_speed = 299_792_458.0 * 1_000_000.0;
    live.craft.warp.set_speed(cruise_speed).unwrap();
    live.craft.engage_warp().unwrap();
    let mut saw_galaxy = false;
    for _ in 0..200 {
        let previous_pc = live.position_pc(live.craft.seconds);
        let ordinary_before = live.basis() * DVec3::from_array(live.craft.state().velocity)
            + frame_velocity(&live.craft.frame, live.system_pc);
        live.previous = live.craft.state();
        live.previous_orientation = live.craft.orientation;
        live.travel_step();
        assert!(live.craft.warp.engaged, "{:?}", live.craft.warp.stop_reason);
        let jump = (DVec3::from_array(live.position_pc(live.craft.seconds))
            - DVec3::from_array(previous_pc))
        .length()
            * PC_METRES;
        assert!(jump < cruise_speed * STEP_SECONDS * 1.001);
        let ordinary_after = live.basis() * DVec3::from_array(live.craft.state().velocity)
            + frame_velocity(&live.craft.frame, live.system_pc);
        assert!((ordinary_after - ordinary_before).length() < 1.0);
        saw_galaxy |= live.star().is_empty();
        if live.star() == "destination" {
            assert!(saw_galaxy);
            live.craft.warp.stop("manual");
            assert!(live.craft.state().velocity.iter().all(|v| v.abs() < 1e6));
            return;
        }
    }
    panic!("did not enter destination");
}

#[test]
fn galactic_reanchoring_retains_local_residual_and_momentum() {
    let mut live = live();
    let origin = live.system_pc;
    live.craft.frame = Frame::Galaxy { origin_pc: origin };
    let state = State {
        position: [REACH_M * 0.75 + 123.5, 25.0, 43.0],
        velocity: [123.0, 45.0, 67.0],
    };
    live.craft.place(state, 0.0);
    live.previous = state;
    live.prepare_nearby(&space_simulation::demo_galaxy());
    let after = live.craft.state();
    let shift = (DVec3::from_array(live.origin_pc()) - DVec3::from_array(origin)) * PC_METRES;
    assert!(
        (shift + DVec3::from_array(after.position) - DVec3::from_array(state.position)).length()
            < 0.25
    );
    assert!(DVec3::from_array(after.position).length() < REACH_M / 2.0);
    assert_eq!(after.velocity, state.velocity);
}

#[test]
fn unassigned_nearby_systems_keep_their_real_origin_not_inspection_placeholders() {
    let mut live = live();
    let origin_pc = live.system_pc;
    let key = live.star().to_owned();
    live.request_arrival(key, origin_pc);
    let empty = Galaxy {
        source: "test".into(),
        systems: Vec::new(),
    };
    for _ in 0..1000 {
        live.prepare_nearby(&empty);
        if live.nearby.pending.is_none() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert!(live.nearby.ready);
    let known = &live.nearby.fields[0];
    let expected = (DVec3::from_array(origin_pc)
        - DVec3::from_array(GalaxyModel::milky_way().home_pc))
        * (PC_METRES / LY_METRES);
    assert!(expected.length() > 0.01);
    assert!(
        (crate::scene::page_origin_ly(known.field.system(), known.field.page()) - expected)
            .length()
            < 1e-12
    );
}
