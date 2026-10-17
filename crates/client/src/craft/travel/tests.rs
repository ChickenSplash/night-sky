use super::*;
use space_simulation::craft::{Order, WarpOrder};
use space_simulation::{
    cells::{Cell, CellStar, Sky},
    craft::warp::{GravityPolicy, MAX_SPEED},
    population::TIERS,
};

fn galaxy() -> Galaxy {
    space_simulation::demo_galaxy()
}

fn live() -> Live {
    let system = galaxy().systems[0].clone();
    let page = system.first_page();
    let body = system.on_page(page).next().unwrap().id.clone();
    let origin = crate::scene::page_origin_ly(&system, page) * (LY_METRES / PC_METRES)
        + DVec3::from_array(GalaxyModel::milky_way().home_pc);
    let mut fleet = Fleet::default();
    fleet
        .spawn(system, page, &body, 400_000.0, Launch::Circular, 0.0)
        .unwrap();
    let mut live = fleet.craft.remove(0);
    live.origin_pc = origin.to_array();
    live
}

/// Step as the frame update does, until the craft reaches `until`.
fn catch_up(live: &mut Live, charts: &mut Charts, until: f64) {
    let galaxy = galaxy();
    while live.craft.seconds < until {
        live.catch_up(charts, &galaxy, until, &mut 0.0);
        assert!(live.waiting.is_none(), "{}", live.arrival_status());
    }
}

/// A generated star near home that no demo page is assigned to.
fn uncharted_star() -> CellStar {
    let home = GalaxyModel::milky_way().home_pc;
    let galaxy = galaxy();
    let mut sky = Sky::default();
    (0..TIERS)
        .rev()
        .filter_map(|level| Cell::containing(level, home))
        .flat_map(|cell| (0..4).map(move |i| (cell, i)))
        .find_map(|(cell, i)| {
            let star = sky.cell_star_at(cell, i)?;
            crate::objects::known_system(&galaxy, &star.id)
                .is_none()
                .then_some(star)
        })
        .expect("an uncharted star")
}

/// A craft in galactic space at rest, this far from a star.
fn beside(star: &CellStar, offset: [f64; 3]) -> Live {
    let mut live = live();
    live.craft.frame = Frame::Galaxy {
        origin_pc: star.position_pc,
    };
    live.origin_pc = star.position_pc;
    let state = State {
        position: offset,
        velocity: [0.0; 3],
    };
    live.craft.place(state, 0.0);
    live.previous = state;
    live
}

#[test]
fn unsafe_warp_crosses_unknown_space_at_ten_light_years_per_second() {
    let mut live = live();
    let mut charts = Charts::default();
    live.craft.warp.gravity = GravityPolicy::Suspended;
    live.craft.engage_warp().unwrap();
    let speed = 10.0 * LY_METRES;
    live.craft.warp.set_speed(speed).unwrap();
    let start = live.position_pc(0.0);
    catch_up(&mut live, &mut charts, 100.0 * STEP_SECONDS);
    assert!(live.craft.warp.engaged && live.voyage.in_galaxy());
    assert!(distance(start, live.position_pc(live.craft.seconds)) * PC_METRES > 40.0 * LY_METRES);
    assert!((live.craft.warp.actual_ms / speed - 1.0).abs() < 1e-6);
    // Too fast to enter systems, it never needed any system data.
    assert!(charts.regions.is_empty() && charts.arrivals.is_empty());
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
    live.craft.warp.set_speed(1e12).unwrap();
    live.craft.engage_warp().unwrap();
    catch_up(&mut live, &mut Charts::default(), STEP_SECONDS);
    assert!(live.craft.warp.engaged);
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
    let voyage = &mut live.voyage;
    Order::Warp {
        command: WarpOrder::Engage,
    }
    .apply(&mut voyage.craft, Some(&voyage.field))
    .unwrap();
    assert!(live.craft.warp.engaged);
}

