//! The `perf` tab: frame times, star stream and star draw facts, and GPU
//! passes.
use crate::actions::UiCommand;
use crate::devtools::FRAMES;
use crate::format::{grouped, number, si, signed_number};
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
    searches: &crate::search::Searches,
    out: &mut Vec<UiCommand>,
) {
    let recent: Vec<f32> = frames.iter().rev().take(60).copied().collect();
    let mean = recent.iter().sum::<f32>() / recent.len().max(1) as f32;
    let worst = recent.iter().copied().fold(0.0, f32::max);
    if terminal::section(ui, "frames") {
        let mut rows = vec![
            (
                "fps",
                number(f64::from(1000.0 / mean.max(0.001)), 0),
                Some("Frames per second, from the mean of the last 60 frames."),
            ),
            (
                "frame",
                format!(
                    "{} ms mean, {} ms worst",
                    number(mean.into(), 1),
                    number(worst.into(), 1)
                ),
                Some("Mean and slowest frame time over the last 60 frames."),
            ),
            (
                "latest / peak",
                format!(
                    "{} / {} ms",
                    number(
                        metrics.history.back().map_or(0.0, |f| f.frame_interval_ms),
                        1
                    ),
                    number(metrics.frame_peak_ms(), 1)
                ),
                Some(
                    "Latest app-frame interval and maximum over the trailing second. The interval precedes that frame's CPU work, so a stall can appear in the following interval.",
                ),
            ),
        ];
        if let Some(s) = &metrics.sample {
            rows.push((
            "last second",
            format!("P95 {} ms, P99 {} ms", number(s.frame_p95_ms,1),number(s.frame_p99_ms,1)),
            Some("95% and 99% of last second's frames were faster than these, so stutters show here."),
        ));
        }
        terminal::facts_with_details(ui, "debug-frames", &rows);
        frame_graph(ui, frames);
    }
    if terminal::section(ui, "craft CPU") {
        let latest = &metrics.craft_latest;
        let peak = metrics.craft_peak();
        terminal::facts_with_details(
            ui,
            "debug-craft-cpu",
            &[
                (
                    "update",
                    format!(
                        "{} / {} ms",
                        number(latest.total_ms, 2),
                        number(peak.total_ms, 2)
                    ),
                    Some(
                        "Latest / trailing-second peak CPU time in the craft update. Includes discovery, fixed steps and frame handoffs, not rendering or orbit forecasting.",
                    ),
                ),
                (
                    "system discovery",
                    format!(
                        "{} / {} ms",
                        number(latest.discovery_ms, 2),
                        number(peak.discovery_ms, 2)
                    ),
                    Some(
                        "Latest / trailing-second peak CPU time checking loaded system/star positions and polling background arrival loads. Body generation runs off-thread. Warp never waits for it.",
                    ),
                ),
                (
                    "movement + handoffs",
                    format!(
                        "{} / {} ms",
                        number(latest.stepping_ms, 2),
                        number(peak.stepping_ms, 2)
                    ),
                    Some(
                        "Latest / trailing-second peak CPU time stepping craft, applying gravity and changing coordinate frames.",
                    ),
                ),
                (
                    "fixed steps",
                    format!("{} / {}", grouped(latest.steps), grouped(peak.steps)),
                    Some(
                        "Fixed physics steps this update / maximum over the trailing second, across all craft.",
                    ),
                ),
            ],
        );
    }
    if terminal::section(ui, "CPU threads") {
        let budget = space_simulation::background::shared();
        let activity = budget.activity();
        terminal::facts_with_details(
            ui,
            "debug-cpu-threads",
            &[
                (
                    "shared budget",
                    budget.limit().to_string(),
                    Some(
                        "Three quarters of available CPU threads shared by visible stars and object search. Active search keeps one slot; stars get priority for extra capacity. Search borrows spare slots while leaving star headroom. Single-slot budgets alternate stars and search. This is not CPU affinity or a frame-time guarantee.",
                    ),
                ),
                (
                    "star activity",
                    format!("{} active", activity.stars),
                    Some(
                        "Currently admitted star-worker slots, grids included. An instantaneous concurrency count, not measured CPU utilisation.",
                    ),
                ),
                (
                    "search",
                    format!("{} active", activity.search),
                    Some(
                        "Currently admitted search-worker slots, including its coordinator. Allocations are reconsidered between bounded work slices; idle service threads do not count. Not measured CPU utilisation.",
                    ),
                ),
            ],
        );
    }
    if terminal::section(ui, "star stream") {
        let s = metrics.live(field);
        let aimed = field.aimed_pc.map_or("nowhere yet".into(), |[x, y, z]| {
            format!("{} pc", [x, y, z].map(|v| signed_number(v, 0)).join("  "))
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
                        "{} ms for {} cells, {} cancelled",
                        number(s.generation_ms, 0),
                        grouped(s.generated_cells),
                        grouped(s.cancelled)
                    ),
                    Some(
                        "Generating time in the trailing second, and jobs dropped because the camera moved on. Refreshed every frame.",
                    ),
                ),
                (
                    "mesh queue",
                    format!(
                        "{} cells, oldest {} ms",
                        grouped(s.mesh_chunks),
                        number(s.mesh_wait_oldest_ms, 0)
                    ),
                    Some(
                        "Loaded cells waiting to become GPU batches, and how long the oldest has waited.",
                    ),
                ),
                (
                    "mesh wait",
                    format!("{} ms average", number(s.mesh_wait_avg_ms, 0)),
                    Some("Average time from a cell loading to its stars showing on screen."),
                ),
                (
                    "render sync",
                    format!(
                        "{} ms for {} chunks built",
                        number(s.render_sync_ms, 0),
                        grouped(s.built_chunks)
                    ),
                    Some(
                        "Frame time spent over the last second turning loaded stars into GPU batches.",
                    ),
                ),
            ],
        );
    }
    if terminal::section(ui, "cells, blocks and boxes") {
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
                        "Cubes of a cell's grid holding up to 262,144 stars, uploaded as one to four batches; those outside the view are skipped when culling is on (settings tab, debug section).",
                    ),
                ),
                (
                    "grid boxes",
                    format!(
                        "{}, {} stars each on average",
                        grouped(boxes),
                        number(field.stars as f64 / boxes.max(1) as f64, 1)
                    ),
                    Some(
                        "The smallest cubes, aiming for about 8 stars each, used to find nearby stars without scanning whole cells; empty ones included.",
                    ),
                ),
            ],
        );
    }
    if terminal::section(ui, "star draw") {
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
                    format!("{} MB", number((uploaded * STAR_BYTES) as f64 / 1e6, 0)),
                    Some("GPU memory holding the uploaded stars, 20 bytes each."),
                ),
                (
                    "precise sprites",
                    grouped(metrics.precise_sprites),
                    Some(
                        "Stars too close to the camera for batch precision, drawn one by one; should stay small.",
                    ),
                ),
            ],
        );
    }
    if terminal::section(ui, "star sync") {
        let latest = &metrics.sync_latest;
        let peak = metrics.sync_peak();
        let ms = |latest: f64, peak: f64| format!("{} / {} ms", number(latest, 2), number(peak, 2));
        terminal::facts_with_details(
            ui,
            "debug-star-sync",
            &[
                (
                    "total latest / peak",
                    ms(latest.total_ms, peak.total_ms),
                    Some(
                        "All of starfield::sync on the main thread, the rows below and split systems included. Latest is the last frame; peaks are trailing-second maxima, possibly from different frames.",
                    ),
                ),
                (
                    "uploads latest / peak",
                    ms(latest.upload_ms, peak.upload_ms),
                    Some(
                        "Packing streamed stars into batches and queueing them for upload, within its per-frame budget.",
                    ),
                ),
                (
                    "draw list latest / peak",
                    ms(latest.draws_ms, peak.draws_ms),
                    Some("Culling uploaded batches and listing one draw per visible batch."),
                ),
                (
                    "precise rescan latest / peak",
                    ms(latest.precise_scan_ms, peak.precise_scan_ms),
                    Some(
                        "Gathering precise candidates again: after moving 0.1 pc, a magnitude change or streamed cells changing. Zero latest means no rescan this frame.",
                    ),
                ),
                (
                    "precise sprites latest / peak",
                    ms(latest.precise_ms, peak.precise_ms),
                    Some("Checking every precise candidate and placing its sprite, every frame."),
                ),
                (
                    "precise",
                    format!(
                        "{} sprites of {} candidates",
                        grouped(latest.precise_sprites),
                        grouped(latest.precise_candidates)
                    ),
                    Some(
                        "Stars placed one by one in f64 this frame, besides split systems' stars, and the candidates checked for them.",
                    ),
                ),
                (
                    "render extract latest / peak",
                    ms(latest.render_extract_ms, peak.render_extract_ms),
                    Some(
                        "Render world, a frame or so behind: copying the star frame across and creating new batch buffers.",
                    ),
                ),
                (
                    "render prepare latest / peak",
                    ms(latest.render_prepare_ms, peak.render_prepare_ms),
                    Some(
                        "Render world, a frame or so behind: filling and writing the per-frame draw and precise sprite buffers. Excludes the GPU's copy.",
                    ),
                ),
            ],
        );
    }
    if terminal::section(ui, "instrumentation") {
        let latest = &metrics.instrumentation_latest;
        let peak = metrics.instrumentation_peak();
        let ms = |latest: f64, peak: f64| format!("{} / {} ms", number(latest, 3), number(peak, 3));
        terminal::facts_with_details(
            ui,
            "debug-instrumentation",
            &[
                (
                    "recording latest / peak",
                    ms(latest.record_ms, peak.record_ms),
                    Some("Appending each frame's sample to the 10-second history."),
                ),
                (
                    "snapshot latest / peak",
                    ms(latest.snapshot_ms, peak.snapshot_ms),
                    Some(
                        "Building the shared state snapshot on the main thread, at most every 250 ms and only with the developer API or snapshot export on; zero on other frames. Encoding the history is left to readers that ask for it.",
                    ),
                ),
                (
                    "debug window latest / peak",
                    ms(latest.debug_window_ms, peak.debug_window_ms),
                    Some(
                        "Laying out this window, the open sections included; zero while it is closed. Excludes egui's tessellation and drawing.",
                    ),
                ),
            ],
        );
    }
    if terminal::section(ui, "split systems") {
        let s = metrics.live(field);
        let latest = &metrics.split_latest;
        let peak = metrics.split_peak();
        terminal::facts_with_details(
            ui,
            "debug-split-systems",
            &[
                (
                    "search latest / peak",
                    format!(
                        "{} / {} ms",
                        number(latest.search_ms, 2),
                        number(peak.search_ms, 2)
                    ),
                    Some(
                        "Search CPU elapsed time this frame and maximum in the trailing second. Zero latest means no search this frame.",
                    ),
                ),
                (
                    "prep latest / peak",
                    format!(
                        "{} / {} ms",
                        number(latest.preparation_ms, 2),
                        number(peak.preparation_ms, 2)
                    ),
                    Some(
                        "Split preparation CPU elapsed time this frame and maximum in the trailing second; excludes search, GPU upload and GPU rendering.",
                    ),
                ),
                (
                    "generation latest / peak",
                    format!(
                        "{} / {} ms",
                        number(latest.generation_ms, 2),
                        number(peak.generation_ms, 2)
                    ),
                    Some(
                        "Uncached system generation within preparation. Peaks for different rows may belong to different frames.",
                    ),
                ),
                (
                    "orbits latest / peak",
                    format!(
                        "{} / {} ms",
                        number(latest.orbit_sprite_ms, 2),
                        number(peak.orbit_sprite_ms, 2)
                    ),
                    Some(
                        "Orbital positions and sprite construction within preparation. Remaining preparation time includes candidate checks, culling and cache maintenance.",
                    ),
                ),
                (
                    "rebuilt latest / peak",
                    format!(
                        "{} / {} systems",
                        grouped(latest.rebuilt),
                        grouped(peak.rebuilt)
                    ),
                    Some(
                        "Systems regenerated this frame and maximum per frame in the trailing second.",
                    ),
                ),
                (
                    "systems",
                    format!(
                        "{} ({} candidates)",
                        grouped(metrics.split_systems),
                        grouped(metrics.split_candidates)
                    ),
                    Some(
                        "Systems wide enough to split this frame, on screen or not, and the cached candidates checked each frame.",
                    ),
                ),
                (
                    "examined",
                    format!(
                        "{} last search / {} per second",
                        grouped(metrics.split_examined),
                        number(s.split_checked_per_second, 0)
                    ),
                    Some(
                        "Work across all retries in the last search, not necessarily per frame. Up to four attempts of 100,000 stars, plus exhaustion probes. The per-second rate uses the trailing second.",
                    ),
                ),
                (
                    "search time",
                    format!("{} ms per search", number(s.split_search_avg_ms, 2)),
                    Some(
                        "CPU elapsed time gathering candidates, including retries and their allocations. Trailing-second average; excludes per-frame sprite preparation and GPU work.",
                    ),
                ),
                (
                    "search rate",
                    format!("{} per second", number(s.split_searches_per_second, 1)),
                    Some(
                        "Searches per second, including movement, cell changes, view changes and recovery attempts. A search may contain several retries.",
                    ),
                ),
                (
                    "search per frame",
                    format!("{} ms", number(s.split_search_ms_per_frame, 2)),
                    Some(
                        "Search CPU elapsed time divided by all frames in the trailing second, including frames without searches.",
                    ),
                ),
                (
                    "preparation",
                    format!(
                        "{} ms per frame",
                        number(s.split_preparation_ms_per_frame, 2)
                    ),
                    Some(
                        "CPU elapsed time checking cached candidates, generating uncached systems, calculating orbital positions and constructing split sprite records. Includes cache maintenance and off-screen culling; excludes GPU upload, GPU rendering and searching.",
                    ),
                ),
                (
                    "rebuilt",
                    format!("{} systems per frame", number(s.split_rebuilt_per_frame, 1)),
                    Some(
                        "Systems generated on cache misses, averaged across all frames in the trailing second. Sustained rebuilding while stationary indicates cache churn.",
                    ),
                ),
                (
                    "cache",
                    format!(
                        "{} systems / {} prunes per second",
                        grouped(metrics.split_cached),
                        number(s.split_cache_clears_per_second, 1)
                    ),
                    Some(
                        "Cached generated systems now, and capacity-triggered prunes per second. A prune drops the systems not split this frame, never those in use. Toggle and streaming resets are not counted.",
                    ),
                ),
                (
                    "sprites",
                    format!(
                        "{} submitted / {} systems culled",
                        grouped(metrics.split_sprites),
                        grouped(metrics.split_offscreen_systems)
                    ),
                    Some(
                        "Split-star sprite records constructed and submitted this frame, and systems skipped because a sphere of their widest separation lies outside the view. Culled systems are neither built nor placed.",
                    ),
                ),
            ],
        );
    }
    if terminal::section(ui, "nearest star") {
        let s = metrics.live(field);
        terminal::facts_with_details(
            ui,
            "debug-nearby",
            &[
                (
                    "lookup time",
                    format!("{} ms per frame", number(s.nearby_ms_per_frame, 2)),
                    Some(
                        "Time finding the nearest star, for automatic arrival and the close-up render scale. Trailing-second average, refreshed every frame.",
                    ),
                ),
                (
                    "checked",
                    format!("{} stars per frame", number(s.nearby_checked_per_frame, 0)),
                    Some("Stars measured by the lookups; each cell's grid keeps this low."),
                ),
                (
                    "cells",
                    format!("{} searched per frame", number(s.nearby_cells_per_frame, 0)),
                    Some("Loaded cells close enough to hold the nearest star."),
                ),
            ],
        );
    }
    if terminal::section(ui, "search") {
        search(ui, searches, out);
    }
    if terminal::section(ui, "gpu passes") {
        gpu_passes(ui, diagnostics, mean);
    }
}

