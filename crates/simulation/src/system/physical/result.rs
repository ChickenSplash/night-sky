//! Model-independent transport invariants. No EOS, structure or partition solve
//! is performed here. Consistency is not a certificate of physical accuracy.
use crate::{
    system::{composition::Budget, materials::Material, structure},
    units::GRAVITATIONAL_CONSTANT,
};
use space_shared::{
    LayeredPlanet, PhysicalRevision, PlanetEnvironment, PlanetResult, RadiusReference,
    RefinementStatus, ResultProvenance, SurfaceKnowledge,
};
use std::{collections::BTreeSet, f64::consts::PI};

type Check = Result<(), &'static str>;

/// These constructors produce candidates, not live geometry updates. Revision
/// identities are caller supplied; validation cannot prove a hash or adoption order.
pub fn initial_result(
    budget: &Budget,
    environment: &PlanetEnvironment,
    revision: PhysicalRevision,
) -> Result<PlanetResult, &'static str> {
    initial_from_fallback(&structure::fallback::initial(budget, environment), revision)
}

/// Reuse the converged retention trial rather than recomputing its fallback.
pub(crate) fn initial_from_fallback(
    fallback: &structure::fallback::Fallback,
    revision: PhysicalRevision,
) -> Result<PlanetResult, &'static str> {
    let record = super::fallback(fallback);
    result(
        record,
        revision,
        ResultProvenance::Fallback,
        RefinementStatus::Pending,
    )
}

pub fn resolved_result(
    outcome: &structure::prototype::Outcome,
    revision: PhysicalRevision,
) -> Result<PlanetResult, &'static str> {
    let record = super::outcome(outcome);
    let (provenance, refinement) = match outcome {
        structure::prototype::Outcome::Layered(_) => {
            (ResultProvenance::Layered, RefinementStatus::Complete)
        }
        structure::prototype::Outcome::Fallback(_) => (
            ResultProvenance::Fallback,
            RefinementStatus::Refused {
                reason: record.fallback_reason.clone().ok_or("missing refusal")?,
            },
        ),
    };
    result(record, revision, provenance, refinement)
}

fn result(
    record: LayeredPlanet,
    revision: PhysicalRevision,
    provenance: ResultProvenance,
    refinement: RefinementStatus,
) -> Result<PlanetResult, &'static str> {
    let surface_knowledge = knowledge(&record, provenance);
    let result = PlanetResult {
        revision,
        provenance,
        refinement,
        surface_knowledge,
        record,
    };
    validate_result(&result)?;
    Ok(result)
}

fn knowledge(record: &LayeredPlanet, provenance: ResultProvenance) -> SurfaceKnowledge {
    if record.physical.radius_reference == RadiusReference::OneBar {
        SurfaceKnowledge::GasReference
    } else if provenance == ResultProvenance::Layered {
        match record.surface.as_str() {
            "ice" => SurfaceKnowledge::SolidPhase,
            "ocean" => SurfaceKnowledge::Liquid,
            _ => SurfaceKnowledge::Unresolved,
        }
    } else {
        SurfaceKnowledge::Unresolved
    }
}

pub fn validate_result(result: &PlanetResult) -> Check {
    if !text(&result.revision.inputs) || !text(&result.revision.model) {
        return Err("missing revision identity");
    }
    validate_record(&result.record)?;
    let fallback = result.record.model == structure::fallback::MODEL_ID;
    if fallback != (result.provenance == ResultProvenance::Fallback) {
        return Err("inconsistent result provenance");
    }
    match (
        &result.provenance,
        &result.refinement,
        &result.record.fallback_reason,
    ) {
        (ResultProvenance::Layered, RefinementStatus::Complete, None) => {}
        (ResultProvenance::Fallback, RefinementStatus::Refused { reason }, Some(saved))
            if text(reason) && reason == saved => {}
        (
            ResultProvenance::Fallback,
            RefinementStatus::Pending | RefinementStatus::Running,
            None,
        ) => {}
        (
            ResultProvenance::Fallback,
            RefinementStatus::Unavailable { reason } | RefinementStatus::Failed { reason },
            None,
        ) if text(reason) => {}
        _ => return Err("inconsistent refinement status"),
    }
    if result.surface_knowledge != knowledge(&result.record, result.provenance) {
        return Err("unsupported surface knowledge");
    }
    Ok(())
}

