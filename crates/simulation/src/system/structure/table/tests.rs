use super::{build::*, *};
use crate::seed::Rng;

fn quantiles(mut values: Vec<f64>) -> String {
    values.retain(|v| !v.is_nan());
    if values.is_empty() {
        return "no samples".to_owned();
    }
    values.sort_by(f64::total_cmp);
    let at = |q: f64| values[((values.len() - 1) as f64 * q) as usize];
    format!(
        "{:.2e} / {:.2e} / {:.2e} / {:.2e} (n={})",
        at(0.5),
        at(0.95),
        at(0.99),
        at(1.0),
        values.len()
    )
}

#[test]
fn embedded_table_decodes_round_trips_and_keeps_physical_order() {
    let table = Table::embedded();
    assert_eq!(Table::decode(&table.encode()).as_ref(), Some(table));
    assert!(Table::decode(&[]).is_none());
    let mut corrupt = table.encode();
    corrupt[100] ^= 1;
    assert!(Table::decode(&corrupt).is_none());

    let radius = |mass: f64, metal: f64, water: f64| {
        table
            .condensed(mass * EARTH_MASS_KG, metal, water, 300.0)
            .radius_m
    };
    for mass in [1e-3, 0.05, 1.0, 10.0, 20.0, 100.0] {
        assert!(radius(mass, 1.0, 0.0) < radius(mass, 0.0, 0.0));
        assert!(radius(mass, 0.0, 0.0) < radius(mass, 0.0, 1.0));
        let doubled = radius(2.0 * mass, 0.3, 0.1);
        assert!(doubled > radius(mass, 0.3, 0.1));
        assert!(doubled / radius(mass, 0.3, 0.1) < 2_f64.cbrt() * (1.0 + 1e-6));
    }
    // Earth's mix lands on Earth's radius to the cold model's accuracy.
    assert!((radius(1.0, 0.325, 0.0) / 6.371e6 - 1.0).abs() < 0.05);
    // Continuous into both labelled continuations.
    for (edge, label) in [(1e-4, true), (20.0, true)] {
        let inside = table.condensed(edge * EARTH_MASS_KG, 0.3, 0.0, 300.0);
        assert!(!inside.approximate);
        for factor in [1.0 - 1e-9, 1.0 + 1e-9] {
            let near = table.condensed(edge * factor * EARTH_MASS_KG, 0.3, 0.0, 300.0);
            assert!((near.radius_m / inside.radius_m - 1.0).abs() < 1e-8);
        }
        let outside = if edge < 1.0 { edge / 2.0 } else { edge * 2.0 };
        assert_eq!(
            table
                .condensed(outside * EARTH_MASS_KG, 0.3, 0.0, 300.0)
                .approximate,
            label
        );
    }
    // More gas, more heat: a thicker envelope. A lighter core: a larger one.
    let envelope = |fraction: f64, equilibrium: f64, water: f64| {
        table.envelope(5.0 * EARTH_MASS_KG, fraction, 0.3, water, equilibrium, 60.0)
    };
    assert!(envelope(0.01, 500.0, 0.0).thickness_m < envelope(0.1, 500.0, 0.0).thickness_m);
    assert!(envelope(0.05, 300.0, 0.0).thickness_m < envelope(0.05, 1000.0, 0.0).thickness_m);
    assert!(envelope(0.05, 500.0, 0.0).thickness_m > 0.0);
    assert!(envelope(0.05, 500.0, 0.0).core_radius_m < envelope(0.05, 500.0, 0.5).core_radius_m);
    // A core is smaller under its gas than under a trace, water most of all.
    let core = |water: f64, load: f64| table.core(5.0 * EARTH_MASS_KG, 0.3, water, load).radius_m;
    for water in [0.0, 0.5] {
        assert!(core(water, 1e11) < core(water, 1e9));
        assert!(core(water, 1e9) < core(water, 0.0));
    }
    assert!(core(0.5, 1e11) / core(0.5, 0.0) < core(0.0, 1e11) / core(0.0, 0.0));
    let body = envelope(0.3, 500.0, 0.2);
    assert!((0.05..0.4).contains(&body.core_inertia_factor));
    assert!((0.05..0.667).contains(&body.gas_inertia_factor));
    assert_eq!(
        envelope(0.05, 500.0, 0.0).thickness_m,
        envelope(0.05, 500.0, 0.5).thickness_m
    );
    // Layers nest and their pressures rise inwards, on and past the grids.
    let nested = |body: &Solid, load: f64| {
        let b = body.boundaries;
        assert!(0.0 <= b.core_radius_m && b.core_radius_m <= b.refractory_radius_m);
        assert!(b.refractory_radius_m <= body.radius_m * (1.0 + 1e-12));
        assert!(b.central_pressure_pa >= b.core_pressure_pa);
        assert!(b.core_pressure_pa >= b.refractory_pressure_pa);
        assert!(b.refractory_pressure_pa >= load * (1.0 - 1e-12));
    };
    for mass in [1e-6, 1e-3, 1.0, 20.0, 100.0] {
        for (metal, water) in [(0.3, 0.0), (0.3, 0.3), (0.0, 0.5), (1.0, 0.0), (0.3, 1.0)] {
            nested(
                &table.condensed(mass * EARTH_MASS_KG, metal, water, 300.0),
                0.0,
            );
            for load in [0.0, 1e9, 1e12] {
                nested(&table.core(mass * EARTH_MASS_KG, metal, water, load), load);
            }
        }
    }
    let earth = table.condensed(EARTH_MASS_KG, 0.325, 0.0, 300.0);
    assert_eq!(earth.boundaries.refractory_radius_m, earth.radius_m);
    // Earth's core is 3,480 km under 1.36 Mbar, its centre at 3.64 Mbar.
    assert!((earth.boundaries.core_radius_m / 3.48e6 - 1.0).abs() < 0.1);
    assert!((earth.boundaries.core_pressure_pa / 1.36e11 - 1.0).abs() < 0.2);
    assert!((earth.boundaries.central_pressure_pa / 3.64e11 - 1.0).abs() < 0.2);
    let rock = table.condensed(EARTH_MASS_KG, 0.0, 0.0, 300.0).boundaries;
    assert_eq!(rock.core_radius_m, 0.0);
    assert_eq!(rock.core_pressure_pa, rock.central_pressure_pa);
    // Below the grid a body keeps its density and its layers' shares, and
    // its pressures fall with its own weight. Ice stays larger than rock.
    let edge = table.condensed(1e-4 * EARTH_MASS_KG, 0.3, 0.3, 200.0);
    let small = table.condensed(1e-7 * EARTH_MASS_KG, 0.3, 0.3, 200.0);
    assert!(small.quality.outside && !edge.quality.outside);
    for ratio in [
        small.radius_m / edge.radius_m,
        small.boundaries.core_radius_m / edge.boundaries.core_radius_m,
        small.boundaries.refractory_radius_m / edge.boundaries.refractory_radius_m,
        (small.boundaries.central_pressure_pa / edge.boundaries.central_pressure_pa).sqrt(),
        (small.boundaries.refractory_pressure_pa / edge.boundaries.refractory_pressure_pa).sqrt(),
    ] {
        assert!((ratio / 0.1 - 1.0).abs() < 1e-9, "{ratio}");
    }
    assert!(radius(1e-7, 0.0, 1.0) > 1.4 * radius(1e-7, 0.0, 0.0));
    // A lookup names why the cells it read were filled.
    let (index, cell) = table
        .core
        .iter()
        .enumerate()
        .find(|(_, c)| c.reason != Reason::Solved)
        .unwrap();
    let [mass, metal, water, load] = table.core_grid.point(table.core_grid.cell(index));
    let looked = table.core(mass.exp() * EARTH_MASS_KG, metal, water, load.exp());
    assert!(looked.approximate && looked.quality.filled > 0);
    assert!(looked.quality.reasons().any(|r| r == cell.reason));
    assert!(!earth.approximate && earth.quality.reasons().next().is_none());
}

