//! Opt-in coverage report for the rock-and-water prototype (plan P1 step 1).
//! Exercises the unified entry point also used by opt-in background generation.
//! See docs/simulation/planet-composition.md#prototype-coverage-report.

use super::{coupled, dry, prototype};
use crate::{
    galaxy::Component,
    star::multiple::Hosted,
    system::{
        composition::Material,
        galactic_bodies,
        materials::water::{self as eos, Unsupported},
    },
    units::EARTH_MASS_KG,
};
use std::{collections::BTreeMap, fmt::Write as _, time::Instant};

const DEFAULT_SYSTEMS_PER_COMPONENT: usize = 20;

mod census;
mod traces;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Category {
    Solved,
    Envelope,
    MassDomain,
    ColumnTemperature,
    WaterCold,
    WaterLowPressure,
    WaterVapour,
    WaterHot,
    WaterDeep,
    SecondaryCondensate,
    TraceRounding,
    GasDomain,
    Runaway,
    Numerics,
    Other,
}

impl Category {
    fn label(self) -> &'static str {
        match self {
            Self::Solved => "solved",
            Self::Envelope => "envelope (H/He beyond thin column)",
            Self::MassDomain => "mass domain (>20 ME)",
            Self::ColumnTemperature => "column temperature (above 1000 K)",
            Self::WaterCold => "water cold (<250 K)",
            Self::WaterLowPressure => "water surface below 1 kPa",
            Self::WaterVapour => "water vapour",
            Self::WaterHot => "water hot (>623 K)",
            Self::WaterDeep => "water deep or phase seam",
            Self::SecondaryCondensate => "protected secondary condensate",
            Self::TraceRounding => "trace rounding (closure)",
            Self::GasDomain => "gas domain (dense, weak binding, mass)",
            Self::Runaway => "greenhouse runaway (beyond thin column)",
            Self::Numerics => "numerics (work, convergence)",
            Self::Other => "other",
        }
    }
    fn plan(self) -> &'static str {
        match self {
            Self::Solved => "",
            Self::Envelope | Self::MassDomain | Self::Runaway => "P2",
            Self::ColumnTemperature => "step 5",
            Self::WaterCold | Self::WaterLowPressure => "step 3",
            Self::WaterVapour | Self::SecondaryCondensate => "step 4",
            Self::WaterHot | Self::WaterDeep => "step 5",
            Self::TraceRounding => "step 6",
            Self::GasDomain | Self::Numerics | Self::Other => "triage",
        }
    }
}

struct Row {
    traces: traces::Audit,
    id: String,
    envelope: bool,
    class: &'static str,
    mass_earth: f64,
    water_fraction: f64,
    light_fraction: f64,
    hydrogen_fraction: f64,
    helium_fraction: f64,
    refractory_fraction: f64,
    temperature_k: f64,
    surface_k: f64,
    category: Category,
    detail: String,
    surface: &'static str,
    surface_pressure_pa: f64,
    deposit_m: f64,
    escaped: String,
    escaped_fraction: f64,
    legacy_pressure_pa: f64,
    radius_change: Option<f64>,
    eos_evaluations: usize,
    milliseconds: f64,
    new_radius_km: f64,
    legacy_radius_km: f64,
    inertia_factor: f64,
    interface_pressure_pa: f64,
    gravity_residual: f64,
    outer_residual_pa: f64,
    model: &'static str,
}

struct Solved {
    surface_k: f64,
    radius_m: f64,
    surface: &'static str,
    surface_pressure_pa: f64,
    deposit_m: f64,
    escaped: String,
    escaped_fraction: f64,
    eos_evaluations: usize,
    inertia_factor: f64,
    interface_pressure_pa: f64,
    gravity_residual: f64,
    outer_residual_pa: f64,
    model: &'static str,
}

