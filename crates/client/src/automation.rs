//! Browser test hook, not a UI. Scripts send `UiCommand`s as JSON through
//! `send_command` and read a small snapshot through `snapshot`; `index.html`
//! wraps both as `window.space.command` and `window.space.state`.
use crate::actions::UiCommand;
use crate::{Navigation, Universe};
use bevy::prelude::*;
use std::cell::RefCell;
use wasm_bindgen::prelude::*;

thread_local! {
    static QUEUE: RefCell<Vec<UiCommand>> = const { RefCell::new(Vec::new()) };
    static SNAPSHOT: RefCell<String> = const { RefCell::new(String::new()) };
}

/// Queues a command for the next frame. Called from JavaScript between frames.
#[wasm_bindgen]
pub fn send_command(json: &str) -> Result<(), JsError> {
    let command: UiCommand = serde_json::from_str(json)?;
    QUEUE.with_borrow_mut(|queue| queue.push(command));
    Ok(())
}

/// The last frame's state as JSON.
#[wasm_bindgen]
pub fn snapshot() -> String {
    SNAPSHOT.with_borrow(Clone::clone)
}

pub fn install(app: &mut App) {
    app.add_systems(PreUpdate, receive)
        .add_systems(Last, publish);
}

fn receive(mut writer: MessageWriter<UiCommand>) {
    QUEUE.with_borrow_mut(|queue| {
        writer.write_batch(queue.drain(..));
    });
}

fn publish(
    universe: Res<Universe>,
    nav: Res<Navigation>,
    clock: Res<crate::Clock>,
    metrics: Res<crate::stream::StreamMetrics>,
    starfield: Res<crate::stream::Starfield>,
    benchmark: Option<Res<crate::benchmark::BenchmarkFlight>>,
) {
    let state = serde_json::json!({
        "lists": universe.0.systems.len(),
        "list": nav.list().and_then(|i| Some(universe.0.systems.get(i)?.id.as_str())),
        "trail": crate::breadcrumb::trail(&nav, &universe.0)
            .into_iter()
            .map(|crumb| crumb.label)
            .collect::<Vec<_>>(),
        "selection_trail": crate::breadcrumb::selection_trail(&nav, &universe.0)
            .into_iter().map(|crumb| crumb.label).collect::<Vec<_>>(),
        "selection": nav.selection.as_ref().map(crate::Selection::object),
        "location_planet": nav.location_planet,
        "inspection": crate::breadcrumb::active_inspection(&nav,
            &crate::breadcrumb::trail(&nav, &universe.0),
            &crate::breadcrumb::selection_trail(&nav, &universe.0)),
        "page": nav.page,
        "presentation_mode": nav.presentation_mode,
        "paused": clock.0.paused,
        "display": {
            "exaggerate": nav.exaggerate,
            "orbits": nav.orbits,
            "labels": nav.labels,
            "markers": nav.markers,
            "velocity_marker": nav.velocity_marker,
            "realistic": nav.realistic,
            "galaxy_glow": nav.galaxy_glow,
            "physical": nav.physical,
            "wireframe": nav.wireframe,
        },
        "fov_deg": nav.fov_deg,
        "stream": metrics.caption,
        "stream_stars": starfield.stars,
        "benchmark": benchmark.and_then(|flight| flight.report.clone()),
    });
    SNAPSHOT.with_borrow_mut(|snapshot| *snapshot = state.to_string());
}
