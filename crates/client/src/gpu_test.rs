//! GPU test (debug window, `tools` tab, native): loads the view 500 ly from
//! the galactic centre, looking at it, from nothing, as the core load test
//! does. Once every star is uploaded it settles briefly, then measures frames
//! and GPU pass times, for before-and-after comparisons of star rendering.
//! Each run is logged and appended to `data/logs/gpu-test.jsonl`.
#![cfg_attr(target_arch = "wasm32", allow(dead_code))]

use crate::Navigation;
use crate::render_debug::RenderDebug;
use crate::scene::CameraRig;
use crate::starfield_render::StarInstance;
use crate::stream::{Starfield, StreamMetrics, StreamRestart};
use bevy::{
    diagnostic::DiagnosticsStore, platform::time::Instant, prelude::*, window::PrimaryWindow,
};
use serde::Serialize;
use space_simulation::{LY_METRES, galaxy::PC_METRES};

const FROM_CENTRE_LY: f64 = 500.0;
/// After the last upload: pipelines compile and the exposure eases.
pub const SETTLE_S: f64 = 2.0;
pub const MEASURE_S: f64 = 20.0;
const STAR_PASS: &str = "main_transparent_pass_3d";

#[derive(Resource, Default)]
pub enum GpuTest {
    #[default]
    Idle,
    Requested,
    Loading(Instant),
    Settling {
        since: Instant,
        load_s: f64,
    },
    Measuring {
        since: Instant,
        load_s: f64,
        frames: Vec<f64>,
        star_pass: Vec<f64>,
        gpu: Vec<f64>,
    },
    Done(Box<GpuResults>),
}

#[derive(Serialize)]
pub struct GpuResults {
    pub commit: String,
    pub unix_time: u64,
    pub load_s: f64,
    pub frames: usize,
    pub fps: f64,
    pub frame_mean_ms: f64,
    pub frame_p95_ms: f64,
    pub frame_p99_ms: f64,
    pub star_pass_ms: f64,
    pub gpu_total_ms: f64,
    pub drawn_stars: usize,
    pub uploaded_stars: usize,
    /// Stars merged into other uploaded points.
    pub merged_stars: usize,
    pub star_memory_mb: f64,
    pub msaa: bool,
    pub size: [u32; 2],
    /// The star point magnitude limit.
    pub magnitude: f64,
    /// The merge sub-pixel stars switch.
    pub merge_stars: bool,
}

/// This frame's star pass and total GPU times in ms, from the latest
/// measurement of each main-view pass (as the perf tab's total).
fn gpu_times(diagnostics: &DiagnosticsStore) -> (Option<f64>, Option<f64>) {
    let (mut star, mut total) = (None, None);
    for diagnostic in diagnostics.iter() {
        let Some(pass) = diagnostic
            .path()
            .as_str()
            .strip_prefix("render/")
            .and_then(|rest| rest.strip_suffix("/elapsed_gpu"))
        else {
            continue;
        };
        let Some(latest) = diagnostic
            .measurement()
            .filter(|m| m.time.elapsed().as_secs_f32() < 1.0)
        else {
            continue;
        };
        if pass == STAR_PASS {
            star = Some(latest.value);
        }
        if !pass.contains('/') {
            *total.get_or_insert(0.0) += latest.value;
        }
    }
    (star, total)
}

fn mean(values: &[f64]) -> f64 {
    values.iter().sum::<f64>() / values.len().max(1) as f64
}

fn percentile(values: &[f64], p: f64) -> f64 {
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    sorted
        .get(((sorted.len().max(1) - 1) as f64 * p).round() as usize)
        .copied()
        .unwrap_or(0.0)
}

/// The short commit, marked `+dirty` with uncommitted changes to tracked files.
fn commit() -> String {
    let git = |args: &[&str]| {
        std::process::Command::new("git")
            .args(args)
            .output()
            .ok()
            .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_string())
    };
    let head = git(&["rev-parse", "--short", "HEAD"]).unwrap_or_default();
    let dirty = git(&["status", "--porcelain", "--untracked-files=no"])
        .is_some_and(|status| !status.is_empty());
    format!("{head}{}", if dirty { "+dirty" } else { "" })
}