/// Every reported planet uses the same entry point intended for generation.
fn route(p: &Hosted) -> Result<Solved, (Category, String)> {
    let retained = p.body.budget.retained();
    let light_mass = retained.mass_kg(Material::Hydrogen) + retained.mass_kg(Material::Helium);
    let light = light_mass > 0.0;
    let escaping = prototype::solve(
        &p.body.budget,
        p.environment.as_ref().unwrap(),
        coupled::Controls::default(),
    )
    .map_err(|e| {
        // The warmed surface temperature is unknown on failure, so water
        // errors are not binned by temperature here.
        let (category, detail) = coupled_error(f64::NAN, &e);
        if light && matches!(category, Category::GasDomain | Category::MassDomain) {
            (Category::Envelope, detail)
        } else {
            (category, detail)
        }
    })?;
    let atmosphere = &escaping.structure;
    let escaped = escaping.blowoff;
    let t = atmosphere.surface_temperature_k();
    let pressure = if atmosphere.envelope().is_some() {
        1e5
    } else {
        atmosphere.atmosphere().map_or(0.0, |a| a.pressure_pa())
    };
    let deposit_m = atmosphere.deposit_thickness_m();
    let work = atmosphere.diagnostics().total_eos_evaluations;
    let surface = escaping.surface();
    // Every solve must also convert to a conserved, contiguous transport record.
    let layered = crate::system::physical::layered(&escaping);
    let layer_mass: f64 = layered.layers.iter().map(|l| l.mass_kg).sum();
    assert!((layer_mass / layered.physical.retained_mass_kg - 1.0).abs() < 1e-9);
    assert!(layered.layers.windows(2).all(|p| {
        (p[0].inner_radius_m - p[1].outer_radius_m).abs() <= 1e-9 * p[0].inner_radius_m
    }));
    assert_eq!(layered.surface, surface);
    Ok(Solved {
        surface_k: t,
        radius_m: atmosphere.surface_radius_m(),
        surface,
        surface_pressure_pa: pressure,
        deposit_m,
        escaped: Material::ALL
            .into_iter()
            .filter(|&m| escaped.mass_kg(m) > 0.0)
            .map(|m| format!("{m:?}"))
            .collect::<Vec<_>>()
            .join("+"),
        escaped_fraction: escaped.total_mass_kg() / retained.total_mass_kg(),
        eos_evaluations: work,
        inertia_factor: layered.physical.inertia_factor,
        interface_pressure_pa: atmosphere.condensed_load_pa(),
        gravity_residual: atmosphere
            .envelope()
            .map_or(f64::NAN, |e| e.gravity_residual()),
        outer_residual_pa: atmosphere
            .envelope()
            .map_or(f64::NAN, |e| e.outer_pressure_residual_pa()),
        model: escaping.model_id(),
    })
}

fn coupled_error(t: f64, e: &coupled::Error) -> (Category, String) {
    use coupled::Error as E;
    match e {
        E::Envelope(e) => (Category::Envelope, e.label().to_owned()),
        E::Radial(e) => radial_error(t, e),
        E::InvalidInput(s) if s.starts_with("column temperature") => {
            (Category::ColumnTemperature, (*s).to_owned())
        }
        E::InvalidClosure(m) => (Category::TraceRounding, format!("coupled {m:?}")),
        E::UnsupportedCondensate(m) => (Category::SecondaryCondensate, format!("{m:?}")),
        E::GasMassOutsideDomain => (Category::GasDomain, "gas mass".to_owned()),
        E::DenseGasUnsupported => (Category::GasDomain, "dense gas".to_owned()),
        E::WeakBindingUnsupported => (Category::GasDomain, "weak binding".to_owned()),
        E::WarmingOutsideDomain => (Category::Runaway, "warming outside domain".to_owned()),
        E::WorkBudgetExhausted => (Category::Numerics, "coupled work".to_owned()),
        E::NonConvergence => (Category::Numerics, "coupled convergence".to_owned()),
        other => (Category::Other, format!("{other:?}")),
    }
}

/// Water failures are binned by the conditions that block them, not the error
/// variant, so each bin maps onto one P1 step.
fn radial_error(t: f64, e: &dry::SolveError) -> (Category, String) {
    use dry::SolveError as E;
    let water = |detail: String| {
        let category = if t < eos::MIN_TEMPERATURE_K {
            Category::WaterCold
        } else if t > eos::MAX_TEMPERATURE_K {
            Category::WaterHot
        } else {
            match e {
                E::Water(w) if w.reason == Unsupported::VapourDensity => Category::WaterVapour,
                E::Water(w) if w.pressure_pa.is_some_and(|p| p < eos::MIN_PRESSURE_PA) => {
                    Category::WaterLowPressure
                }
                _ => Category::WaterDeep,
            }
        };
        (category, detail)
    };
    match e {
        E::Water(w) => water(format!("water {:?}", w.reason)),
        E::DenseWater(d) => water(format!("dense water {d:?}").chars().take(60).collect()),
        E::WaterPhaseCrossing {
            expected, actual, ..
        } => water(format!("phase crossing {expected:?}->{actual:?}")),
        E::UnsupportedWaterProfile => water("unsupported water profile".to_owned()),
        E::OutsideMassDomain(_) => (Category::MassDomain, "mass above 20 ME".to_owned()),
        E::InvalidClosure { material, quantity } => {
            (Category::TraceRounding, format!("{material:?} {quantity}"))
        }
        E::WorkBudgetExhausted(limit) => (Category::Numerics, format!("work {limit:?}")),
        E::NonConvergence { material, .. } => {
            (Category::Numerics, format!("convergence {material:?}"))
        }
        E::Unbracketed { .. } => (Category::Numerics, "unbracketed".to_owned()),
        other => (Category::Other, format!("{other:?}")),
    }
}

