//! Input-only audit of the envelope's two trace guards. Never bypasses a guard,
//! solves a substitute planet or infers a deep pressure/temperature profile.
use super::{Category, Row, dry};
use crate::system::{
    composition::{Budget, Inventory, Material},
    generation::retention::accessible_fraction,
};
use std::fmt::Write as _;

const SECONDARIES: [Material; 5] = [
    Material::CarbonDioxide,
    Material::Nitrogen,
    Material::Methane,
    Material::Oxygen,
    Material::Argon,
];

#[derive(Default)]
pub(super) struct Audit {
    pub(super) thin_refusal: String,
    protected: Inventory,
    accessible: Inventory,
    heavy_kg: f64,
    envelope_kg: f64,
}

impl Audit {
    pub(super) fn new(budget: &Budget) -> Self {
        let retained = budget.retained();
        // Match prototype allocation exactly, including the source-based
        // protection floor after accessible escape, rather than 90% of retained.
        let protected = Inventory::new(&SECONDARIES.map(|m| {
            (
                m,
                (budget.source().mass_kg(m) * (1.0 - accessible_fraction(m)))
                    .min(retained.mass_kg(m)),
            )
        }))
        .unwrap();
        let accessible =
            Inventory::new(&SECONDARIES.map(|m| (m, retained.mass_kg(m) - protected.mass_kg(m))))
                .unwrap();
        let heavy_kg = [Material::Metal, Material::Silicate, Material::Water]
            .into_iter()
            .map(|m| retained.mass_kg(m))
            .sum::<f64>()
            + protected.total_mass_kg();
        let envelope_kg = accessible.total_mass_kg()
            + retained.mass_kg(Material::Hydrogen)
            + retained.mass_kg(Material::Helium);
        Self {
            thin_refusal: String::new(),
            protected,
            accessible,
            heavy_kg,
            envelope_kg,
        }
    }

    fn protected_fraction(&self) -> f64 {
        self.protected.total_mass_kg() / self.heavy_kg
    }

    fn accessible_fraction(&self) -> f64 {
        self.accessible.total_mass_kg() / self.envelope_kg
    }

    fn blocker(&self) -> &'static str {
        let limit = dry::protected::MAX_TRACE_FRACTION;
        match (
            self.protected_fraction() > limit,
            self.accessible_fraction() > limit,
        ) {
            (true, true) => "both",
            (true, false) => "protected interior",
            (false, true) => "accessible envelope",
            (false, false) => "neither",
        }
    }

    pub(super) fn exceeds_original_guard(&self) -> bool {
        self.blocker() != "neither"
    }
}

pub(super) fn table(rows: &[Row]) -> String {
    let mut out = String::from(
        "id\tclass\tmass_earth\tequilibrium_k\twater_fraction\thhe_fraction\tprotected_fraction_of_heavy\taccessible_fraction_of_envelope\tblocker\theavy_kg\tenvelope_kg\tthin_refusal\tcurrent_outcome\tcurrent_detail",
    );
    for m in SECONDARIES {
        let _ = write!(out, "\t{}_protected_kg\t{}_accessible_kg", m.id(), m.id());
    }
    out.push('\n');
    for r in rows.iter().filter(|r| !r.traces.thin_refusal.is_empty()) {
        let a = &r.traces;
        assert_ne!(a.blocker(), "neither", "trace audit disagrees for {}", r.id);
        let _ = write!(
            out,
            "{}\t{}\t{:.16e}\t{:.16e}\t{:.16e}\t{:.16e}\t{:.16e}\t{:.16e}\t{}\t{:.16e}\t{:.16e}\t{}\t{}\t{}",
            r.id,
            r.class,
            r.mass_earth,
            r.temperature_k,
            r.water_fraction,
            r.light_fraction,
            a.protected_fraction(),
            a.accessible_fraction(),
            a.blocker(),
            a.heavy_kg,
            a.envelope_kg,
            a.thin_refusal,
            r.category.label(),
            r.detail
        );
        for m in SECONDARIES {
            let _ = write!(
                out,
                "\t{:.16e}\t{:.16e}",
                a.protected.mass_kg(m),
                a.accessible.mass_kg(m)
            );
        }
        out.push('\n');
    }
    out
}