/// Boundary errors of a lookup against its solve: both radii over the body's
/// radius, then the three pressures. Absent layers and unloaded surfaces
/// have nothing to compare.
fn boundary_errors(looked: &Solid, solved: &Body, load_pa: f64) -> [f64; 5] {
    let mut b = looked.boundaries;
    b.central_pressure_pa -= load_pa;
    b.core_pressure_pa -= load_pa;
    b.refractory_pressure_pa -= load_pa;
    let relative = |a: f64, b: f64| {
        if b > 0.0 {
            (a / b - 1.0).abs()
        } else {
            f64::NAN
        }
    };
    [
        (b.core_radius_m - solved.core_radius_m).abs() / solved.radius_m,
        (b.refractory_radius_m - solved.refractory_radius_m).abs() / solved.radius_m,
        relative(b.central_pressure_pa, solved.central_pa),
        relative(b.core_pressure_pa, solved.core_pa),
        relative(b.refractory_pressure_pa, solved.refractory_pa),
    ]
}

fn report_boundaries(title: &str, errors: &[[f64; 5]]) {
    for (i, name) in [
        "core radius / R",
        "rock top radius / R",
        "central pressure",
        "core top pressure",
        "rock top pressure",
    ]
    .into_iter()
    .enumerate()
    {
        eprintln!(
            "{title} {name:<20} {}",
            quantiles(errors.iter().map(|e| e[i]).collect())
        );
    }
}