fn row(p: &Hosted) -> Row {
    let retained = p.body.budget.retained();
    let mass = retained.total_mass_kg();
    let start = Instant::now();
    let outcome = route(p);
    let milliseconds = start.elapsed().as_secs_f64() * 1e3;
    let legacy = p.body.radius_km * 1e3;
    let env = p.environment.as_ref().unwrap();
    let (category, detail, solved) = match outcome {
        Ok(s) => (Category::Solved, String::new(), Some(s)),
        Err((c, d)) => (c, d, None),
    };
    let mut trace_audit = traces::Audit::new(&p.body.budget);
    if trace_audit.exceeds_original_guard()
        && (category == Category::Envelope
            || solved.as_ref().is_some_and(|s| s.surface == "envelope"))
    {
        trace_audit.thin_refusal = format!(
            "{:?}",
            prototype::thin_route_diagnostic(&p.body.budget, env).unwrap_err()
        );
    }
    Row {
        traces: trace_audit,
        id: p.id(),
        envelope: category == Category::Envelope
            || solved.as_ref().is_some_and(|s| s.surface == "envelope"),
        class: p.body.class.name(),
        mass_earth: mass / EARTH_MASS_KG,
        water_fraction: retained.mass_kg(Material::Water) / mass,
        light_fraction: (retained.mass_kg(Material::Hydrogen) + retained.mass_kg(Material::Helium))
            / mass,
        hydrogen_fraction: retained.mass_kg(Material::Hydrogen) / mass,
        helium_fraction: retained.mass_kg(Material::Helium) / mass,
        refractory_fraction: (retained.mass_kg(Material::Metal)
            + retained.mass_kg(Material::Silicate))
            / mass,
        temperature_k: env.equilibrium_temperature_k,
        surface_k: solved.as_ref().map_or(f64::NAN, |s| s.surface_k),
        category,
        detail,
        surface: solved.as_ref().map_or("", |s| s.surface),
        surface_pressure_pa: solved.as_ref().map_or(f64::NAN, |s| s.surface_pressure_pa),
        deposit_m: solved.as_ref().map_or(f64::NAN, |s| s.deposit_m),
        escaped: solved.as_ref().map_or(String::new(), |s| s.escaped.clone()),
        escaped_fraction: solved.as_ref().map_or(f64::NAN, |s| s.escaped_fraction),
        legacy_pressure_pa: env.atmosphere.as_ref().map_or(0.0, |a| a.pressure_pa),
        radius_change: solved.as_ref().map(|s| s.radius_m / legacy - 1.0),
        eos_evaluations: solved.as_ref().map_or(0, |s| s.eos_evaluations),
        milliseconds,
        new_radius_km: solved.as_ref().map_or(f64::NAN, |s| s.radius_m / 1000.0),
        legacy_radius_km: p.body.radius_km,
        inertia_factor: solved.as_ref().map_or(f64::NAN, |s| s.inertia_factor),
        interface_pressure_pa: solved
            .as_ref()
            .map_or(f64::NAN, |s| s.interface_pressure_pa),
        gravity_residual: solved.as_ref().map_or(f64::NAN, |s| s.gravity_residual),
        outer_residual_pa: solved.as_ref().map_or(f64::NAN, |s| s.outer_residual_pa),
        model: solved.as_ref().map_or("", |s| s.model),
    }
}

fn env_usize(name: &str, default: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse().ok())
        .filter(|&v| v > 0)
        .unwrap_or(default)
}

