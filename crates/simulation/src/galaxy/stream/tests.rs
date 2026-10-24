use super::*;
use crate::galaxy::GalaxyModel;
use crate::seed::Rng;
use std::collections::HashMap;

/// A few bright tiers only, so the tests stay quick.
const MAGNITUDE: f64 = -1.0;

fn run(loader: &mut Loader, more: &mut dyn FnMut() -> bool) -> HashMap<Cell, StarCell> {
    let mut cells = HashMap::new();
    while loader.pending() > 0 {
        for event in loader.step(more) {
            match event {
                Event::Loaded(stars) => {
                    assert!(cells.insert(stars.cell, stars).is_none());
                }
                Event::Dropped(cell) => {
                    assert!(cells.remove(&cell).is_some());
                }
            }
        }
    }
    cells
}

#[test]
fn wanted_cells_cover_each_tier_reach() {
    let home = GalaxyModel::milky_way().home_pc;
    let cells: HashSet<Cell> = wanted(home, 6.5).into_iter().collect();
    let mut rng = Rng::seeded(3);
    for tier in 0..TIERS {
        let reach = reach_pc(tier, 6.5);
        let level: Vec<_> = cells.iter().filter(|c| c.level() == tier).collect();
        assert!((1..=27).contains(&level.len()), "tier {tier}");
        for _ in 0..200 {
            // A random point within reach, and inside the galaxy's cube, lies
            // in a wanted cell of the tier.
            let d = [rng.unit(), rng.unit(), rng.unit()].map(|u| 2.0 * u - 1.0);
            let length = d.iter().map(|x| x * x).sum::<f64>().sqrt();
            let r = reach * rng.unit().cbrt() / length;
            let p = std::array::from_fn(|axis| home[axis] + d[axis] * r);
            if Cell::ROOT.contains(p) {
                assert!(level.iter().any(|c| c.contains(p)), "tier {tier}");
            }
        }
    }
    // The cells the camera touches come first, brightest first.
    let order = wanted(home, 6.5);
    let touching = order.iter().take_while(|c| gap_pc(**c, home) == 0.0);
    assert!(touching.clone().is_sorted_by_key(|c| c.level()));
    for tier in 0..TIERS {
        let own = Cell::containing(tier, home).unwrap();
        assert!(touching.clone().any(|&c| c == own));
    }
}

#[test]
fn loads_the_same_stars_in_any_slices() {
    let home = GalaxyModel::milky_way().home_pc;
    let mut whole = Loader::new(Sky::default(), MAGNITUDE);
    whole.aim(home);
    let cells = run(&mut whole, &mut || true);
    assert_eq!(cells.len(), wanted(home, MAGNITUDE).len());
    assert_eq!(whole.loaded(), cells.len());
    // One chunk of stars per step gives the same cells.
    let mut sliced = Loader::new(Sky::default(), MAGNITUDE);
    sliced.aim(home);
    assert_eq!(run(&mut sliced, &mut || false), cells);
    // So do several threads sharing each cell.
    for more in [true, false] {
        let mut shared = Loader::new(Sky::default(), MAGNITUDE).with_threads(3);
        shared.aim(home);
        assert_eq!(run(&mut shared, &mut || more), cells);
    }
    // And they are the sky's own stars, in grid order rather than the sky's.
    let mut sky = Sky::default();
    for (cell, stars) in &cells {
        let own = sky.cell_stars(*cell);
        assert_eq!(own.len(), stars.stars.len());
        let mut points = stars.stars.clone();
        let mut own = own.to_vec();
        own.sort_by(|a, b| a.id.cmp(&b.id));
        points.sort_by_key(|point| point.id(*cell));
        for (star, point) in own.iter().zip(&points) {
            assert_eq!(point.id(*cell), star.id);
            assert_eq!(point.position_pc, star.position_pc);
            assert_eq!(point.temperature_k, star.star.temperature_k as f32);
        }
    }
    assert!(cells.values().map(|c| c.stars.len()).sum::<usize>() > 1_000);
}

#[test]
fn moving_drops_cells_left_behind_and_loads_new_ones() {
    let home = GalaxyModel::milky_way().home_pc;
    let mut loader = Loader::new(Sky::default(), MAGNITUDE);
    loader.aim(home);
    let mut cells = run(&mut loader, &mut || true);
    // A small step back and forth drops nothing.
    let step = reach_pc(TIERS - 1, MAGNITUDE) * 0.1;
    for x in [home[0] + step, home[0]] {
        loader.aim([x, home[1], home[2]]);
        assert!(
            loader
                .step(&mut || true)
                .iter()
                .all(|e| matches!(e, Event::Loaded(_)))
        );
    }
    // Across the disc, near cells go and new ones come, with nothing loaded
    // twice or dropped unloaded.
    let away = [home[0] - 3_000.0, home[1], home[2] + 2_000.0];
    loader.aim(away);
    let mut dropped = 0;
    loop {
        for event in loader.step(&mut || true) {
            match event {
                Event::Loaded(stars) => assert!(cells.insert(stars.cell, stars).is_none()),
                Event::Dropped(cell) => {
                    dropped += 1;
                    assert!(cells.remove(&cell).is_some());
                }
            }
        }
        if loader.pending() == 0 {
            break;
        }
    }
    assert!(dropped > 0);
    let wanted: HashSet<Cell> = wanted(away, MAGNITUDE).into_iter().collect();
    assert!(wanted.iter().all(|cell| cells.contains_key(cell)));
    assert!(cells.keys().all(|&cell| loader.keeps(cell, away)));
    assert_eq!(loader.loaded(), cells.len());
}

#[test]
fn moving_away_from_building_grids_drops_only_delivered_cells() {
    let home = GalaxyModel::milky_way().home_pc;
    let mut loader = Loader::new(Sky::default(), MAGNITUDE).with_threads(3);
    loader.aim(home);
    let mut cells = HashMap::new();
    while loader.gridding.is_empty() {
        for event in loader.step(&mut || false) {
            if let Event::Loaded(stars) = event {
                cells.insert(stars.cell, stars);
            }
        }
    }
    loader.aim([1_000_000.0, home[1], home[2]]);
    assert!(loader.take_cancelled_jobs() > 0);
    assert!(loader.gridding.is_empty());
    for event in loader.step(&mut || true) {
        match event {
            Event::Loaded(stars) => assert!(cells.insert(stars.cell, stars).is_none()),
            Event::Dropped(cell) => assert!(cells.remove(&cell).is_some()),
        }
    }
    assert!(cells.is_empty());
}

#[test]
fn abandoned_partial_cell_is_counted_once() {
    let home = GalaxyModel::milky_way().home_pc;
    let mut loader = Loader::new(Sky::default(), MAGNITUDE);
    loader.aim(home);
    loader.step(&mut || false);
    assert_eq!(loader.take_cancelled_jobs(), 0);
    loader.aim([1_000_000.0, home[1], home[2]]);
    assert_eq!(loader.take_cancelled_jobs(), 1);
    assert_eq!(loader.take_cancelled_jobs(), 0);
}