/// Recent object searches' counters, newest first, and the debug settings.
fn search(ui: &mut egui::Ui, searches: &crate::search::Searches, out: &mut Vec<UiCommand>) {
    let recent = searches.recent();
    let labels: Vec<String> = recent
        .iter()
        .map(|e| format!("#{} {:?}", e.id, e.asker).to_lowercase())
        .collect();
    let details: Vec<String> = recent
        .iter()
        .map(|e| {
            let p = &e.progress;
            let mut text = format!(
                "{} · searched out to {} ly · {} stars tried, {} rejected early, {} placed · {} systems tested · {} cells · cache {} MB",
                e.query,
                number(p.complete_to_ly,1),
                si(p.stars as f64),
                si(p.rejected_early as f64),
                si(p.placed as f64),
                grouped(p.evaluated as usize),
                grouped(p.cells as usize),
                number(p.cache_bytes as f64 / 1e6,0),
            );
            if let Some(error) = &e.error {
                text = format!("{} · {error}", e.query);
            }
            text
        })
        .collect();
    let mut rows: Vec<(&str, String, Option<&str>)> = vec![
        (
            "runs on",
            searches.mode().to_owned(),
            Some(
                "A background service sharing the CPU budget with visible-star streaming. See CPU threads for current allocations.",
            ),
        ),
        (
            "work cap",
            format!("{} M units", searches.work_cap / 1_000_000),
            Some(
                "Work a new request may do: stars tried plus 32 per cell, about 0.2 µs each. A request that reaches it stops with \"searched out to\" and exact results within that radius.",
            ),
        ),
    ];
    rows.extend(
        recent
            .iter()
            .zip(&labels)
            .zip(&details)
            .map(|((e, label), detail)| {
                (
                    label.as_str(),
                    format!(
                        "{} · {} results · {} s",
                        e.state(),
                        grouped(e.result_count),
                        number(e.progress.elapsed_ms / 1e3, 2)
                    ),
                    Some(detail.as_str()),
                )
            }),
    );
    terminal::facts_with_details(ui, "debug-search", &rows);
    ui.horizontal_wrapped(|ui| {
        let cap = (searches.work_cap / 1_000_000) as f64;
        let mut set =
            |work_cap_millions: Option<f64>| out.push(UiCommand::SetSearch { work_cap_millions });
        if terminal::button(ui, "cap ÷10") {
            set(Some((cap / 10.0).max(1.0)));
        }
        if terminal::button(ui, "cap ×10") {
            set(Some((cap * 10.0).min(10_000.0)));
        }
    });
    if recent.is_empty() {
        terminal::note(
            ui,
            "No searches yet: type search kind=planet within_ly=20 in the terminal tab, or use ns search.",
        );
    }
}