fn quantiles(mut values: Vec<f64>) -> String {
    if values.is_empty() {
        return "-".to_owned();
    }
    values.sort_by(f64::total_cmp);
    [0.5, 0.95, 1.0]
        .map(|q| {
            let v = values[((values.len() - 1) as f64 * q).round() as usize];
            format!("{v:.3}")
        })
        .join(" / ")
}

fn ratio(solved: usize, total: usize) -> String {
    if total == 0 {
        "-".to_owned()
    } else {
        format!(
            "{solved}/{total} ({:.0}%)",
            100.0 * solved as f64 / total as f64
        )
    }
}

fn scientific_quantiles(mut values: Vec<f64>) -> String {
    if values.is_empty() {
        return "-".into();
    }
    values.sort_by(f64::total_cmp);
    [0.5, 0.95, 1.0]
        .map(|q| {
            format!(
                "{:.3e}",
                values[((values.len() - 1) as f64 * q).round() as usize]
            )
        })
        .join(" / ")
}

fn report(rows: &[Row], systems: usize, threads: usize, seconds: f64) -> String {
    let mut out = String::new();
    let rocky: Vec<&Row> = rows.iter().filter(|r| !r.envelope).collect();
    let solved = rocky
        .iter()
        .filter(|r| r.category == Category::Solved)
        .count();
    let _ = writeln!(
        out,
        "# Composition prototype coverage\n\n\
         - Systems: {systems} per component, {} components, {} planets\n\
          - Non-envelope planets solved: {}\n\
          - Envelope-route planets solved: {}\n\
         - Wall time: {seconds:.1} s on {threads} threads\n\
          - Thin temperature: grey greenhouse; envelope: radiative/deep profile, 100 K intrinsic\n",
        Component::ALL.len(),
        rows.len(),
        ratio(solved, rocky.len()),
        ratio(
            rows.iter()
                .filter(|r| r.envelope && r.category == Category::Solved)
                .count(),
            rows.iter().filter(|r| r.envelope).count()
        ),
    );
    let mut categories = BTreeMap::<Category, Vec<&Row>>::new();
    for r in rows {
        categories.entry(r.category).or_default().push(r);
    }
    let _ = writeln!(
        out,
        "## Outcomes\n\n| Outcome | Planets | Of all | Plan |\n| --- | --- | --- | --- |"
    );
    for (category, members) in &categories {
        let share = format!("{:.1}%", 100.0 * members.len() as f64 / rows.len() as f64);
        let _ = writeln!(
            out,
            "| {} | {} | {share} | {} |",
            category.label(),
            members.len(),
            category.plan()
        );
    }
    let _ = writeln!(out, "\n## Failure details\n");
    for (category, members) in &categories {
        if *category == Category::Solved {
            continue;
        }
        let mut details = BTreeMap::<&str, usize>::new();
        for r in members {
            *details.entry(&r.detail).or_default() += 1;
        }
        let mut details: Vec<_> = details.into_iter().collect();
        details.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
        let list = details
            .iter()
            .take(5)
            .map(|(d, n)| format!("{d} x{n}"))
            .collect::<Vec<_>>()
            .join("; ");
        let _ = writeln!(out, "- {}: {list}", category.label());
    }
    let _ = writeln!(
        out,
        "\n## Non-envelope by equilibrium temperature\n\n| Band | Solved |\n| --- | --- |"
    );
    for (label, low, high) in [
        ("below 50 K", 0.0, 50.0),
        ("50-250 K", 50.0, 250.0),
        ("250-623 K", 250.0, 623.15),
        ("above 623 K", 623.15, f64::INFINITY),
    ] {
        let band: Vec<_> = rocky
            .iter()
            .filter(|r| (low..high).contains(&r.temperature_k))
            .collect();
        let ok = band
            .iter()
            .filter(|r| r.category == Category::Solved)
            .count();
        let _ = writeln!(out, "| {label} | {} |", ratio(ok, band.len()));
    }
    let _ = writeln!(
        out,
        "\n## Non-envelope by class\n\n| Class | Solved |\n| --- | --- |"
    );
    let mut classes = BTreeMap::<&str, (usize, usize)>::new();
    for r in &rocky {
        let entry = classes.entry(r.class).or_default();
        entry.1 += 1;
        entry.0 += usize::from(r.category == Category::Solved);
    }
    for (class, (ok, total)) in classes {
        let _ = writeln!(out, "| {class} | {} |", ratio(ok, total));
    }
    let ok: Vec<&&Row> = rocky
        .iter()
        .filter(|r| r.category == Category::Solved)
        .collect();
    let escapers: Vec<&&Row> = ok
        .iter()
        .copied()
        .filter(|r| !r.escaped.is_empty())
        .collect();
    let mut surfaces = BTreeMap::<&str, usize>::new();
    for r in &ok {
        *surfaces.entry(r.surface).or_default() += 1;
    }
    let _ = writeln!(
        out,
        "\n## Solved non-envelope planets\n\n- Outer layer: {}\n\
         - Radius change vs legacy, |relative| p50 / p95 / max: {}\n\
         - Surface pressure, bar, p50 / p95 / max: {}\n\
         - Greenhouse warming, K, p50 / p95 / max: {}\n\
         - Frost deposit thickness, m, p50 / p95 / max (frost surfaces): {}\n\
         - Unbound gas escaped: {} planets ({}); mass fraction p50 / p95 / max: {}\n\
         - EOS evaluations, p50 / p95 / max: {}\n\
         - Solve time, ms, p50 / p95 / max: {}\n\
         - Failed attempt time, ms, p50 / p95 / max: {}",
        surfaces
            .iter()
            .map(|(s, n)| format!("{s} {n}"))
            .collect::<Vec<_>>()
            .join(", "),
        quantiles(
            ok.iter()
                .filter_map(|r| r.radius_change)
                .map(f64::abs)
                .collect()
        ),
        quantiles(ok.iter().map(|r| r.surface_pressure_pa / 1e5).collect()),
        quantiles(
            ok.iter()
                .map(|r| r.surface_k - r.temperature_k.max(coupled::MIN_COLUMN_TEMPERATURE_K))
                .collect()
        ),
        quantiles(
            ok.iter()
                .filter(|r| r.deposit_m > 0.0)
                .map(|r| r.deposit_m)
                .collect()
        ),
        escapers.len(),
        {
            let mut species = BTreeMap::<&str, usize>::new();
            for r in &escapers {
                *species.entry(&r.escaped).or_default() += 1;
            }
            species
                .iter()
                .map(|(s, n)| format!("{s} {n}"))
                .collect::<Vec<_>>()
                .join(", ")
        },
        quantiles(escapers.iter().map(|r| r.escaped_fraction).collect()),
        quantiles(ok.iter().map(|r| r.eos_evaluations as f64).collect()),
        quantiles(ok.iter().map(|r| r.milliseconds).collect()),
        quantiles(
            rocky
                .iter()
                .filter(|r| r.category != Category::Solved)
                .map(|r| r.milliseconds)
                .collect()
        ),
    );
    let envelopes: Vec<_> = rows
        .iter()
        .filter(|r| r.envelope && r.category == Category::Solved)
        .collect();
    let _ = writeln!(
        out,
        "\n## Solved envelopes\n\nAll statistics p50 / p95 / max. Legacy differences are not accuracy measurements.\n\n- One-bar radius, km: {}\n- Inertia factor at one bar: {}\n- Interior interface pressure, bar: {}\n- Absolute reference-gravity residual: {}\n- Outer-shell pressure residual, mbar: {}\n- EOS evaluations: {}\n- Solve time, ms: {}\n\n| Legacy class | Solved envelopes |\n| --- | --- |",
        quantiles(envelopes.iter().map(|r| r.new_radius_km).collect()),
        quantiles(envelopes.iter().map(|r| r.inertia_factor).collect()),
        quantiles(
            envelopes
                .iter()
                .map(|r| r.interface_pressure_pa / 1e5)
                .collect()
        ),
        scientific_quantiles(envelopes.iter().map(|r| r.gravity_residual.abs()).collect()),
        quantiles(
            envelopes
                .iter()
                .map(|r| r.outer_residual_pa / 100.0)
                .collect()
        ),
        quantiles(envelopes.iter().map(|r| r.eos_evaluations as f64).collect()),
        quantiles(envelopes.iter().map(|r| r.milliseconds).collect())
    );
    let mut classes = BTreeMap::<&str, usize>::new();
    for r in envelopes {
        *classes.entry(r.class).or_default() += 1;
    }
    for (class, n) in classes {
        let _ = writeln!(out, "| {class} | {n} |");
    }
    out
}

