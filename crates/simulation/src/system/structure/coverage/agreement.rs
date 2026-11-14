//! Agreement census: every bulk fact generation publishes synchronously must
//! survive the layered solve. See docs/plans/planet-generation.md (G2).
use super::{env_usize, quantiles, scientific_quantiles};
use crate::{
    galaxy::Component,
    star::multiple::Hosted,
    system::{galactic_bodies, generated::PlanetJob, physical, structure::prototype::Outcome},
};
use space_shared::LayeredPlanet;
use std::{fmt::Write as _, time::Instant};

/// Systems per component in the default check run; NS_AGREEMENT_SYSTEMS overrides.
const DEFAULT_SYSTEMS: usize = 2;

struct Row {
    id: String,
    class: &'static str,
    decision: String,
    mass_earth: f64,
    light_fraction: f64,
    water_fraction: f64,
    equilibrium_k: f64,
    sync: LayeredPlanet,
    layered: Option<LayeredPlanet>,
    refusal: String,
}

/// Water phase and frost detail follow from the decided temperature; the
/// synchronous record leaves them unresolved, so compare what it does decide.
fn surface_kind(surface: &str) -> &'static str {
    match surface {
        "envelope" => "envelope",
        "rock" => "rock",
        "frost" | "unresolved volatiles" => "frost",
        _ => "water",
    }
}

fn escaped_kg(record: &LayeredPlanet) -> f64 {
    record.physical.materials.iter().map(|m| m.escaped_kg).sum()
}

fn row(p: &Hosted) -> Row {
    let job = PlanetJob {
        id: p.id(),
        designation: String::new(),
        budget: p.body.budget,
        environment: p.environment.clone().unwrap(),
        initial_result: p.body.initial_result.clone(),
        decision: p.body.decision,
    };
    let (layered, refusal) = match job.resolve() {
        Outcome::Layered(solved) => (Some(physical::layered(&solved)), String::new()),
        Outcome::Fallback(f) => (None, format!("{:?}", f.reason.unwrap())),
    };
    let retained = p.body.budget.retained();
    let mass = retained.total_mass_kg();
    use crate::system::composition::Material as M;
    Row {
        id: p.id(),
        class: p.body.class.name(),
        decision: format!("{:?}", p.body.decision)
            .split([' ', '{'])
            .next()
            .unwrap()
            .to_owned(),
        mass_earth: mass / crate::units::EARTH_MASS_KG,
        light_fraction: (retained.mass_kg(M::Hydrogen) + retained.mass_kg(M::Helium)) / mass,
        water_fraction: retained.mass_kg(M::Water) / mass,
        equilibrium_k: job.environment.equilibrium_temperature_k,
        sync: p.body.initial_result.record.clone(),
        layered,
        refusal,
    }
}

struct Summary {
    text: String,
    surface_flips: usize,
    escape_changes: usize,
    mislabelled: usize,
    /// Worst |difference| where generation decided a thin surface.
    decided_temperature_k: f64,
    /// Worst relative radius and ln pressure ratio on those surfaces.
    thin_radius: f64,
    thin_pressure: f64,
}

