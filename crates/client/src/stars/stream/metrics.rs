//! One-second log reports and continuously refreshed trailing-second UI measurements.
use super::Starfield;
use bevy::{platform::time::Instant, prelude::*};
use space_simulation::galaxy::cells::Cell;
use std::collections::{HashMap, VecDeque};
use std::time::Duration;

const COUNTERS: usize = 18;

#[derive(Clone, Debug, Default, serde::Serialize)]
pub struct SplitFrame {
    pub search_ms: f64,
    pub preparation_ms: f64,
    pub generation_ms: f64,
    pub orbit_sprite_ms: f64,
    pub rebuilt: usize,
    pub cache_clears: usize,
}

/// Where `starfield::sync` spent one frame, and the render world's star work.
#[derive(Clone, Debug, Default, serde::Serialize)]
pub struct SyncFrame {
    /// All of `sync`, including the split work in `SplitFrame`.
    pub total_ms: f64,
    /// Packing stars into batches and queueing them for upload.
    pub upload_ms: f64,
    /// Culling uploaded batches and listing their draws.
    pub draws_ms: f64,
    /// Gathering precise candidates again, on rescans only.
    pub precise_scan_ms: f64,
    pub precise_rescans: usize,
    /// Checking every precise candidate and placing its sprite.
    pub precise_ms: f64,
    pub precise_candidates: usize,
    pub precise_sprites: usize,
    /// Render world, a frame or so behind: copying this frame out of the main
    /// world and creating new batch buffers, then filling and writing the
    /// per-frame buffers (`stars::render`).
    pub render_extract_ms: f64,
    pub render_prepare_ms: f64,
}

/// What measuring and reporting cost, so it can be told apart from the work
/// it measures. Recording and snapshot are the previous frame's.
#[derive(Clone, Debug, Default, serde::Serialize)]
pub struct InstrumentationFrame {
    /// Appending the frame sample and trimming history.
    pub record_ms: f64,
    /// Building the shared snapshot: zero on frames without one (every 250 ms).
    pub snapshot_ms: f64,
    /// Laying out the debug window this frame: zero while it is closed.
    pub debug_window_ms: f64,
}

/// CPU work in the craft update, independent of camera/star rendering.
#[derive(Clone, Debug, Default, serde::Serialize)]
pub struct CraftFrame {
    pub total_ms: f64,
    /// Loaded-system proximity checks and polling background arrival loads.
    pub discovery_ms: f64,
    /// Fixed-step movement, gravity and frame handoffs.
    pub stepping_ms: f64,
    pub steps: usize,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct FrameSample {
    pub sequence: u64,
    pub elapsed_seconds: f64,
    /// Interval since the preceding app update, not GPU duration for this frame.
    pub frame_interval_ms: f64,
    pub split: SplitFrame,
    pub sync: SyncFrame,
    pub instrumentation: InstrumentationFrame,
    pub craft: CraftFrame,
}

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
    pub split_search: Duration,
    pub split_searches: usize,
    pub split_preparation: Duration,
    pub split_rebuilt: usize,
    pub split_cache_clears: usize,
    pub split_checked: usize,
    pub split_sprites: usize,
    pub split_offscreen_systems: usize,
    pub split_cached: usize,
    pub split_candidates: usize,
    pub split_latest: SplitFrame,
    pub sync_latest: SyncFrame,
    pub instrumentation_latest: InstrumentationFrame,
    pub craft_latest: CraftFrame,
    pub history: VecDeque<FrameSample>,
    history_started: Instant,
    sequence: u64,
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
    rolling: VecDeque<(Instant, [f64; COUNTERS])>,
    offset: [f64; COUNTERS],
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
            split_search: Duration::ZERO,
            split_searches: 0,
            split_preparation: Duration::ZERO,
            split_rebuilt: 0,
            split_cache_clears: 0,
            split_checked: 0,
            split_sprites: 0,
            split_offscreen_systems: 0,
            split_cached: 0,
            split_candidates: 0,
            split_latest: SplitFrame::default(),
            sync_latest: SyncFrame::default(),
            instrumentation_latest: InstrumentationFrame::default(),
            craft_latest: CraftFrame::default(),
            history: VecDeque::new(),
            history_started: Instant::now(),
            sequence: 0,
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
            rolling: VecDeque::new(),
            offset: [0.0; COUNTERS],
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
    pub split_search_avg_ms: f64,
    pub split_searches_per_second: f64,
    pub split_search_ms_per_frame: f64,
    pub split_preparation_ms_per_frame: f64,
    pub split_rebuilt_per_frame: f64,
    pub split_cache_clears_per_second: f64,
    pub split_checked_per_second: f64,
    pub frame_p95_ms: f64,
    pub frame_p99_ms: f64,
}

