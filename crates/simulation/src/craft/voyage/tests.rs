use super::*;
use crate::craft::{CraftKind, route::Stage, warp::GravityPolicy};
use crate::galaxy::GalaxyModel;
use crate::galaxy::cells::{Cell, CellStar, Sky};
use crate::galaxy::population::TIERS;

/// A real generated star near home, from the finest populated cell there.
fn star() -> CellStar {
    let home = GalaxyModel::milky_way().home_pc;
    let mut sky = Sky::default();
    (0..TIERS)
        .rev()
        .find_map(|level| sky.cell_star_at(Cell::containing(level, home)?, 0))
        .expect("a star near home")
}

fn arrival(star: &CellStar) -> Arrival {
    generated_arrival(&Candidate {
        id: star.id.clone(),
        position_pc: star.position_pc,
        reach_pc: 0.0,
    })
}

/// A craft in galactic space, anchored at a star, at this offset in metres.
fn galactic(star: &CellStar, position: [f64; 3], velocity: [f64; 3]) -> Voyage {
    let mut craft = Craft::new(
        "craft-1".into(),
        "Ship 1".into(),
        CraftKind::Ship,
        Frame::Galaxy {
            origin_pc: star.position_pc,
        },
        0,
    );
    craft.place(State { position, velocity }, 0);
    Voyage::new(craft, arrival(star).field, star.position_pc)
}

/// Position from `reference`, velocity and heading, in galactic axes.
fn galactic_state(voyage: &Voyage, reference: [f64; 3]) -> (DVec3, DVec3, DVec3) {
    let state = voyage.craft.state();
    let basis = voyage.basis();
    (
        (DVec3::from_array(voyage.origin_pc) - DVec3::from_array(reference)) * PC_METRES
            + basis * DVec3::from_array(state.position),
        basis * DVec3::from_array(state.velocity) + voyage.frame_velocity(),
        basis * DQuat::from_array(voyage.craft.orientation) * -DVec3::Z,
    )
}

const AU: f64 = crate::AU_METRES;

#[test]
fn frame_changes_rotate_persistent_frame_axis_inputs() {
    use crate::craft::{AttitudePilot, PointTarget};
    let mut voyage = empty_voyage();
    voyage.craft.input.thrust = [1.0, 2.0, 3.0];
    voyage
        .craft
        .point(PointTarget::Vector([2.0, -1.0, 0.5]), Some([0.0, 1.0, 0.0]))
        .unwrap();
    let original = voyage.craft.clone();
    // Exercise an arbitrary change of axes, independently
    // of arrival discovery. Body-local spin/controls must not rotate.
    voyage.craft.spin = [0.1, 0.2, 0.3];
    let basis = DQuat::from_euler(glam::EulerRot::XYZ, 0.4, -0.7, 1.1);
    let rebase = voyage.rebase(
        Frame::System { star: "new".into() },
        voyage.origin_pc,
        basis,
        None,
    );
    assert_eq!(
        voyage.craft.input.thrust,
        rebase.direction(original.input.thrust)
    );
    assert_eq!(voyage.craft.spin, [0.1, 0.2, 0.3]);
    let AttitudePilot::Point {
        target: PointTarget::Vector(v),
        up,
        attitude,
    } = &voyage.craft.autopilot
    else {
        panic!("pointing lost")
    };
    let AttitudePilot::Point {
        attitude: old_attitude,
        ..
    } = original.autopilot
    else {
        unreachable!()
    };
    assert_eq!(*v, rebase.direction([2.0, -1.0, 0.5]));
    assert_eq!(*up, Some(rebase.direction([0.0, 1.0, 0.0])));
    assert_eq!(*attitude, rebase.orientation(old_attitude));
    assert!((basis * DVec3::from_array(*v) - DVec3::new(2.0, -1.0, 0.5)).length() < 1e-14);
    assert!(
        (basis * DVec3::from_array(voyage.craft.input.thrust)
            - DVec3::from_array(original.input.thrust))
        .length()
            < 1e-14
    );
}

