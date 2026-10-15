use super::*;
use crate::actions::UiCommand;

#[test]
fn test_hull_has_a_forward_nose_and_low_mounted_wings_inside_its_pick_bound() {
    use bevy::math::DVec3;
    for vertex in HULL_VERTICES {
        assert!(DVec3::from_array(vertex).length() <= SHIP_RADIUS_M);
    }
    let nose = HULL_VERTICES[8];
    assert_eq!(nose, [0.0, 0.0, -SHIP_SIZE_M / 2.0]);
    assert!(HULL_VERTICES[..8].iter().all(|v| v[2] > nose[2]));
    for tip in &HULL_VERTICES[9..] {
        assert_eq!(tip[1], -3.0);
        assert!(tip[0].abs() > 4.0);
    }
    for (a, b) in HULL_EDGES {
        assert!(a < HULL_VERTICES.len() && b < HULL_VERTICES.len() && a != b);
    }
}

#[test]
fn drawn_attitude_interpolates_on_the_same_step_as_position() {
    use bevy::math::{DQuat, DVec3};
    let (system, page, body) = planet_system();
    let mut fleet = Fleet::default();
    let id = fleet
        .spawn(system, page, &body, 400_000.0, Launch::Circular, 0.0)
        .unwrap();
    let live = fleet.get_mut(&id).unwrap();
    live.previous_orientation = DQuat::IDENTITY.to_array();
    // The opposite quaternion sign must still take the shortest rotation.
    live.craft.orientation = (-DQuat::from_rotation_y(1.0)).to_array();
    live.craft.seconds = STEP_SECONDS;
    let halfway = live.orientation_at(STEP_SECONDS / 2.0);
    assert!((halfway * -DVec3::Z).distance(DQuat::from_rotation_y(0.5) * -DVec3::Z) < 1e-12);
    assert!(live.orientation_at(-1.0).dot(DQuat::IDENTITY).abs() > 1.0 - 1e-12);
    assert!(
        live.orientation_at(1.0)
            .dot(DQuat::from_rotation_y(1.0))
            .abs()
            > 1.0 - 1e-12
    );
}

#[test]
fn cli_order_pipeline_points_burns_brakes_and_tracks_a_moving_body() {
    use bevy::math::{DQuat, DVec3};
    use space_simulation::craft::{AttitudePilot, PointTarget};
    fn advance(app: &mut App, steps: usize) {
        for _ in 0..steps {
            app.world_mut().resource_mut::<Clock>().0.seconds += STEP_SECONDS;
            app.update();
        }
    }
    fn error(app: &App) -> f64 {
        let live = &app.world().resource::<Fleet>().craft[0];
        let AttitudePilot::Point { target, .. } = &live.craft.autopilot else {
            panic!("not pointing");
        };
        let direction = target
            .direction(live.craft.state(), live.craft.seconds, &live.field)
            .unwrap();
        (DQuat::from_array(live.craft.orientation) * -DVec3::Z)
            .angle_between(DVec3::from_array(direction.to_array()))
    }
    let mut app = app();
    let (_, _, body) = planet_system();
    send(
        &mut app,
        UiCommand::SpawnShip {
            body: Some(body.clone()),
            altitude_km: 400.0,
            speed_kms: None,
            velocity_kms: None,
        },
    );
    let steer = |order| UiCommand::SteerCraft { id: None, order };
    send(
        &mut app,
        steer(Order::Point {
            target: PointTarget::Prograde,
            up: None,
        }),
    );
    advance(&mut app, 1000);
    assert!(error(&app) < 0.004);
    let speed = |app: &App| {
        let live = &app.world().resource::<Fleet>().craft[0];
        let (state, dominant) = live.surroundings(live.craft.seconds);
        distance(relative(state, dominant.unwrap().state).velocity, [0.0; 3])
    };
    let before = speed(&app);
    send(&mut app, steer(Order::Throttle { value: 0.5 }));
    advance(&mut app, 200);
    assert!(speed(&app) > before + 20.0);
    send(&mut app, steer(Order::Cut));
    advance(&mut app, 200);
    assert!(
        app.world().resource::<Fleet>().craft[0]
            .craft
            .spin
            .iter()
            .all(|v| v.abs() < 1e-6)
    );
    send(
        &mut app,
        steer(Order::Point {
            target: PointTarget::Body(body),
            up: None,
        }),
    );
    advance(&mut app, 1200);
    assert!(error(&app) < 0.004);
}

