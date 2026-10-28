//! Active result detail and a separately labelled, unapplied refinement candidate.
use super::{Info, Section, atmosphere::pressure_display, composition::material_label};
use crate::planet_loading::{Layered, PlanetState};
use space_shared::{AtmosphereReference, Layer, LayeredPlanet, ModelFidelity, Planet};

pub(super) fn facts(info: &mut Info, planet: &Planet, status: Layered) {
    info.section = Section::Layered;
    let state = match status {
        Layered::Off => return,
        Layered::Waiting => {
            info.field(
                "layered.status",
                "Status",
                None,
                "Waiting · active result unchanged",
            );
            return;
        }
        Layered::State(state) => state,
    };
    let solved = match state {
        PlanetState::Solved(solved) | PlanetState::Fallback(solved) => solved,
        PlanetState::Queued | PlanetState::Running => {
            let text = format!("{} · active result unchanged", state.label());
            info.field("layered.status", "Status", None, text);
            return;
        }
        PlanetState::Failed(e) => {
            let text = format!("Active result retained (calculation failed: {e})");
            info.field("layered.status", "Status", None, text);
            return;
        }
    };
    if planet
        .initial_result
        .as_ref()
        .is_some_and(|r| r.revision.sequence > 0 && r.record == **solved)
    {
        info.field("layered.status", "Status", None, "Applied · active result");
        return;
    }
    info.field(
        "layered.status",
        "Status",
        None,
        format!("{} · candidate, not applied", state.label()),
    );
    info.field(
        "layered.surface",
        if solved.surface == "envelope" {
            "Reference level"
        } else {
            "Surface"
        },
        None,
        surface_label(&solved.surface),
    );
    solved_facts(info, planet, solved);
}

fn solved_facts(info: &mut Info, planet: &Planet, l: &LayeredPlanet) {
    info.field(
        "layered.fidelity",
        "Model fidelity",
        None,
        fidelity_label(l.model_fidelity),
    );
    info.field(
        "layered.limitations",
        "Model limitations",
        None,
        l.model_limitations.join(" · "),
    );
    let envelope = l.surface == "envelope";
    let radius_km = l.physical.reference_radius_m / 1000.0;
    info.field(
        "layered.radius",
        "Radius (candidate vs active)",
        Some("km"),
        format!(
            "{radius_km:.0} km vs {:.0} km ({:+.2}%) · active radius drawn",
            planet.radius_km,
            (radius_km / planet.radius_km - 1.0) * 100.0
        ),
    );
    let active_air = planet
        .environment
        .as_ref()
        .and_then(|e| e.atmosphere.as_ref());
    let active_temperature = active_air.map_or("no active gas estimate".into(), |air| {
        format!("{:.0} K", air.temperature_k)
    });
    info.field(
        "layered.temperature",
        if envelope {
            "1 bar temperature (candidate vs active)"
        } else {
            "Surface temperature (candidate vs active)"
        },
        Some("K"),
        format!("{:.0} K vs {active_temperature}", l.surface_temperature_k),
    );
    let active_pressure = match active_air {
        None => "no active gas column".into(),
        Some(air) if air.reference_level == AtmosphereReference::OneBar => "1 bar reference".into(),
        Some(air) => pressure_display(air.pressure_pa).1,
    };
    info.field(
        "layered.pressure",
        if envelope {
            "Reference pressure (candidate vs active)"
        } else {
            "Surface pressure (candidate vs active)"
        },
        None,
        format!(
            "{} vs {active_pressure}",
            if l.surface_pressure_pa > 0.0 {
                pressure_display(l.surface_pressure_pa).1
            } else {
                "no gas".into()
            }
        ),
    );
    if let Some(tau) = l.greenhouse_optical_depth {
        info.field(
            "layered.greenhouse",
            "Greenhouse optical depth",
            None,
            format!("{tau:.3} (grey estimate)"),
        );
    }
    if l.blowoff_kg > 0.0 {
        info.field(
            "layered.blowoff",
            "Lost to blow-off",
            Some("kg"),
            format!(
                "{:.3e} kg ({} of retained mass)",
                l.blowoff_kg,
                super::composition::percent(l.blowoff_kg / l.physical.retained_mass_kg)
            ),
        );
    }
    regions(info, "layered", l);
    info.field(
        "layered.models",
        "Models",
        None,
        l.physical.approximations.join(", "),
    );
}

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
        format!("{:?} · {:?}", result.provenance, result.refinement),
    );
    info.field(
        "result.sequence",
        "Physical revision",
        None,
        format!(
            "{} · sequence {}",
            result.revision.model, result.revision.sequence
        ),
    );
    info.field(
        "result.inputs",
        "Input identity",
        None,
        result.revision.inputs.clone(),
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
                "{:.1} km · {:?} phase · separate from gas reference",
                geometry.condensed_radius_m / 1000.0,
                geometry.condensed_phase
            ),
        );
    }
    regions(info, "result", &result.record);
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

fn fidelity_label(fidelity: ModelFidelity) -> &'static str {
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
                    format!("{t:.0} K at top (varies with depth)")
                } else if layer.kind == "water_compression" {
                    format!("{t:.0} K at outer boundary (thermal state unresolved)")
                } else {
                    format!("{t:.0} K")
                }
            }),
    );
    parts.join(" · ")
}

fn surface_label(surface: &str) -> String {
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