fn summarise(rows: &[Row], sync_seconds: f64, layered_seconds: f64) -> Summary {
    let pairs: Vec<(&Row, &LayeredPlanet)> = rows
        .iter()
        .filter_map(|r| Some((r, r.layered.as_ref()?)))
        .collect();
    let thin = |l: &LayeredPlanet| l.surface != "envelope";
    let relative = |a: f64, b: f64| (a / b - 1.0).abs();
    let mut text = format!(
        "# Generation agreement\n\n- Planets: {}, layered: {} ({:.0}%)\n- Synchronous generation: {:.3} ms per planet\n- Layered solve: {:.1} ms per planet\n",
        rows.len(),
        pairs.len(),
        100.0 * pairs.len() as f64 / rows.len().max(1) as f64,
        1e3 * sync_seconds / rows.len().max(1) as f64,
        1e3 * layered_seconds / rows.len().max(1) as f64,
    );
    let flips: Vec<_> = pairs
        .iter()
        .filter(|(r, l)| surface_kind(&r.sync.surface) != surface_kind(&l.surface))
        .collect();
    let escapes: Vec<_> = pairs
        .iter()
        .filter(|(r, l)| {
            (escaped_kg(l) - escaped_kg(&r.sync)).abs() > 1e-9 * l.physical.retained_mass_kg
        })
        .collect();
    let mislabelled = rows
        .iter()
        .filter(|r| {
            let record = r.layered.as_ref().unwrap_or(&r.sync);
            let facts = &record.physical;
            // Same display rule as generation: H/He alone holds one bar.
            record.surface == "envelope"
                && facts.h_he_mass_fraction
                    * facts.retained_mass_kg.powi(2)
                    * crate::units::GRAVITATIONAL_CONSTANT
                    >= 4.0 * std::f64::consts::PI * 1e5 * facts.condensed_radius_m.powi(4)
                && matches!(
                    r.class,
                    "Rocky planet" | "Metal-rich planet" | "Water-rich planet"
                )
        })
        .count();
    let _ = writeln!(
        text,
        "- Surface kind changed by the layered solve: {}\n- Escaped inventory changed by the layered solve: {}\n- Condensed-class labels on an H/He envelope surface: {mislabelled}\n\n| Quantity | Route | p50 / p95 / max |\n| --- | --- | --- |",
        flips.len(),
        escapes.len(),
    );
    let decided = |value: &dyn Fn(&Row, &LayeredPlanet) -> f64| {
        pairs
            .iter()
            .filter(|(r, _)| r.decision == "Thin")
            .map(|(r, l)| value(r, l))
            .filter(|v| v.is_finite())
            .fold(0.0_f64, f64::max)
    };
    let decided_temperature_k =
        decided(&|r, l| (r.sync.surface_temperature_k - l.surface_temperature_k).abs());
    let thin_radius = decided(&|r, l| {
        relative(
            r.sync.physical.reference_radius_m,
            l.physical.reference_radius_m,
        )
    });
    let thin_pressure = decided(&|r, l| {
        if l.surface_pressure_pa.max(r.sync.surface_pressure_pa) < 0.1 {
            f64::NAN
        } else {
            (r.sync.surface_pressure_pa / l.surface_pressure_pa)
                .ln()
                .abs()
        }
    });
    for (route, envelope) in [("thin", false), ("envelope", true)] {
        let of = |value: &dyn Fn(&Row, &LayeredPlanet) -> f64| -> Vec<f64> {
            pairs
                .iter()
                .filter(|(_, l)| thin(l) != envelope)
                .map(|(r, l)| value(r, l))
                .filter(|v| v.is_finite())
                .collect()
        };
        let radius = of(&|r, l| {
            relative(
                r.sync.physical.reference_radius_m,
                l.physical.reference_radius_m,
            )
        });
        let temperature =
            of(&|r, l| (r.sync.surface_temperature_k - l.surface_temperature_k).abs());
        let pressure = of(&|r, l| {
            if l.surface_pressure_pa.max(r.sync.surface_pressure_pa) < 0.1 {
                f64::NAN
            } else {
                (r.sync.surface_pressure_pa / l.surface_pressure_pa)
                    .ln()
                    .abs()
            }
        });
        let _ = writeln!(
            text,
            "| Radius, relative | {route} | {} |\n| Surface temperature, K | {route} | {} |\n| Surface pressure, ln ratio | {route} | {} |",
            scientific_quantiles(radius),
            quantiles(temperature),
            quantiles(pressure),
        );
    }
    let _ = writeln!(text, "\n## Refusals\n");
    let mut reasons = std::collections::BTreeMap::<&str, usize>::new();
    for r in rows.iter().filter(|r| r.layered.is_none()) {
        *reasons.entry(&r.refusal).or_default() += 1;
    }
    let mut reasons: Vec<_> = reasons.into_iter().collect();
    reasons.sort_by_key(|r| std::cmp::Reverse(r.1));
    for (reason, count) in reasons.iter().take(12) {
        let _ = writeln!(text, "- {count}: {}", &reason[..reason.len().min(100)]);
    }
    let _ = writeln!(text, "\n## Disagreements (first 20)\n");
    for (r, l) in flips.iter().chain(&escapes).take(20) {
        let _ = writeln!(
            text,
            "- {}: surface {} -> {}, escaped {:.3e} -> {:.3e} kg",
            r.id,
            r.sync.surface,
            l.surface,
            escaped_kg(&r.sync),
            escaped_kg(l)
        );
    }
    Summary {
        text,
        surface_flips: flips.len(),
        escape_changes: escapes.len(),
        mislabelled,
        decided_temperature_k,
        thin_radius,
        thin_pressure,
    }
}

