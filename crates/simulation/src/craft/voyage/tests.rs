use super::*;
use crate::cells::{Cell, CellStar, Sky};
use crate::craft::{CraftKind, warp::GravityPolicy};
use crate::galaxy::GalaxyModel;
use crate::population::TIERS;

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

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
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

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn queued_warp_orders_replay_after_a_snapshot() {
    use crate::craft::{Order, WarpOrder};
    let mut voyage = empty_voyage();
    let log = vec![
        ScheduledOrder {
            tick: 1,
            order: Order::Warp {
                command: WarpOrder::Set {
                    speed_ms: 100_000.0,
                },
            },
        },
        ScheduledOrder {
            tick: 1,
            order: Order::Warp {
                command: WarpOrder::Engage,
            },
        },
        ScheduledOrder {
            tick: 6,
            order: Order::Warp {
                command: WarpOrder::Set { speed_ms: 1_000.0 },
            },
        },
        ScheduledOrder {
            tick: 10,
            order: Order::Warp {
                command: WarpOrder::Disengage,
            },
        },
        ScheduledOrder {
            tick: 10,
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
        restored.order(input.tick, input.order).unwrap();
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

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
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

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
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

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
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

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
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

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
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