#[test]
fn future_orders_reject_invalid_values_and_excessive_horizons_at_submission() {
    use crate::craft::{MAX_THRUST, PointTarget, Thrusters, WarpOrder};
    let mut voyage = empty_voyage();
    let invalid = [
        Order::Throttle { value: f64::NAN },
        Order::Translate {
            right: 2.0,
            up: 0.0,
            forward: 0.0,
        },
        Order::Rotate {
            pitch: 0.0,
            yaw: f64::INFINITY,
            roll: 0.0,
        },
        Order::Point {
            target: PointTarget::Vector([0.0; 3]),
            up: None,
        },
        Order::Point {
            target: PointTarget::Prograde,
            up: Some([f64::NAN; 3]),
        },
        Order::Configure {
            thrusters: Thrusters {
                forward: -1.0,
                ..Default::default()
            },
        },
        Order::Thrust {
            thrust: [MAX_THRUST + 1.0, 0.0, 0.0],
        },
        Order::Warp {
            command: WarpOrder::Set { speed_ms: f64::NAN },
        },
        Order::Point {
            target: PointTarget::Body(String::new()),
            up: None,
        },
        Order::Warp {
            command: WarpOrder::Match {
                body: "x".repeat(crate::craft::MAX_BODY_ID + 1),
                orbit: false,
            },
        },
    ];
    let before = voyage.craft.clone();
    for order in invalid {
        assert!(voyage.order(1, order).is_err());
        assert_eq!(voyage.craft, before);
        assert!(voyage.pending_orders().is_empty());
    }
    assert!(
        voyage
            .order(MAX_ORDER_AHEAD_TICKS + 1, Order::Release)
            .is_err()
    );
    assert!(voyage.order(i64::MAX, Order::Release).is_err());
    voyage.order(MAX_ORDER_AHEAD_TICKS, Order::Release).unwrap();
    // A state-dependent conflict is still allowed at submission: a preceding
    // order may resolve it before this boundary is reached.
    voyage
        .order(
            1,
            Order::Warp {
                command: WarpOrder::Engage,
            },
        )
        .unwrap();
    voyage
        .order(
            2,
            Order::Warp {
                command: WarpOrder::Disengage,
            },
        )
        .unwrap();
    voyage.order(2, Order::Throttle { value: 1.0 }).unwrap();
    voyage.step(&Blank).unwrap();
    assert!(voyage.craft.warp.engaged);
    assert!(voyage.step(&Blank).unwrap().refused.is_empty());
    assert_eq!(voyage.craft.input.controls.throttle, 1.0);
}

fn empty_voyage() -> Voyage {
    let system = space_shared::StarSystem {
        id: "orders".into(),
        name: "Orders".into(),
        position_ly: [0.0; 3],
        planets: Vec::new(),
        stars: Vec::new(),
        barycentres: Vec::new(),
        page_stars: Vec::new(),
    };
    let craft = Craft::new(
        "ship".into(),
        "Ship".into(),
        CraftKind::Ship,
        Frame::System {
            star: page_star_id(&system, 0),
        },
        0,
    );
    Voyage::new(craft, Field::new(system, 0), [0.0; 3])
}

#[test]
fn orders_are_step_keyed_ordered_and_refused_atomically() {
    use crate::craft::{Order, WarpOrder};
    let mut voyage = empty_voyage();
    let before = voyage.craft.clone();
    assert!(voyage.order(-1, Order::Throttle { value: 1.0 }).is_err());
    assert_eq!(voyage.craft, before);
    voyage.order(2, Order::Throttle { value: 0.5 }).unwrap();
    voyage.order(2, Order::Throttle { value: 1.0 }).unwrap();
    voyage.step(&Blank).unwrap();
    assert_eq!(voyage.craft.state(), State::default());
    assert_eq!(voyage.craft.input.controls.throttle, 0.0);
    voyage.step(&Blank).unwrap();
    assert_eq!(voyage.craft.state(), State::default(), "no early burn");
    assert_eq!(voyage.craft.input.controls.throttle, 1.0, "same-tick FIFO");
    voyage
        .order(
            3,
            Order::Warp {
                command: WarpOrder::Engage,
            },
        )
        .unwrap();
    let result = voyage.step(&Blank).unwrap();
    assert_eq!(result.from, State::default());
    assert!(voyage.craft.state().velocity[2] < 0.0);
    assert_eq!(result.refused.len(), 1);
    assert_eq!(result.refused[0].tick, 3);
    assert!(!voyage.craft.warp.engaged);
    assert!(voyage.scheduled().next().is_none());
    assert!(
        voyage.step(&Blank).unwrap().refused.is_empty(),
        "refused once"
    );
}

