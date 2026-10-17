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
    let rebase = library.step(&mut voyage).expect("a handoff");
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
        anchors += usize::from(rebase.is_some());
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
