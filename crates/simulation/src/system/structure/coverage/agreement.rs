//! Agreement census: every bulk fact generation publishes synchronously must
//! survive the layered solve. See docs/plans/planet-generation.md (G2).
use super::{env_usize, quantiles, scientific_quantiles};
use crate::{
    galaxy::Component,
    star::multiple::Hosted,
    system::{galactic_bodies, generated::PlanetJob, physical, structure::prototype::Outcome},
};
use space_shared::{AtmosphereData, LayeredPlanet};
use std::{fmt::Write as _, time::Instant};

/// Systems per component in the default check run; NS_AGREEMENT_SYSTEMS overrides.
const DEFAULT_SYSTEMS: usize = 2;

/// Diagnostic only: this samples the route guard, it does not enforce or fix
/// continuity. No retention redraw obscures the change in offered gas share.
#[test]
#[ignore = "optional route-threshold diagnostic"]
fn generation_one_percent_threshold_probe() {
    use crate::system::{
        composition::{Budget, Inventory, Material},
        structure::fallback,
    };
    let mut out = String::from(
        "mass_earth\twater_share\tequilibrium_k\tlower_route\tupper_route\tlower_radius_m\tupper_radius_m\tradius_relative_jump\tlower_pressure_bar\tupper_pressure_bar\tlower_blowoff_kg\tupper_blowoff_kg\n",
    );
    let mut jumps = Vec::new();
    let mut flips = 0;
    for mass in [0.0001, 0.001, 0.01, 0.1, 1.0, 5.0, 20.0] {
        for water in [0.0, 0.3, 0.8] {
            for temperature in [20.0, 50.0, 100.0, 250.0, 500.0, 1000.0] {
                let decide = |fraction: f64| {
                    let total = mass * crate::units::EARTH_MASS_KG;
                    let heavy = total * (1.0 - fraction);
                    let interior = Inventory::new(&[
                        (Material::Metal, heavy * (1.0 - water) * 0.325),
                        (Material::Silicate, heavy * (1.0 - water) * 0.675),
                    ])
                    .unwrap();
                    let sea = Inventory::new(&[(Material::Water, heavy * water)]).unwrap();
                    let gas = Inventory::new(&[
                        (Material::Hydrogen, total * fraction * 0.75),
                        (Material::Helium, total * fraction * 0.25),
                    ])
                    .unwrap();
                    let budget = Budget::new(
                        interior
                            .checked_add(&sea)
                            .unwrap()
                            .checked_add(&gas)
                            .unwrap(),
                        interior,
                        sea,
                        gas,
                    )
                    .unwrap();
                    let env = space_shared::PlanetEnvironment {
                        irradiation_w_m2: 0.0,
                        assumed_bond_albedo: 0.3,
                        equilibrium_temperature_k: temperature,
                        age_gyr: 4.57,
                        atmosphere: None,
                    };
                    fallback::decide(&fallback::initial(&budget, &env), &env).unwrap()
                };
                let a = decide(0.01 - 1e-8);
                let b = decide(0.01 + 1e-8);
                let route = |d| match d {
                    crate::system::structure::prototype::Decision::Thin { .. } => "thin",
                    crate::system::structure::prototype::Decision::Envelope => "envelope",
                    _ => "refused",
                };
                flips += usize::from(route(a.decision) != route(b.decision));
                let jump = (b.fallback.radius_m / a.fallback.radius_m - 1.0).abs();
                jumps.push(jump);
                let _ = writeln!(
                    out,
                    "{mass}\t{water}\t{temperature}\t{}\t{}\t{}\t{}\t{jump}\t{}\t{}\t{}\t{}",
                    route(a.decision),
                    route(b.decision),
                    a.fallback.radius_m,
                    b.fallback.radius_m,
                    a.fallback.surface_pressure_pa / 1e5,
                    b.fallback.surface_pressure_pa / 1e5,
                    a.fallback.blowoff.total_mass_kg(),
                    b.fallback.blowoff.total_mass_kg()
                );
            }
        }
    }
    eprintln!(
        "1% threshold: {} pairs, {flips} route changes; radius relative jump p50/p95/max {}",
        jumps.len(),
        scientific_quantiles(jumps)
    );
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/tmp");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("g3-atmosphere-threshold.tsv"), out).unwrap();
}

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
    /// The record a visit installs: the layered profile on generation's radius.
    adopted: Option<Result<LayeredPlanet, &'static str>>,
    atmosphere: Option<AtmosphereData>,
    adopted_atmosphere: Option<AtmosphereData>,
    refusal: String,
}