fn table(rows: &[Row]) -> String {
    let mut out =
        "id\tclass\tmass_earth\twater_fraction\thhe_fraction\tequilibrium_k\tsurface_k\toutcome\tdetail\t\
                   surface\tsurface_pa\tdeposit_m\tescaped\tescaped_fraction\tlegacy_pa\tradius_change\teos_evaluations\tms\thydrogen_fraction\thelium_fraction\trefractory_fraction\thhe_mass_earth\theavy_mass_earth\thelium_in_hhe\tnew_radius_km\tlegacy_radius_km\tinertia_factor\tinterface_pa\tgravity_residual\touter_residual_pa\tmodel\n"
            .to_owned();
    for r in rows {
        let _ = writeln!(
            out,
            "{}\t{}\t{:.16e}\t{:.16e}\t{:.16e}\t{:.16e}\t{:.16e}\t{}\t{}\t{}\t{:.16e}\t{:.16e}\t{}\t{:.16e}\t{:.16e}\t{}\t{}\t{:.2}\t{:.16e}\t{:.16e}\t{:.16e}\t{:.16e}\t{:.16e}\t{}\t{:.16e}\t{:.16e}\t{:.16e}\t{:.16e}\t{:.16e}\t{:.16e}\t{}",
            r.id,
            r.class,
            r.mass_earth,
            r.water_fraction,
            r.light_fraction,
            r.temperature_k,
            r.surface_k,
            r.category.label(),
            r.detail,
            r.surface,
            r.surface_pressure_pa,
            r.deposit_m,
            r.escaped,
            r.escaped_fraction,
            r.legacy_pressure_pa,
            r.radius_change
                .map_or(String::new(), |c| format!("{c:.16e}")),
            r.eos_evaluations,
            r.milliseconds,
            r.hydrogen_fraction,
            r.helium_fraction,
            r.refractory_fraction,
            r.mass_earth * r.light_fraction,
            r.mass_earth * (1.0 - r.light_fraction),
            if r.light_fraction > 0.0 {
                format!("{:.16e}", r.helium_fraction / r.light_fraction)
            } else {
                String::new()
            },
            r.new_radius_km,
            r.legacy_radius_km,
            r.inertia_factor,
            r.interface_pressure_pa,
            r.gravity_residual,
            r.outer_residual_pa,
            r.model,
        );
    }
    out
}

