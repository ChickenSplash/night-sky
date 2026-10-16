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
            .direction(
                live.craft.state(),
                live.craft.seconds,
                &live.field,
                live.craft.reference.as_deref(),
            )
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
        let (state, reference) = live.surroundings(live.craft.seconds);
        distance(relative(state, reference.unwrap().state).velocity, [0.0; 3])
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
    assert_eq!(fact("Two-body periapsis"), "400.0 km");
    assert_eq!(fact("Two-body apoapsis"), "400.0 km");
    assert_eq!(fact("Prediction"), "Working…");
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

/// Forecast until nothing is being worked on, at most `frames` times.
fn settle(fleet: &mut Fleet, frames: usize) -> usize {
    for frame in 1..=frames {
        fleet.forecast(forecast::STEPS_PER_FRAME);
        if fleet.craft.iter().all(|live| !live.forecast.updating()) {
            return frame;
        }
    }
    panic!("still forecasting after {frames} frames");
}

#[test]
fn forecasts_spread_over_frames_and_hold_while_paused() {
    let (system, page, body) = planet_system();
    let mut fleet = Fleet::default();
    fleet
        .spawn(system, page, &body, 400_000.0, Launch::Circular, 0.0)
        .unwrap();
    fleet.forecast(forecast::STEPS_PER_FRAME);
    let live = &fleet.craft[0];
    assert!(live.forecast.updating() && live.forecast.shown.is_none());
    let frames = settle(&mut fleet, 100);
    assert!(frames > 2, "one lap takes several frames: {frames}");
    let shown = fleet.craft[0].forecast.shown.clone().unwrap();
    assert!(shown.bound && fleet.craft[0].forecast.current(&fleet.craft[0].craft));
    // Paused, nothing moves, so nothing is redone.
    for _ in 0..10 {
        fleet.forecast(forecast::STEPS_PER_FRAME);
        assert!(!fleet.craft[0].forecast.updating());
    }
    assert_eq!(fleet.craft[0].forecast.shown.as_ref(), Some(&shown));
    let readout = readout(&fleet, 0.0);
    let prediction = &readout["craft"][0]["prediction"];
    assert_eq!(prediction["end"], "horizon");
    assert_eq!(prediction["age_seconds"], 0.0);
}

#[test]
fn orders_make_a_forecast_obsolete_and_burns_still_finish_theirs() {
    let (system, page, body) = planet_system();
    let mut fleet = Fleet::default();
    fleet
        .spawn(system, page, &body, 400_000.0, Launch::Circular, 0.0)
        .unwrap();
    settle(&mut fleet, 100);
    let coasting = fleet.craft[0].forecast.shown.clone().unwrap();
    let live = &mut fleet.craft[0];
    Order::Throttle { value: 0.2 }
        .apply(&mut live.craft, Some(&live.field))
        .unwrap();
    fleet.forecast(forecast::STEPS_PER_FRAME);
    let live = &fleet.craft[0];
    // The old one stays on show, marked as no longer current.
    assert!(live.forecast.updating() && !live.forecast.current(&live.craft));
    assert_eq!(live.forecast.shown.as_ref(), Some(&coasting));
    // A burn keeps refreshing, yet every refresh finishes.
    let mut finished = 0;
    for _ in 0..60 {
        let live = &mut fleet.craft[0];
        for _ in 0..20 {
            live.craft.step(&live.field);
        }
        let before = live.forecast.shown.as_ref().map(|p| p.start_seconds);
        fleet.forecast(forecast::STEPS_PER_FRAME);
        let after = fleet.craft[0]
            .forecast
            .shown
            .as_ref()
            .map(|p| p.start_seconds);
        finished += usize::from(before != after);
    }
    assert!(finished >= 3, "{finished}");
    let live = &fleet.craft[0];
    assert!(live.forecast.current(&live.craft));
    let shown = live.forecast.shown.as_ref().unwrap();
    assert!(shown.start_seconds > coasting.start_seconds);
}