/// `cargo test --release -p space-simulation structure_table_accuracy -- --ignored --nocapture`.
/// Interpolation error against the solver at points between the knots.
#[test]
#[ignore = "accuracy report against the reference solvers"]
fn structure_table_accuracy() {
    let table = Table::embedded();
    let samples = 20_000;
    let draws: Vec<[f64; 4]> = {
        let mut rng = Rng::new("structure-table-accuracy");
        (0..2 * samples)
            .map(|_| std::array::from_fn(|_| rng.unit()))
            .collect()
    };
    let span = |range: [f64; 2], u: f64| range[0] + (range[1] - range[0]) * u;

    let rows = parallel(samples, |i| {
        let [a, b, c, d] = draws[i];
        let mass = span(CONDENSED_MASS_EARTH.map(f64::ln), a).exp();
        // Half the draws sit at small water shares, where most planets do.
        let water = if c < 0.2 {
            0.0
        } else if c < 0.6 {
            10_f64.powf(span([-4.0, -1.0], (c - 0.2) / 0.4))
        } else {
            span([0.1, 1.0], (c - 0.6) / 0.4)
        };
        let temperature = span([20.0, MAX_WATER_TEMPERATURE_K], d);
        let looked = table.condensed(mass * EARTH_MASS_KG, b, water, temperature);
        let solved = solve_condensed(mass, b, water, temperature).ok()?;
        Some((
            (looked.radius_m / solved.radius_m - 1.0).abs(),
            (looked.inertia_factor / solved.inertia_factor - 1.0).abs(),
            looked.approximate,
            format!("M={mass:.4} metal={b:.3} water={water:.5} T={temperature:.1}"),
            boundary_errors(&looked, &solved, 0.0),
        ))
    });
    let rows: Vec<_> = rows.into_iter().flatten().collect();
    eprintln!(
        "condensed radius, p50 / p95 / p99 / max: {}",
        quantiles(rows.iter().map(|r| r.0).collect())
    );
    eprintln!(
        "condensed radius, cells all solved:      {}",
        quantiles(rows.iter().filter(|r| !r.2).map(|r| r.0).collect())
    );
    eprintln!(
        "condensed inertia factor:                {}",
        quantiles(rows.iter().map(|r| r.1).collect())
    );
    let mut worst: Vec<_> = rows.iter().collect();
    worst.sort_by(|a, b| b.0.total_cmp(&a.0));
    for row in worst.iter().take(12) {
        eprintln!("  {:.3e} filled={} {}", row.0, row.2, row.3);
    }
    report_boundaries(
        "condensed, cells all solved:",
        &rows
            .iter()
            .filter(|r| !r.2)
            .map(|r| r.4)
            .collect::<Vec<_>>(),
    );
    let mut worst: Vec<_> = rows.iter().filter(|r| !r.2).collect();
    worst.sort_by(|a, b| b.4[1].total_cmp(&a.4[1]));
    for row in worst.iter().take(8) {
        eprintln!("  rock top {:.3e} {}", row.4[1], row.3);
    }
    for column in [2, 4] {
        let mut worst: Vec<_> = rows
            .iter()
            .filter(|r| !r.2 && !r.4[column].is_nan())
            .collect();
        worst.sort_by(|a, b| b.4[column].total_cmp(&a.4[column]));
        for row in worst.iter().take(8) {
            eprintln!("  pressure {column} {:.3e} {}", row.4[column], row.3);
        }
    }

    // Cores under a load, over the masses and loads envelope planets reach.
    let rows = parallel(samples, |i| {
        let [a, b, c, d] = draws[i];
        let mass = span([1e-2_f64.ln(), CORE_MASS_EARTH[1].ln()], a).exp();
        let water = if c < 0.4 { 0.0 } else { (c - 0.4) / 0.6 };
        let load = 10_f64.powf(span([3.0, 13.0], d));
        let looked = table.core(mass * EARTH_MASS_KG, b, water, load);
        let solved = solve_core(mass, b, water, load).ok()?;
        Some((
            (looked.radius_m / solved.radius_m - 1.0).abs(),
            (looked.inertia_factor / solved.inertia_factor - 1.0).abs(),
            looked.approximate,
            format!("M={mass:.4} metal={b:.3} water={water:.5} load={load:.2e}"),
            boundary_errors(&looked, &solved, load),
        ))
    });
    let rows: Vec<_> = rows.into_iter().flatten().filter(|r| !r.2).collect();
    eprintln!(
        "core radius, cells all solved:           {}",
        quantiles(rows.iter().map(|r| r.0).collect())
    );
    eprintln!(
        "core inertia factor, cells all solved:   {}",
        quantiles(rows.iter().map(|r| r.1).collect())
    );
    report_boundaries(
        "core, cells all solved:",
        &rows.iter().map(|r| r.4).collect::<Vec<_>>(),
    );
    for column in [2, 4] {
        let mut worst: Vec<_> = rows.iter().filter(|r| !r.4[column].is_nan()).collect();
        worst.sort_by(|a, b| b.4[column].total_cmp(&a.4[column]));
        for row in worst.iter().take(8) {
            eprintln!("  pressure {column} {:.3e} {}", row.4[column], row.3);
        }
    }

    let grid = Grid::envelope();
    let rows = parallel(samples, |i| {
        let [a, b, c, d] = draws[samples + i];
        let range = |axis: usize| {
            let knots = &grid.knots[axis];
            [knots[0], knots[knots.len() - 1]]
        };
        let [mass, fraction, equilibrium, intrinsic] = [
            span(range(0), a).exp(),
            span(range(1), b).exp(),
            span(range(2).map(f64::ln), c).exp(),
            span(range(3).map(f64::ln), d).exp(),
        ];
        let looked = table.envelope(
            mass * EARTH_MASS_KG,
            fraction,
            REFERENCE_CORE_METAL,
            0.0,
            equilibrium,
            intrinsic,
        );
        let solved = solve_envelope(mass, fraction, equilibrium, intrinsic).ok()?;
        let radius = looked.core_radius_m + looked.thickness_m;
        Some((
            (radius / solved.radius_m - 1.0).abs(),
            looked.approximate,
            format!("M={mass:.3} f={fraction:.2e} Teq={equilibrium:.0} Tint={intrinsic:.0}"),
            (inertia_factor(&looked, fraction) / solved.inertia_factor - 1.0).abs(),
        ))
    });
    let solved: Vec<_> = rows.into_iter().flatten().collect();
    eprintln!(
        "envelope radius, p50 / p95 / p99 / max:  {}",
        quantiles(solved.iter().map(|r| r.0).collect())
    );
    eprintln!(
        "envelope radius, cells all solved:       {}",
        quantiles(solved.iter().filter(|r| !r.1).map(|r| r.0).collect())
    );
    eprintln!(
        "envelope inertia, cells all solved:      {}",
        quantiles(solved.iter().filter(|r| !r.1).map(|r| r.3).collect())
    );
    let mut worst: Vec<_> = solved.iter().filter(|r| !r.1).collect();
    worst.sort_by(|a, b| b.0.total_cmp(&a.0));
    for row in worst.iter().take(20) {
        eprintln!("  {:.3e} {}", row.0, row.2);
    }

    // The body's own core in place of the reference one: any metal share,
    // water from none to all, on the masses and gas shares planets have.
    let rows = parallel(samples / 4, |i| {
        let [a, b, c, d] = draws[samples + i];
        let [metal, water] = [draws[i][0], draws[i][1].powi(2)];
        let range = |axis: usize| {
            let knots = &grid.knots[axis];
            [knots[0], knots[knots.len() - 1]]
        };
        let [mass, fraction, equilibrium, intrinsic] = [
            span([0.5_f64.ln(), range(0)[1]], a).exp(),
            span([1e-3_f64.ln(), range(1)[1]], b).exp(),
            span(range(2).map(f64::ln), c).exp(),
            span(range(3).map(f64::ln), d).exp(),
        ];
        let looked = table.envelope(
            mass * EARTH_MASS_KG,
            fraction,
            metal,
            water,
            equilibrium,
            intrinsic,
        );
        let solved =
            solve_envelope_over(mass, fraction, equilibrium, intrinsic, metal, water).ok()?;
        let radius = looked.core_radius_m + looked.thickness_m;
        Some((
            (radius / solved.radius_m - 1.0).abs(),
            looked.approximate,
            format!(
                "M={mass:.3} f={fraction:.2e} Teq={equilibrium:.0} Tint={intrinsic:.0} metal={metal:.2} water={water:.2}"
            ),
            (inertia_factor(&looked, fraction) / solved.inertia_factor - 1.0).abs(),
        ))
    });
    let own: Vec<_> = rows.into_iter().flatten().filter(|r| !r.1).collect();
    eprintln!(
        "own core radius, cells all solved:       {}",
        quantiles(own.iter().map(|r| r.0).collect())
    );
    eprintln!(
        "own core inertia, cells all solved:      {}",
        quantiles(own.iter().map(|r| r.3).collect())
    );
    let mut worst: Vec<_> = own.iter().collect();
    worst.sort_by(|a, b| b.0.total_cmp(&a.0));
    for row in worst.iter().take(20) {
        eprintln!("  {:.3e} inertia {:.3e} {}", row.0, row.3, row.2);
    }
}