#[test]
fn timed_orders_end_themselves_unless_replaced() {
    use crate::craft::{Order, WarpOrder};
    let throttle = |value| Order::Throttle { value };
    let steps = |voyage: &mut Voyage, n| {
        for _ in 0..n {
            assert!(voyage.step(&Blank).unwrap().refused.is_empty());
        }
    };
    let mut voyage = empty_voyage();
    assert!(voyage.timed_order(0, 10, Order::Stop).is_err(), "no off");
    assert!(voyage.timed_order(5, 5, throttle(0.5)).is_err());
    assert!(
        voyage
            .timed_order(0, MAX_ORDER_AHEAD_TICKS + 1, throttle(0.5))
            .is_err()
    );
    assert!(voyage.pending_orders().is_empty());

    // A one-second burn, surviving a snapshot.
    voyage.timed_order(0, 20, throttle(0.5)).unwrap();
    assert_eq!(voyage.craft.input.controls.throttle, 0.5);
    let saved = serde_json::to_string(&voyage.pending_orders()).unwrap();
    let mut restored = Voyage::new(voyage.craft.clone(), voyage.field.clone(), [0.0; 3]);
    for input in serde_json::from_str::<Vec<ScheduledOrder>>(&saved).unwrap() {
        assert_eq!(input.ends_from, Some(0));
        restored.schedule(input).unwrap();
    }
    steps(&mut voyage, 19);
    assert_eq!(voyage.craft.input.controls.throttle, 0.5);
    steps(&mut voyage, 1);
    assert_eq!(voyage.craft.input.controls.throttle, 0.0);
    steps(&mut restored, 20);
    assert_eq!(restored.craft, voyage.craft);

    // A later order on the same controls replaces the end; others do not.
    voyage.timed_order(20, 40, throttle(0.5)).unwrap();
    steps(&mut voyage, 5);
    voyage
        .order(
            25,
            Order::Rotate {
                pitch: 0.1,
                yaw: 0.0,
                roll: 0.0,
            },
        )
        .unwrap();
    assert_eq!(voyage.pending_orders().len(), 1);
    voyage.order(30, throttle(0.8)).unwrap();
    steps(&mut voyage, 5);
    assert!(voyage.pending_orders().is_empty());
    steps(&mut voyage, 20);
    assert_eq!(voyage.craft.input.controls.throttle, 0.8);

    // An order given before a delayed burn starts does not cancel its end,
    // and one for the end's own step wins over it.
    let now = voyage.craft.tick;
    voyage
        .timed_order(now + 10, now + 20, throttle(0.5))
        .unwrap();
    voyage.order(now, throttle(0.3)).unwrap();
    assert_eq!(voyage.pending_orders().len(), 2);
    steps(&mut voyage, 20);
    assert_eq!(voyage.craft.input.controls.throttle, 0.0);
    let now = voyage.craft.tick;
    voyage.timed_order(now, now + 10, throttle(0.5)).unwrap();
    voyage.order(now + 10, throttle(0.7)).unwrap();
    steps(&mut voyage, 10);
    assert_eq!(voyage.craft.input.controls.throttle, 0.7);

    // A refused start takes its end with it.
    let now = voyage.craft.tick;
    let engage = Order::Warp {
        command: WarpOrder::Engage,
    };
    voyage.timed_order(now + 2, now + 10, engage).unwrap();
    assert_eq!(voyage.step(&Blank).unwrap().refused.len(), 0);
    assert_eq!(voyage.step(&Blank).unwrap().refused.len(), 1);
    assert!(voyage.pending_orders().is_empty());
}

#[test]
fn timed_orders_need_room_for_their_end() {
    use crate::craft::Order;
    let mut voyage = empty_voyage();
    for tick in 1..MAX_PENDING_ORDERS as i64 {
        voyage.order(tick, Order::Release).unwrap();
    }
    let throttle = Order::Throttle { value: 0.5 };
    assert!(voyage.timed_order(1, 2, throttle.clone()).is_err());
    assert_eq!(voyage.pending_orders().len(), MAX_PENDING_ORDERS - 1);
    voyage.timed_order(0, 2, throttle).unwrap();
    assert_eq!(voyage.pending_orders().len(), MAX_PENDING_ORDERS);
}