#[test]
fn craft_share_the_budget_and_fast_time_ages_a_forecast() {
    let (system, page, body) = planet_system();
    let mut fleet = Fleet::default();
    for height in [400_000.0, 2_000_000.0] {
        fleet
            .spawn(system.clone(), page, &body, height, Launch::Circular, 0.0)
            .unwrap();
    }
    settle(&mut fleet, 200);
    assert!(fleet.craft.iter().all(|live| live.forecast.shown.is_some()));
    // Most of a lap passes in a few frames: the forecast is redone, and
    // until then reads as aged.
    let live = &mut fleet.craft[0];
    let horizon = live.forecast.shown.as_ref().unwrap().horizon_seconds;
    let until = live.craft.seconds + horizon * 0.6;
    live.craft.advance(&live.field, until, usize::MAX);
    let age = readout(&fleet, until)["craft"][0]["prediction"]["age_seconds"]
        .as_f64()
        .unwrap();
    assert!((age / (horizon * 0.6) - 1.0).abs() < 0.01, "{age}");
    fleet.forecast(forecast::STEPS_PER_FRAME);
    assert!(fleet.craft[0].forecast.updating());
    assert!(!fleet.craft[1].forecast.updating());
    settle(&mut fleet, 100);
    let shown = fleet.craft[0].forecast.shown.as_ref().unwrap();
    assert_eq!(shown.start_seconds, fleet.craft[0].craft.seconds);
}

#[test]
fn a_predicted_path_is_anchored_where_its_planet_is_now() {
    let (system, page, body) = planet_system();
    let galaxy = space_simulation::demo_galaxy();
    let mut fleet = Fleet::default();
    // Slightly eccentric, so it has distinct predicted apsides.
    let reference = Field::new(system.clone(), page)
        .reference_by_id(&body, 0.0)
        .unwrap();
    let speed = 1.05 * circular_speed(reference.gm, reference.radius_m + 400_000.0);
    let id = fleet
        .spawn(
            system.clone(),
            page,
            &body,
            400_000.0,
            Launch::Speed(speed),
            0.0,
        )
        .unwrap();
    settle(&mut fleet, 100);
    // A fifth of a lap on, still current, it is one smooth closed loop:
    // starting and ending at the craft, with no jump on either side of it.
    let live = fleet.get_mut(&id).unwrap();
    let period = live
        .forecast
        .shown
        .as_ref()
        .unwrap()
        .period_seconds
        .unwrap();
    let started = live.craft.seconds;
    let soon = started + period * 0.2;
    live.craft.advance(&live.field, soon, usize::MAX);
    live.previous = live.craft.state();
    let live = fleet.get(&id).unwrap();
    let loop_ = predicted_path(live, soon).unwrap();
    let points = &loop_.points;
    let here = live.craft.state().position;
    assert_eq!((points[0], *points.last().unwrap()), (here, here));
    let step = |i: usize| distance(points[i], points[i + 1]);
    let longest = (1..points.len() - 3).map(step).fold(0.0, f64::max);
    for i in [0, points.len() - 3, points.len() - 2] {
        assert!(
            step(i) <= 1.5 * longest,
            "jump at {i}: {} > {longest}",
            step(i)
        );
    }
    let live = fleet.get_mut(&id).unwrap();
    let horizon = live.forecast.shown.as_ref().unwrap().horizon_seconds;
    let until = started + horizon * 0.3;
    live.craft.advance(&live.field, until, usize::MAX);
    live.previous = live.craft.state();
    let live = fleet.get(&id).unwrap();
    let p = live.forecast.shown.as_ref().unwrap();
    let path = predicted_path(live, until).unwrap();
    assert!(path.current);
    assert_eq!(path.points[0], live.craft.state().position);
    // Each later point is the sample relative to the planet, plus where
    // the planet is now, not where it will be at the sample's time, give
    // or take the craft's drift from the prediction, eased out.
    let planet = system.planets.iter().find(|p| p.id == body).unwrap();
    let now = space_simulation::planet_position(&system, planet, until);
    let ahead = p.ahead(until);
    assert_eq!(path.points.len(), ahead.len() + 1);
    let craft: [f64; 3] = std::array::from_fn(|i| path.points[0][i] - now[i]);
    let drift = distance(craft, p.position_at(until).unwrap());
    assert!(drift < 1_000.0, "{drift}");
    for (point, sample) in path.points[1..].iter().zip(ahead) {
        let relative: [f64; 3] = std::array::from_fn(|i| point[i] - now[i]);
        assert!(distance(relative, sample.position) <= drift + 1e-3);
    }
    // The planet moves on by far more than the orbit's size meanwhile, so
    // anchoring at future positions would smear the path.
    let later = space_simulation::planet_position(&system, planet, p.end_seconds());
    assert!(distance(later, now) > 10.0 * distance(ahead[0].position, [0.0; 3]));
    // Predicted apsides, apart from the two-body ones.
    assert!(path.marks.iter().any(|(_, mark)| *mark == PathMark::Apsis));
    let (_, rows) = facts(live, &galaxy, until);
    let labels: Vec<_> = rows.iter().map(|(label, _)| label.as_str()).collect();
    for label in [
        "Two-body periapsis",
        "Two-body apoapsis",
        "Prediction",
        "Prediction age",
        "Predicted periapsis",
        "Predicted apoapsis",
    ] {
        assert!(labels.contains(&label), "{label}: {rows:?}");
    }
    let readout = readout(&fleet, until);
    let events = readout["craft"][0]["prediction"]["events"]
        .as_array()
        .unwrap();
    assert!(events.iter().any(|e| e["event"] == "apoapsis"));
    assert!(readout["craft"][0]["around"]["two_body_orbit"].is_object());
}

