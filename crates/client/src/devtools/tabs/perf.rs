//! The `perf` tab: frame times, star stream and star draw facts, and GPU
//! passes.
use crate::devtools::FRAMES;
use crate::format::{grouped, si};
use crate::stars::stream::{Starfield, StreamMetrics};
use crate::ui::terminal::{self, DIM, TEXT};
use bevy::diagnostic::DiagnosticsStore;
use bevy::prelude::*;
use bevy_egui::egui;
use std::collections::VecDeque;

pub fn perf(
    ui: &mut egui::Ui,
    frames: &VecDeque<f32>,
    metrics: &StreamMetrics,
    field: &Starfield,
    diagnostics: Option<&DiagnosticsStore>,
) {
    terminal::heading(ui, "frames");
    let recent: Vec<f32> = frames.iter().rev().take(60).copied().collect();
    let mean = recent.iter().sum::<f32>() / recent.len().max(1) as f32;
    let worst = recent.iter().copied().fold(0.0, f32::max);
    let mut rows = vec![
        (
            "fps",
            format!("{:.0}", 1000.0 / mean.max(0.001)),
            Some("Frames per second, from the mean of the last 60 frames."),
        ),
        (
            "frame",
            format!("{mean:.1} ms mean, {worst:.1} ms worst"),
            Some("Mean and slowest frame time over the last 60 frames."),
        ),
    ];
    if let Some(s) = &metrics.sample {
        rows.push((
            "last second",
            format!("P95 {:.1} ms, P99 {:.1} ms", s.frame_p95_ms, s.frame_p99_ms),
            Some("95% and 99% of last second's frames were faster than these, so stutters show here."),
        ));
    }
    terminal::facts_with_details(ui, "debug-frames", &rows);
    frame_graph(ui, frames);
    terminal::heading(ui, "star stream");
    let Some(s) = &metrics.sample else {
        terminal::note(ui, "Measuring…");
        return;
    };
    let aimed = field.aimed_pc.map_or("nowhere yet".into(), |[x, y, z]| {
        format!("{x:+.0}  {y:+.0}  {z:+.0} pc")
    });
    terminal::facts_with_details(
        ui,
        "debug-stream",
        &[
            (
                "mode",
                s.mode.to_lowercase(),
                Some(
                    "Where stars are generated: a background thread or worker, or slices of each frame as a fallback.",
                ),
            ),
            (
                "threads",
                format!("{} started, {:.1} busy", s.threads, s.busy_threads),
                Some(
                    "Threads making stars (natively three quarters of the CPU's), and how many were working on average over the last second, grids included.",
                ),
            ),
            (
                "loaded",
                format!("{} stars in {} cells", grouped(s.stars), grouped(s.cells)),
                Some("Stars generated and held in memory, in cells around the camera."),
            ),
            (
                "pending",
                format!("{} cells", grouped(s.pending)),
                Some("Cells wanted around the camera but not generated yet."),
            ),
            (
                "aimed at",
                aimed,
                Some("Where loading is centred, in parsecs from the central black hole."),
            ),
            (
                "generation",
                format!(
                    "{:.0} ms for {} cells, {} cancelled",
                    s.generation_ms, s.generated_cells, s.cancelled
                ),
                Some(
                    "Generating time over the last second, and jobs dropped because the camera moved on.",
                ),
            ),
            (
                "mesh queue",
                format!(
                    "{} cells, oldest {:.0} ms",
                    s.mesh_chunks, s.mesh_wait_oldest_ms
                ),
                Some(
                    "Loaded cells waiting to become GPU batches, and how long the oldest has waited.",
                ),
            ),
            (
                "mesh wait",
                format!("{:.0} ms average", s.mesh_wait_avg_ms),
                Some("Average time from a cell loading to its stars showing on screen."),
            ),
            (
                "render sync",
                format!(
                    "{:.0} ms for {} chunks built",
                    s.render_sync_ms, s.built_chunks
                ),
                Some(
                    "Frame time spent over the last second turning loaded stars into GPU batches.",
                ),
            ),
        ],
    );
    terminal::heading(ui, "cells, blocks and boxes");
    let boxes: usize = field.cells.values().map(|cell| cell.boxes()).sum();
    terminal::facts_with_details(
        ui,
        "debug-star-layout",
        &[
            (
                "cells",
                format!("{} loaded", grouped(field.cells.len())),
                Some(
                    "The loader's cubes, one brightness tier each, halving in size per tier; each is split into the blocks below.",
                ),
            ),
            (
                "blocks",
                format!(
                    "{} drawn of {} on the GPU",
                    grouped(metrics.blocks_drawn),
                    grouped(metrics.blocks)
                ),
                Some(
                    "Cubes of a cell's grid holding up to 262,144 stars, uploaded as one to four batches; those outside the view are skipped when culling is on (render tab).",
                ),
            ),
            (
                "grid boxes",
                format!(
                    "{}, {:.1} stars each on average",
                    grouped(boxes),
                    field.stars as f64 / boxes.max(1) as f64
                ),
                Some(
                    "The smallest cubes, aiming for about 8 stars each, used to find nearby stars without scanning whole cells; empty ones included.",
                ),
            ),
        ],
    );
    terminal::heading(ui, "star draw");
    let uploaded = metrics.uploaded_stars;
    terminal::facts_with_details(
        ui,
        "debug-star-draw",
        &[
            (
                "draw calls",
                format!(
                    "{} of {} batches",
                    grouped(metrics.batch_draws),
                    grouped(metrics.batches)
                ),
                Some(
                    "Star batches drawn each frame, up to 65,536 stars each; each costs CPU time, on screen or not.",
                ),
            ),
            (
                "uploaded",
                format!(
                    "{} of {} loaded stars",
                    grouped(uploaded),
                    grouped(field.stars)
                ),
                Some(
                    "Stars bright enough to pass the CPU filter; the shader drops the rest of the faint ones and those behind the camera.",
                ),
            ),
            (
                "vertices",
                format!(
                    "{} drawn of {}",
                    grouped(metrics.drawn_stars * 3),
                    grouped(uploaded * 3)
                ),
                Some(
                    "Vertex shader runs, three per star: one instanced triangle the shader shapes into a point.",
                ),
            ),
            (
                "vertex memory",
                format!("{:.0} MB", (uploaded * STAR_BYTES) as f64 / 1e6),
                Some("GPU memory holding the uploaded stars, 20 bytes each."),
            ),
            (
                "precise sprites",
                grouped(metrics.precise_sprites),
                Some(
                    "Stars too close to the camera for batch precision, drawn one by one; should stay small.",
                ),
            ),
            (
                "split systems",
                format!(
                    "{} ({} examined)",
                    grouped(metrics.split_systems),
                    grouped(metrics.split_examined)
                ),
                Some(
                    "Multiple systems wide enough to show apart, drawn star by star as precise sprites, and the stars measured at the last search for them.",
                ),
            ),
        ],
    );
    terminal::heading(ui, "nearest star");
    terminal::facts_with_details(
        ui,
        "debug-nearby",
        &[
            (
                "lookup time",
                format!("{:.2} ms per frame", s.nearby_ms_per_frame),
                Some(
                    "Time finding the nearest star, for automatic arrival and the close-up render scale.",
                ),
            ),
            (
                "checked",
                format!("{:.0} stars per frame", s.nearby_checked_per_frame),
                Some("Stars measured by the lookups; each cell's grid keeps this low."),
            ),
            (
                "cells",
                format!("{:.0} searched per frame", s.nearby_cells_per_frame),
                Some("Loaded cells close enough to hold the nearest star."),
            ),
        ],
    );
    gpu_passes(ui, diagnostics, mean);
}