/// GPU bytes per uploaded star (`stars::render::StarInstance`).
const STAR_BYTES: usize = size_of::<crate::stars::render::StarInstance>();

/// One render pass's smoothed figures, per frame.
#[derive(Default)]
pub(super) struct Pass {
    pub gpu: Option<f64>,
    pub cpu: Option<f64>,
    pub vertices: Option<f64>,
    pub fragments: Option<f64>,
}

/// Each render pass that ran in the last second, by name, from Bevy's render
/// diagnostics. Nested passes are named `outer/inner`.
pub(super) fn pass_times(
    diagnostics: Option<&DiagnosticsStore>,
) -> std::collections::BTreeMap<&str, Pass> {
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
    passes
}

/// GPU time of the top-level passes, which hold the nested ones.
pub(super) fn gpu_total(passes: &std::collections::BTreeMap<&str, Pass>) -> f64 {
    passes
        .iter()
        .filter(|(pass, _)| !pass.contains('/'))
        .filter_map(|(_, p)| p.gpu)
        .sum()
}

/// GPU and render-thread time, and shader invocations, per render pass.
fn gpu_passes(ui: &mut egui::Ui, diagnostics: Option<&DiagnosticsStore>, frame_ms: f32) {
    let passes = pass_times(diagnostics);
    if passes.is_empty() {
        terminal::note(ui, "Waiting for render diagnostics…");
        return;
    }
    let total = gpu_total(&passes);
    terminal::facts_with_details(
        ui,
        "debug-gpu-total",
        &[
            (
                "gpu total",
                format!(
                    "{} ms of a {} ms frame",
                    number(total, 1),
                    number(frame_ms.into(), 1)
                ),
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
    let ms = |v: Option<f64>| v.map_or("·".into(), |v| format!("{} ms", number(v, 2)));
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
            egui::pos2(rect.left() + 8.0, y(ms) - 2.0),
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
