use super::*;
use crate::galaxy::GalaxyModel;

/// A cell with stars scattered through it and a dense cluster inside.
fn scattered_cell() -> StarCell {
    let cell = Cell::containing(8, GalaxyModel::milky_way().home_pc).unwrap();
    let (min, size) = (cell.min_pc(), cell.size_pc());
    let mut seed = 12345_u64;
    let mut next = || {
        seed = seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (seed >> 11) as f64 / (1u64 << 53) as f64
    };
    let stars = (0..20_000_u32)
        .map(|index| {
            let position_pc = if index % 2 == 0 {
                std::array::from_fn(|axis| min[axis] + next() * size)
            } else {
                std::array::from_fn(|axis| min[axis] + size * 0.3 + next())
            };
            // A few wide multiples among single stars.
            let separation_au = if index % 7 == 0 {
                10f64.powf(1.0 + 4.0 * next()) as f32
            } else {
                0.0
            };
            StarPoint {
                position_pc,
                luminosity_solar: 1.0,
                temperature_k: 5772.0,
                radius_solar: 1.0,
                separation_au,
                component: Component::ThinDisc,
                index,
            }
        })
        .collect();
    StarCell::new(cell, stars)
}

fn distance(a: [f64; 3], b: [f64; 3]) -> f64 {
    (0..3).map(|i| (a[i] - b[i]).powi(2)).sum::<f64>().sqrt()
}

#[test]
fn grid_finds_the_same_stars_as_a_full_scan() {
    let cell = scattered_cell();
    let (min, size) = (cell.cell.min_pc(), cell.cell.size_pc());
    let mut checked = 0;
    // Probes inside, in the cluster, and a little outside the cell.
    for step in 0..300 {
        let t = step as f64 / 300.0;
        let p: [f64; 3] =
            std::array::from_fn(|axis| min[axis] - 3.0 + (size + 6.0) * t * [1.0, 0.7, 0.4][axis]);
        let cluster: [f64; 3] = std::array::from_fn(|axis| min[axis] + size * 0.3 + t);
        for (p, in_cluster) in [(p, false), (cluster, true)] {
            for cap in [0.006, 0.3, 6.1, 40.0] {
                let expected = cell
                    .stars
                    .iter()
                    .map(|star| distance(star.position_pc, p))
                    .filter(|&d| d < cap)
                    .min_by(f64::total_cmp);
                let mut counted = 0;
                let found = cell.nearest(p, cap, &mut counted).map(|(d, _)| d);
                if !in_cluster {
                    checked += counted;
                }
                assert_eq!(found, expected, "{p:?} within {cap}");
                let mut near = 0;
                cell.near(p, cap, |index, star| {
                    assert_eq!(&cell.stars[index], star);
                    near += usize::from(distance(star.position_pc, p) <= cap);
                });
                let within = cell
                    .stars
                    .iter()
                    .filter(|star| distance(star.position_pc, p) <= cap)
                    .count();
                assert_eq!(near, within, "{p:?} within {cap}");
            }
        }
    }
    // Outside the cluster, a few dozen stars per query, not 20,000.
    assert!(checked < 300 * 4 * 100, "{checked}");
}

#[test]
fn resolved_stops_measuring_at_the_budget_even_inside_a_leaf() {
    let cell = scattered_cell();
    let mut examined = 0;
    let mut found = 0;
    cell.resolved(
        cell.cell.centre_pc(),
        (1e9, 1e-10, 0.0),
        3,
        &mut examined,
        &mut |_| found += 1,
    );
    assert_eq!(examined, 4);
    assert!(found <= 3);
}

#[test]
fn resolved_finds_every_multiple_that_could_show_apart() {
    let cell = scattered_cell();
    let (min, size) = (cell.cell.min_pc(), cell.cell.size_pc());
    let mut total = 0;
    for step in 0..60 {
        let t = step as f64 / 60.0;
        let p: [f64; 3] =
            std::array::from_fn(|axis| min[axis] - 3.0 + (size + 6.0) * t * [1.0, 0.6, 0.3][axis]);
        for (reach, angle, slack) in [(1e9, 3e-4, 0.0), (5.0, 1e-4, 0.5), (40.0, 1e-3, 2.0)] {
            let shows = |star: &StarPoint| {
                let d = distance(star.position_pc, p);
                star.separation_au > 0.0
                    && d <= reach
                    && f64::from(star.separation_au) * AU_PC >= angle * (d - slack)
            };
            let expected = cell.stars.iter().filter(|s| shows(s)).count();
            let (mut found, mut examined) = (0, 0);
            cell.resolved(
                p,
                (reach, angle, slack),
                usize::MAX,
                &mut examined,
                &mut |star| {
                    assert!(shows(star));
                    found += 1;
                },
            );
            assert_eq!(found, expected, "{p:?}");
            total += examined;
        }
    }
    // The widest separations prune most of the cell.
    assert!(total < 60 * 3 * 20_000 / 4, "{total}");
}

#[test]
fn blocks_cover_the_stars_in_order_within_their_cubes() {
    let cell = scattered_cell();
    for most in [50, 700, 5_000, 100_000] {
        let blocks = cell.blocks(most);
        assert_eq!(blocks.first().map(|b| b.start), Some(0));
        assert_eq!(blocks.last().map(|b| b.end), Some(cell.stars.len()));
        for pair in blocks.windows(2) {
            assert_eq!(pair[0].end, pair[1].start);
        }
        for block in &blocks {
            assert!(block.end > block.start);
            assert!(block.end - block.start <= most || block.size == 1);
            for star in &cell.stars[block.start..block.end] {
                for axis in 0..3 {
                    let offset = (star.position_pc[axis] - block.centre_pc[axis]).abs();
                    assert!(offset <= block.half_pc * (1.0 + 1e-12), "{most}");
                }
            }
            let mut seen = 0;
            cell.near_in(
                block,
                block.centre_pc,
                block.half_diagonal_pc(),
                |index, _| {
                    assert!((block.start..block.end).contains(&index));
                    seen += 1;
                },
            );
            assert_eq!(seen, block.end - block.start);
        }
    }
}