/// GPU bytes per uploaded star (`stars::render::StarInstance`).
const STAR_BYTES: usize = size_of::<crate::stars::render::StarInstance>();

/// GPU and render-thread time, and shader invocations, per render pass, from
/// Bevy's render diagnostics (native only: WebGL 2 has no timestamp queries).
fn gpu_passes(ui: &mut egui::Ui, diagnostics: Option<&DiagnosticsStore>, frame_ms: f32) {
    #[derive(Default)]
    struct Pass {
        gpu: Option<f64>,
        cpu: Option<f64>,
        vertices: Option<f64>,
        fragments: Option<f64>,
    }
    terminal::heading(ui, "gpu passes");
    let mut passes = std::collections::BTreeMap::<&str, Pass>::new();
    for diagnostic in diagnostics.iter().flat_map(|store| store.iter()) {
        let Some((pass, field)) = diagnostic
            .path()
            .as_str()
            .strip_prefix("render/")
            .and_then(|rest| rest.rsplit_once('/'))
        else {
            continue;
        };
        // Passes that stopped running, such as bloom once it is off, keep
        // their last values; leave them out.
        let Some(latest) = diagnostic
            .measurement()
            .filter(|m| m.time.elapsed().as_secs_f32() < 1.0)
        else {
            continue;
        };
        let value = Some(diagnostic.smoothed().unwrap_or(latest.value));
        let entry = passes.entry(pass).or_default();
        match field {
            "elapsed_gpu" => entry.gpu = value,
            "elapsed_cpu" => entry.cpu = value,
            "vertex_shader_invocations" => entry.vertices = value,
            "fragment_shader_invocations" => entry.fragments = value,
            _ => {}
        }
    }
    if passes.is_empty() {
        terminal::note(
            ui,
            if cfg!(target_arch = "wasm32") {
                "Per-pass GPU timings are native only."
            } else {
                "Waiting for render diagnostics…"
            },
        );
        return;
    }
    let total: f64 = passes
        .iter()
        .filter(|(pass, _)| !pass.contains('/'))
        .filter_map(|(_, p)| p.gpu)
        .sum();
    terminal::facts_with_details(
        ui,
        "debug-gpu-total",
        &[
            (
                "gpu total",
                format!("{total:.1} ms of a {frame_ms:.1} ms frame"),
                Some("Near the frame time the GPU is the limit; well below it, the CPU is."),
            ),
            (
                "passes",
                format!("{}, slowest first", passes.len()),
                Some(
                    "Vertices and fragments are shader runs per frame; galactic stars draw in the transparent 3D pass. ~1, ~2 mark the same pass on an earlier camera: the galaxy glow draws before the main view, the interface after it.",
                ),
            ),
        ],
    );
    let mut passes: Vec<_> = passes.into_iter().collect();
    passes.sort_by(|a, b| b.1.gpu.unwrap_or(0.0).total_cmp(&a.1.gpu.unwrap_or(0.0)));
    let cell = |ui: &mut egui::Ui, text: String, colour| {
        ui.label(
            egui::RichText::new(text)
                .font(terminal::font())
                .color(colour),
        );
    };
    let ms = |v: Option<f64>| v.map_or("·".into(), |v| format!("{v:.2} ms"));
    let count = |v: Option<f64>| v.map_or("·".into(), si);
    egui::Grid::new("debug-gpu-passes")
        .num_columns(5)
        .spacing(egui::vec2(16.0, 2.0))
        .show(ui, |ui| {
            for heading in ["pass", "gpu", "cpu", "vertices", "fragments"] {
                cell(ui, heading.into(), DIM);
            }
            ui.end_row();
            for (name, pass) in passes {
                cell(ui, name.into(), DIM);
                cell(ui, ms(pass.gpu), TEXT);
                cell(ui, ms(pass.cpu), TEXT);
                cell(ui, count(pass.vertices), TEXT);
                cell(ui, count(pass.fragments), TEXT);
                ui.end_row();
            }
        });
}

