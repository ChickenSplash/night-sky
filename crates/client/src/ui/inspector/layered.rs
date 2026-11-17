//! Generation's result: the planet's layers, read from the structure table.
use super::{Info, Section, atmosphere::pressure_display, composition::material_label};
use crate::format::number;
use space_shared::{Layer, LayeredPlanet, ModelFidelity, Planet, ResultProvenance};

pub(super) fn active(info: &mut Info, planet: &Planet) {
    let Some(result) = &planet.initial_result else {
        return;
    };
    info.section = Section::Composition;
    if space_simulation::system::physical::validate(planet).is_err() {
        info.unknown(
            "result.status",
            "Active result",
            "Invalid projections or result",
        );
        return;
    }
    info.field(
        "result.status",
        "Active result",
        None,
        match (&result.provenance, &result.record.fallback_reason) {
            (ResultProvenance::Layered, _) => "Layered solve".to_owned(),
            (ResultProvenance::Fallback, None) => "Generation, from the structure table".to_owned(),
            (ResultProvenance::Fallback, Some(reason)) => format!("Fallback: {reason}"),
        },
    );
    info.field(
        "result.revision",
        "Physical revision",
        None,
        result.revision.model.clone(),
    );
    info.field(
        "result.inputs",
        "Input identity",
        None,
        result.revision.inputs.clone(),
    );
    info.field(
        "result.surface_kind",
        if result.record.surface == "envelope" {
            "Reference level"
        } else {
            "Surface"
        },
        None,
        surface_label(&result.record.surface),
    );
    info.field(
        "result.surface",
        "Boundary phase knowledge",
        None,
        format!("{:?} · not landing permission", result.surface_knowledge),
    );
    info.field(
        "result.fidelity",
        "Model fidelity",
        None,
        fidelity_label(result.record.model_fidelity),
    );
    info.field(
        "result.limitations",
        "Model limitations",
        None,
        result.record.model_limitations.join(" · "),
    );
    if let Ok(geometry) = space_simulation::system::surface::Geometry::from_result(result) {
        info.field(
            "result.condensed_radius",
            "Condensed top",
            Some("km"),
            format!(
                "{} km · {:?} phase · separate from gas reference",
                number(geometry.condensed_radius_m / 1000.0, 1),
                geometry.condensed_phase
            ),
        );
    }
    let l = &result.record;
    if let Some(tau) = l.greenhouse_optical_depth {
        info.field(
            "result.greenhouse",
            "Greenhouse optical depth",
            None,
            format!("{tau:.3} (grey estimate)"),
        );
    }
    if l.blowoff_kg > 0.0 {
        info.field(
            "result.blowoff",
            "Lost to blow-off",
            Some("kg"),
            format!(
                "{:.3e} kg ({} of retained mass)",
                l.blowoff_kg,
                super::composition::percent(l.blowoff_kg / l.physical.retained_mass_kg)
            ),
        );
    }
    regions(info, "result", l);
    info.field(
        "result.models",
        "Models",
        None,
        l.physical.approximations.join(", "),
    );
}

fn regions(info: &mut Info, prefix: &str, l: &LayeredPlanet) {
    for (index, layer) in l.layers.iter().enumerate() {
        info.field(
            &format!("{prefix}.layer.{index}"),
            &format!(
                "{} · {}",
                layer_label(&layer.kind),
                material_label(&layer.material)
            ),
            None,
            layer_text(layer),
        );
        info.field(
            &format!("{prefix}.assessment.{index}"),
            "Region model",
            None,
            format!(
                "{} · {} · {}",
                fidelity_label(layer.model_fidelity),
                layer.model,
                layer.model_limitations.join(" · ")
            ),
        );
        if !layer.phase_allocations.is_empty() {
            info.field(
                &format!("{prefix}.phases.{index}"),
                "Suspended phase split",
                None,
                layer
                    .phase_allocations
                    .iter()
                    .map(|p| {
                        format!(
                            "{}: {} condensed (locally suspended)",
                            material_label(&p.material),
                            super::composition::percent(
                                p.condensate_kg / (p.vapour_kg + p.condensate_kg)
                            )
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(" · "),
            );
        }
    }
}

pub(super) fn fidelity_label(fidelity: ModelFidelity) -> &'static str {
    match fidelity {
        ModelFidelity::ReferenceChecked => {
            "Reference-checked local model (not a whole-planet accuracy guarantee)"
        }
        ModelFidelity::Approximate => "Approximate (physical uncertainty not quantified)",
        ModelFidelity::Extrapolated => "Extrapolated (outside original model mask)",
    }
}

fn layer_text(layer: &Layer) -> String {
    let thickness = layer.outer_radius_m - layer.inner_radius_m;
    let mut parts = vec![
        if layer.kind == "atmosphere" {
            format!(
                "{} represented shell",
                crate::format::distance_label(thickness)
            )
        } else {
            format!("{} thick", crate::format::distance_label(thickness))
        },
        format!("{:.3e} kg", layer.mass_kg),
        match layer.outer_pressure_pa {
            Some(top) => format!(
                "{} to {}",
                pressure_display(top).1,
                pressure_display(layer.inner_pressure_pa).1
            ),
            None => format!("{} at base", pressure_display(layer.inner_pressure_pa).1),
        },
    ];
    parts.push(
        layer
            .temperature_k
            .map_or("temperature not modelled".into(), |t| {
                if layer.kind == "envelope" {
                    format!("{} K at top (varies with depth)", number(t, 0))
                } else if layer.kind == "water_compression" {
                    format!(
                        "{} K at outer boundary (thermal state unresolved)",
                        number(t, 0)
                    )
                } else {
                    format!("{} K", number(t, 0))
                }
            }),
    );
    parts.join(" · ")
}

pub(super) fn surface_label(surface: &str) -> String {
    match surface {
        "envelope" => "1 bar in H/He envelope (not ground)".into(),
        "rock" => "Rock".into(),
        "ice" => "Water ice".into(),
        "ocean" => "Liquid water ocean".into(),
        "frost" => "Frost deposit (phase not modelled)".into(),
        "unresolved water" => "Water (phase unresolved)".into(),
        "unresolved volatiles" => "Secondary volatiles (phase unresolved)".into(),
        other => format!("{} (unresolved)", capitalise(other)),
    }
}

fn layer_label(kind: &str) -> String {
    match kind {
        "envelope" => "H/He envelope".into(),
        "water_compression" => "Water-rich layer (phase unresolved)".into(),
        "volatiles" => "Secondary volatiles (phase unresolved)".into(),
        "deep_ice" => "High-pressure ice".into(),
        "hot_water" => "Hot water (unresolved)".into(),
        "water_coexistence" => "Water at a phase boundary".into(),
        other => capitalise(other),
    }
}

fn capitalise(text: &str) -> String {
    let mut chars = text.chars();
    chars
        .next()
        .map_or_else(String::new, |c| c.to_uppercase().chain(chars).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn envelope_is_labelled_as_a_reference_not_ground() {
        assert!(surface_label("envelope").contains("not ground"));
        assert!(layer_label("water_compression").contains("unresolved"));
    }
}