#[test]
fn queued_warp_orders_replay_after_a_snapshot() {
    use crate::craft::{Order, WarpOrder};
    let mut voyage = empty_voyage();
    let log = vec![
        ScheduledOrder {
            tick: 1,
            ends_from: None,
            order: Order::Warp {
                command: WarpOrder::Set {
                    speed_ms: 100_000.0,
                },
            },
        },
        ScheduledOrder {
            tick: 1,
            ends_from: None,
            order: Order::Warp {
                command: WarpOrder::Engage,
            },
        },
        ScheduledOrder {
            tick: 6,
            ends_from: None,
            order: Order::Warp {
                command: WarpOrder::Set { speed_ms: 1_000.0 },
            },
        },
        ScheduledOrder {
            tick: 10,
            ends_from: None,
            order: Order::Warp {
                command: WarpOrder::Disengage,
            },
        },
        ScheduledOrder {
            tick: 10,
            ends_from: None,
            order: Order::Throttle { value: 1.0 },
        },
    ];
    for input in &log {
        voyage.order(input.tick, input.order.clone()).unwrap();
    }
    for _ in 0..4 {
        voyage.step(&Blank).unwrap();
    }
    let craft: Craft =
        serde_json::from_str(&serde_json::to_string(&voyage.craft).unwrap()).unwrap();
    let saved = serde_json::to_string(&voyage.pending_orders()).unwrap();
    let pending: Vec<ScheduledOrder> = serde_json::from_str(&saved).unwrap();
    let mut restored = Voyage::new(craft, voyage.field.clone(), voyage.origin_pc);
    for input in pending {
        restored.schedule(input).unwrap();
    }
    // A server receives the same log just in time instead of ahead of time.
    let mut server = empty_voyage();
    while server.craft.tick < 4 {
        let tick = server.craft.tick;
        for input in log.iter().filter(|input| input.tick == tick) {
            server.order(input.tick, input.order.clone()).unwrap();
        }
        server.step(&Blank).unwrap();
    }
    for _ in 4..20 {
        let tick = server.craft.tick;
        for input in log.iter().filter(|input| input.tick == tick) {
            server.order(input.tick, input.order.clone()).unwrap();
        }
        let a = voyage.step(&Blank).unwrap();
        let b = restored.step(&Blank).unwrap();
        server.step(&Blank).unwrap();
        assert_eq!(a, b);
        assert_eq!(voyage.craft, restored.craft);
        // Future commands apply at a boundary, so compare motion rather than
        // the server's not-yet-submitted boundary input.
        assert_eq!(voyage.craft.state(), server.craft.state());
    }
    assert!(!voyage.craft.warp.engaged);
    assert_eq!(voyage.craft.input.controls.throttle, 1.0);
}

#[test]
fn pending_orders_are_bounded_and_waits_do_not_consume_them() {
    use crate::craft::Order;
    let mut voyage = empty_voyage();
    for tick in 1..=MAX_PENDING_ORDERS as i64 {
        voyage.order(tick, Order::Release).unwrap();
    }
    assert!(voyage.order(10_000, Order::Release).is_err());
    voyage.craft.frame = Frame::Galaxy {
        origin_pc: voyage.origin_pc,
    };
    let pending = voyage.pending_orders();
    let craft = voyage.craft.clone();
    for _ in 0..3 {
        assert!(voyage.step(&Blank).is_err());
    }
    assert_eq!(voyage.craft, craft);
    assert_eq!(voyage.pending_orders(), pending);
}

#[test]
fn dropout_beside_an_uncharted_star_enters_it_from_seeds_alone() {
    let star = star();
    let mut voyage = galactic(&star, [100.0 * AU, 0.0, 0.0], [0.0; 3]);
    let before = galactic_state(&voyage, star.position_pc);
    // Nothing is known yet: the step waits, changing nothing.
    let unchanged = voyage.craft.clone();
    assert!(matches!(voyage.step(&Blank), Err(Wait::Region(_))));
    assert_eq!(voyage.craft, unchanged);
    let mut library = Library::default();
    let rebase = library.step(&mut voyage).rebase.expect("a handoff");
    assert!(rebase.frame_changed);
    assert_eq!(
        voyage.craft.frame,
        Frame::System {
            star: star.id.clone()
        }
    );
    assert_eq!(voyage.origin_pc, star.position_pc);
    let after = galactic_state(&voyage, star.position_pc);
    // One step of drift at the system's speed through the galaxy.
    assert!((after.0 - before.0).length() < 1e6);
    assert!((after.1 - before.1).length() < 1e-6);
    library.step(&mut voyage);
    assert!(
        voyage.craft.reference.is_some(),
        "gravity and an orbit reference"
    );
}