/// The broad kind of surface. The exact text, water phase included, is
/// compared with the layers.
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
    let outcome = job.resolve();
    if std::env::var_os("NS_AGREEMENT_HEADLESS_COMPARISON").is_some() {
        let previous = crate::system::structure::prototype::resolve(
            &job.budget,
            &job.environment,
            job.decision,
            Default::default(),
        );
        if let Outcome::Layered(solved) = previous {
            let record = physical::layered(&solved);
            if record
                .layers
                .iter()
                .any(|l| l.phase_allocations.iter().any(|p| p.condensate_kg > 0.0))
            {
                eprintln!(
                    "Headless suspended column {}: generated route {}",
                    job.id,
                    match &outcome {
                        Outcome::Layered(_) => "solved".to_owned(),
                        Outcome::Fallback(f) => format!("{:?}", f.reason),
                    }
                );
            }
        }
    }
    let refinement = matches!(outcome, Outcome::Layered(_))
        .then(|| physical::prepare_refinement(&job.initial_result, &job.environment, &outcome));
    if let Some(Ok(refined)) = &refinement {
        use crate::system::atmosphere::{GasAtmosphere, optics::boundary::OpticalColumn};
        let planet = physical::export_planet(p.clone(), 0, String::new(), None);
        refined
            .project(&planet)
            .unwrap_or_else(|e| panic!("{}: {e}", job.id));
        if let Some(air) = &job.environment.atmosphere {
            let gas = GasAtmosphere::try_from(air).unwrap();
            let before = OpticalColumn::from_result(&job.initial_result, &gas).unwrap();
            let after = OpticalColumn::from_result(&refined.result, &gas)
                .unwrap_or_else(|e| panic!("{}: {e}", job.id));
            assert_eq!(before, after, "{} optical column", job.id);
        }
    }
    let adopted_atmosphere = refinement
        .as_ref()
        .and_then(|r| r.as_ref().ok())
        .and_then(|r| r.environment.atmosphere.clone());
    let adopted = refinement.map(|r| r.map(|r| r.result.record));
    let (layered, refusal) = match outcome {
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
        adopted,
        atmosphere: job.environment.atmosphere.clone(),
        adopted_atmosphere,
        refusal,
    }
}

fn mole_fractions(record: &LayeredPlanet) -> [f64; 8] {
    use crate::system::{atmosphere::Gas, materials::Material};
    let weights = Gas::ALL.map(|g| {
        record
            .physical
            .materials
            .iter()
            .find(|m| m.material == Material::from_gas(g).id())
            .map_or(0.0, |m| m.gas_kg)
            / g.molecular_mass_u()
    });
    let total: f64 = weights.iter().sum();
    weights.map(|w| if total > 0.0 { w / total } else { 0.0 })
}

fn gas_difference(a: &LayeredPlanet, b: &LayeredPlanet) -> f64 {
    mole_fractions(a)
        .into_iter()
        .zip(mole_fractions(b))
        .map(|(a, b)| (a - b).abs())
        .fold(0.0, f64::max)
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
    /// Relative radius on envelope surfaces: median, 95th percentile, worst.
    envelope_radius: [f64; 3],
    /// Layered results a visit could not install, or installed on another radius.
    adoption_failures: usize,
    radius_changes: usize,
    layers: Layers,
}

