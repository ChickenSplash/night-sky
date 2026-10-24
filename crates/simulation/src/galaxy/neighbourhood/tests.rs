use super::*;
use crate::galaxy::GalaxyModel;

fn places() -> [(&'static str, [f64; 3]); 4] {
    let home = GalaxyModel::milky_way().home_pc;
    [
        ("home", home),
        ("inner disc", [2_000.0, 10.0, 0.0]),
        ("bulge", [300.0, 50.0, 100.0]),
        ("outer disc", [14_000.0, 0.0, 3_000.0]),
    ]
}

fn ids(stars: &[NearbyStar]) -> Vec<String> {
    let mut ids: Vec<_> = stars.iter().map(|s| s.id.clone()).collect();
    ids.sort();
    ids
}

/// A star from the finest populated cell around a point.
fn star_near(sky: &mut Sky, p: [f64; 3]) -> crate::galaxy::cells::CellStar {
    (0..TIERS)
        .rev()
        .find_map(|level| {
            let cell = Cell::containing(level, p)?;
            sky.cell_star_at(cell, 0)
        })
        .expect("a star")
}

#[test]
fn finds_a_known_star_the_same_way_with_any_threads_or_cache() {
    let home = GalaxyModel::milky_way().home_pc;
    let star = star_near(&mut Sky::default(), home);
    let cold = stars_within(&mut Sky::default(), star.position_pc, 0.5, 1);
    assert!(cold.iter().any(|s| s.id == star.id));
    assert!(
        cold.iter()
            .all(|s| distance(s.position_pc, star.position_pc) <= 0.5)
    );
    let mut sky = Sky::default();
    stars_within(&mut sky, home, 0.1, 1);
    let warm = stars_within(&mut sky, star.position_pc, 0.5, 4);
    assert_eq!(ids(&cold), ids(&warm));
    for s in &warm {
        assert_eq!(sky.star(&s.id).map(|c| c.position_pc), Some(s.position_pc));
    }
}

#[test]
fn region_candidates_cover_every_system_that_can_reach_it() {
    let home = GalaxyModel::milky_way().home_pc;
    let mut sky = Sky::default();
    let star = star_near(&mut sky, home);
    let region = Region::of(star.position_pc);
    let candidates = candidates(&mut sky, region, 2);
    let own = candidates
        .iter()
        .find(|c| c.id == star.id)
        .expect("own star");
    assert!(own.reach_pc >= reach_pc(0.0));
    assert!(candidates.windows(2).all(|w| w[0].id < w[1].id));
    // Generation bounds every system inside its reach.
    let bodies = crate::galaxy::cells::cell_bodies(&star.id).unwrap();
    let system = crate::generated_system(&star.id, [0.0; 3]).unwrap();
    let extent = crate::system_extent_au(&system, 0);
    assert!(
        range_m(extent) / PC_METRES <= own.reach_pc + 1e-12,
        "{bodies:?}"
    );
}

/// Run with `cargo test -p space-simulation --release neighbourhood_timing -- --ignored --nocapture`.
#[test]
#[ignore]
fn neighbourhood_timing() {
    use std::time::Instant;
    let threads = std::thread::available_parallelism().map_or(1, |n| n.get());
    for (name, p) in places() {
        for threads in [1, threads] {
            let mut sky = Sky::default();
            let start = Instant::now();
            let found = candidates(&mut sky, Region::of(p), threads);
            let cold = start.elapsed();
            let start = Instant::now();
            candidates(
                &mut sky,
                Region::of([p[0] + REGION_PC, p[1], p[2]]),
                threads,
            );
            let warm = start.elapsed();
            println!(
                "{name:>10} {threads:>2} threads: {:>4} candidates, cold {cold:?}, next region {warm:?}",
                found.len()
            );
        }
    }
}
