use super::*;
use crate::craft::voyage::{Blank, Field};
use crate::craft::{Craft, CraftKind, Frame, Order, WarpOrder};
use std::{cell::RefCell, rc::Rc};

type ResultSlot = Rc<RefCell<Option<Result<Reply, String>>>>;

#[derive(Clone, Default)]
struct Fake {
    submitted: Rc<RefCell<Vec<(Request, ResultSlot)>>>,
}

impl Backend for Fake {
    type Job = ResultSlot;
    fn start(&mut self, request: Request) -> Result<Self::Job, String> {
        let result = ResultSlot::default();
        self.submitted.borrow_mut().push((request, result.clone()));
        Ok(result)
    }
    fn poll(job: &mut Self::Job) -> Option<Result<Reply, String>> {
        job.borrow_mut().take()
    }
}

fn voyage() -> Voyage {
    let craft = Craft::new(
        "ship".into(),
        "Ship".into(),
        CraftKind::Ship,
        Frame::Galaxy {
            origin_pc: [0.5; 3],
        },
        0,
    );
    let field = Field::new(crate::demo_galaxy().systems.remove(0), 0);
    Voyage::new(craft, field, [0.5; 3])
}

fn region(i: i64) -> Wait {
    Wait::Region(Region([i, 0, 0]))
}

#[test]
fn bounded_deduplicated_demands_precede_speculation_and_expire() {
    let lists = Galaxy::default();
    let fake = Fake::default();
    let mut loader = Loader::new(fake.clone());
    loader.begin_round(1, &lists);
    for i in 0..MAX_QUEUED as i64 + 10 {
        loader.request(&region(i), false, &lists);
    }
    for _ in 0..10 {
        loader.request(&region(999), true, &lists);
    }
    assert_eq!(loader.counts(), (MAX_QUEUED, 0));
    loader.dispatch();
    assert_eq!(loader.counts(), (MAX_QUEUED - MAX_RUNNING, MAX_RUNNING));
    assert_eq!(fake.submitted.borrow()[0].0, Request::of(&region(999)));
    // Changing course discards queued work, but lets the two running jobs finish.
    loader.begin_round(2, &lists);
    assert_eq!(loader.counts(), (0, MAX_RUNNING));
    for (_, slot) in fake.submitted.borrow().iter() {
        *slot.borrow_mut() = Some(Ok(Reply::Region {
            candidates: Vec::new(),
        }));
    }
    loader.begin_round(3, &lists);
    assert!(loader.request(&region(999), true, &lists));
    assert_eq!(loader.counts(), (0, 0));
}

#[test]
fn one_ship_waits_while_another_advances_then_replays_identically() {
    let lists = Galaxy::default();
    let fake = Fake::default();
    let mut loader = Loader::new(fake.clone());
    let mut slow = voyage();
    slow.order(
        2,
        Order::Thrust {
            thrust: [1.0, 0.0, 0.0],
        },
    )
    .unwrap();
    let mut prompt = slow.clone();
    let mut fast = voyage();
    fast.craft.engage_warp().unwrap();
    fast.craft.warp.actual_ms = crate::LIGHT_SPEED_MS * 2.0;
    fast.craft
        .warp
        .set_speed(crate::LIGHT_SPEED_MS * 2.0)
        .unwrap();
    loader.begin_round(1, &lists);
    let before = slow.craft.clone();
    let waiting = loader.catch_up(&mut slow, 10, 20, &lists);
    assert_eq!(waiting.steps, 0);
    assert!(waiting.waiting.is_some());
    assert_eq!(slow.craft, before);
    assert_eq!(loader.catch_up(&mut fast, 10, 20, &lists).steps, 10);
    loader.dispatch();
    assert_eq!(fake.submitted.borrow().len(), 1);
    *fake.submitted.borrow()[0].1.borrow_mut() = Some(Ok(Reply::Region {
        candidates: Vec::new(),
    }));
    loader.begin_round(2, &lists);
    assert_eq!(loader.catch_up(&mut slow, 10, 3, &lists).steps, 3);
    assert_eq!(slow.craft.tick, 3, "catch-up budget is respected");
    assert_eq!(loader.catch_up(&mut slow, 10, 20, &lists).steps, 7);
    for _ in 0..10 {
        prompt.step(&loader.cache).unwrap();
    }
    assert_eq!(slow.craft, prompt.craft);
    assert!(slow.pending_orders().is_empty());
}