#[test]
fn warp_orders_reject_thrust_and_turning_and_validate_speed() {
    let mut live = live();
    let voyage = &mut live.voyage;
    let (craft, field) = (&mut voyage.craft, Some(&voyage.field));
    let order = |command| Order::Warp { command };
    order(WarpOrder::Engage).apply(craft, field).unwrap();
    assert!(Order::Throttle { value: 0.1 }.apply(craft, field).is_err());
    assert!(
        Order::Rotate {
            pitch: 0.1,
            yaw: 0.0,
            roll: 0.0
        }
        .apply(craft, field)
        .is_err()
    );
    assert!(Order::Stop.apply(craft, field).is_err());
    assert!(
        order(WarpOrder::Set { speed_ms: f64::NAN })
            .apply(craft, field)
            .is_err()
    );
    order(WarpOrder::Up).apply(craft, field).unwrap();
    assert!(craft.warp.requested_ms > 1.0);
    order(WarpOrder::Disengage).apply(craft, field).unwrap();
    Order::Throttle { value: 0.1 }.apply(craft, field).unwrap();
    assert!(order(WarpOrder::Engage).apply(craft, field).is_err());
}

#[test]
fn an_unfollowed_ship_enters_an_uncharted_system_without_streamed_stars() {
    let star = uncharted_star();
    let mut live = beside(&star, [200.0 * space_simulation::AU_METRES, 0.0, 0.0]);
    let mut charts = Charts::default();
    catch_up(&mut live, &mut charts, 2.0);
    assert_eq!(
        live.craft.frame,
        Frame::System {
            star: star.id.clone()
        }
    );
    assert_eq!(live.origin_pc, star.position_pc);
    assert!(
        live.craft.reference.is_some(),
        "gravity and an orbit reference"
    );
    assert!(
        live.forecast.shown.is_none(),
        "no stale forecast across frames"
    );
}

#[test]
fn background_loading_holds_the_clock_and_never_changes_the_path() {
    let star = uncharted_star();
    let offset = [300.0 * space_simulation::AU_METRES, 0.0, 0.0];
    let galaxy = galaxy();
    let mut prompt = beside(&star, offset);
    let mut slow = beside(&star, offset);
    let mut charts = Charts::default();
    let mut background = Charts {
        background: true,
        ..Charts::default()
    };
    let until = 3.0;
    catch_up(&mut prompt, &mut charts, until);
    let mut waited = 0;
    for _ in 0..20_000 {
        background.poll(&galaxy);
        slow.catch_up(&mut background, &galaxy, until, &mut 0.0);
        if slow.craft.seconds >= until {
            break;
        }
        waited += usize::from(slow.waiting.is_some());
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert!(waited > 0, "it waited for the background worker");
    assert!(background.error.is_none(), "{:?}", background.error);
    assert_eq!(slow.craft, prompt.craft);
    assert!(!slow.voyage.in_galaxy());
}

#[test]
fn list_pages_take_the_place_of_their_generated_systems() {
    let galaxy = galaxy();
    let list = &galaxy.systems[0];
    let page = list.first_page();
    let page_star = list.page_star(page).expect("a mapped demo page");
    let candidate = Candidate {
        id: page_star.id.clone(),
        position_pc: [1.0, 2.0, 3.0],
        reach_pc: 1.0,
    };
    let mut charts = Charts::default();
    assert!(charts.offer(&Wait::System(candidate), &galaxy));
    let arrival = charts.arrival(&page_star.id).unwrap();
    assert_eq!(arrival.field.system().id, list.id);
    assert_eq!(arrival.field.page(), page);
    assert_eq!(arrival.origin_pc, [1.0, 2.0, 3.0]);
}

#[test]
fn maximum_warp_dropout_then_continues_in_galactic_space() {
    let mut live = live();
    let mut charts = Charts::default();
    live.craft.warp.gravity = GravityPolicy::Suspended;
    live.craft.warp.set_speed(MAX_SPEED).unwrap();
    live.craft.engage_warp().unwrap();
    catch_up(&mut live, &mut charts, 20.0 * STEP_SECONDS);
    assert!(live.voyage.in_galaxy());
    live.craft.warp.stop("manual");
    // Slow again, it asks for the systems around it before each step.
    catch_up(&mut live, &mut charts, 40.0 * STEP_SECONDS);
    assert!(live.voyage.in_galaxy());
    assert!(!charts.regions.is_empty());
    assert_eq!(live.arrival_status(), "entering systems");
}