fn table(rows: &[Row]) -> String {
    let mut out = "id\tclass\tdecision\tmass_earth\thhe_fraction\twater_fraction\tequilibrium_k\tsync_surface\tsync_radius_km\tsync_k\tsync_pa\tsync_escaped_kg\tsync_blowoff_kg\tlayered_surface\tlayered_radius_km\tlayered_k\tlayered_pa\tlayered_escaped_kg\trefusal\n".to_owned();
    for r in rows {
        let facts = |l: &LayeredPlanet| {
            format!(
                "{}\t{:.9e}\t{:.6e}\t{:.6e}\t{:.9e}",
                l.surface,
                l.physical.reference_radius_m / 1e3,
                l.surface_temperature_k,
                l.surface_pressure_pa,
                escaped_kg(l)
            )
        };
        let _ = writeln!(
            out,
            "{}\t{}\t{}\t{:.6e}\t{:.6e}\t{:.6e}\t{:.4e}\t{}\t{:.6e}\t{}\t{}",
            r.id,
            r.class,
            r.decision,
            r.mass_earth,
            r.light_fraction,
            r.water_fraction,
            r.equilibrium_k,
            facts(&r.sync),
            r.sync.blowoff_kg,
            r.layered.as_ref().map_or("\t\t\t\t".to_owned(), facts),
            r.refusal
        );
    }
    out
}

/// Runs in the default check at a small size. For the full probe:
/// `NS_AGREEMENT_SYSTEMS=57 cargo test --release -p space-simulation generation_agreement -- --nocapture`.
/// Writes data/tmp/generation-agreement.{md,tsv}.
#[test]
fn generation_agreement() {
    let systems = env_usize("NS_AGREEMENT_SYSTEMS", DEFAULT_SYSTEMS);
    let keys: Vec<(Component, usize)> = Component::ALL
        .into_iter()
        .flat_map(|c| (0..systems).map(move |i| (c, i)))
        .collect();
    let start = Instant::now();
    let bodies: Vec<Hosted> = keys
        .iter()
        .flat_map(|(component, index)| {
            galactic_bodies(
                &format!("composition-calibration:{component:?}:{index}"),
                *component,
            )
            .planets
        })
        .collect();
    let sync_seconds = start.elapsed().as_secs_f64();
    let threads = std::thread::available_parallelism().map_or(1, |n| n.get());
    let next = std::sync::atomic::AtomicUsize::new(0);
    let (mut rows, layered_seconds) = std::thread::scope(|scope| {
        let workers: Vec<_> = (0..threads)
            .map(|_| {
                scope.spawn(|| {
                    let mut rows = Vec::new();
                    let mut seconds = 0.0;
                    while let Some(p) =
                        bodies.get(next.fetch_add(1, std::sync::atomic::Ordering::Relaxed))
                    {
                        let start = Instant::now();
                        rows.push(row(p));
                        seconds += start.elapsed().as_secs_f64();
                    }
                    (rows, seconds)
                })
            })
            .collect();
        workers.into_iter().map(|w| w.join().unwrap()).fold(
            (Vec::new(), 0.0),
            |(mut rows, seconds), (r, s)| {
                rows.extend(r);
                (rows, seconds + s)
            },
        )
    });
    rows.sort_by(|a, b| a.id.cmp(&b.id));
    let summary = summarise(&rows, sync_seconds, layered_seconds);
    eprintln!("{}", summary.text);
    if systems != DEFAULT_SYSTEMS {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/tmp");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("generation-agreement.md"), &summary.text).unwrap();
        std::fs::write(dir.join("generation-agreement.tsv"), table(&rows)).unwrap();
    }
    assert!(!rows.is_empty());
    // What generation decides, the layered solve keeps.
    // One allowance until G3: whether a trace of frost saturates depends on
    // the radius, which the two stages still take from different models.
    assert!(
        summary.surface_flips * 2000 <= rows.len(),
        "surface kind changed: {}",
        summary.surface_flips
    );
    assert_eq!(summary.escape_changes, 0, "escape changed");
    assert_eq!(summary.mislabelled, 0, "bulk label on an H/He envelope");
    assert_eq!(summary.decided_temperature_k, 0.0, "decided temperature");
    // Radius is still the fast fit until G3's table: regression bounds only.
    assert!(summary.thin_radius < 0.2, "{}", summary.thin_radius);
    assert!(summary.thin_pressure < 1.0, "{}", summary.thin_pressure);
}
