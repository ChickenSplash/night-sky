//! Opt-in, reproducible browser star-stream benchmark (`?benchmark=star-flight`).
//! A fresh page has an empty loader and starfield. Drive the camera from home
//! toward the galactic centre, independent of keyboard focus or frame rate.

use crate::Navigation;
use crate::stars::stream::{Starfield, StreamMetrics, StreamSample};
use bevy::{math::DVec3, platform::time::Instant, prelude::*};
use bevy_egui::{EguiContexts, egui};
use serde::Serialize;
use space_simulation::LY_METRES;
use wasm_bindgen::prelude::*;

pub const SECONDS: f64 = 15.0;
const SPEED: f64 = 1_000.0 * LY_METRES;

#[derive(Resource, Default)]
pub struct BenchmarkFlight {
    pub elapsed: f64,
    started: Option<Instant>,
    seen: u64,
    samples: Vec<Sample>,
    /// The finished run's results, as saved by the download button.
    pub report: Option<serde_json::Value>,
}

#[derive(Serialize)]
struct Sample {
    seconds: f64,
    #[serde(flatten)]
    stream: StreamSample,
}

#[wasm_bindgen(inline_js = "export function download(name, text) {
  const url = URL.createObjectURL(new Blob([text], { type: 'application/json' }));
  const link = document.createElement('a');
  link.href = url;
  link.download = name;
  link.click();
  setTimeout(() => URL.revokeObjectURL(url), 1000);
}")]
extern "C" {
    fn download(name: &str, text: &str);
}

pub fn requested() -> bool {
    web_sys::window()
        .and_then(|window| window.location().search().ok())
        .is_some_and(|query| {
            query
                .split('&')
                .any(|part| part == "benchmark=star-flight" || part == "?benchmark=star-flight")
        })
}

/// Reloads the page into a fresh, empty starfield with the benchmark running.
pub fn restart() {
    if let Some(window) = web_sys::window() {
        let _ = window.location().set_search("benchmark=star-flight");
    }
}

/// Saves the finished run's results as a JSON file.
pub fn save(report: &serde_json::Value) {
    let mode = report["final"]["mode"].as_str().unwrap_or("unknown");
    let name = format!("star-flight-{}.json", mode.to_lowercase().replace(' ', "-"));
    if let Ok(text) = serde_json::to_string_pretty(report) {
        download(&name, &text);
    }
}

pub fn at_home(nav: &mut Navigation) {
    nav.visit = None;
    nav.eye = Some(DVec3::ZERO);
    nav.rotation = Quat::from_rotation_y(std::f32::consts::FRAC_PI_2);
    nav.move_speed = SPEED;
    nav.target = -DVec3::X * nav.distance;
}

pub fn advance(
    time: Res<Time>,
    mut flight: Option<ResMut<BenchmarkFlight>>,
    mut nav: ResMut<Navigation>,
) {
    let Some(flight) = flight.as_deref_mut() else {
        return;
    };
    flight.elapsed = (flight.elapsed + time.delta_secs_f64()).min(SECONDS);
    nav.visit = None;
    nav.following = false;
    nav.fly = false;
    nav.presentation_mode = crate::presentation::PresentationMode::Simulation;
    nav.move_speed = SPEED;
    let eye = -DVec3::X * (SPEED * flight.elapsed);
    nav.eye = Some(eye);
    nav.target = eye - (nav.rotation * Vec3::Z).as_dvec3() * nav.distance;
}

/// The finished run's summary over the 3D view, with its results to save.
pub fn results_window(mut contexts: EguiContexts, flight: Option<Res<BenchmarkFlight>>) {
    let Some(report) = flight.as_ref().and_then(|flight| flight.report.as_ref()) else {
        return;
    };
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };
    let last = &report["final"];
    egui::Window::new("Star-stream benchmark")
        .anchor(egui::Align2::RIGHT_BOTTOM, [-16.0, -16.0])
        .collapsible(false)
        .resizable(false)
        .show(ctx, |ui| {
            ui.label(format!(
                "15-second flight complete\n{}: {} stars, {} cells pending\nP99 frame: {:.1} ms",
                last["mode"].as_str().unwrap_or("unknown mode"),
                last["stars"],
                last["pending"],
                last["frame_p99_ms"].as_f64().unwrap_or(0.0),
            ));
            if ui.button("Download results JSON").clicked() {
                save(report);
            }
        });
}

/// Collects each one-second stream report during the flight, and the results
/// once it ends. They also go to the browser console.
pub fn record(
    metrics: Res<StreamMetrics>,
    field: Res<Starfield>,
    mut flight: Option<ResMut<BenchmarkFlight>>,
) {
    let Some(flight) = flight.as_deref_mut() else {
        return;
    };
    if flight.report.is_some() {
        return;
    }
    let started = *flight.started.get_or_insert_with(Instant::now);
    if metrics.reports != flight.seen {
        flight.seen = metrics.reports;
        if let Some(stream) = metrics.sample.clone() {
            flight.samples.push(Sample {
                seconds: flight.elapsed,
                stream,
            });
        }
    }
    if flight.elapsed < SECONDS {
        return;
    }
    let samples = &flight.samples;
    let sum = |f: fn(&StreamSample) -> f64| samples.iter().map(|s| f(&s.stream)).sum::<f64>();
    let max =
        |f: fn(&StreamSample) -> f64| samples.iter().map(|s| f(&s.stream)).fold(0.0, f64::max);
    let mode = samples.last().map_or("unknown", |s| s.stream.mode);
    let last = samples.last().map(|s| &s.stream);
    let report = serde_json::json!({
        "done": true,
        "route": "home → galactic centre",
        "speed_ly_s": 1000,
        "duration_s": SECONDS,
        "initial_loaded_stars": 0,
        "samples": samples,
        "wall_seconds": started.elapsed().as_secs_f64(),
        "final": {
            "mode": mode,
            "seconds": flight.elapsed,
            "stars": field.stars,
            "cells": field.cells.len(),
            "pending": field.pending,
            "mesh_chunks": metrics.mesh_queue,
            "frame_p95_ms": last.map_or(0.0, |s| s.frame_p95_ms),
            "frame_p99_ms": last.map_or(0.0, |s| s.frame_p99_ms),
        },
        "summary": {
            "generated_cells": sum(|s| s.generated_cells as f64),
            "generation_ms": sum(|s| s.generation_ms),
            "render_sync_ms": sum(|s| s.render_sync_ms),
            "max_pending": max(|s| s.pending as f64),
            "max_mesh_chunks": max(|s| s.mesh_chunks as f64),
            "max_frame_p99_ms": max(|s| s.frame_p99_ms),
        },
    });
    info!("Star-stream benchmark: {report:#}");
    flight.report = Some(report);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_route_starts_at_home_and_ends_fifteen_thousand_ly_toward_centre() {
        let mut nav = Navigation::default();
        at_home(&mut nav);
        assert_eq!(nav.eye, Some(DVec3::ZERO));
        assert_eq!(nav.move_speed, SPEED);
        assert_eq!(
            -DVec3::X * (SPEED * SECONDS) / LY_METRES,
            -DVec3::X * 15_000.0
        );
    }
}