#[cfg(not(target_arch = "wasm32"))]
fn save(results: &GpuResults) {
    use std::io::Write;
    let path = crate::diagnostics::logs_dir().join("gpu-test.jsonl");
    let line = serde_json::to_string(results).unwrap_or_default();
    let written = std::fs::create_dir_all(crate::diagnostics::logs_dir()).and_then(|()| {
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .and_then(|mut file| writeln!(file, "{line}"))
    });
    if let Err(error) = written {
        warn!("GPU test: could not write {}: {error}", path.display());
    }
}

/// Before `scene::place_camera`, so the stream restarts at the new eye in the
/// same frame.
pub fn run(
    mut test: ResMut<GpuTest>,
    place: (ResMut<Navigation>, ResMut<CameraRig>, ResMut<StreamRestart>),
    stream: (Res<Starfield>, Res<StreamMetrics>),
    measure: (Res<Time>, Option<Res<DiagnosticsStore>>, Res<RenderDebug>),
    window: Query<&Window, With<PrimaryWindow>>,
) {
    let (mut nav, mut rig, mut restart) = place;
    let (field, metrics) = stream;
    let (time, diagnostics, debug) = measure;
    match &mut *test {
        GpuTest::Requested => {
            crate::core_load::facing_centre(&mut nav, FROM_CENTRE_LY * LY_METRES / PC_METRES);
            rig.land();
            restart.0 = true;
            *test = GpuTest::Loading(Instant::now());
        }
        GpuTest::Loading(started)
            if field.pending == 0
                && !field.cells.is_empty()
                && field.added.is_empty()
                && metrics.mesh_queue == 0
                && metrics.uploaded_stars > 0 =>
        {
            *test = GpuTest::Settling {
                since: Instant::now(),
                load_s: started.elapsed().as_secs_f64(),
            };
        }
        GpuTest::Settling { since, load_s } if since.elapsed().as_secs_f64() >= SETTLE_S => {
            *test = GpuTest::Measuring {
                since: Instant::now(),
                load_s: *load_s,
                frames: Vec::new(),
                star_pass: Vec::new(),
                gpu: Vec::new(),
            };
        }
        GpuTest::Measuring {
            since,
            load_s,
            frames,
            star_pass,
            gpu,
        } => {
            frames.push(time.delta_secs_f64() * 1000.0);
            let (star, total) = diagnostics.as_deref().map_or((None, None), gpu_times);
            star_pass.extend(star);
            gpu.extend(total);
            let seconds = since.elapsed().as_secs_f64();
            if seconds < MEASURE_S {
                return;
            }
            let size = window
                .single()
                .map_or([0, 0], |w| [w.physical_width(), w.physical_height()]);
            let results = GpuResults {
                commit: commit(),
                unix_time: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0, |d| d.as_secs()),
                load_s: *load_s,
                frames: frames.len(),
                fps: frames.len() as f64 / seconds,
                frame_mean_ms: mean(frames),
                frame_p95_ms: percentile(frames, 0.95),
                frame_p99_ms: percentile(frames, 0.99),
                star_pass_ms: mean(star_pass),
                gpu_total_ms: mean(gpu),
                drawn_stars: metrics.drawn_stars,
                uploaded_stars: metrics.uploaded_stars,
                merged_stars: metrics.merged_stars,
                star_memory_mb: (metrics.uploaded_stars * size_of::<StarInstance>()) as f64 / 1e6,
                msaa: debug.msaa,
                size,
                magnitude: nav.magnitude,
                merge_stars: debug.merge_stars,
            };
            info!(
                "GPU test ({}): {:.1} fps, frame {:.2} ms (p99 {:.2}), star pass {:.2} ms, GPU {:.2} ms, {} stars drawn",
                results.commit,
                results.fps,
                results.frame_mean_ms,
                results.frame_p99_ms,
                results.star_pass_ms,
                results.gpu_total_ms,
                results.drawn_stars
            );
            #[cfg(not(target_arch = "wasm32"))]
            save(&results);
            *test = GpuTest::Done(Box::new(results));
        }
        _ => {}
    }
}