/// `cargo test --release -p space-simulation composition_coverage -- --ignored --nocapture`.
/// NS_COVERAGE_SYSTEMS sets systems per component; NS_COVERAGE_THREADS=1 gives
/// uncontended timings. Writes data/tmp/composition-coverage.{md,tsv}.
#[test]
#[ignore = "opt-in report; run in release"]
fn composition_coverage() {
    let systems = env_usize("NS_COVERAGE_SYSTEMS", DEFAULT_SYSTEMS_PER_COMPONENT);
    let threads = env_usize(
        "NS_COVERAGE_THREADS",
        std::thread::available_parallelism().map_or(1, |n| n.get()),
    );
    let keys: Vec<(Component, usize)> = Component::ALL
        .into_iter()
        .flat_map(|c| (0..systems).map(move |i| (c, i)))
        .collect();
    let start = Instant::now();
    let next = std::sync::atomic::AtomicUsize::new(0);
    let mut rows: Vec<Row> = std::thread::scope(|scope| {
        let workers: Vec<_> = (0..threads)
            .map(|_| {
                scope.spawn(|| {
                    let mut rows = Vec::new();
                    loop {
                        let k = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        let Some(&(component, index)) = keys.get(k) else {
                            break rows;
                        };
                        let key = format!("composition-calibration:{component:?}:{index}");
                        rows.extend(galactic_bodies(&key, component).planets.iter().map(row));
                    }
                })
            })
            .collect();
        workers
            .into_iter()
            .flat_map(|w| w.join().unwrap())
            .collect()
    });
    rows.sort_by(|a, b| a.id.cmp(&b.id));
    let seconds = start.elapsed().as_secs_f64();
    let summary = report(&rows, systems, threads, seconds);
    eprintln!("{summary}");
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/tmp");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("composition-coverage.md"), &summary).unwrap();
    std::fs::write(dir.join("composition-coverage.tsv"), table(&rows)).unwrap();
    let census = census::report(&rows);
    eprintln!("{census}");
    std::fs::write(dir.join("composition-census.md"), census).unwrap();
    std::fs::write(dir.join("composition-traces.tsv"), traces::table(&rows)).unwrap();
    std::fs::write(dir.join("composition-traces.md"), traces::report(&rows)).unwrap();
    assert!(!rows.is_empty());
}