/// Supplies a wait only after refusing it a few times, as a slow background
/// loader would.
struct Slow {
    library: Library,
    refusals: usize,
}

#[test]
fn waiting_for_data_never_changes_the_path() {
    let star = star();
    let start = |voyage: &mut Voyage| {
        voyage.craft.warp.set_speed(1e5).unwrap();
        voyage.craft.engage_warp().unwrap();
    };
    let mut prompt = galactic(&star, [300.0 * AU, 0.0, 0.0], [20_000.0, 0.0, 0.0]);
    let mut slow = prompt.clone();
    start(&mut prompt);
    start(&mut slow);
    let mut library = Library::default();
    let mut delayed = Slow {
        library: Library::default(),
        refusals: 0,
    };
    let mut waits = 0;
    for _ in 0..400 {
        library.step(&mut prompt);
        loop {
            match slow.step(&delayed.library) {
                Ok(_) => break,
                Err(wait) => {
                    waits += 1;
                    delayed.refusals += 1;
                    if delayed.refusals.is_multiple_of(5) {
                        delayed.library.supply(&wait);
                    }
                }
            }
        }
        assert_eq!(slow.craft, prompt.craft);
    }
    assert!(waits >= 5);
    assert!(!slow.in_galaxy());
}

#[test]
fn fast_warp_needs_no_system_data_and_keeps_local_precision() {
    let star = star();
    let mut voyage = galactic(&star, [0.0; 3], [0.0; 3]);
    voyage.craft.warp.gravity = GravityPolicy::Suspended;
    let speed = 10.0 * LY_METRES;
    voyage.craft.warp.set_speed(speed).unwrap();
    voyage.craft.engage_warp().unwrap();
    voyage.craft.warp.actual_ms = speed;
    let start = galactic_state(&voyage, star.position_pc).0;
    let mut anchors = 0;
    for _ in 0..100 {
        let rebase = voyage
            .step(&Blank)
            .expect("no data needed above the threshold");
        anchors += usize::from(rebase.rebase.is_some());
        assert!(voyage.in_galaxy());
        assert!(
            DVec3::from_array(voyage.craft.state().position).length()
                <= ANCHOR_M * 1.01 + speed * STEP
        );
    }
    assert!(anchors > 10);
    let travelled = (galactic_state(&voyage, star.position_pc).0 - start).length();
    assert!((travelled / (speed * 100.0 * STEP) - 1.0).abs() < 1e-9);
}

const STEP: f64 = crate::craft::STEP_SECONDS;

#[test]
fn leaving_and_reanchoring_preserve_position_heading_and_momentum() {
    let star = star();
    let arrival = arrival(&star);
    let mut craft = Craft::new(
        "craft-1".into(),
        "Ship 1".into(),
        CraftKind::Ship,
        Frame::System {
            star: star.id.clone(),
        },
        0,
    );
    craft.orientation = DQuat::from_rotation_y(0.7).to_array();
    craft.place(
        State {
            position: [3.0 * arrival.range_m, 1e9, -2e9],
            velocity: [123.0, 45.0, 67.0],
        },
        0,
    );
    let mut voyage = Voyage::new(craft, arrival.field, star.position_pc);
    let before = galactic_state(&voyage, star.position_pc);
    let rebase = voyage.settle(&Blank).unwrap().expect("left");
    assert!(rebase.frame_changed && voyage.in_galaxy());
    let after = galactic_state(&voyage, star.position_pc);
    assert!((after.0 - before.0).length() < 1.0);
    assert!((after.1 - before.1).length() < 1e-8);
    assert!((after.2 - before.2).length() < 1e-12);
    // Far from anything, fast enough to need no data, it re-anchors.
    voyage.craft.warp.set_speed(ARRIVAL_MAX_SPEED).unwrap();
    voyage.craft.engage_warp().unwrap();
    voyage.craft.warp.actual_ms = ARRIVAL_MAX_SPEED;
    let mut state = voyage.craft.state();
    state.position = [ANCHOR_M * 1.5 + 123.5, 25.0, 43.0];
    voyage.craft.place(state, 0);
    let before = galactic_state(&voyage, star.position_pc);
    let rebase = voyage.settle(&Blank).unwrap().expect("re-anchored");
    assert!(!rebase.frame_changed);
    let after = galactic_state(&voyage, star.position_pc);
    assert!((after.0 - before.0).length() < 1.0);
    assert_eq!(voyage.craft.state().velocity, state.velocity);
    assert!(DVec3::from_array(voyage.craft.state().position).length() < 1e6);
}