fn text(value: &str) -> bool {
    !value.trim().is_empty()
}
fn positive(value: f64) -> bool {
    value.is_finite() && value > 0.0
}
fn nonnegative(value: f64) -> bool {
    value.is_finite() && value >= 0.0
}
fn close(a: f64, b: f64) -> bool {
    a.is_finite() && b.is_finite() && (a - b).abs() <= 1e-9 * a.abs().max(b.abs())
}

/// Check a refinement's lineage, not whether live geometry can safely adopt it.
/// The caller must still check camera/craft/routes and replace every projection
/// atomically. No state is mutated, and stale work is never renumbered here.
pub fn validate_refinement(active: &PlanetResult, candidate: &PlanetResult) -> Check {
    validate_result(active)?;
    validate_result(candidate)?;
    if active.provenance != ResultProvenance::Fallback
        || !matches!(
            active.refinement,
            RefinementStatus::Pending
                | RefinementStatus::Running
                | RefinementStatus::Failed { .. }
                | RefinementStatus::Unavailable { .. }
        )
        || !matches!(
            candidate.refinement,
            RefinementStatus::Complete | RefinementStatus::Refused { .. }
        )
        || active.revision.inputs != candidate.revision.inputs
        || active.revision.model != candidate.revision.model
        || active.revision.sequence.checked_add(1) != Some(candidate.revision.sequence)
    {
        return Err("stale or unrelated refinement");
    }
    let mut additional_loss = 0.0;
    for previous in &active.record.physical.materials {
        let next = candidate
            .record
            .physical
            .materials
            .iter()
            .find(|m| m.material == previous.material)
            .ok_or("missing refinement material")?;
        let tolerance = 8.0 * f64::EPSILON * previous.source_kg;
        if !close(previous.source_kg, next.source_kg)
            || next.escaped_kg + tolerance < previous.escaped_kg
        {
            return Err("refinement changed source or restored escaped mass");
        }
        additional_loss += next.escaped_kg - previous.escaped_kg;
    }
    let tolerance = 8.0
        * f64::EPSILON
        * active
            .record
            .physical
            .materials
            .iter()
            .map(|m| m.source_kg)
            .sum::<f64>();
    if (additional_loss - candidate.record.blowoff_kg).abs()
        > tolerance + 1e-9 * candidate.record.blowoff_kg
    {
        return Err("inconsistent refinement blow-off");
    }
    Ok(())
}