#[test]
fn body_orders_validate_and_patch_inputs_without_cancelling_pointing() {
    use space_simulation::craft::{AttitudePilot, PointTarget};
    let mut app = app();
    let (_, _, planet) = planet_system();
    send(
        &mut app,
        UiCommand::SpawnShip {
            body: Some(planet),
            altitude_km: 400.0,
            speed_kms: None,
            velocity_kms: None,
        },
    );
    let order = |order| UiCommand::SteerCraft { id: None, order };
    send(
        &mut app,
        order(Order::Point {
            target: PointTarget::Prograde,
            up: None,
        }),
    );
    send(&mut app, order(Order::Throttle { value: 0.25 }));
    send(
        &mut app,
        order(Order::Translate {
            forward: -0.1,
            up: 0.2,
            right: -0.3,
        }),
    );
    let craft = &app.world().resource::<Fleet>().craft[0].craft;
    assert!(matches!(craft.autopilot, AttitudePilot::Point { .. }));
    assert_eq!(craft.input.controls.throttle, 0.25);
    assert_eq!(craft.input.controls.translation, [-0.3, 0.2, 0.1]);
    let before = craft.clone();
    let mut refused = craft.clone();
    assert!(
        Order::Throttle { value: f64::NAN }
            .apply(&mut refused, None)
            .is_err()
    );
    assert_eq!(refused, before);
    send(
        &mut app,
        order(Order::Rotate {
            pitch: 0.0,
            yaw: 0.0,
            roll: 0.0,
        }),
    );
    assert_eq!(
        app.world().resource::<Fleet>().craft[0].craft.autopilot,
        AttitudePilot::Manual
    );
    send(&mut app, order(Order::Cut));
    let craft = &app.world().resource::<Fleet>().craft[0].craft;
    assert_eq!(craft.input, Default::default());
    assert_eq!(craft.autopilot, AttitudePilot::Stop);
    send(&mut app, order(Order::Release));
    let mut limits = app.world().resource::<Fleet>().craft[0].craft.thrusters;
    limits.reverse = 0.0;
    send(&mut app, order(Order::Configure { thrusters: limits }));
    let live = &app.world().resource::<Fleet>().craft[0];
    assert!(
        Order::Throttle { value: -0.5 }
            .apply(&mut live.craft.clone(), Some(&live.field))
            .is_err()
    );
    assert!(
        Order::Point {
            target: PointTarget::Body("missing".into()),
            up: None
        }
        .apply(&mut live.craft.clone(), Some(&live.field))
        .is_err()
    );
    let state = readout(app.world().resource::<Fleet>(), 0.0);
    for key in [
        "orientation_xyzw",
        "forward",
        "up",
        "spin_rads",
        "autopilot",
        "thrusters",
        "around",
    ] {
        assert!(!state["craft"][0][key].is_null(), "{key}");
    }
}

#[test]
fn manoeuvres_cap_the_clock_before_physics_and_preserve_pause_and_requested_rate() {
    let mut app = app();
    app.init_resource::<Time>().add_systems(
        Update,
        crate::tick.after(crate::actions::apply).before(step),
    );
    let (_, _, planet) = planet_system();
    send(
        &mut app,
        UiCommand::SpawnShip {
            body: Some(planet),
            altitude_km: 400.0,
            speed_kms: None,
            velocity_kms: None,
        },
    );
    send(
        &mut app,
        UiCommand::SteerCraft {
            id: None,
            order: Order::Throttle { value: 0.5 },
        },
    );
    app.world_mut()
        .resource_mut::<Time>()
        .advance_by(std::time::Duration::from_secs(1));
    app.update();
    let clock = &app.world().resource::<Clock>().0;
    assert_eq!(clock.seconds, 10.0);
    assert_eq!(clock.rate, 86_400.0);
    assert!(!app.world().resource::<Fleet>().held);
    app.world_mut().resource_mut::<Clock>().0.paused = true;
    app.update();
    assert_eq!(app.world().resource::<Clock>().0.seconds, 10.0);
    assert_eq!(app.world().resource::<Fleet>().safe_rate(1.0), 1.0);
    // A spinning, manually released ship is still unsafe at day-per-second rates.
    let mut fleet = app.world_mut().resource_mut::<Fleet>();
    fleet.craft[0].craft.input = Default::default();
    fleet.craft[0].craft.spin = [0.0, 0.1, 0.0];
    assert_eq!(fleet.safe_rate(86_400.0), 10.0);
    fleet.craft[0].craft.spin = [0.0; 3];
    assert_eq!(fleet.safe_rate(86_400.0), 86_400.0);
}