#[test]
fn frames_are_anchored_from_seeds_else_from_the_list() {
    let star = star();
    let mut library = Library::default();
    // A stored position is ignored for a seeded star: arrival uses the seeds.
    let system = crate::generated_system(&star.id, [0.0; 3]).unwrap();
    assert_eq!(library.origin_pc(&system, 0), star.position_pc);
    assert_eq!(library.origin_pc(&system, 0), arrival(&star).origin_pc);
    // A legacy page has no cell star, so its list places it.
    let legacy = space_shared::StarSystem {
        page_stars: Vec::new(),
        position_ly: [1.0, 2.0, 3.0],
        ..system
    };
    let home = GalaxyModel::milky_way().home_pc;
    let origin = library.origin_pc(&legacy, 0);
    for i in 0..3 {
        let ly = (origin[i] - home[i]) * PC_METRES / LY_METRES;
        assert!((ly - legacy.position_ly[i]).abs() < 1e-9);
    }
}

/// A generated neighbour of `star` with planets, a few light years off.
fn neighbour(
    star: &CellStar,
) -> (
    crate::galaxy::neighbourhood::Candidate,
    space_shared::StarSystem,
) {
    let mut sky = Sky::default();
    let mut near =
        crate::galaxy::neighbourhood::stars_within(&mut sky, star.position_pc, 2.0, threads());
    near.sort_by(|a, b| a.id.cmp(&b.id));
    near.into_iter()
        .filter(|other| {
            let ly = DVec3::from_array(other.position_pc)
                .distance(DVec3::from_array(star.position_pc))
                * PC_METRES
                / LY_METRES;
            (1.0..6.0).contains(&ly)
        })
        .find_map(|other| {
            let candidate = crate::galaxy::neighbourhood::candidate(&mut sky, &other.id)?;
            let system = crate::generated_system(&candidate.id, candidate_ly(&candidate))?;
            (system.planets.len() >= 2).then_some((candidate, system))
        })
        .expect("a neighbour with planets")
}

/// A ship at rest in `star`'s system, `position` from its centre.
fn in_system(star: &CellStar, position: [f64; 3]) -> Voyage {
    let mut craft = Craft::new(
        "craft-1".into(),
        "Ship 1".into(),
        CraftKind::Ship,
        Frame::System {
            star: star.id.clone(),
        },
        0,
    );
    craft.place(
        State {
            position,
            velocity: [0.0; 3],
        },
        0,
    );
    Voyage::new(craft, arrival(star).field, star.position_pc)
}

fn warp_to(target: &str, orbit: bool, time: Option<f64>) -> Order {
    Order::Warp {
        command: crate::craft::WarpOrder::To {
            target: target.into(),
            matched: false,
            orbit,
            time,
        },
    }
}

/// What a route's flight did, step by step, until it ended.
#[derive(Debug, Default)]
struct Trip {
    seconds: f64,
    top_speed: f64,
    /// The step's frame change into the destination, and the warp speed
    /// then.
    entered: Option<(i64, f64)>,
    /// Nearest any of the start system's bodies came, in keep-clear radii.
    nearest: f64,
}

fn fly_trip(
    voyage: &mut Voyage,
    atlas: &mut impl FnMut(&mut Voyage) -> Step,
    start: &Field,
) -> Trip {
    let begin = voyage.craft.seconds();
    let mut trip = Trip {
        nearest: f64::INFINITY,
        ..Trip::default()
    };
    for _ in 0..20_000 {
        let step = atlas(voyage);
        let route = voyage.craft.route.as_ref().unwrap();
        trip.top_speed = trip.top_speed.max(voyage.craft.warp.actual_ms);
        if step.rebase.is_some_and(|r| r.frame_changed) && !voyage.in_galaxy() {
            trip.entered = Some((voyage.craft.tick, voyage.craft.warp.actual_ms));
        }
        if voyage.craft.frame.star() == start.system().id {
            let at = DVec3::from_array(voyage.craft.state().position);
            for body in start.bodies(voyage.craft.seconds()) {
                let clear = crate::craft::route::KEEP_CLEAR * body.radius_m;
                trip.nearest = trip
                    .nearest
                    .min(at.distance(DVec3::from_array(body.state.position)) / clear);
            }
        }
        if !route.active() {
            break;
        }
    }
    trip.seconds = voyage.craft.seconds() - begin;
    trip
}