/// Accept supported records on their transported facts, not agreement with a
/// second physical model. Unknown model IDs do not acquire trusted semantics.
pub fn validate_record(record: &LayeredPlanet) -> Check {
    let fallback = record.model == structure::fallback::MODEL_ID;
    if !fallback
        && ![
            structure::prototype::MODEL_ID,
            structure::envelope::MODEL_ID,
        ]
        .contains(&record.model.as_str())
    {
        return Err("unknown structure model");
    }
    if (!fallback && record.fallback_reason.is_some())
        || record.fallback_reason.as_ref().is_some_and(|r| !text(r))
    {
        return Err("invalid fallback reason");
    }
    let p = &record.physical;
    if ![
        p.retained_mass_kg,
        p.reference_radius_m,
        p.condensed_radius_m,
        p.mean_density_kg_m3,
        p.reference_gravity_m_s2,
        p.inertia_factor,
        record.surface_temperature_k,
    ]
    .into_iter()
    .all(positive)
        || !nonnegative(record.surface_pressure_pa)
        || !nonnegative(record.blowoff_kg)
        || !(0.0..=1.0).contains(&p.h_he_mass_fraction)
        || record
            .greenhouse_optical_depth
            .is_some_and(|v| !nonnegative(v))
        || record.one_bar_radius_m.is_some_and(|v| !positive(v))
    {
        return Err("invalid reference facts");
    }
    if !p.approximations.contains(&record.model)
        || !p.approximations.iter().all(|v| text(v))
        || record.model_limitations.is_empty()
        || !record.model_limitations.iter().all(|v| text(v))
    {
        return Err("missing model limitations");
    }
    if p.materials.len() != Material::ALL.len() {
        return Err("incomplete material catalogue");
    }
    let mut seen = BTreeSet::new();
    let mut total = 0.0;
    let mut escaped = 0.0;
    let mut hhe = 0.0;
    for m in &p.materials {
        let material = Material::from_id(&m.material).ok_or("unknown material")?;
        if !seen.insert(m.material.as_str()) {
            return Err("duplicate material");
        }
        if ![
            m.source_kg,
            m.retained_kg,
            m.escaped_kg,
            m.interior_kg,
            m.condensed_kg,
            m.gas_kg,
        ]
        .into_iter()
        .all(nonnegative)
        {
            return Err("invalid material mass");
        }
        // Species-local subtraction allowance, never borrowed from planet mass.
        let loss_roundoff = 8.0 * f64::EPSILON * m.source_kg;
        if !close(m.retained_kg, m.interior_kg + m.condensed_kg + m.gas_kg)
            || (m.escaped_kg - (m.source_kg - m.retained_kg)).abs()
                > loss_roundoff + 1e-9 * m.escaped_kg.max((m.source_kg - m.retained_kg).abs())
        {
            return Err("inconsistent material totals");
        }
        total += m.retained_kg;
        escaped += m.escaped_kg;
        if matches!(material, Material::Hydrogen | Material::Helium) {
            hhe += m.retained_kg;
        }
    }
    let radius = p.reference_radius_m;
    let outer = record
        .layers
        .first()
        .ok_or("missing layers")?
        .outer_radius_m;
    // Inertia is normalised by reference radius, not necessarily the outer edge.
    let inertia_bound = (2.0 / 3.0) * (outer / radius).powi(2);
    let gravity_bound = GRAVITATIONAL_CONSTANT * p.retained_mass_kg / radius.powi(2);
    if !close(total, p.retained_mass_kg)
        || !close(hhe / total, p.h_he_mass_fraction)
        || !close(
            p.mean_density_kg_m3,
            total / (4.0 / 3.0 * PI * radius.powi(3)),
        )
        || p.condensed_radius_m > radius * (1.0 + 1e-9)
        || radius > outer * (1.0 + 1e-9)
        || !positive(inertia_bound)
        || p.inertia_factor > inertia_bound * (1.0 + 1e-9)
        || !positive(gravity_bound)
        || p.reference_gravity_m_s2 > gravity_bound * (1.0 + 1e-9)
        || record.blowoff_kg > escaped * (1.0 + 1e-9)
    {
        return Err("inconsistent reference facts");
    }
    validate_layers(record)?;
    // The record has no radial mass samples. Fully enclosed regions provide a
    // lower bound; total mass provides the upper bound. Do not demand GM_total
    // at a reference inside an envelope, or pretend to verify its exact gravity.
    let enclosed_min: f64 = record
        .layers
        .iter()
        .filter(|l| l.outer_radius_m <= radius)
        .map(|l| l.mass_kg)
        .sum();
    let gravity_min = GRAVITATIONAL_CONSTANT * enclosed_min / radius.powi(2);
    if p.reference_gravity_m_s2 < gravity_min * (1.0 - 1e-9) {
        return Err("reference gravity below enclosed mass bound");
    }
    let boundary = record
        .layers
        .iter()
        .find(|l| !["atmosphere", "envelope"].contains(&l.kind.as_str()))
        .ok_or("missing condensed boundary")?;
    let condensed = record
        .layers
        .iter()
        .find(|l| !["atmosphere", "envelope", "frost"].contains(&l.kind.as_str()))
        .ok_or("missing condensed interior")?;
    if !close(condensed.outer_radius_m, p.condensed_radius_m) {
        return Err("inconsistent condensed radius");
    }
    let surface_matches = match record.surface.as_str() {
        "envelope" => record.layers.iter().any(|l| l.kind == "envelope"),
        "rock" => ["core", "mantle"].contains(&boundary.kind.as_str()),
        "ice" | "ocean" | "frost" => record.surface == boundary.kind,
        "deep ice" => boundary.kind == "deep_ice",
        "water coexistence" => boundary.kind == "water_coexistence",
        "unresolved hot water" => boundary.kind == "hot_water",
        "unresolved compressed water" | "unresolved water" => boundary.kind == "water_compression",
        "unresolved volatiles" => boundary.kind == "volatiles",
        _ => false,
    };
    if !surface_matches {
        return Err("surface does not match boundary layer");
    }
    let one_bar = record.one_bar_radius_m;
    let gas_reference = p.radius_reference == RadiusReference::OneBar;
    if gas_reference {
        if record.surface != "envelope"
            || !close(record.surface_pressure_pa, 1e5)
            || !one_bar.is_some_and(|r| close(r, radius))
        {
            return Err("inconsistent one-bar reference");
        }
    } else {
        if !close(radius, boundary.outer_radius_m)
            || boundary
                .outer_pressure_pa
                .is_none_or(|v| (v - record.surface_pressure_pa).abs() > 0.02_f64.max(1e-9 * v))
        {
            return Err("inconsistent boundary reference");
        }
        if record.surface == "envelope"
            || ![
                "rock",
                "ocean",
                "ice",
                "frost",
                "deep ice",
                "water coexistence",
                "unresolved hot water",
                "unresolved compressed water",
                "unresolved water",
                "unresolved volatiles",
            ]
            .contains(&record.surface.as_str())
        {
            return Err("unknown condensed surface");
        }
        if p.radius_reference == RadiusReference::SolidSurface
            && !["rock", "ice"].contains(&record.surface.as_str())
        {
            return Err("inconsistent condensed reference");
        }
        if one_bar.is_some_and(|r| r < radius * (1.0 - 1e-9) || r > outer * (1.0 + 1e-9))
            || (record.surface_pressure_pa < 1e5 && one_bar.is_some())
        {
            return Err("invalid one-bar level");
        }
    }
    Ok(())
}