fn summarise(rows: &[Row], sync_seconds: f64, layered_seconds: f64) -> Summary {
    let pairs: Vec<(&Row, &LayeredPlanet)> = rows
        .iter()
        .filter_map(|r| Some((r, r.layered.as_ref()?)))
        .collect();
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
    let mut envelope_radius = [0.0; 3];
    for (route, envelope) in [("thin", false), ("envelope", true)] {
        let count = pairs
            .iter()
            .filter(|(r, _)| (r.decision == "Envelope") == envelope)
            .count();
        let _ = writeln!(text, "| Layered count | {route} | {count} |");
        let of = |value: &dyn Fn(&Row, &LayeredPlanet) -> f64| -> Vec<f64> {
            pairs
                .iter()
                .filter(|(r, _)| (r.decision == "Envelope") == envelope)
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
        if envelope && !radius.is_empty() {
            let mut sorted = radius.clone();
            sorted.sort_by(f64::total_cmp);
            let at = |q: f64| sorted[((sorted.len() - 1) as f64 * q) as usize];
            envelope_radius = [at(0.5), at(0.95), at(1.0)];
        }
        let inertia =
            of(&|r, l| relative(r.sync.physical.inertia_factor, l.physical.inertia_factor));
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
            "| Radius, relative | {route} | {} |\n| Inertia factor, relative | {route} | {} |\n| Surface temperature, K | {route} | {} |\n| Surface pressure, ln ratio | {route} | {} |",
            scientific_quantiles(radius),
            scientific_quantiles(inertia),
            quantiles(temperature),
            quantiles(pressure),
        );
        for (name, metric) in [
            (
                "Gas mole fraction, largest absolute difference",
                gas_difference as fn(&LayeredPlanet, &LayeredPlanet) -> f64,
            ),
            (
                "Condensed radius, relative",
                |a: &LayeredPlanet, b: &LayeredPlanet| {
                    (a.physical.condensed_radius_m / b.physical.condensed_radius_m - 1.0).abs()
                },
            ),
            (
                "Reference gravity, relative",
                |a: &LayeredPlanet, b: &LayeredPlanet| {
                    (a.physical.reference_gravity_m_s2 / b.physical.reference_gravity_m_s2 - 1.0)
                        .abs()
                },
            ),
        ] {
            let _ = writeln!(
                text,
                "| {name}, raw solve | {route} | {} |",
                scientific_quantiles(of(&|r, l| metric(&r.sync, l)))
            );
            let values = pairs
                .iter()
                .filter(|(r, _)| (r.decision == "Envelope") == envelope)
                .filter_map(|(r, _)| Some(metric(&r.sync, r.adopted.as_ref()?.as_ref().ok()?)))
                .collect();
            let _ = writeln!(
                text,
                "| {name}, installed | {route} | {} |",
                scientific_quantiles(values)
            );
        }
        for (name, metric) in [
            (
                "Atmosphere scale height, relative",
                (|a: &AtmosphereData| a.scale_height_m) as fn(&AtmosphereData) -> f64,
            ),
            ("Atmosphere extent, relative", |a: &AtmosphereData| {
                a.extent_m
            }),
        ] {
            let values: Vec<_> = pairs
                .iter()
                .filter(|(r, _)| (r.decision == "Envelope") == envelope)
                .filter_map(|(r, _)| {
                    Some(relative(
                        metric(r.atmosphere.as_ref()?),
                        metric(r.adopted_atmosphere.as_ref()?),
                    ))
                })
                .collect();
            let _ = writeln!(
                text,
                "| {name}, installed ({} pairs) | {route} | {} |",
                values.len(),
                scientific_quantiles(values)
            );
        }
    }
    let adoption_failures = rows
        .iter()
        .filter(|r| matches!(r.adopted, Some(Err(_))))
        .count();
    let radius_changes = rows
        .iter()
        .filter(|r| {
            matches!(&r.adopted, Some(Ok(adopted))
                if adopted.physical.reference_radius_m != r.sync.physical.reference_radius_m)
        })
        .count();
    let _ = writeln!(
        text,
        "\n- Layered results a visit cannot install: {adoption_failures}\n- Installed on a radius other than generation's: {radius_changes}"
    );
    let mut reasons = std::collections::BTreeMap::<&str, usize>::new();
    for r in rows {
        if let Some(Err(reason)) = &r.adopted {
            *reasons.entry(reason).or_default() += 1;
        }
    }
    for (reason, count) in reasons {
        let _ = writeln!(text, "  - {count}: {reason}");
    }
    let layers = layers(rows, &mut text);
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
        envelope_radius,
        adoption_failures,
        radius_changes,
        layers,
    }
}

const WATER_KINDS: [&str; 6] = [
    "ocean",
    "ice",
    "deep_ice",
    "hot_water",
    "water_coexistence",
    "water_compression",
];