fn planet_system() -> (StarSystem, u32, String) {
    let galaxy = space_simulation::demo_galaxy();
    let system = galaxy.systems[0].clone();
    let page = system.first_page();
    let planet = system.on_page(page).next().unwrap().id.clone();
    (system, page, planet)
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    (0..3).map(|i| a[i] * b[i]).sum()
}

#[test]
fn a_ship_spawns_in_a_prograde_circular_orbit_above_its_planet() {
    let (system, page, id) = planet_system();
    let planet = system.planets.iter().find(|p| p.id == id).unwrap().clone();
    let mut fleet = Fleet::default();
    let ship = fleet
        .spawn(system.clone(), page, &id, 400_000.0, Launch::Circular, 1e6)
        .unwrap();
    let live = fleet.get(&ship).unwrap();
    assert_eq!(live.star(), objects::star_id(&system, page));
    let body = space_simulation::planet_state(&system, &planet, 1e6);
    let local = relative(live.craft.state(), body);
    let r = planet.radius_km * 1_000.0 + 400_000.0;
    let gm = planet.mass_earth * EARTH_SOLAR * SUN_GM;
    assert!((distance(local.position, [0.0; 3]) / r - 1.0).abs() < 1e-9);
    let speed = distance(local.velocity, [0.0; 3]);
    assert!((speed / circular_speed(gm, r) - 1.0).abs() < 1e-9);
    assert!(dot(local.position, local.velocity).abs() < 1e-6 * r * speed);
    // It turns the way its planet goes round the star.
    let host = space_simulation::member_state(&system, page, &planet.host, 1e6);
    let around = relative(body, host);
    let pole = cross(around.position, around.velocity);
    assert!(dot(cross(local.position, local.velocity), pole) > 0.0);
    // The inspector reads a circular 400 km orbit around that planet.
    let galaxy = space_simulation::demo_galaxy();
    let (title, rows) = facts(live, &galaxy, 1e6);
    assert_eq!(title, "Ship 1");
    let fact = |label: &str| {
        rows.iter()
            .find(|(l, _)| l == label)
            .map(|(_, v)| v.clone())
            .unwrap_or_else(|| panic!("{label}: {rows:?}"))
    };
    assert_eq!(fact("Around"), planet.designation);
    assert_eq!(fact("Altitude"), "400.0 km");
    assert_eq!(fact("Periapsis"), "400.0 km");
    assert_eq!(fact("Apoapsis"), "400.0 km");
}

#[test]
fn launches_set_speed_or_velocity_relative_to_the_body() {
    let (system, page, id) = planet_system();
    let planet = system.planets.iter().find(|p| p.id == id).unwrap().clone();
    let body = space_simulation::planet_state(&system, &planet, 0.0);
    let mut fleet = Fleet::default();
    let fast = fleet
        .spawn(system.clone(), page, &id, 1e6, Launch::Speed(9_000.0), 0.0)
        .unwrap();
    let local = relative(fleet.get(&fast).unwrap().craft.state(), body);
    assert!((distance(local.velocity, [0.0; 3]) - 9_000.0).abs() < 1e-6);
    let still = fleet
        .spawn(system, page, &id, 1e6, Launch::Velocity([0.0; 3]), 0.0)
        .unwrap();
    let local = relative(fleet.get(&still).unwrap().craft.state(), body);
    assert!(distance(local.velocity, [0.0; 3]) < 1e-6);
    assert_eq!(fleet.despawn(Some(&fast)), 1);
    assert!(fleet.get(&fast).is_none() && fleet.get(&still).is_some());
    assert_eq!(fleet.despawn(None), 1);
    assert!(fleet.craft.is_empty());
}

fn app() -> App {
    let mut app = App::new();
    app.add_message::<UiCommand>()
        .insert_resource(crate::Universe(space_simulation::demo_galaxy()))
        .init_resource::<Navigation>()
        .init_resource::<Clock>()
        .init_resource::<crate::stars::stream::Starfield>()
        .init_resource::<crate::render::debug::RenderDebug>()
        .init_resource::<Fleet>()
        .add_systems(Update, (crate::actions::apply, step).chain());
    app
}