impl StreamMetrics {
    pub fn record_frame(&mut self, frame_interval_ms: f64) {
        self.sequence += 1;
        let elapsed_seconds = self.history_started.elapsed().as_secs_f64();
        self.history.push_back(FrameSample {
            sequence: self.sequence,
            elapsed_seconds,
            frame_interval_ms,
            split: self.split_latest.clone(),
            sync: self.sync_latest.clone(),
            instrumentation: self.instrumentation_latest.clone(),
            craft: self.craft_latest.clone(),
        });
        while self.history.len() > 4096
            || self
                .history
                .front()
                .is_some_and(|f| elapsed_seconds - f.elapsed_seconds > 10.0)
        {
            self.history.pop_front();
        }
    }

    /// Component-wise maxima, not necessarily from the same frame.
    pub fn split_peak(&self) -> SplitFrame {
        let now = self.history_started.elapsed().as_secs_f64();
        self.history
            .iter()
            .rev()
            .take_while(|f| now - f.elapsed_seconds <= 1.0)
            .fold(SplitFrame::default(), |mut peak, frame| {
                let s = &frame.split;
                peak.search_ms = peak.search_ms.max(s.search_ms);
                peak.preparation_ms = peak.preparation_ms.max(s.preparation_ms);
                peak.generation_ms = peak.generation_ms.max(s.generation_ms);
                peak.orbit_sprite_ms = peak.orbit_sprite_ms.max(s.orbit_sprite_ms);
                peak.rebuilt = peak.rebuilt.max(s.rebuilt);
                peak.cache_clears = peak.cache_clears.max(s.cache_clears);
                peak
            })
    }

    /// Component-wise maxima over the trailing second, like `split_peak`.
    pub fn sync_peak(&self) -> SyncFrame {
        self.last_second()
            .fold(SyncFrame::default(), |mut peak, frame| {
                let s = &frame.sync;
                peak.total_ms = peak.total_ms.max(s.total_ms);
                peak.upload_ms = peak.upload_ms.max(s.upload_ms);
                peak.draws_ms = peak.draws_ms.max(s.draws_ms);
                peak.precise_scan_ms = peak.precise_scan_ms.max(s.precise_scan_ms);
                peak.precise_rescans = peak.precise_rescans.max(s.precise_rescans);
                peak.precise_ms = peak.precise_ms.max(s.precise_ms);
                peak.precise_candidates = peak.precise_candidates.max(s.precise_candidates);
                peak.precise_sprites = peak.precise_sprites.max(s.precise_sprites);
                peak.render_extract_ms = peak.render_extract_ms.max(s.render_extract_ms);
                peak.render_prepare_ms = peak.render_prepare_ms.max(s.render_prepare_ms);
                peak
            })
    }

    pub fn craft_peak(&self) -> CraftFrame {
        self.last_second()
            .fold(CraftFrame::default(), |mut peak, frame| {
                let s = &frame.craft;
                peak.total_ms = peak.total_ms.max(s.total_ms);
                peak.discovery_ms = peak.discovery_ms.max(s.discovery_ms);
                peak.stepping_ms = peak.stepping_ms.max(s.stepping_ms);
                peak.steps = peak.steps.max(s.steps);
                peak
            })
    }