#[test]
fn escaping_paths_mark_the_exit_and_pairs_anchor_at_their_barycentre() {
    let (system, page, body) = planet_system();
    let galaxy = space_simulation::demo_galaxy();
    let mut fleet = Fleet::default();
    let id = fleet
        .spawn(
            system.clone(),
            page,
            &body,
            400_000.0,
            Launch::Speed(20_000.0),
            0.0,
        )
        .unwrap();
    settle(&mut fleet, 200);
    let live = fleet.get(&id).unwrap();
    let path = predicted_path(live, 0.0).unwrap();
    assert!(
        path.marks
            .iter()
            .any(|(_, mark)| *mark == PathMark::Reference)
    );
    let (_, rows) = facts(live, &galaxy, 0.0);
    let exit = rows.iter().find(|(label, _)| label == "Predicted exit");
    let star = galaxy_star_label(live, &galaxy);
    assert!(
        exit.is_some_and(|(_, text)| text.starts_with(&star)),
        "{rows:?}"
    );
    let events = &readout(&fleet, 0.0)["craft"][0]["prediction"]["events"];
    assert!(
        events
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["event"] == "exit")
    );

    // Far out around a binary, the path is anchored at its barycentre.
    let (pair_system, pair_page, pair) = galaxy
        .systems
        .iter()
        .find_map(|s| {
            let page = s
                .pages()
                .into_iter()
                .find(|&page| s.barycentres_on(page).next().is_some())?;
            Some((s.clone(), page, s.root(page)?.to_owned()))
        })
        .expect("a demo multiple system");
    let mut fleet = Fleet::default();
    let extent =
        space_simulation::system_extent_au(&pair_system, pair_page) * space_simulation::AU_METRES;
    let id = fleet
        .spawn(
            pair_system.clone(),
            pair_page,
            &pair,
            3.0 * extent,
            Launch::Circular,
            0.0,
        )
        .unwrap();
    settle(&mut fleet, 200);
    let live = fleet.get(&id).unwrap();
    let p = live.forecast.shown.as_ref().unwrap();
    assert_eq!(p.reference, pair);
    let path = predicted_path(live, 0.0).unwrap();
    let centre = space_simulation::member_position(&pair_system, pair_page, &pair, 0.0);
    for (point, sample) in path.points[1..].iter().zip(p.ahead(0.0)) {
        let relative: [f64; 3] = std::array::from_fn(|i| point[i] - centre[i]);
        assert!(distance(relative, sample.position) < 1.0);
    }
}

fn galaxy_star_label(live: &Live, galaxy: &Galaxy) -> String {
    let star = live
        .field
        .system()
        .stars_on(live.field.page())
        .next()
        .unwrap();
    body_label(live, galaxy, &star.id)
}