#[test]
fn a_route_to_another_system_waits_for_it_then_enters_on_the_slowdown_on_time() {
    let star = star();
    let (candidate, system) = neighbour(&star);
    let target = system.planets[1].id.clone();
    let mut voyage = in_system(&star, [AU, 0.0, 0.0]);
    let start = voyage.field.clone();
    voyage.order(0, warp_to(&target, true, Some(40.0))).unwrap();
    // Where the destination is, then its bodies, before anything moves.
    let unchanged = voyage.craft.clone();
    assert_eq!(
        voyage.step(&Blank).unwrap_err(),
        Wait::Star(candidate.id.clone())
    );
    let mut library = Library::default();
    library.supply(&Wait::Star(candidate.id.clone()));
    assert_eq!(
        voyage.step(&library).unwrap_err(),
        Wait::System(candidate.clone())
    );
    assert_eq!(voyage.craft, unchanged);
    let trip = fly_trip(&mut voyage, &mut |v| library.step(v), &start);
    let route = voyage.craft.route.as_ref().unwrap();
    assert_eq!(route.stage, Stage::Arrived, "{route:?}");
    assert_eq!(
        route.end.as_deref(),
        Some(format!("Arrived at `{target}`").as_str())
    );
    assert!((trip.seconds - 40.0).abs() < 0.5, "{} s", trip.seconds);
    assert_eq!(voyage.craft.frame.star(), candidate.id);
    // Faster than light between the stars, entering only once below it.
    assert!(trip.top_speed > 0.1 * LY_METRES, "{trip:?}");
    let (_, speed) = trip.entered.expect("entered the destination");
    assert!(speed < ARRIVAL_MAX_SPEED, "{trip:?}");
    // Into a rough orbit round the planet.
    library.step(&mut voyage);
    assert_eq!(voyage.craft.reference.as_deref(), Some(target.as_str()));
}

#[test]
fn a_charted_list_page_is_flown_to_from_galactic_space() {
    let lists = crate::demo_galaxy();
    let list = &lists.systems[0];
    let page = list.first_page();
    let page_star = page_star_id(list, page);
    let target = list.on_page(page).next().unwrap().id.clone();
    let mut library = Library::new(lists.clone());
    library.supply(&Wait::Star(page_star.clone()));
    let candidate = library.locate(&page_star).unwrap().unwrap().clone();
    // Half a light year out, in galactic space.
    let origin = (DVec3::from_array(candidate.position_pc)
        + DVec3::new(0.5, 0.0, 0.0) * LY_METRES / PC_METRES)
        .to_array();
    let mut craft = Craft::new(
        "craft-1".into(),
        "Ship 1".into(),
        CraftKind::Ship,
        Frame::Galaxy { origin_pc: origin },
        0,
    );
    craft.place(State::default(), 0);
    let mut voyage = Voyage::new(craft, arrival(&star()).field, origin);
    voyage.order(0, warp_to(&target, false, None)).unwrap();
    let start = voyage.field.clone();
    let trip = fly_trip(&mut voyage, &mut |v| library.step(v), &start);
    let route = voyage.craft.route.as_ref().unwrap();
    assert_eq!(route.stage, Stage::Arrived, "{route:?}");
    assert_eq!(voyage.craft.frame.star(), page_star);
    // The charted page's bodies, not the generated ones it replaces.
    assert_eq!(voyage.field.system(), list);
    assert!(
        (trip.seconds - route.trip_s).abs() < 0.5,
        "{} s of {}",
        trip.seconds,
        route.trip_s
    );
}