/// Frame times, newest on the right, against 60 and 30 fps guides.
fn frame_graph(ui: &mut egui::Ui, frames: &VecDeque<f32>) {
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 80.0), egui::Sense::hover());
    let painter = ui.painter();
    // Spikes past 15 fps clip, so the guides stay apart.
    let top = frames.iter().copied().fold(40.0, f32::max).min(66.7);
    let y = |ms: f32| rect.bottom() - rect.height() * (ms / top).min(1.0);
    for (ms, label) in [(1000.0 / 60.0, "60 fps"), (1000.0 / 30.0, "30 fps")] {
        painter.hline(rect.x_range(), y(ms), (1.0, DIM.gamma_multiply(0.4)));
        painter.text(
            egui::pos2(rect.left() + 4.0, y(ms) - 2.0),
            egui::Align2::LEFT_BOTTOM,
            label,
            egui::FontId::monospace(10.0),
            DIM,
        );
    }
    let step = rect.width() / (FRAMES - 1) as f32;
    let start = rect.right() - step * (frames.len().max(1) - 1) as f32;
    let points = frames
        .iter()
        .enumerate()
        .map(|(i, ms)| egui::pos2(start + step * i as f32, y(*ms)))
        .collect();
    painter.add(egui::Shape::line(points, (1.0, TEXT)));
}
