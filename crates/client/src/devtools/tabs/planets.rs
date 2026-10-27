//! Step-8 control/readout, separate from the future planet inspector adapter.
use crate::{
    planet_loading::{PlanetLoading, PlanetState},
    ui::terminal,
};
use bevy_egui::egui;

pub fn planets(ui: &mut egui::Ui, loading: &mut PlanetLoading) {
    if !terminal::section(ui, "experimental planet loading") {
        return;
    }
    if terminal::check(
        ui,
        loading.enabled,
        "layered calculations",
        true,
        "Opt-in background rock/water solves. Live visuals and the planet inspector stay on the legacy model until step 9. Stars have priority within the shared CPU budget; running solves finish before releasing their slot.",
    ) {
        loading.enabled = !loading.enabled;
    }
    let mut rows = Vec::new();
    if let Some((id, c)) = loading.active() {
        rows.push(("system", id.to_owned(), None));
        rows.push(("initial generation", c.initial_ms.map_or_else(|| "Preparing / waiting for stars".into(), |ms| format!("{ms:.2} ms")), Some("Background reconstruction of the same deterministic system to obtain immutable solve inputs. Original main-thread generation timings are logged separately.")));
        if let Some(e) = &c.error {
            rows.push(("preparation failed", e.clone(), None));
        }
        for (id, name) in &c.names {
            let Some(state) = c.planets.get(id) else {
                continue;
            };
            let text = match state {
                PlanetState::Queued => "Queued".into(),
                PlanetState::Running => "Calculating".into(),
                PlanetState::Solved(s) => format!(
                    "Solved: {:.1} K, {:.1} km",
                    s.structure.surface_temperature_k(),
                    s.structure.surface_radius_m() / 1000.0
                ),
                PlanetState::Unsupported(e) => format!("Unsupported: {e}"),
                PlanetState::Failed(e) => format!("Worker failed: {e}"),
            };
            rows.push((name, text, None));
        }
    }
    terminal::facts_with_details(ui, "experimental-planets", &rows);
}
