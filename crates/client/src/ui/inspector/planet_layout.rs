//! Player-facing hierarchy over the shared physical facts. No generation changes.
use super::{Info, Section, Value};
use space_shared::Planet;

pub(super) fn organise(info: &mut Info, planet: &Planet) {
    info.fields.retain(|field| field.id != "atmosphere.state");
    for field in &mut info.fields {
        field.section = match field.id.as_str() {
            "composition.gravity" | "atmosphere.pressure" => Section::Overview,
            "result.status" | "result.surface" | "result.condensed_radius" => Section::Overview,
            "result.inputs" | "result.revision" | "result.limitations" | "result.fidelity"
            | "result.models" => Section::ModelDetails,
            id if id.starts_with("result.assessment.") => Section::ModelDetails,
            "composition.materials" | "composition.density" => Section::Composition,
            id if id.starts_with("composition.material.") => Section::Composition,
            "atmosphere.mixture" | "atmosphere.column_temperature" | "atmosphere.scale_height" => {
                Section::Atmosphere
            }
            id if id.starts_with("composition.") || id.starts_with("atmosphere.") => {
                Section::ModelDetails
            }
            _ => field.section,
        };
        if field.id == "atmosphere.pressure" {
            field.label = "Atmospheric pressure".into();
            if let Value::Known(value) = &mut field.value
                && planet
                    .environment
                    .as_ref()
                    .and_then(|e| e.atmosphere.as_ref())
                    .is_some_and(|air| air.hydrostatic_unresolved)
            {
                value.push_str(" · approximate (escape/thermal structure unresolved)");
            }
        }
        if field.id == "composition.gravity" {
            field.label = "Gravity at radius".into();
        }
        if field.id == "atmosphere.column_temperature" {
            field.label = "Estimated temperature".into();
        }
    }
    info.section = Section::Overview;
    let physical = space_simulation::system::physical::validate(planet);
    match physical {
        Ok(Some(data)) => {
            let surface = match planet.initial_result.as_ref().map(|r| r.surface_knowledge) {
                Some(space_shared::SurfaceKnowledge::GasReference) => {
                    "Radius at 1 bar · condensed boundary below, not a cloud deck or ground"
                }
                Some(space_shared::SurfaceKnowledge::Liquid) => {
                    "Liquid boundary · not solid ground"
                }
                Some(space_shared::SurfaceKnowledge::SolidPhase) => {
                    "Known solid phase · not certified landable terrain"
                }
                _ => "Condensed boundary · phase and landing safety unresolved",
            };
            info.field("planet.surface", "Surface / radius", None, surface);
            let gases: Vec<_> = data
                .materials
                .iter()
                .filter(|p| !matches!(p.material.as_str(), "iron_nickel" | "silicate" | "water"))
                .collect();
            let gas_mass: f64 = gases.iter().map(|p| p.retained_kg).sum();
            let breakdown: Vec<_> = info
                .fields
                .iter()
                .filter(|f| {
                    f.id.strip_prefix("composition.material.")
                        .is_some_and(|id| gases.iter().any(|p| p.material == id))
                })
                .map(|f| format!("{} {}", f.label, f.value.text()))
                .collect();
            info.fields.retain(|f| {
                !f.id
                    .strip_prefix("composition.material.")
                    .is_some_and(|id| gases.iter().any(|p| p.material == id))
            });
            if !gases.is_empty() {
                info.section = Section::Composition;
                info.field(
                    "composition.gases",
                    "Gases (bulk inventory)",
                    Some("% retained mass"),
                    super::composition::percent(gas_mass / data.retained_mass_kg),
                );
                info.field(
                    "composition.gases.breakdown",
                    "Gas species",
                    Some("% retained mass"),
                    breakdown.join(", "),
                );
            }
        }
        _ => info.unknown(
            "planet.surface",
            "Surface / radius",
            "Physical boundary not resolved",
        ),
    }
    if !info.fields.iter().any(|f| f.id == "atmosphere.pressure") {
        info.section = Section::Overview;
        // The model combines absent and sub-cutoff columns. Never invent an exact zero.
        info.unknown(
            "atmosphere.pressure",
            "Atmospheric pressure",
            "Not resolved (absent or below model cutoff)",
        );
    }
    info.fields.sort_by_key(|field| match field.id.as_str() {
        "planet.class" => 0,
        "planet.mass" => 1,
        "planet.radius" => 2,
        "composition.gravity" => 3,
        "planet.surface" => 4,
        "atmosphere.pressure" => 5,
        _ => 6,
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{craft::Now, navigation::objects::ObjectRef};

    #[test]
    fn main_facts_are_separated_from_model_details_and_bulk_gases_are_grouped() {
        let galaxy = space_simulation::demo_galaxy();
        for planet in galaxy.systems.iter().flat_map(|s| &s.planets) {
            let info = super::super::describe(
                &ObjectRef::Planet {
                    id: planet.id.clone(),
                },
                &galaxy,
                None,
                Now::none(),
            );
            for id in [
                "planet.surface",
                "atmosphere.pressure",
                "composition.gravity",
            ] {
                assert!(
                    info.fields
                        .iter()
                        .any(|f| f.id == id && f.section == Section::Overview)
                );
            }
            assert!(!info.fields.iter().any(|f| f.id == "atmosphere.state"));
            for id in [
                "composition.approximations",
                "composition.appearance",
                "atmosphere.model_limits",
            ] {
                assert!(
                    info.fields
                        .iter()
                        .filter(|f| f.id == id)
                        .all(|f| f.section == Section::ModelDetails)
                );
            }
            assert!(
                !info
                    .fields
                    .iter()
                    .any(|f| f.id == "composition.material.hydrogen")
            );
            assert!(info.fields.windows(2).all(|f| f[0].section <= f[1].section));
        }
    }
}