    pub fn instrumentation_peak(&self) -> InstrumentationFrame {
        self.last_second()
            .fold(InstrumentationFrame::default(), |mut peak, frame| {
                let s = &frame.instrumentation;
                peak.record_ms = peak.record_ms.max(s.record_ms);
                peak.snapshot_ms = peak.snapshot_ms.max(s.snapshot_ms);
                peak.debug_window_ms = peak.debug_window_ms.max(s.debug_window_ms);
                peak
            })
    }

    fn last_second(&self) -> impl Iterator<Item = &FrameSample> {
        let now = self.history_started.elapsed().as_secs_f64();
        self.history
            .iter()
            .rev()
            .take_while(move |f| now - f.elapsed_seconds <= 1.0)
    }

    pub fn frame_peak_ms(&self) -> f64 {
        let now = self.history_started.elapsed().as_secs_f64();
        self.history
            .iter()
            .rev()
            .take_while(|f| now - f.elapsed_seconds <= 1.0)
            .map(|f| f.frame_interval_ms)
            .fold(0.0, f64::max)
    }

    fn counters(&self) -> [f64; COUNTERS] {
        [
            self.frames.len() as f64,
            self.generation.as_secs_f64() * 1000.0,
            (self.generation + self.worked).as_secs_f64(),
            self.generated as f64,
            self.cancelled as f64,
            self.rendering.as_secs_f64() * 1000.0,
            self.built_batches as f64,
            self.mesh_wait_ms,
            self.mesh_wait_samples as f64,
            self.nearby.as_secs_f64() * 1000.0,
            self.nearby_checked as f64,
            self.nearby_cells as f64,
            self.split_search.as_secs_f64() * 1000.0,
            self.split_searches as f64,
            self.split_preparation.as_secs_f64() * 1000.0,
            self.split_rebuilt as f64,
            self.split_cache_clears as f64,
            self.split_checked as f64,
        ]
    }

    /// Live counts and trailing one-second timings, independent of log reports.
    pub fn live(&self, field: &Starfield) -> StreamSample {
        let now = Instant::now();
        let current = self.counters();
        let (at, baseline) = self.rolling.front().copied().unwrap_or((now, self.offset));
        let delta: [f64; COUNTERS] =
            std::array::from_fn(|i| current[i] + self.offset[i] - baseline[i]);
        let seconds = now.duration_since(at).as_secs_f64().max(0.001);
        let frames = delta[0].max(1.0);
        StreamSample {
            mode: self.mode,
            stars: field.stars,
            cells: field.cells.len(),
            pending: field.pending,
            mesh_chunks: self.mesh_queue,
            batch_draws: self.batch_draws,
            uploaded_stars: self.uploaded_stars,
            generation_ms: delta[1],
            threads: self.threads,
            busy_threads: delta[2] / seconds,
            generated_cells: delta[3] as usize,
            cancelled: delta[4] as usize,
            render_sync_ms: delta[5],
            built_chunks: delta[6] as usize,
            mesh_wait_avg_ms: delta[7] / delta[8].max(1.0),
            mesh_wait_oldest_ms: self.oldest_mesh_ms,
            nearby_ms_per_frame: delta[9] / frames,
            nearby_checked_per_frame: delta[10] / frames,
            nearby_cells_per_frame: delta[11] / frames,
            split_search_avg_ms: delta[12] / delta[13].max(1.0),
            split_searches_per_second: delta[13] / seconds,
            split_search_ms_per_frame: delta[12] / frames,
            split_preparation_ms_per_frame: delta[14] / frames,
            split_rebuilt_per_frame: delta[15] / frames,
            split_cache_clears_per_second: delta[16] / seconds,
            split_checked_per_second: delta[17] / seconds,
            frame_p95_ms: self.sample.as_ref().map_or(0.0, |s| s.frame_p95_ms),
            frame_p99_ms: self.sample.as_ref().map_or(0.0, |s| s.frame_p99_ms),
        }
    }

