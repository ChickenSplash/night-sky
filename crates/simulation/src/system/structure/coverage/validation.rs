//! Population-wide transport contract, independent of the solver's own checks.
use crate::system::composition::{Budget, Material};
use space_shared::LayeredPlanet;

pub(super) fn check_record(record: &LayeredPlanet, budget: &Budget, id: &str) {
    crate::system::physical::validate_record(record)
        .unwrap_or_else(|error| panic!("{id}: transport: {error}"));
    let positive = |v: f64| assert!(v.is_finite() && v > 0.0, "{id}: positive {v}");
    let nonnegative = |v: f64| assert!(v.is_finite() && v >= 0.0, "{id}: nonnegative {v}");
    let close = |a: f64, b: f64| {
        assert!(
            (a - b).abs() <= 1e-9 * a.abs().max(b.abs()),
            "{id}: closure {a} != {b}"
        );
    };
    let p = &record.physical;
    for v in [
        p.retained_mass_kg,
        p.reference_radius_m,
        p.condensed_radius_m,
        p.mean_density_kg_m3,
        p.reference_gravity_m_s2,
        p.inertia_factor,
        record.surface_temperature_k,
    ] {
        positive(v);
    }
    nonnegative(record.surface_pressure_pa);
    nonnegative(record.blowoff_kg);
    assert!(
        (0.0..=1.0).contains(&p.h_he_mass_fraction),
        "{id}: H/He fraction"
    );
    assert!(
        !record.model.is_empty() && !record.surface.is_empty(),
        "{id}: labels"
    );
    assert!(
        !p.approximations.is_empty() && !record.model_limitations.is_empty(),
        "{id}: provenance"
    );
    assert_eq!(
        p.materials.len(),
        Material::ALL.len(),
        "{id}: material count"
    );
    for material in Material::ALL {
        let entries: Vec<_> = p
            .materials
            .iter()
            .filter(|m| m.material == material.id())
            .collect();
        assert_eq!(entries.len(), 1, "{id}: material {}", material.id());
        let m = entries[0];
        for v in [
            m.source_kg,
            m.retained_kg,
            m.escaped_kg,
            m.interior_kg,
            m.condensed_kg,
            m.gas_kg,
        ] {
            nonnegative(v);
        }
        close(m.source_kg, budget.source().mass_kg(material));
        close(m.retained_kg + m.escaped_kg, m.source_kg);
        close(m.interior_kg + m.condensed_kg + m.gas_kg, m.retained_kg);
        assert!(
            m.escaped_kg >= budget.escaped().mass_kg(material) * (1.0 - 1e-9),
            "{id}: prior escape"
        );
    }
    close(
        p.materials.iter().map(|m| m.retained_kg).sum(),
        p.retained_mass_kg,
    );
    close(
        p.materials.iter().map(|m| m.escaped_kg).sum::<f64>(),
        budget.escaped().total_mass_kg() + record.blowoff_kg,
    );
    close(
        record.layers.iter().map(|l| l.mass_kg).sum(),
        p.retained_mass_kg,
    );
    assert!(!record.layers.is_empty(), "{id}: empty layers");
    assert_eq!(
        record.layers.last().unwrap().inner_radius_m,
        0.0,
        "{id}: centre"
    );
    for l in &record.layers {
        positive(l.mass_kg);
        nonnegative(l.inner_radius_m);
        positive(l.outer_radius_m);
        assert!(l.outer_radius_m >= l.inner_radius_m, "{id}: radii");
        nonnegative(l.inner_pressure_pa);
        if let Some(v) = l.outer_pressure_pa {
            nonnegative(v);
            assert!(l.inner_pressure_pa >= v, "{id}: pressure order");
        }
        if let Some(v) = l.temperature_k {
            positive(v);
        }
        assert!(
            !l.model.is_empty() && !l.kind.is_empty() && !l.material.is_empty(),
            "{id}: layer labels"
        );
        for phase in &l.phase_allocations {
            nonnegative(phase.vapour_kg);
            nonnegative(phase.condensate_kg);
            assert!(
                phase.vapour_kg + phase.condensate_kg <= l.mass_kg * (1.0 + 1e-9),
                "{id}: phase mass"
            );
        }
    }
    for pair in record.layers.windows(2) {
        close(pair[0].inner_radius_m, pair[1].outer_radius_m);
        if let Some(pressure) = pair[1].outer_pressure_pa {
            // Thin coupling's existing absolute interface contract is 0.02 Pa;
            // a relative-only check is invalid for nearly airless surfaces.
            assert!(
                (pair[0].inner_pressure_pa - pressure).abs() <= 0.02_f64.max(1e-9 * pressure),
                "{id}: interface pressure"
            );
        }
    }
    if let Some(v) = record.one_bar_radius_m {
        positive(v);
    }
    if let Some(v) = record.greenhouse_optical_depth {
        nonnegative(v);
    }
    // serde_json otherwise silently serializes non-finite floats as null.
    let json = serde_json::to_string(record).unwrap();
    let decoded: LayeredPlanet =
        serde_json::from_str(&json).unwrap_or_else(|e| panic!("{id}: transport round trip: {e}"));
    assert_eq!(&decoded, record, "{id}: transport round trip");
}