#[test]
fn leaving_from_behind_a_planet_for_another_system_climbs_round_it() {
    let star = star();
    let (candidate, system) = neighbour(&star);
    let home = arrival(&star);
    let planet = home
        .field
        .bodies(0.0)
        .into_iter()
        .filter(|body| body.kind == crate::craft::BodyKind::Planet)
        .max_by(|a, b| a.radius_m.total_cmp(&b.radius_m))
        .expect("a planet");
    // The far side from the destination, inside the keep-clear sphere.
    let probe = in_system(&star, [0.0; 3]);
    let away = -(probe.basis().inverse()
        * (DVec3::from_array(candidate.position_pc) - DVec3::from_array(star.position_pc)))
    .normalize();
    let altitude = 1.2 * planet.radius_m;
    let at = DVec3::from_array(planet.state.position) + away * altitude;
    let mut voyage = in_system(&star, at.to_array());
    voyage.craft.orientation =
        DQuat::from_rotation_arc(-DVec3::Z, away.any_orthonormal_vector()).to_array();
    voyage
        .order(0, warp_to(&system.planets[0].id, false, Some(30.0)))
        .unwrap();
    let mut library = Library::default();
    library.step(&mut voyage);
    let around = voyage.craft.route.as_ref().unwrap().around.clone();
    assert_eq!(around.as_deref(), Some(planet.id.as_str()));
    let trip = fly_trip(&mut voyage, &mut |v| library.step(v), &home.field);
    let route = voyage.craft.route.as_ref().unwrap();
    assert_eq!(route.stage, Stage::Arrived, "{route:?}");
    // It starts inside the sphere and never sinks deeper.
    let start = altitude / (crate::craft::route::KEEP_CLEAR * planet.radius_m);
    assert!(trip.nearest > 0.98 * start, "{trip:?}");
    assert!((trip.seconds - 30.0).abs() < 1.0, "{} s", trip.seconds);
}

#[test]
fn a_route_to_no_system_ends_unplotted_with_warp_off() {
    let star = star();
    let mut voyage = in_system(&star, [AU, 0.0, 0.0]);
    voyage
        .order(0, warp_to("nowhere#A/1", false, None))
        .unwrap();
    let mut library = Library::default();
    library.step(&mut voyage);
    let route = voyage.craft.route.as_ref().unwrap();
    assert_eq!(route.stage, Stage::Released);
    assert!(route.end.as_ref().unwrap().contains("No body"), "{route:?}");
    assert!(!voyage.craft.warp.engaged);
}

#[test]
fn waiting_for_a_destination_never_changes_the_route() {
    let star = star();
    let (_, system) = neighbour(&star);
    let mut prompt = in_system(&star, [AU, 0.0, 0.0]);
    prompt
        .order(0, warp_to(&system.planets[0].id, false, Some(20.0)))
        .unwrap();
    let mut slow = prompt.clone();
    let mut library = Library::default();
    let mut delayed = Slow {
        library: Library::default(),
        refusals: 0,
    };
    let mut waits = 0;
    while prompt.craft.route.as_ref().unwrap().active() {
        library.step(&mut prompt);
        loop {
            match slow.step(&delayed.library) {
                Ok(_) => break,
                Err(wait) => {
                    waits += 1;
                    delayed.refusals += 1;
                    if delayed.refusals.is_multiple_of(5) {
                        delayed.library.supply(&wait);
                    }
                }
            }
        }
        assert_eq!(slow.craft, prompt.craft);
    }
    assert!(waits >= 10, "{waits}");
    assert_eq!(prompt.craft.route.as_ref().unwrap().stage, Stage::Arrived);
}

#[test]
fn a_whole_system_is_matched_on_arrival_wherever_the_ship_drops_out() {
    let star = star();
    let (candidate, _) = neighbour(&star);
    let mut voyage = in_system(&star, [AU, 0.0, 0.0]);
    voyage
        .order(
            0,
            Order::Warp {
                command: crate::craft::WarpOrder::To {
                    target: candidate.id.clone(),
                    matched: true,
                    orbit: false,
                    time: Some(10.0),
                },
            },
        )
        .unwrap();
    let start = voyage.field.clone();
    let mut library = Library::default();
    let trip = fly_trip(&mut voyage, &mut |v| library.step(v), &start);
    let route = voyage.craft.route.clone().unwrap();
    assert_eq!(route.stage, Stage::Arrived, "{route:?}");
    // Too short a time to slow down on the profile: still braking hard, it
    // drops out a step early, never inside the standoff, and faster than
    // light, so still in galactic space. It enters at the next step.
    assert!(route.distance_m > route.standoff_m, "{route:?}");
    assert!(trip.entered.is_none() && voyage.in_galaxy(), "{trip:?}");
    library.step(&mut voyage);
    assert_eq!(voyage.craft.frame.star(), candidate.id);
    // At rest beside the system's root, in its own frame.
    let root = voyage
        .field
        .target(&candidate.id, voyage.craft.seconds())
        .unwrap();
    let relative =
        DVec3::from_array(voyage.craft.state().velocity) - DVec3::from_array(root.state.velocity);
    assert!(relative.length() < 1.0, "{relative}");
}

mod ships;
