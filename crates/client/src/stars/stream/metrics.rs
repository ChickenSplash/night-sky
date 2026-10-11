//! One-second star-stream measurements, shown in the debug window's perf tab.
use super::Starfield;
use bevy::{platform::time::Instant, prelude::*};
use space_simulation::cells::Cell;
use std::collections::HashMap;
use std::time::Duration;

/// One-second measurements of generation and mesh preparation. The render
/// queue is measured separately from the loader's pending-cell queue.
#[derive(Resource)]
pub struct StreamMetrics {
    pub caption: String,
    /// The last one-second report, as the caption shows it.
    pub sample: Option<StreamSample>,
    /// Reports so far, so readers can tell a new sample from the last one.
    pub reports: u64,
    pub(super) mode: &'static str,
    pub loaded_at: HashMap<Cell, Instant>,
    pub mesh_queue: usize,
    pub oldest_mesh_ms: f64,
    /// Star batches drawn this frame, one draw call each, of `batches`.
    pub batch_draws: usize,
    pub batches: usize,
    /// Grid blocks with uploaded batches, and those drawn this frame.
    pub blocks: usize,
    pub blocks_drawn: usize,
    /// Stars in the batches drawn this frame.
    pub drawn_stars: usize,
    /// Stars in uploaded batches, after the CPU magnitude filter.
    pub uploaded_stars: usize,
    pub precise_sprites: usize,
    /// Multiple systems drawn star by star, and stars examined finding them
    /// at the last gather.
    pub split_systems: usize,
    pub split_examined: usize,
    pub built_batches: usize,
    pub mesh_wait_ms: f64,
    pub mesh_wait_samples: usize,
    /// `Starfield::nearest` lookups: time, stars measured and cells searched.
    pub nearby: Duration,
    pub nearby_checked: usize,
    pub nearby_cells: usize,
    pub generated: usize,
    pub cancelled: usize,
    pub generation: Duration,
    /// Time the loader's other threads worked, besides `generation`.
    pub worked: Duration,
    pub(super) threads: usize,
    pub rendering: Duration,
    pub frames: Vec<f64>,
    started: Instant,
}

impl Default for StreamMetrics {
    fn default() -> Self {
        Self {
            caption: "STAR STREAM / MEASURING…".into(),
            sample: None,
            reports: 0,
            mode: "",
            loaded_at: HashMap::new(),
            mesh_queue: 0,
            oldest_mesh_ms: 0.0,
            batch_draws: 0,
            batches: 0,
            blocks: 0,
            blocks_drawn: 0,
            drawn_stars: 0,
            uploaded_stars: 0,
            precise_sprites: 0,
            split_systems: 0,
            split_examined: 0,
            built_batches: 0,
            mesh_wait_ms: 0.0,
            mesh_wait_samples: 0,
            nearby: Duration::ZERO,
            nearby_checked: 0,
            nearby_cells: 0,
            generated: 0,
            cancelled: 0,
            generation: Duration::ZERO,
            worked: Duration::ZERO,
            threads: 1,
            rendering: Duration::ZERO,
            frames: Vec::with_capacity(120),
            started: Instant::now(),
        }
    }
}

/// One second of star-stream measurements.
#[derive(Clone, Debug, serde::Serialize)]
pub struct StreamSample {
    pub mode: &'static str,
    pub stars: usize,
    pub cells: usize,
    pub pending: usize,
    pub mesh_chunks: usize,
    pub batch_draws: usize,
    pub uploaded_stars: usize,
    pub generation_ms: f64,
    /// Threads the loader makes stars on, and how many were busy on average.
    pub threads: usize,
    pub busy_threads: f64,
    pub generated_cells: usize,
    pub cancelled: usize,
    pub render_sync_ms: f64,
    pub built_chunks: usize,
    pub mesh_wait_avg_ms: f64,
    pub mesh_wait_oldest_ms: f64,
    /// Nearest-star lookups per frame, on average.
    pub nearby_ms_per_frame: f64,
    pub nearby_checked_per_frame: f64,
    pub nearby_cells_per_frame: f64,
    pub frame_p95_ms: f64,
    pub frame_p99_ms: f64,
}

impl StreamMetrics {
    pub(super) fn report(&mut self, field: &Starfield) {
        let elapsed = self.started.elapsed();
        if elapsed < Duration::from_secs(1) {
            return;
        }
        let frames = self.frames.len().max(1) as f64;
        self.frames.sort_by(f64::total_cmp);
        let percentile = |p: usize| {
            self.frames
                .get((self.frames.len().saturating_sub(1) * p) / 100)
                .copied()
                .unwrap_or(0.0)
        };
        let s = StreamSample {
            mode: self.mode,
            stars: field.stars,
            cells: field.cells.len(),
            pending: field.pending,
            mesh_chunks: self.mesh_queue,
            batch_draws: self.batch_draws,
            uploaded_stars: self.uploaded_stars,
            generation_ms: self.generation.as_secs_f64() * 1000.0,
            threads: self.threads,
            busy_threads: (self.generation + self.worked).as_secs_f64() / elapsed.as_secs_f64(),
            generated_cells: self.generated,
            cancelled: self.cancelled,
            render_sync_ms: self.rendering.as_secs_f64() * 1000.0,
            built_chunks: self.built_batches,
            mesh_wait_avg_ms: self.mesh_wait_ms / self.mesh_wait_samples.max(1) as f64,
            mesh_wait_oldest_ms: self.oldest_mesh_ms,
            nearby_ms_per_frame: self.nearby.as_secs_f64() * 1000.0 / frames,
            nearby_checked_per_frame: self.nearby_checked as f64 / frames,
            nearby_cells_per_frame: self.nearby_cells as f64 / frames,
            frame_p95_ms: percentile(95),
            frame_p99_ms: percentile(99),
        };
        self.caption = format!(
            "STAR STREAM [{}] {} stars / {} cells  ·  {} pending  ·  {} mesh chunks\nGEN {:.0} ms / {} cells  ·  {} cancelled  ·  RENDER SYNC {:.0} ms / {} chunks\nMESH WAIT {:.0} ms avg / {:.0} ms oldest  ·  FRAME P95 {:.1} / P99 {:.1} ms",
            s.mode,
            s.stars,
            s.cells,
            s.pending,
            s.mesh_chunks,
            s.generation_ms,
            s.generated_cells,
            s.cancelled,
            s.render_sync_ms,
            s.built_chunks,
            s.mesh_wait_avg_ms,
            s.mesh_wait_oldest_ms,
            s.frame_p95_ms,
            s.frame_p99_ms,
        );
        self.sample = Some(s);
        self.reports += 1;
        self.started = Instant::now();
        self.generated = 0;
        self.cancelled = 0;
        self.generation = Duration::ZERO;
        self.worked = Duration::ZERO;
        self.rendering = Duration::ZERO;
        self.built_batches = 0;
        self.mesh_wait_ms = 0.0;
        self.mesh_wait_samples = 0;
        self.nearby = Duration::ZERO;
        self.nearby_checked = 0;
        self.nearby_cells = 0;
        self.frames.clear();
    }
}