fn send(app: &mut App, command: UiCommand) {
    let world = app.world();
    command
        .check(
            &world.resource::<crate::Universe>().0,
            world.resource::<Navigation>(),
            world.resource::<crate::stars::stream::Starfield>(),
            world.resource::<Fleet>(),
        )
        .unwrap_or_else(|err| panic!("{command:?}: {err}"));
    app.world_mut().write_message(command);
    app.update();
}

#[test]
fn ships_follow_the_clock_in_fixed_steps_and_hold_it_when_behind() {
    let mut app = app();
    app.world_mut().resource_mut::<Clock>().0.rate = 1.0;
    let (_, _, planet) = planet_system();
    send(
        &mut app,
        UiCommand::SpawnShip {
            body: Some(planet),
            altitude_km: 400.0,
            speed_kms: None,
            velocity_kms: None,
        },
    );
    let start = app.world().resource::<Fleet>().craft[0].craft.seconds;
    app.world_mut().resource_mut::<Clock>().0.seconds = start + 10.01;
    app.update();
    let fleet = app.world().resource::<Fleet>();
    let craft = &fleet.craft[0].craft;
    assert!(!fleet.held);
    assert!(
        (craft.seconds - (start + 10.05)).abs() < 1e-9,
        "{}",
        craft.seconds
    );
    // Drawn between its last two steps, at the clock's time.
    let live = &fleet.craft[0];
    let drawn = live.state_at(start + 10.01);
    let [before, after] = [live.state_at(start + 10.0), live.craft.state()];
    for i in 0..3 {
        let expected = before.position[i] + (after.position[i] - before.position[i]) * 0.2;
        assert!((drawn.position[i] - expected).abs() < 1e-3);
    }
    // A day in one frame is too much: the clock waits for the ship.
    app.world_mut().resource_mut::<Clock>().0.seconds = start + 86_400.0;
    app.update();
    let reached = app.world().resource::<Fleet>().craft[0].craft.seconds;
    assert!(app.world().resource::<Fleet>().held);
    assert_eq!(app.world().resource::<Clock>().0.seconds, reached);
    let steps = (reached - start) / STEP_SECONDS;
    assert!(
        (steps - (201.0 + MAX_STEPS_PER_FRAME as f64)).abs() < 1e-6,
        "{steps}"
    );
}

#[test]
fn commands_spawn_select_follow_steer_and_remove_ships() {
    let mut app = app();
    let refused = |app: &App, command: &UiCommand| {
        let world = app.world();
        command
            .check(
                &world.resource::<crate::Universe>().0,
                world.resource::<Navigation>(),
                world.resource::<crate::stars::stream::Starfield>(),
                world.resource::<Fleet>(),
            )
            .unwrap_err()
    };
    let spawn = UiCommand::SpawnShip {
        body: None,
        altitude_km: 400.0,
        speed_kms: None,
        velocity_kms: None,
    };
    assert!(refused(&app, &spawn).contains("Give a body"));
    assert!(refused(&app, &UiCommand::DespawnCraft { id: None }).contains("no craft"));
    let list = space_simulation::demo_galaxy().systems[0].id.clone();
    send(&mut app, UiCommand::Jump { list });
    // With nothing selected, it orbits the visited system as a whole.
    app.world_mut().resource_mut::<Navigation>().select(None);
    send(&mut app, spawn);
    let id = app.world().resource::<Fleet>().craft[0].craft.id.clone();
    assert_eq!(
        app.world().resource::<Navigation>().selection,
        Some(crate::Selection::Craft(id.clone()))
    );
    let thrust = UiCommand::SetThrust {
        thrust: [0.0, 1.0, 0.0],
        id: None,
    };
    send(&mut app, thrust);
    assert_eq!(
        app.world().resource::<Fleet>().craft[0].craft.input.thrust,
        [0.0, 1.0, 0.0]
    );
    let too_hard = UiCommand::SetThrust {
        thrust: [0.0, 1e3, 0.0],
        id: None,
    };
    assert!(too_hard.check_values().is_err());
    send(&mut app, UiCommand::FlyToCraft { id: id.clone() });
    let nav = app.world().resource::<Navigation>();
    assert!(nav.following && nav.fly && nav.distance == FOLLOW_M);
    send(&mut app, UiCommand::Up);
    assert!(!app.world().resource::<Navigation>().following);
    send(&mut app, UiCommand::SelectCraft { id: id.clone() });
    send(&mut app, UiCommand::DespawnCraft { id: Some(id) });
    assert!(app.world().resource::<Fleet>().craft.is_empty());
    assert_eq!(app.world().resource::<Navigation>().selection, None);
}
