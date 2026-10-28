//! Input-only P2 requirements census. No new physical route or phase claims.
use super::{Category, Row};
use std::{collections::BTreeMap, fmt::Write as _};

type InputAxis = (&'static str, fn(&Row) -> f64);

fn range(rows: &[&Row], value: impl Fn(&Row) -> f64) -> String {
    let mut values: Vec<_> = rows.iter().map(|r| value(r)).collect();
    values.sort_by(f64::total_cmp);
    match (values.first(), values.last()) {
        (Some(lo), Some(hi)) => format!("{lo:.6e} to {hi:.6e}"),
        _ => "-".to_owned(),
    }
}

fn group_table(out: &mut String, title: &str, groups: BTreeMap<String, Vec<&Row>>) {
    let _ = writeln!(
        out,
        "\n## {title}\n\n| Group | Count | Mass, Earth masses | Equilibrium, K | H/He mass fraction | Water mass fraction |\n| --- | --- | --- | --- | --- | --- |"
    );
    for (name, rows) in groups {
        let _ = writeln!(
            out,
            "| {name} | {} | {} | {} | {} | {} |",
            rows.len(),
            range(&rows, |r| r.mass_earth),
            range(&rows, |r| r.temperature_k),
            range(&rows, |r| r.light_fraction),
            range(&rows, |r| r.water_fraction)
        );
    }
}

fn groups<'a>(rows: &[&'a Row], key: impl Fn(&Row) -> String) -> BTreeMap<String, Vec<&'a Row>> {
    let mut result = BTreeMap::<String, Vec<&Row>>::new();
    for r in rows {
        result.entry(key(r)).or_default().push(r);
    }
    result
}

fn mass_band(mass: f64) -> &'static str {
    if mass < 2.0 {
        "<2"
    } else if mass < 20.0 {
        "2-20"
    } else if mass < 100.0 {
        "20-100"
    } else {
        ">=100"
    }
}

fn gas_band(fraction: f64) -> &'static str {
    if fraction < 0.01 {
        "<1%"
    } else if fraction < 0.5 {
        "1-50%"
    } else {
        ">=50%"
    }
}

fn temperature_band(k: f64) -> &'static str {
    if k < 50.0 {
        "<50 K"
    } else if k < 250.0 {
        "50-250 K"
    } else if k < 623.15 {
        "250-623.15 K"
    } else {
        ">=623.15 K"
    }
}

pub(super) fn report<'a>(rows: &'a [Row]) -> String {
    let envelopes: Vec<_> = rows.iter().filter(|r| r.envelope).collect();
    let failures: Vec<_> = rows
        .iter()
        .filter(|r| !r.envelope && r.category != Category::Solved)
        .collect();
    let mut out = format!(
        "# P2 generated-input census\n\n- {} planets: {} envelope-route bodies, {} other refusals.\n- Same deterministic calibration population; thin solver unchanged.\n- All fractions are retained mass fractions, before prototype blow-off. H/He mass is not the total solved envelope mass when passive secondary traces are present.\n- Temperature is irradiation equilibrium, not a deep-envelope temperature. This input census does not infer envelope pressure, phase or radius.\n- Envelope is a routing label (now including solved envelopes), not a giant classification. Classes are legacy generator labels.\n- Diagnostic bins select requirements and examples only; they introduce no physics thresholds.\n",
        rows.len(),
        envelopes.len(),
        failures.len()
    );
    group_table(
        &mut out,
        "Envelope-route outcomes",
        groups(&envelopes, |r| {
            if r.category == Category::Solved {
                "solved".into()
            } else {
                r.detail.clone()
            }
        }),
    );
    group_table(
        &mut out,
        "Envelope-route bodies by legacy class",
        groups(&envelopes, |r| r.class.to_owned()),
    );
    group_table(
        &mut out,
        "Envelope input intersections",
        groups(&envelopes, |r| {
            format!(
                "{} Earth masses / {} H/He / {}",
                mass_band(r.mass_earth),
                gas_band(r.light_fraction),
                temperature_band(r.temperature_k)
            )
        }),
    );
    let _ = writeln!(
        out,
        "\n## Envelope composition bounds\n\n| Quantity | Min to max |\n| --- | --- |"
    );
    for (name, bounds) in [
        (
            "H/He mass, Earth masses",
            range(&envelopes, |r| r.mass_earth * r.light_fraction),
        ),
        (
            "Non-H/He mass, Earth masses",
            range(&envelopes, |r| r.mass_earth * (1.0 - r.light_fraction)),
        ),
        (
            "Helium fraction within H/He",
            range(&envelopes, |r| r.helium_fraction / r.light_fraction),
        ),
        (
            "Metal + silicate mass fraction",
            range(&envelopes, |r| r.refractory_fraction),
        ),
        (
            "Water fraction within non-H/He",
            range(&envelopes, |r| r.water_fraction / (1.0 - r.light_fraction)),
        ),
    ] {
        let _ = writeln!(out, "| {name} | {bounds} |");
    }
    let _ = writeln!(
        out,
        "\n- Envelope-labelled bodies with non-H/He mass above 20 Earth masses: {}.\n- Other refused bodies above 20 Earth masses: {}.\n- First blockers can mask further failures; non-H/He mass includes water and secondaries, not just a rock/metal core.",
        envelopes
            .iter()
            .filter(|r| r.mass_earth * (1.0 - r.light_fraction) > 20.0)
            .count(),
        failures.iter().filter(|r| r.mass_earth > 20.0).count()
    );
    group_table(
        &mut out,
        "Other refusals",
        groups(&failures, |r| {
            format!("{} / {}", r.category.label(), r.detail)
        }),
    );
    let _ = writeln!(
        out,
        "\n## Representative regression candidates\n\nOne median-mass member per envelope class/first-blocker pair, plus input extremes and every other refusal. Selection is deterministic (mass, then ID); these are candidate inputs, not new-model reference answers.\n\n| Reason | Planet ID | Legacy class | Mass, Earth masses | H/He fraction | Water fraction | Equilibrium, K | First blocker |\n| --- | --- | --- | --- | --- | --- | --- | --- |"
    );
    let mut selected: BTreeMap<&str, (&Row, Vec<String>)> = BTreeMap::new();
    let mut add = |r: &'a Row, reason: String| {
        selected
            .entry(&r.id)
            .or_insert((r, Vec::new()))
            .1
            .push(reason);
    };
    for (key, mut members) in groups(&envelopes, |r| format!("{} / {}", r.class, r.detail)) {
        members.sort_by(|a, b| a.mass_earth.total_cmp(&b.mass_earth).then(a.id.cmp(&b.id)));
        add(members[members.len() / 2], format!("median: {key}"));
    }
    let axes: [InputAxis; 5] = [
        ("mass", |r| r.mass_earth),
        ("temperature", |r| r.temperature_k),
        ("H/He fraction", |r| r.light_fraction),
        ("water fraction", |r| r.water_fraction),
        ("heavy mass", |r| r.mass_earth * (1.0 - r.light_fraction)),
    ];
    for (name, value) in axes {
        let mut members = envelopes.clone();
        members.sort_by(|a, b| value(a).total_cmp(&value(b)).then(a.id.cmp(&b.id)));
        if let Some(r) = members.first() {
            add(r, format!("minimum {name}"));
        }
        if let Some(r) = members.last() {
            add(r, format!("maximum {name}"));
        }
    }
    for r in failures {
        add(r, "other refusal".to_owned());
    }
    for (_, (r, reasons)) in selected {
        let _ = writeln!(
            out,
            "| {} | `{}` | {} | {:.6e} | {:.6e} | {:.6e} | {:.3} | {} |",
            reasons.join("; "),
            r.id,
            r.class,
            r.mass_earth,
            r.light_fraction,
            r.water_fraction,
            r.temperature_k,
            r.detail
        );
    }
    out
}

