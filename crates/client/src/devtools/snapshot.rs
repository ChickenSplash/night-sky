//! Shared read-only developer snapshot. No transport commands or HTTP server.
use crate::render::debug::RenderDebug;
use crate::scene::{CameraPose, CameraRig};
use crate::stars::stream::{Starfield, StreamMetrics};
use crate::{Clock, Navigation, Universe};
use bevy::{diagnostic::DiagnosticsStore, platform::time::Instant, prelude::*};
use serde_json::{Value, json};
use std::time::Duration;

#[derive(Resource, Default)]
pub struct Snapshot {
    pub revision: u64,
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    pub json: String,
    published_at: Option<Instant>,
    inspector: crate::ui::inspector::Cache,
    #[cfg(not(target_arch = "wasm32"))]
    writer: Option<std::sync::mpsc::SyncSender<Value>>,
    #[cfg(not(target_arch = "wasm32"))]
    pub(super) http: Option<super::http::SharedState>,
}

pub fn install(app: &mut App) {
    let snapshot = Snapshot {
        #[cfg(not(target_arch = "wasm32"))]
        writer: file_writer(),
        ..Default::default()
    };
    app.insert_resource(snapshot).add_systems(Last, publish);
}

pub(super) fn publish(
    mut snapshot: ResMut<Snapshot>,
    mut metrics: ResMut<StreamMetrics>,
    field: Res<Starfield>,
    scene: (Res<Universe>, Res<Navigation>, Res<Clock>),
    camera: (Res<CameraPose>, Res<CameraRig>),
    render: (Res<RenderDebug>, Option<Res<DiagnosticsStore>>),
    time: Res<Time>,
) {
    metrics.record_frame(time.delta_secs_f64() * 1000.0);
    // Invalidate even on frames when snapshot publishing is rate-limited.
    if scene.0.is_changed() {
        snapshot.inspector.invalidate();
    }
    #[cfg(not(target_arch = "wasm32"))]
    if snapshot.writer.is_none() && snapshot.http.is_none() {
        return;
    }
    // Snapshot construction is deliberately not on every frame. The history
    // still records every frame, so transient spikes are not lost.
    if snapshot
        .published_at
        .is_some_and(|at| at.elapsed() < Duration::from_millis(250))
    {
        return;
    }
    let (universe, nav, clock) = scene;
    let (pose, rig) = camera;
    let (debug, diagnostics) = render;
    let trail = crate::ui::breadcrumb::trail(&nav, &universe.0);
    let selection_trail = crate::ui::breadcrumb::selection_trail(&nav, &universe.0);
    let inspection =
        crate::ui::breadcrumb::active_inspection(&nav, &universe.0, &trail, &selection_trail);
    let readout = crate::ui::inspector::readout(
        &inspection,
        &nav,
        &universe.0,
        &field,
        &mut snapshot.inspector,
    );
    let home = space_simulation::galaxy::GalaxyModel::milky_way().home_pc;
    let position_pc = bevy::math::DVec3::from_array(home)
        + rig.galactic_ly(&pose)
            * (space_simulation::LY_METRES / space_simulation::galaxy::PC_METRES);
    let gpu: Vec<Value> = diagnostics
        .iter()
        .flat_map(|store| store.iter())
        .filter(|d| d.path().as_str().starts_with("render/"))
        .filter_map(|d| {
            let measurement = d.measurement()?;
            let age = measurement.time.elapsed().as_secs_f64();
            (age < 1.0).then(|| {
                json!({
                    "path": d.path().as_str(), "latest": measurement.value,
                    "smoothed": d.smoothed(), "age_seconds": age,
                })
            })
        })
        .collect();
    let state = json!({
        "schema_version": 1,
        // IDs for `jump`, `find_task` and `visit_star`; tasks via the task index.
        "lists": universe.0.systems.iter().map(|list| json!({
            "id": list.id, "name": list.name, "tasks": list.tasks().count(),
            "systems": list.page_stars.iter()
                .map(|star| json!({"page": star.page, "star": star.id}))
                .collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
        "list": nav.list().and_then(|i| Some(universe.0.systems.get(i)?.id.as_str())),
        "trail": trail.iter().map(|c| &c.label).collect::<Vec<_>>(),
        "selection_trail": selection_trail.iter().map(|c| &c.label).collect::<Vec<_>>(),
        "selection": nav.selection.as_ref().map(crate::Selection::object),
        "location_planet": nav.location_planet,
        "inspection": inspection,
        "inspector": readout,
        "page": nav.page, "presentation_mode": nav.presentation_mode,
        "paused": clock.0.paused,
        "display": {
            "exaggerate": nav.exaggerate, "orbits": nav.orbits, "labels": nav.labels,
            "markers": nav.markers, "velocity_marker": nav.velocity_marker,
            "realistic": nav.realistic, "galaxy_glow": nav.galaxy_glow,
            "exposure": nav.exposure, "compression": nav.compression, "wireframe": nav.wireframe,
        },
        "fov_deg": nav.fov_deg, "magnitude": nav.magnitude,
        "stream": metrics.caption, "stream_stars": field.stars,
        "camera": {
            "placed": rig.placed(), "position_pc": position_pc.to_array(),
            "rotation_xyzw": rig.rotation().to_array(), "flying": rig.flying(),
            "flight_progress": rig.progress(), "render_scale_metres": pose.render_scale(),
        },
        "debug": {
            "freeze_stream": debug.freeze_stream, "galactic_points": debug.galactic_points,
            "point_pixels": debug.point_pixels, "msaa": debug.msaa, "bloom": debug.bloom,
            "cull_blocks": debug.cull_blocks, "point_stars": debug.point_stars,
            "split_multiples": debug.split_multiples,
        },
        "performance": {
            "averages": metrics.live(&field),
            "latest": metrics.history.back(),
            "split_peak_last_second": metrics.split_peak(),
            "frame_peak_last_second_ms": metrics.frame_peak_ms(),
            "split_systems": metrics.split_systems, "split_candidates": metrics.split_candidates,
            "split_cached": metrics.split_cached, "split_sprites": metrics.split_sprites,
            "split_offscreen_sprites": metrics.split_offscreen_sprites,
            "gpu_diagnostics": gpu,
            "history": metrics.history,
            "history_limit_seconds": 10, "history_limit_frames": 4096,
            "timing_note": "CPU elapsed timings exclude GPU uploads/rendering. Frame interval precedes the recorded CPU work; a CPU stall can lengthen the following interval. GPU diagnostics are delayed and not frame-correlated. Peaks are independent maxima.",
        },
    });
    #[cfg(not(target_arch = "wasm32"))]
    let state = {
        let mut state = state;
        state["process_id"] = json!(std::process::id());
        state["sampled_unix_ms"] = json!(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis()
        );
        if let Some(shared) = &snapshot.http {
            // Never wait for HTTP readers on the app thread.
            if let Ok(mut cached) = shared.try_lock() {
                *cached = Some(std::sync::Arc::new(state.clone()));
            }
        }
        state
    };
    #[cfg(not(target_arch = "wasm32"))]
    if let Some(writer) = &snapshot.writer {
        // Bounded and nonblocking: final JSON encoding/disk writes never wait
        // on the app thread (snapshot construction above still runs here).
        let _ = writer.try_send(state);
    }
    #[cfg(target_arch = "wasm32")]
    {
        snapshot.json = state.to_string();
    }
    snapshot.published_at = Some(Instant::now());
    snapshot.revision += 1;
}

#[cfg(not(target_arch = "wasm32"))]
fn file_writer() -> Option<std::sync::mpsc::SyncSender<Value>> {
    if std::env::var("NIGHT_SKY_SNAPSHOT").as_deref() != Ok("1") {
        return None;
    }
    let (sender, receiver) = std::sync::mpsc::sync_channel::<Value>(1);
    let result = std::thread::Builder::new()
        .name("developer snapshot".into())
        .spawn(move || {
            let directory = std::path::Path::new("data/logs");
            let path = directory.join("live-snapshot.json");
            let temporary = directory.join(format!("live-snapshot-{}.tmp", std::process::id()));
            if let Err(error) = std::fs::create_dir_all(directory) {
                warn!("Cannot create snapshot directory: {error}");
                return;
            }
            for mut state in receiver {
                state["process_id"] = json!(std::process::id());
                state["published_unix_ms"] = json!(
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis()
                );
                let result = serde_json::to_vec(&state)
                    .map_err(std::io::Error::other)
                    .and_then(|bytes| std::fs::write(&temporary, bytes))
                    .and_then(|()| std::fs::rename(&temporary, &path));
                if let Err(error) = result {
                    warn!("Cannot publish developer snapshot: {error}");
                    break;
                }
            }
            let _ = std::fs::remove_file(temporary);
        });
    match result {
        Ok(_) => Some(sender),
        Err(error) => {
            warn!("Cannot start snapshot writer: {error}");
            None
        }
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;

    #[test]
    fn shared_snapshot_preserves_browser_fields_and_exports_history() {
        let (sender, receiver) = std::sync::mpsc::sync_channel(1);
        let mut app = App::new();
        app.insert_resource(Snapshot {
            writer: Some(sender),
            ..Default::default()
        })
        .insert_resource(StreamMetrics::default())
        .insert_resource(Starfield::default())
        .insert_resource(Universe(space_simulation::demo_galaxy()))
        .insert_resource(Navigation::default())
        .insert_resource(Clock::default())
        .insert_resource(CameraPose::default())
        .insert_resource(CameraRig::default())
        .insert_resource(RenderDebug::default())
        .insert_resource(Time::<()>::default())
        .add_systems(Last, publish);
        app.update();
        let state = receiver.try_recv().unwrap();
        assert_eq!(state["schema_version"], 1);
        let galaxy = &app.world().resource::<Universe>().0;
        assert_eq!(state["lists"][0]["id"], galaxy.systems[0].id);
        assert_eq!(
            state["lists"][0]["systems"][0]["star"],
            galaxy.systems[0].page_stars[0].id
        );
        assert!(state["lists"][0]["tasks"].as_u64().unwrap() > 0);
        assert!(state["trail"].is_array());
        assert!(state["display"]["realistic"].is_boolean());
        assert_eq!(state["debug"]["split_multiples"], true);
        assert_eq!(state["camera"]["position_pc"].as_array().unwrap().len(), 3);
        assert_eq!(state["performance"]["history"].as_array().unwrap().len(), 1);
        assert_eq!(state["performance"]["latest"]["sequence"], 1);
        assert!(state["performance"]["gpu_diagnostics"].is_array());
        assert_eq!(state["inspector"]["title"], "Milky Way");
        assert_eq!(state["inspector"]["inspection"], state["inspection"]);
        assert!(state["inspector"]["facts"].is_array());
        assert_eq!(state["inspector"]["actions"][0]["label"], "View from above");
        // Rate-limited publishing must not stop per-frame history recording.
        app.update();
        assert!(receiver.try_recv().is_err());
        assert_eq!(app.world().resource::<StreamMetrics>().history.len(), 2);
        let shared = std::sync::Arc::new(std::sync::Mutex::new(None));
        let planet = app.world().resource::<Universe>().0.systems[0].planets[0]
            .id
            .clone();
        {
            let mut nav = app.world_mut().resource_mut::<Navigation>();
            nav.selection = Some(crate::Selection::Planet(planet));
            nav.inspection = Some(crate::ui::breadcrumb::Inspection {
                source: crate::ui::breadcrumb::TrailSource::Location,
                object: crate::objects::ObjectRef::Galaxy,
            });
        }
        {
            let mut snapshot = app.world_mut().resource_mut::<Snapshot>();
            snapshot.writer = None;
            snapshot.http = Some(shared.clone());
            snapshot.published_at = None;
        }
        app.update();
        let cached = shared.lock().unwrap();
        let cached = cached.as_ref().unwrap();
        assert_eq!(cached["performance"]["latest"]["sequence"], 3);
        assert!(cached["sampled_unix_ms"].is_number());
        assert_eq!(cached["inspector"]["title"], "Milky Way");
        assert_ne!(cached["selection"], cached["inspector"]["object"]);
    }
}