fn validate_layers(record: &LayeredPlanet) -> Check {
    let mut mass = 0.0;
    for (index, layer) in record.layers.iter().enumerate() {
        if !positive(layer.mass_kg)
            || !nonnegative(layer.inner_radius_m)
            || !positive(layer.outer_radius_m)
            || layer.outer_radius_m < layer.inner_radius_m
            || !nonnegative(layer.inner_pressure_pa)
            || layer
                .outer_pressure_pa
                .is_some_and(|p| !nonnegative(p) || p > layer.inner_pressure_pa)
            || (layer.outer_pressure_pa.is_none()
                && !["atmosphere", "envelope"].contains(&layer.kind.as_str()))
            || (layer.kind == "atmosphere" && index != 0)
            || layer.temperature_k.is_some_and(|t| !positive(t))
            || !text(&layer.model)
            || Material::from_id(&layer.material).is_none()
            || ![
                "atmosphere",
                "frost",
                "ocean",
                "ice",
                "deep_ice",
                "hot_water",
                "water_coexistence",
                "water_compression",
                "envelope",
                "mantle",
                "core",
                "volatiles",
            ]
            .contains(&layer.kind.as_str())
            || !layer.model_limitations.iter().all(|v| text(v))
            || layer.model_fidelity > record.model_fidelity
        {
            return Err("invalid layer");
        }
        if !layer.materials.is_empty() {
            let mut seen = BTreeSet::new();
            let mut total = 0.0;
            for material in &layer.materials {
                if Material::from_id(&material.material).is_none()
                    || !seen.insert(&material.material)
                    || !positive(material.mass_kg)
                {
                    return Err("invalid layer material");
                }
                total += material.mass_kg;
            }
            if !close(total, layer.mass_kg) {
                return Err("inconsistent layer material mass");
            }
        }
        let mut seen = BTreeSet::new();
        let mut phase_mass = 0.0;
        for phase in &layer.phase_allocations {
            if Material::from_id(&phase.material).is_none()
                || !seen.insert(&phase.material)
                || !nonnegative(phase.vapour_kg)
                || !nonnegative(phase.condensate_kg)
            {
                return Err("invalid phase allocation");
            }
            phase_mass += phase.vapour_kg + phase.condensate_kg;
        }
        if !nonnegative(phase_mass) || phase_mass > layer.mass_kg * (1.0 + 1e-9) {
            return Err("inconsistent phase mass");
        }
        mass += layer.mass_kg;
    }
    if !close(mass, record.physical.retained_mass_kg)
        || record.layers.last().is_none_or(|l| l.inner_radius_m != 0.0)
    {
        return Err("inconsistent layer mass or centre");
    }
    for pair in record.layers.windows(2) {
        if !close(pair[0].inner_radius_m, pair[1].outer_radius_m)
            || pair[1]
                .outer_pressure_pa
                .is_some_and(|p| (pair[0].inner_pressure_pa - p).abs() > 0.02_f64.max(1e-9 * p))
        {
            return Err("discontinuous layer boundary");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