/// Whole-body inertia factor at the one-bar radius, as generation composes it.
fn inertia_factor(looked: &Envelope, fraction: f64) -> f64 {
    let radius = looked.core_radius_m + looked.thickness_m;
    (looked.core_inertia_factor * (1.0 - fraction) * looked.core_radius_m.powi(2)
        + looked.gas_inertia_factor * fraction * radius.powi(2))
        / radius.powi(2)
}

/// `cargo test --release -p space-simulation giants_and_cold_envelopes_solve -- --ignored`.
#[test]
#[ignore = "local pinned source resource; run in release"]
fn giants_and_cold_envelopes_solve() {
    let earth_radii = |mass, fraction, equilibrium, intrinsic| {
        solve_envelope(mass, fraction, equilibrium, intrinsic).map(|s| s.radius_m / 6.371e6)
    };
    // Jupiter and Saturn analogues (real: 11.2 and 9.4 at one bar), a
    // Neptune-cold ice giant, and three Jupiter masses.
    for (mass, fraction, equilibrium, intrinsic, range) in [
        (318.0, 0.97, 110.0, 100.0, 10.5..12.0),
        (95.0, 0.9, 80.0, 77.0, 9.0..10.5),
        (17.0, 0.1, 47.0, 53.0, 3.0..4.0),
        (950.0, 0.97, 300.0, 130.0, 10.5..13.0),
    ] {
        let radius = earth_radii(mass, fraction, equilibrium, intrinsic).unwrap();
        assert!(range.contains(&radius), "{mass} M: {radius}");
    }
    // Gas that stays under 40 K at depth is still refused, not extrapolated.
    assert!(earth_radii(17.0, 0.1, 25.0, 25.0).is_err());
    // The table's core under a load is the envelope solver's own core under
    // the same weight of gas, wet or dry.
    for (mass, fraction, water) in [(5.0, 1e-6, 0.2), (10.0, 0.05, 0.0), (30.0, 0.3, 0.5)] {
        let whole = solve_envelope_over(mass, fraction, 300.0, 60.0, 0.3, water).unwrap();
        let core = solve_core(mass * (1.0 - fraction), 0.3, water, whole.base_pa)
            .unwrap()
            .radius_m;
        assert!(
            (core / whole.core_radius_m - 1.0).abs() < 1e-5,
            "{mass} {fraction} {water}: {core} against {}",
            whole.core_radius_m
        );
    }
}