#[test]
fn failed_jobs_are_retryable_without_inline_generation() {
    let lists = Galaxy::default();
    let fake = Fake::default();
    let mut loader = Loader::new(fake.clone());
    loader.request(&region(0), true, &lists);
    loader.dispatch();
    *fake.submitted.borrow()[0].1.borrow_mut() = Some(Err("offline".into()));
    loader.begin_round(1, &lists);
    assert_eq!(loader.error.as_deref(), Some("offline"));
    assert!(!loader.request(&region(0), true, &lists));
    loader.dispatch();
    assert_eq!(fake.submitted.borrow().len(), 2);
    assert!(loader.cache.region(Region([0; 3])).is_none());
    // A successful retry clears the reported failure.
    *fake.submitted.borrow()[1].1.borrow_mut() = Some(Ok(Reply::Region {
        candidates: Vec::new(),
    }));
    loader.begin_round(2, &lists);
    assert!(loader.error.is_none());
    assert!(loader.request(&region(0), true, &lists));
}

#[test]
fn prefetch_tracks_boundaries_slowdown_and_turns_without_scanning_ftl_routes() {
    let mut ship = voyage();
    ship.origin_pc = [0.5, 0.5, 0.000_000_01];
    ship.craft.frame = Frame::Galaxy {
        origin_pc: ship.origin_pc,
    };
    ship.craft.engage_warp().unwrap();
    ship.craft
        .warp
        .set_speed(crate::LIGHT_SPEED_MS / 2.0)
        .unwrap();
    let ahead = prefetch(&ship, &Blank);
    assert_eq!(ahead, vec![region(0), Wait::Region(Region([0, 0, -1]))]);
    ship.craft.orientation = glam::DQuat::from_rotation_y(std::f64::consts::PI).to_array();
    assert_eq!(prefetch(&ship, &Blank), vec![region(0)]);
    ship.craft.warp.actual_ms = crate::LY_METRES * 10.0;
    ship.craft.warp.set_speed(crate::LY_METRES * 10.0).unwrap();
    assert!(prefetch(&ship, &Blank).is_empty());
    ship.order(
        0,
        Order::Warp {
            command: WarpOrder::Set { speed_ms: 1.0 },
        },
    )
    .unwrap();
    ship.craft.warp.actual_ms = crate::LY_METRES * 100.0;
    let predicted = prefetch(&ship, &Blank);
    assert_eq!(
        predicted.len(),
        1,
        "only the slowdown endpoint, not its entire route"
    );
    assert_ne!(predicted[0], region(0));
}

#[test]
fn only_approached_systems_are_prefetched() {
    use crate::neighbourhood::Candidate;
    let lists = Galaxy::default();
    let ship = voyage();
    let mut cache = Cache::default();
    let near = Candidate {
        id: "near".into(),
        position_pc: [0.5; 3],
        reach_pc: 0.01,
    };
    let far = Candidate {
        id: "far".into(),
        position_pc: [0.9; 3],
        reach_pc: 0.01,
    };
    cache.accept(
        &region(0),
        Reply::Region {
            candidates: vec![near.clone(), far],
        },
        &lists,
    );
    assert_eq!(prefetch(&ship, &cache), vec![Wait::System(near)]);
}

#[test]
fn an_orbit_needs_no_census_but_a_near_departure_prepares_one() {
    let mut ship = voyage();
    ship.craft.frame = Frame::System {
        star: super::super::page_star_id(ship.field.system(), ship.field.page()),
    };
    assert!(prefetch(&ship, &Blank).is_empty());
    ship.craft.place(
        crate::State {
            position: [super::super::LEAVE_RANGES * ship.range_m - 1.0, 0.0, 0.0],
            velocity: [1_000.0, 0.0, 0.0],
        },
        0,
    );
    assert!(!prefetch(&ship, &Blank).is_empty());
}

#[test]
fn a_speculative_request_is_promoted_when_a_step_needs_it() {
    let lists = Galaxy::default();
    let fake = Fake::default();
    let mut loader = Loader::new(fake.clone());
    for i in 0..5 {
        loader.request(&region(i), false, &lists);
    }
    loader.request(&region(4), true, &lists);
    loader.dispatch();
    assert_eq!(fake.submitted.borrow()[0].0, Request::of(&region(4)));
    assert_eq!(loader.counts(), (3, 2));
}