#[test]
fn empty_census_and_diagnostic_edges() {
    assert!(report(&[]).contains("0 planets"));
    assert_eq!(mass_band(20.0), "20-100");
    assert_eq!(gas_band(0.01), "1-50%");
    assert_eq!(temperature_band(623.15), ">=623.15 K");
}

#[test]
fn census_candidates_and_input_columns() {
    fn sample(id: &str, category: Category) -> Row {
        Row {
            suspended: None,
            condensation: None,
            traces: Default::default(),
            id: id.to_owned(),
            envelope: category == Category::Envelope,
            class: "test",
            mass_earth: 40.0,
            water_fraction: 0.2,
            light_fraction: 0.4,
            hydrogen_fraction: 0.3,
            helium_fraction: 0.1,
            refractory_fraction: 0.4,
            temperature_k: 100.0,
            surface_k: f64::NAN,
            category,
            detail: "dense gas".to_owned(),
            surface: "",
            surface_pressure_pa: f64::NAN,
            deposit_m: f64::NAN,
            escaped: String::new(),
            escaped_fraction: f64::NAN,
            legacy_pressure_pa: 0.0,
            radius_change: None,
            eos_evaluations: 0,
            milliseconds: 0.0,
            new_radius_km: f64::NAN,
            legacy_radius_km: 0.0,
            inertia_factor: f64::NAN,
            interface_pressure_pa: f64::NAN,
            gravity_residual: f64::NAN,
            outer_residual_pa: f64::NAN,
            model: "",
        }
    }
    let a = sample("a", Category::Envelope);
    let b = sample("b", Category::Envelope);
    let c = sample("c", Category::GasDomain);
    let rows = [a, b, c];
    let text = report(&rows);
    assert!(text.contains("3 planets: 2 envelope-route bodies, 1 other refusals"));
    assert!(text.contains("Helium fraction within H/He | 2.500000e-1"));
    assert!(text.contains("non-H/He mass above 20 Earth masses: 2"));
    // Reversing input order cannot change deterministic representative selection.
    assert_eq!(
        text,
        report(&[
            sample("c", Category::GasDomain),
            sample("b", Category::Envelope),
            sample("a", Category::Envelope)
        ])
    );
    assert_eq!(text.matches("| `a` |").count(), 1);
    assert_eq!(text.matches("| `b` |").count(), 1);
    assert_eq!(text.matches("| `c` |").count(), 1);
    let tsv = super::table(&rows);
    let mut lines = tsv.lines();
    let headers: Vec<_> = lines.next().unwrap().split('\t').collect();
    for line in lines {
        let cells: Vec<_> = line.split('\t').collect();
        assert_eq!(headers.len(), cells.len());
        let number = |name: &str| {
            cells[headers.iter().position(|h| *h == name).unwrap()]
                .parse::<f64>()
                .unwrap()
        };
        assert!(
            (number("hydrogen_fraction") + number("helium_fraction") - number("hhe_fraction"))
                .abs()
                < 1e-15
        );
        assert!(
            (number("hhe_mass_earth") + number("heavy_mass_earth") - number("mass_earth")).abs()
                < 1e-12
        );
        assert_eq!(number("helium_in_hhe"), 0.25);
    }
}
