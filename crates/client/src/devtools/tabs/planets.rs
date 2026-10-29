//! Background planet refinement: pause switch and per-planet progress. The
//! results themselves show in the planet inspector.
use crate::{
    planet_loading::{PlanetLoading, PlanetState},
    ui::terminal,
};
use bevy_egui::egui;

pub fn planets(ui: &mut egui::Ui, loading: &mut PlanetLoading) {
    if !terminal::section(ui, "planet refinement") {
        return;
    }
    if terminal::check(
        ui,
        loading.enabled,
        "background refinement",
        true,
        "Background layered solves publish whole physical revisions when safe. Close cameras, craft system snapshots and retained route geometry defer publication. Unsupported results retain the valid fallback with a reason. Turning this off pauses work/adoption, not already adopted facts. Stars retain priority within the shared CPU budget.",
    ) {
        loading.enabled = !loading.enabled;
    }
    let mut rows = Vec::new();
    if let Some((id, c)) = loading.active() {
        rows.push(("system", id.to_owned(), None));
        rows.push(("initial generation", c.initial_ms.map_or_else(|| "Preparing / waiting for stars".into(), |ms| format!("{} ms", crate::format::number(ms,2))), Some("Background reconstruction of the same deterministic system to obtain immutable solve inputs. Original main-thread generation timings are logged separately.")));
        if let Some(e) = &c.error {
            rows.push(("preparation failed", e.clone(), None));
        }
        for (id, name) in &c.names {
            let Some(state) = c.planets.get(id) else {
                continue;
            };
            let mut text = match state {
                PlanetState::Solved(l) | PlanetState::Fallback(l) => format!(
                    "{}: {}, {} K, {} km",
                    if l.fallback_reason.is_some() {
                        "Fallback"
                    } else {
                        "Solved"
                    },
                    l.surface,
                    crate::format::number(l.surface_temperature_k, 1),
                    crate::format::number(l.physical.reference_radius_m / 1000.0, 1)
                ),
                state => state.label(),
            };
            if let Some(adoption) = c.adoption.get(id) {
                text.push_str(&format!(" · {adoption}"));
            }
            rows.push((name, text, None));
        }
    }
    terminal::facts_with_details(ui, "planet-refinement", &rows);
}