/// One entry a kind, outermost first. The solver changes model inside a
/// phase and inside its gas; generation states one layer for each. Gas above
/// one bar is the solver's top envelope region and generation's atmosphere.
fn merged(record: &LayeredPlanet) -> Vec<space_shared::Layer> {
    let envelope = record.surface == "envelope";
    let mut out: Vec<space_shared::Layer> = Vec::new();
    for layer in &record.layers {
        let mut layer = layer.clone();
        if envelope && layer.kind == "atmosphere" {
            layer.kind = "envelope".into();
        }
        match out.last_mut() {
            Some(above)
                if above.kind == layer.kind
                    && !(layer.kind == "envelope" && above.inner_pressure_pa == 1e5) =>
            {
                above.inner_radius_m = layer.inner_radius_m;
                above.inner_pressure_pa = layer.inner_pressure_pa;
                above.mass_kg += layer.mass_kg;
            }
            _ => out.push(layer),
        }
    }
    out
}

struct Layers {
    surface_changes: usize,
    /// Thin-route planets whose layer kinds differ from the solver's.
    thin_sequence_changes: usize,
    /// Worst boundary radius difference on the thin route, over the radius.
    thin_boundary: f64,
}

/// Generation's layer list against the layered solve's, per route.
fn layers(rows: &[Row], text: &mut String) -> Layers {
    let pairs: Vec<(&Row, &LayeredPlanet)> = rows
        .iter()
        .filter_map(|r| Some((r, r.layered.as_ref()?)))
        .collect();
    let surface_changes = pairs
        .iter()
        .filter(|(r, l)| r.sync.surface != l.surface)
        .count();
    let _ = writeln!(
        text,
        "\n## Layers\n\nGeneration's layers against the layered solve's, where it solves. Adjacent solver regions of one kind count as one layer.\n\n- Surface text changed by the layered solve: {surface_changes}"
    );
    let relative = |a: f64, b: f64| (a / b - 1.0).abs();
    let mut thin_sequence_changes = 0;
    let mut thin_boundary = 0.0_f64;
    let mut table = String::new();
    for (route, envelope) in [("thin", false), ("envelope", true)] {
        let mut stats = std::collections::BTreeMap::<(u8, String), Vec<f64>>::new();
        let mut note = |order: u8, name: &str, value: f64| {
            if value.is_finite() {
                stats
                    .entry((order, name.to_owned()))
                    .or_default()
                    .push(value);
            }
        };
        let mut differing = std::collections::BTreeMap::<String, usize>::new();
        let mut count = 0;
        for (r, l) in pairs
            .iter()
            .filter(|(r, _)| (r.decision == "Envelope") == envelope)
        {
            count += 1;
            let (ours, theirs) = (merged(&r.sync), merged(l));
            let kinds = |layers: &[space_shared::Layer]| {
                layers
                    .iter()
                    .map(|l| l.kind.as_str())
                    .collect::<Vec<_>>()
                    .join(" ")
            };
            let same = kinds(&ours) == kinds(&theirs);
            if !same {
                *differing
                    .entry(format!("{} | {}", kinds(&ours), kinds(&theirs)))
                    .or_default() += 1;
            }
            let condensed = theirs
                .iter()
                .filter(|l| !["atmosphere", "envelope"].contains(&l.kind.as_str()))
                .map(|l| l.outer_radius_m)
                .fold(0.0, f64::max);
            let find = |layers: &[space_shared::Layer], kind: &str| {
                layers.iter().find(|l| l.kind == kind).cloned()
            };
            for (order, kind, name) in [(0, "core", "metal"), (1, "mantle", "rock")] {
                let (Some(a), Some(b)) = (find(&ours, kind), find(&theirs, kind)) else {
                    continue;
                };
                let radius = (a.outer_radius_m - b.outer_radius_m).abs() / condensed;
                note(order, &format!("Top of the {name}, radius over R"), radius);
                if !envelope {
                    thin_boundary = thin_boundary.max(radius);
                }
                if let (Some(p), Some(q)) = (a.outer_pressure_pa, b.outer_pressure_pa)
                    && q > 1e3
                {
                    note(
                        order + 3,
                        &format!("Top of the {name}, pressure"),
                        relative(p, q),
                    );
                }
            }
            if let (Some(a), Some(b)) = (ours.last(), theirs.last()) {
                note(
                    2,
                    "Central pressure",
                    relative(a.inner_pressure_pa, b.inner_pressure_pa),
                );
            }
            let water = |layers: &[space_shared::Layer]| {
                let mut water = layers
                    .iter()
                    .filter(|l| WATER_KINDS.contains(&l.kind.as_str()));
                let top = water.next()?;
                let base = water.next_back().unwrap_or(top);
                Some(top.outer_radius_m - base.inner_radius_m)
            };
            if let (Some(a), Some(b)) = (water(&ours), water(&theirs))
                && b > 0.0
            {
                note(5, "Water, whole thickness", relative(a, b));
            }
            if same {
                for (a, b) in ours.iter().zip(&theirs) {
                    let thickness = b.outer_radius_m - b.inner_radius_m;
                    if WATER_KINDS.contains(&a.kind.as_str()) && thickness > 0.0 {
                        note(
                            6,
                            &format!("Layer `{}`, thickness", a.kind),
                            relative(a.outer_radius_m - a.inner_radius_m, thickness),
                        );
                    }
                }
            }
            if envelope {
                let gas = |layers: &[space_shared::Layer]| {
                    layers.iter().rfind(|l| l.kind == "envelope").cloned()
                };
                if let (Some(a), Some(b)) = (gas(&ours), gas(&theirs)) {
                    note(
                        7,
                        "Gas, pressure at its base",
                        relative(a.inner_pressure_pa, b.inner_pressure_pa),
                    );
                }
                let condensate = |record: &LayeredPlanet| -> f64 {
                    record
                        .layers
                        .iter()
                        .flat_map(|l| &l.phase_allocations)
                        .map(|p| p.condensate_kg)
                        .sum()
                };
                let (a, b) = (condensate(&r.sync), condensate(l));
                if a > 0.0 || b > 0.0 {
                    note(
                        8,
                        "Suspended condensate, mass",
                        if a > 0.0 && b > 0.0 {
                            relative(a, b)
                        } else {
                            1.0
                        },
                    );
                }
            }
        }
        let changed: usize = differing.values().sum();
        if !envelope {
            thin_sequence_changes = changed;
        }
        let _ = writeln!(text, "- Layer kinds differ, {route}: {changed} of {count}");
        for (kinds, n) in &differing {
            let _ = writeln!(text, "  - {n}: generation `{kinds}` solver");
        }
        for ((_, name), values) in stats {
            let _ = writeln!(
                table,
                "| {name} | {route} | {} | {} |",
                values.len(),
                scientific_quantiles(values)
            );
        }
    }
    let _ = writeln!(
        text,
        "\n| Quantity, relative | Route | Planets | p50 / p95 / max |\n| --- | --- | --- | --- |\n{table}"
    );
    // What every planet carries in place of a per-planet refusal.
    let mut surfaces = std::collections::BTreeMap::<&str, usize>::new();
    let mut labels = std::collections::BTreeMap::<String, usize>::new();
    let mut labelled = 0;
    for r in rows {
        *surfaces.entry(&r.sync.surface).or_default() += 1;
        let mut any = false;
        for line in r
            .sync
            .model_limitations
            .iter()
            .filter(|l| l.starts_with("Structure table:"))
        {
            any = true;
            let reasons = line
                .split_once('(')
                .map_or("outside the tabulated range", |(_, rest)| {
                    rest.trim_end_matches(')')
                });
            for reason in reasons.split(", ") {
                *labels.entry(reason.to_owned()).or_default() += 1;
            }
        }
        labelled += usize::from(any);
    }
    let _ = writeln!(
        text,
        "- Generated surfaces: {}\n- Planets whose layers stand on filled or out-of-range table knots: {labelled} of {}",
        surfaces
            .iter()
            .map(|(surface, n)| format!("{surface} {n}"))
            .collect::<Vec<_>>()
            .join(", "),
        rows.len()
    );
    for (reason, n) in labels {
        let _ = writeln!(text, "  - {n}: {reason}");
    }
    Layers {
        surface_changes,
        thin_sequence_changes,
        thin_boundary,
    }
}