pub(super) fn report(rows: &[Row]) -> String {
    let selected: Vec<_> = rows
        .iter()
        .filter(|r| !r.traces.thin_refusal.is_empty())
        .collect();
    let mut out = format!(
        "# Original secondary trace cohort\n\n- Selected bodies: {}. Same deterministic population, selected by the original trace guard and envelope routing.\n- Fractions below refer to the allocated envelope parcel, not the whole planet, unless stated otherwise.\n- This audit does not bypass guards or present rejected structures as solved. Current outcomes include bounded mixture attempts.\n\n## Original guard, prior thin route and current outcomes\n\n",
        selected.len()
    );
    let mut guards = std::collections::BTreeMap::new();
    let mut thin = std::collections::BTreeMap::new();
    let mut classes = std::collections::BTreeMap::new();
    let mut dominant = std::collections::BTreeMap::new();
    let mut outcomes = std::collections::BTreeMap::new();
    for r in &selected {
        *guards.entry(r.traces.blocker()).or_insert(0) += 1;
        *thin.entry(r.traces.thin_refusal.as_str()).or_insert(0) += 1;
        *classes.entry(r.class).or_insert(0) += 1;
        *outcomes
            .entry(if r.category == Category::Solved {
                "solved"
            } else {
                r.detail.as_str()
            })
            .or_insert(0) += 1;
        let material = SECONDARIES
            .into_iter()
            .max_by(|a, b| {
                r.traces
                    .accessible
                    .mass_kg(*a)
                    .total_cmp(&r.traces.accessible.mass_kg(*b))
            })
            .unwrap();
        *dominant.entry(material.id()).or_insert(0) += 1;
    }
    for (title, groups) in [
        ("Guard", guards),
        ("Prior thin refusal", thin),
        ("Legacy class", classes),
        ("Dominant accessible secondary", dominant),
        ("Current outcome", outcomes),
    ] {
        let _ = writeln!(out, "### {title}\n\n| Value | Bodies |\n| --- | ---: |");
        for (name, n) in groups {
            let _ = writeln!(out, "| {name} | {n} |");
        }
        out.push('\n');
    }
    out.push_str(
        "## Secondary fraction of envelope\n\n| Mass fraction | Bodies |\n| --- | ---: |\n",
    );
    for (name, low, high) in [
        ("0.301% to <1%", 0.00301, 0.01),
        ("1% to <5%", 0.01, 0.05),
        ("5% to <10%", 0.05, 0.1),
        ("10% to <50%", 0.1, 0.5),
        ("50% to 100%", 0.5, f64::INFINITY),
    ] {
        let n = selected
            .iter()
            .filter(|r| (low..high).contains(&r.traces.accessible_fraction()))
            .count();
        let _ = writeln!(out, "| {name} | {n} |");
    }
    let range = |mut values: Vec<f64>| {
        values.sort_by(f64::total_cmp);
        if values.is_empty() {
            return "-".to_owned();
        }
        format!(
            "{:.6e} / {:.6e} / {:.6e}",
            values[0],
            values[((values.len() - 1) as f64 * 0.5).round() as usize],
            values[values.len() - 1]
        )
    };
    out.push_str("\n## Input ranges\n\n| Quantity | Minimum / median / maximum |\n| --- | --- |\n");
    for (name, values) in [
        (
            "Mass, Earth masses",
            selected.iter().map(|r| r.mass_earth).collect(),
        ),
        (
            "Irradiation equilibrium, K (not envelope temperature)",
            selected.iter().map(|r| r.temperature_k).collect(),
        ),
        (
            "H/He, % of planet",
            selected.iter().map(|r| 100.0 * r.light_fraction).collect(),
        ),
        (
            "Water, % of planet",
            selected.iter().map(|r| 100.0 * r.water_fraction).collect(),
        ),
        (
            "Protected secondaries, % of heavy inventory",
            selected
                .iter()
                .map(|r| 100.0 * r.traces.protected_fraction())
                .collect(),
        ),
        (
            "Accessible secondaries, % of envelope",
            selected
                .iter()
                .map(|r| 100.0 * r.traces.accessible_fraction())
                .collect(),
        ),
        (
            "Accessible envelope, % of planet",
            selected
                .iter()
                .map(|r| 100.0 * r.traces.envelope_kg / (r.mass_earth * super::EARTH_MASS_KG))
                .collect(),
        ),
    ] {
        let _ = writeln!(out, "| {name} | {} |", range(values));
    }
    out.push_str("\n## Accessible species\n\n| Material | Present in bodies | Minimum / median / maximum, % of envelope |\n| --- | ---: | --- |\n");
    for m in SECONDARIES {
        let present = selected
            .iter()
            .filter(|r| r.traces.accessible.mass_kg(m) > 0.0)
            .count();
        let values = selected
            .iter()
            .map(|r| 100.0 * r.traces.accessible.mass_kg(m) / r.traces.envelope_kg)
            .collect();
        let _ = writeln!(out, "| {} | {present} | {} |", m.id(), range(values));
    }
    out.push_str("\n## Interpretation and limits\n\n- The original limit is a fraction of a host parcel: small secondary inventories can dominate a very small residual H/He inventory. This does not establish percent-level secondary mass fractions for the whole planet.\n- Historical trace refusals preceded envelope integration. New mixture attempts can reach integration, but rejected shooting trials still do not establish accepted deep profiles. Legacy atmosphere pressure is not a substitute.\n- Irradiation temperature is not the prescribed envelope temperature: the runtime adds mass/age-dependent intrinsic forcing and a 20 K irradiation floor.\n- The bounded H/He-rich molecular extension and a secondary-dominated dense-atmosphere extension are distinct requirements. Neither follows from raising the trace limit.\n- Pure-component condensation screens are conservative, not mixture dew points. Component state bounds, water boundaries and numerical failures remain explicit; clearing the original guard is not a new solve.\n");
    out
}

#[test]
fn trace_audit_distinguishes_both_guards_and_source_protection() {
    let sample = |rock, light, co2| {
        let retained = Inventory::new(&[
            (Material::Silicate, rock),
            (Material::Hydrogen, light),
            (Material::CarbonDioxide, co2),
        ])
        .unwrap();
        Audit::new(
            &Budget::new(
                retained,
                retained,
                Inventory::default(),
                Inventory::default(),
            )
            .unwrap(),
        )
    };
    assert_eq!(sample(1000.0, 1000.0, 1.0).blocker(), "neither");
    assert_eq!(sample(1000.0, 1.0, 1.0).blocker(), "accessible envelope");
    assert_eq!(sample(1.0, 1000.0, 1.0).blocker(), "protected interior");
    assert_eq!(sample(1.0, 1.0, 1.0).blocker(), "both");
    let source =
        Inventory::new(&[(Material::Silicate, 1000.0), (Material::CarbonDioxide, 1.0)]).unwrap();
    let retained =
        Inventory::new(&[(Material::Silicate, 1000.0), (Material::CarbonDioxide, 0.9)]).unwrap();
    let audit = Audit::new(
        &Budget::new(source, retained, Inventory::default(), Inventory::default()).unwrap(),
    );
    assert_eq!(audit.protected.mass_kg(Material::CarbonDioxide), 0.9);
    assert_eq!(audit.accessible.total_mass_kg(), 0.0);
}