    pub(super) fn report(&mut self, field: &Starfield) {
        let now = Instant::now();
        let current = self.counters();
        self.rolling
            .push_back((now, std::array::from_fn(|i| current[i] + self.offset[i])));
        while self.rolling.len() > 1
            && now.duration_since(self.rolling[1].0) >= Duration::from_secs(1)
        {
            self.rolling.pop_front();
        }
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
            split_search_avg_ms: self.split_search.as_secs_f64() * 1000.0
                / self.split_searches.max(1) as f64,
            split_searches_per_second: self.split_searches as f64 / elapsed.as_secs_f64(),
            split_search_ms_per_frame: self.split_search.as_secs_f64() * 1000.0 / frames,
            split_preparation_ms_per_frame: self.split_preparation.as_secs_f64() * 1000.0 / frames,
            split_rebuilt_per_frame: self.split_rebuilt as f64 / frames,
            split_cache_clears_per_second: self.split_cache_clears as f64 / elapsed.as_secs_f64(),
            split_checked_per_second: self.split_checked as f64 / elapsed.as_secs_f64(),
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
        for (offset, value) in self.offset.iter_mut().zip(current) {
            *offset += value;
        }
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
        self.split_search = Duration::ZERO;
        self.split_searches = 0;
        self.split_preparation = Duration::ZERO;
        self.split_rebuilt = 0;
        self.split_cache_clears = 0;
        self.split_checked = 0;
        self.frames.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn craft_timings_record_spikes_and_expire_without_stale_values() {
        let mut metrics = StreamMetrics {
            craft_latest: CraftFrame {
                total_ms: 122.0,
                discovery_ms: 120.0,
                stepping_ms: 1.0,
                steps: 2,
            },
            ..Default::default()
        };
        metrics.record_frame(6.0);
        metrics.craft_latest = CraftFrame::default();
        metrics.record_frame(122.0);
        assert_eq!(metrics.craft_peak().discovery_ms, 120.0);
        assert_eq!(metrics.craft_peak().steps, 2);
        let json = serde_json::to_value(metrics.history.front()).unwrap();
        assert_eq!(json["craft"]["discovery_ms"], 120.0);
        assert_eq!(metrics.history.back().unwrap().craft.total_ms, 0.0);
        metrics.history.front_mut().unwrap().elapsed_seconds = -2.0;
        assert_eq!(metrics.craft_peak().total_ms, 0.0);
    }

    #[test]
    fn history_preserves_spikes_and_has_time_and_capacity_bounds() {
        let mut metrics = StreamMetrics::default();
        metrics.split_latest.preparation_ms = 80.0;
        metrics.split_latest.generation_ms = 70.0;
        metrics.split_latest.rebuilt = 4000;
        metrics.record_frame(6.0);
        metrics.split_latest = SplitFrame::default();
        metrics.record_frame(80.0);
        assert_eq!(metrics.split_peak().preparation_ms, 80.0);
        assert_eq!(metrics.split_peak().rebuilt, 4000);
        assert_eq!(metrics.frame_peak_ms(), 80.0);
        assert_eq!(metrics.history.back().unwrap().split.preparation_ms, 0.0);
        metrics.history.front_mut().unwrap().elapsed_seconds = -2.0;
        assert_eq!(metrics.split_peak().preparation_ms, 0.0);
        metrics.history.front_mut().unwrap().elapsed_seconds = -11.0;
        metrics.record_frame(6.0);
        assert_eq!(metrics.history.len(), 2);
        for _ in 0..4100 {
            metrics.record_frame(6.0);
        }
        assert_eq!(metrics.history.len(), 4096);
        assert!(metrics.history.front().unwrap().sequence > 1);
        let json = serde_json::to_value(metrics.history.back()).unwrap();
        assert_eq!(json["split"]["preparation_ms"], 0.0);
    }

    #[test]
    fn split_timings_survive_reports_and_live_counts_stay_current() {
        let mut metrics = StreamMetrics::default();
        let field = Starfield::default();
        metrics
            .rolling
            .push_back((Instant::now() - Duration::from_millis(500), [0.0; COUNTERS]));
        metrics.started = Instant::now() - Duration::from_secs(1);
        metrics.frames.extend([10.0; 10]);
        metrics.split_search = Duration::from_millis(12);
        metrics.split_searches = 2;
        metrics.split_preparation = Duration::from_millis(20);
        metrics.split_rebuilt = 40;
        metrics.split_cache_clears = 1;
        metrics.split_checked = 100;
        metrics.split_sprites = 123;
        metrics.report(&field);
        let report = metrics.sample.as_ref().unwrap();
        assert_eq!(report.split_search_avg_ms, 6.0);
        assert_eq!(report.split_preparation_ms_per_frame, 2.0);
        assert_eq!(metrics.split_searches, 0);
        assert_eq!(metrics.split_sprites, 123);
        metrics.frames.extend([10.0; 10]);
        metrics.split_search = Duration::from_millis(8);
        metrics.split_searches = 2;
        metrics.split_preparation = Duration::from_millis(40);
        metrics.split_rebuilt = 20;
        let live = metrics.live(&field);
        assert_eq!(live.split_search_avg_ms, 5.0);
        assert_eq!(live.split_search_ms_per_frame, 1.0);
        assert_eq!(live.split_preparation_ms_per_frame, 3.0);
        assert_eq!(live.split_rebuilt_per_frame, 3.0);
        assert!(live.split_searches_per_second > 0.0);
        assert!(live.split_checked_per_second > 0.0);
        assert!(live.split_cache_clears_per_second > 0.0);
    }

    #[test]
    fn live_counts_do_not_wait_for_a_report() {
        let metrics = StreamMetrics::default();
        let field = Starfield {
            stars: 123,
            pending: 7,
            ..Default::default()
        };
        let live = metrics.live(&field);
        assert_eq!(live.stars, 123);
        assert_eq!(live.pending, 7);
        assert!(metrics.sample.is_none());
    }

    #[test]
    fn rolling_timings_survive_report_counter_resets() {
        let mut metrics = StreamMetrics::default();
        let field = Starfield::default();
        metrics
            .rolling
            .push_back((Instant::now() - Duration::from_millis(500), [0.0; COUNTERS]));
        metrics.started = Instant::now() - Duration::from_secs(1);
        metrics.frames.extend([10.0; 10]);
        metrics.nearby = Duration::from_millis(20);
        metrics.nearby_checked = 100;
        metrics.nearby_cells = 30;
        metrics.report(&field);
        assert_eq!(metrics.reports, 1);
        assert!(metrics.frames.is_empty());
        metrics.frames.extend([10.0; 10]);
        metrics.nearby = Duration::from_millis(40);
        metrics.nearby_checked = 200;
        metrics.nearby_cells = 50;
        let live = metrics.live(&field);
        assert_eq!(live.nearby_ms_per_frame, 3.0);
        assert_eq!(live.nearby_checked_per_frame, 15.0);
        assert_eq!(live.nearby_cells_per_frame, 4.0);
    }

    #[test]
    fn rolling_window_drops_old_measurements() {
        let mut metrics = StreamMetrics::default();
        let field = Starfield::default();
        let now = Instant::now();
        metrics
            .rolling
            .push_back((now - Duration::from_secs(2), [0.0; COUNTERS]));
        let mut baseline = [0.0; COUNTERS];
        baseline[0] = 10.0;
        baseline[9] = 20.0;
        metrics
            .rolling
            .push_back((now - Duration::from_millis(1100), baseline));
        metrics.frames.extend([10.0; 20]);
        metrics.nearby = Duration::from_millis(60);
        metrics.report(&field);
        assert_eq!(metrics.rolling.len(), 2);
        assert_eq!(metrics.live(&field).nearby_ms_per_frame, 4.0);
    }
}