fn boundaries(rows: &[Row]) -> String {
    let facts = |r: &LayeredPlanet| {
        serde_json::json!({
            "gas_mole_fractions": mole_fractions(r),
            "condensed_radius_m": r.physical.condensed_radius_m,
            "reference_gravity_m_s2": r.physical.reference_gravity_m_s2,
            "reservoirs": r.physical.materials,
        })
    };
    let mut out = String::new();
    for r in rows {
        let value = serde_json::json!({
            "id": r.id, "route": r.decision,
            "initial": facts(&r.sync), "raw": r.layered.as_ref().map(facts),
            "adopted": r.adopted.as_ref().and_then(|r| r.as_ref().ok()).map(facts),
            "adoption_error": r.adopted.as_ref().and_then(|r| r.as_ref().err()),
            "initial_atmosphere": r.atmosphere, "adopted_atmosphere": r.adopted_atmosphere,
            "surface": [&r.sync.surface, r.layered.as_ref().map_or("", |l| &l.surface)],
            "initial_layers": r.sync.layers, "raw_layers": r.layered.as_ref().map(|l| &l.layers),
        });
        let _ = writeln!(out, "{value}");
    }
    out
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
    // Half the machine: leave the rest for whoever is using it.
    let threads = (std::thread::available_parallelism().map_or(1, |n| n.get()) / 2).max(1);
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
        std::fs::write(
            dir.join("generation-agreement-boundaries.jsonl"),
            boundaries(&rows),
        )
        .unwrap();
    }
    assert!(!rows.is_empty());
    // What generation decides, the layered solve keeps.
    assert_eq!(summary.surface_flips, 0, "surface kind changed");
    // Generation walks the solver's own water models and reads its layer
    // boundaries from the table: same surface, same layers, within the
    // table's interpolation. A phase that barely exists may appear in one only.
    let layers = &summary.layers;
    assert_eq!(layers.surface_changes, 0, "surface text changed");
    assert!(
        layers.thin_sequence_changes * 100 <= rows.len(),
        "{} thin layer lists differ",
        layers.thin_sequence_changes
    );
    assert!(layers.thin_boundary < 0.01, "{}", layers.thin_boundary);
    assert_eq!(summary.escape_changes, 0, "escape changed");
    assert_eq!(summary.mislabelled, 0, "bulk label on an H/He envelope");
    assert_eq!(summary.decided_temperature_k, 0.0, "decided temperature");
    // Generation reads the layered solver's table: condensed surfaces agree
    // to its interpolation, and supply-limited pressure follows the radius.
    assert!(summary.thin_radius < 0.01, "{}", summary.thin_radius);
    assert!(summary.thin_pressure < 0.05, "{}", summary.thin_pressure);
    // Envelopes are tabulated over a reference core and gas mix. Regression
    // bounds on what that simplification costs, not the 1% target.
    let [median, p95, worst] = summary.envelope_radius;
    assert!(median < 0.01, "envelope median {median}");
    assert!(p95 < 0.05, "envelope p95 {p95}");
    assert!(worst < 0.25, "envelope worst {worst}");
    // A visit installs the layered profile on generation's radius.
    assert_eq!(summary.radius_changes, 0, "adoption changed a radius");
    assert_eq!(
        summary.adoption_failures, 0,
        "layered installation failures"
    );
    for r in &rows {
        let Some(Ok(adopted)) = &r.adopted else {
            continue;
        };
        assert_eq!(
            serde_json::to_vec(&r.atmosphere).unwrap(),
            serde_json::to_vec(&r.adopted_atmosphere).unwrap(),
            "{} atmosphere record",
            r.id
        );
        for (a, b) in [
            (r.sync.surface_pressure_pa, adopted.surface_pressure_pa),
            (r.sync.surface_temperature_k, adopted.surface_temperature_k),
            (
                r.sync.physical.condensed_radius_m,
                adopted.physical.condensed_radius_m,
            ),
            (
                r.sync.physical.reference_gravity_m_s2,
                adopted.physical.reference_gravity_m_s2,
            ),
            (
                r.sync.physical.inertia_factor,
                adopted.physical.inertia_factor,
            ),
        ] {
            assert_eq!(a.to_bits(), b.to_bits(), "{} decided boundary", r.id);
        }
        assert_eq!(
            r.sync.physical.materials, adopted.physical.materials,
            "{} reservoir split",
            r.id
        );
        assert_eq!(gas_difference(&r.sync, adopted), 0.0, "{} gas mix", r.id);
    }
}
