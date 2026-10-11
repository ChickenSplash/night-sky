//! Streams the galaxy's stars around the camera: on a worker thread natively,
//! in a slice of each frame in the browser.

use crate::scene::{CameraPose, CameraRig};
use bevy::{
    log::{debug, info, warn},
    math::DVec3,
    platform::time::Instant,
    prelude::*,
};
use space_simulation::{
    LY_METRES,
    cells::{Cell, Sky},
    galaxy::{GalaxyModel, PC_METRES},
    stream::{Event, Loader, StarCell, StarPoint, gap_pc},
};
use std::collections::{HashMap, VecDeque};
use std::time::Duration;

/// Camera moves shorter than this, in parsecs, do not re-aim the loader: about
/// 2,000 AU, so moving within a planetary system never does.
const AIM_PC: f64 = 0.01;

/// Stars loaded around the camera, by cell.
#[derive(Resource, Default)]
pub struct Starfield {
    pub cells: HashMap<Cell, StarCell>,
    /// Newly loaded cells awaiting upload to the point-cloud renderer.
    pub added: VecDeque<Cell>,
    /// Cells whose GPU batches should be removed.
    pub removed: VecDeque<Cell>,
    pub stars: usize,
    /// Cells wanted and not loaded yet.
    pub pending: usize,
    /// Where the loader was last aimed, in parsecs from the galactic centre.
    pub aimed_pc: Option<[f64; 3]>,
    started: Option<Instant>,
    loaded_once: bool,
}

/// Set to drop every loaded star and start a new, empty loader, as at startup.
#[derive(Resource, Default)]
pub struct StreamRestart(pub bool);

/// One-second measurements of generation and mesh preparation. The render
/// queue is measured separately from the loader's pending-cell queue.
#[derive(Resource)]
pub struct StreamMetrics {
    pub caption: String,
    /// The last one-second report, as the caption shows it.
    pub sample: Option<StreamSample>,
    /// Reports so far, so readers can tell a new sample from the last one.
    pub reports: u64,
    mode: &'static str,
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
    threads: usize,
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
    fn report(&mut self, field: &Starfield) {
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

impl Starfield {
    /// The loaded star nearest `eye_pc` and closer than `cap_pc`: its
    /// distance in parsecs, cell and point. Equal distances resolve by ID.
    /// Each cell's grid searches only the boxes around the eye, so this costs
    /// the same at the galactic centre as anywhere else.
    pub fn nearest(
        &self,
        eye_pc: DVec3,
        cap_pc: f64,
        metrics: &mut StreamMetrics,
    ) -> Option<(f64, Cell, StarPoint)> {
        let start = Instant::now();
        let eye = eye_pc.to_array();
        let mut best: Option<(f64, Cell, StarPoint)> = None;
        for (&cell, stars) in &self.cells {
            let limit = best.map_or(cap_pc, |(distance, ..)| distance);
            if gap_pc(cell, eye) > limit {
                continue;
            }
            metrics.nearby_cells += 1;
            // A star exactly at the best distance so far may still win on ID.
            let Some((distance, star)) =
                stars.nearest(eye, limit.next_up(), &mut metrics.nearby_checked)
            else {
                continue;
            };
            if distance >= cap_pc {
                continue;
            }
            if best.is_none_or(|(old, old_cell, old_star)| {
                distance < old || (distance == old && star.id(cell) < old_star.id(old_cell))
            }) {
                best = Some((distance, cell, star));
            }
        }
        metrics.nearby += start.elapsed();
        best
    }

    fn apply(&mut self, events: Vec<Event>) {
        for event in events {
            match event {
                Event::Loaded(cell) => {
                    self.stars += cell.stars.len();
                    self.added.push_back(cell.cell);
                    if let Some(old) = self.cells.insert(cell.cell, cell) {
                        warn!("Galaxy cell {:x} loaded twice", old.cell.0);
                        self.stars -= old.stars.len();
                    }
                }
                Event::Dropped(cell) => {
                    self.removed.push_back(cell);
                    if let Some(old) = self.cells.remove(&cell) {
                        self.stars -= old.stars.len();
                    }
                }
            }
        }
    }

    /// Drops every loaded star, as before the first load.
    fn clear(&mut self) {
        self.removed
            .extend(self.cells.drain().map(|(cell, _)| cell));
        self.added.clear();
        self.stars = 0;
        self.pending = 0;
        self.aimed_pc = None;
        self.started = None;
    }

    pub fn summary(&self) -> String {
        format!(
            "{} stars in {} cells, {} cells pending, aimed at {:?} pc",
            self.stars,
            self.cells.len(),
            self.pending,
            self.aimed_pc
        )
    }
}

/// The camera in parsecs from the galactic centre. Lists sit around home until
/// they move onto galactic stars.
fn camera_pc(model: &GalaxyModel, rig: &CameraRig, pose: &CameraPose) -> [f64; 3] {
    let ly = rig.galactic_ly(pose);
    let home = model.home_pc;
    std::array::from_fn(|axis| home[axis] + ly[axis] * LY_METRES / PC_METRES)
}

pub struct StreamPlugin;

impl Plugin for StreamPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Starfield>()
            .init_resource::<StreamMetrics>()
            .init_resource::<StreamRestart>()
            .insert_resource(Backend::start(
                space_simulation::population::VISIBLE_MAGNITUDE,
            ))
            .add_systems(Update, stream.after(crate::scene::place_camera));
    }
}

fn stream(
    time: Res<Time>,
    mut backend: ResMut<Backend>,
    mut field: ResMut<Starfield>,
    mut metrics: ResMut<StreamMetrics>,
    (rig, pose): (Res<CameraRig>, Res<CameraPose>),
    (debug, nav): (
        Res<crate::render_debug::RenderDebug>,
        Res<crate::Navigation>,
    ),
    mut restart: ResMut<StreamRestart>,
) {
    // A new magnitude limit changes which cells and stars load: start over.
    if std::mem::take(&mut restart.0) || backend.magnitude() != nav.magnitude {
        *backend = Backend::start(nav.magnitude);
        field.clear();
        metrics.loaded_at.clear();
    }
    let camera = camera_pc(&GalaxyModel::milky_way(), &rig, &pose);
    // Frozen, the loader finishes its current aim and nothing else.
    let moved = !debug.freeze_stream
        && field.aimed_pc.is_none_or(|aimed| {
            (0..3)
                .map(|axis| (camera[axis] - aimed[axis]).powi(2))
                .sum::<f64>()
                > AIM_PC * AIM_PC
        });
    let aim = moved.then(|| {
        field.aimed_pc = Some(camera);
        camera
    });
    metrics.frames.push(time.delta_secs_f64() * 1000.0);
    metrics.mode = backend.mode();
    metrics.threads = backend.threads();
    metrics.worked += backend.take_worked();
    let Some((events, pending, generation, cancelled)) = backend.exchange(aim) else {
        metrics.report(&field);
        return;
    };
    metrics.generation += generation;
    metrics.cancelled += cancelled;
    for event in &events {
        match event {
            Event::Loaded(cell) => {
                metrics.generated += 1;
                metrics.loaded_at.insert(cell.cell, Instant::now());
            }
            Event::Dropped(cell) => {
                metrics.loaded_at.remove(cell);
            }
        }
    }
    if field.pending == 0 && pending > 0 {
        field.started = Some(Instant::now());
    }
    field.apply(events);
    field.pending = pending;
    metrics.report(&field);
    if pending == 0
        && let Some(started) = field.started.take()
    {
        let seconds = started.elapsed().as_secs_f64();
        if field.loaded_once {
            debug!(
                "Galaxy stars reloaded in {seconds:.2} s: {}",
                field.summary()
            );
        } else {
            info!("Galaxy stars loaded in {seconds:.2} s: {}", field.summary());
        }
        field.loaded_once = true;
    }
}

#[cfg(not(target_arch = "wasm32"))]
use native::Backend;
#[cfg(target_arch = "wasm32")]
use web::Backend;

#[cfg(not(target_arch = "wasm32"))]
mod native {
    use super::*;
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
        mpsc,
    };
    use std::time::Duration;

    /// Work between checks for a new camera position.
    const SLICE: Duration = Duration::from_millis(20);
    /// Threads making each cell's stars, the stream thread included: three
    /// quarters of the machine's, leaving the rest for the app.
    fn star_threads() -> usize {
        let available = std::thread::available_parallelism().map_or(1, |n| n.get());
        (available * 3 / 4).max(1)
    }

    type Batch = (Vec<Event>, usize, Duration, usize);

    #[derive(Resource)]
    pub struct Backend {
        magnitude: f64,
        aim: mpsc::Sender<[f64; 3]>,
        batches: Mutex<mpsc::Receiver<Batch>>,
        threads: usize,
        /// Nanoseconds the loader's other threads have worked, not yet taken.
        worked: Arc<AtomicU64>,
    }

    impl Backend {
        pub fn mode(&self) -> &'static str {
            "NATIVE THREAD"
        }

        pub fn magnitude(&self) -> f64 {
            self.magnitude
        }

        pub fn threads(&self) -> usize {
            self.threads
        }

        pub fn take_worked(&self) -> Duration {
            Duration::from_nanos(self.worked.swap(0, Ordering::Relaxed))
        }

        pub fn start(magnitude: f64) -> Self {
            let (aim, aims) = mpsc::channel();
            let (sender, batches) = mpsc::channel();
            let threads = star_threads();
            bevy::log::info!("Making stars on {threads} threads");
            let worked = Arc::new(AtomicU64::new(0));
            let tally = worked.clone();
            let work = move || {
                let mut loader = Loader::new(Sky::default(), magnitude).with_threads(threads);
                loop {
                    // Idle until the camera moves.
                    let mut latest = None;
                    if loader.pending() == 0 {
                        match aims.recv() {
                            Ok(camera) => latest = Some(camera),
                            Err(_) => return,
                        }
                    }
                    latest = aims.try_iter().last().or(latest);
                    if let Some(camera) = latest {
                        loader.aim(camera);
                    }
                    let start = std::time::Instant::now();
                    let events = loader.step(&mut || start.elapsed() < SLICE);
                    let other = loader.take_worked().as_nanos() as u64;
                    tally.fetch_add(other, Ordering::Relaxed);
                    if sender
                        .send((
                            events,
                            loader.pending(),
                            start.elapsed(),
                            loader.take_cancelled_jobs(),
                        ))
                        .is_err()
                    {
                        return;
                    }
                }
            };
            if let Err(err) = std::thread::Builder::new()
                .name("galaxy-stream".into())
                .spawn(work)
            {
                bevy::log::error!("Cannot start the galaxy stream: {err}");
            }
            Self {
                magnitude,
                aim,
                batches: Mutex::new(batches),
                threads,
                worked,
            }
        }

        /// Sends a new camera position, and gathers what the worker has built.
        pub fn exchange(&mut self, aim: Option<[f64; 3]>) -> Option<Batch> {
            if let Some(camera) = aim {
                let _ = self.aim.send(camera);
            }
            let batches = self.batches.get_mut().ok()?;
            let mut merged: Option<Batch> = None;
            for (events, pending, duration, cancelled) in batches.try_iter() {
                let batch = merged.get_or_insert_with(Default::default);
                batch.0.extend(events);
                batch.1 = pending;
                batch.2 += duration;
                batch.3 += cancelled;
            }
            merged
        }
    }
}

#[cfg(target_arch = "wasm32")]
mod web {
    use super::*;
    use std::{
        cell::RefCell,
        sync::{
            Arc, Mutex,
            atomic::{AtomicBool, Ordering},
            mpsc,
        },
    };
    use wasm_bindgen::{JsCast, closure::Closure};
    use web_sys::{ErrorEvent, MessageEvent, Worker, WorkerOptions, WorkerType};

    /// Share of each frame spent loading.
    const SLICE: Duration = Duration::from_millis(4);

    thread_local! {
        static WORKER: RefCell<Option<(Worker, Closure<dyn FnMut(MessageEvent)>, Closure<dyn FnMut(ErrorEvent)>)>> = const { RefCell::new(None) };
    }

    #[derive(Resource)]
    pub enum Backend {
        Worker {
            magnitude: f64,
            batches: Mutex<mpsc::Receiver<Batch>>,
            failed: Arc<AtomicBool>,
            next_aim: Option<[f64; 3]>,
            camera: Option<[f64; 3]>,
            awaiting_response: bool,
        },
        Inline(Loader),
    }

    type Batch = (Vec<Event>, usize, Duration, usize);

    impl Backend {
        pub fn mode(&self) -> &'static str {
            match self {
                Self::Worker { .. } => "WEB WORKER",
                Self::Inline(_) => "FRAME SLICE",
            }
        }

        pub fn magnitude(&self) -> f64 {
            match self {
                Self::Worker { magnitude, .. } => *magnitude,
                Self::Inline(loader) => loader.magnitude(),
            }
        }

        /// One: the browser makes stars on one worker, or in frame slices.
        pub fn threads(&self) -> usize {
            1
        }

        pub fn take_worked(&self) -> Duration {
            Duration::ZERO
        }

        pub fn start(magnitude: f64) -> Self {
            // A restart replaces the worker; the old one would keep loading.
            WORKER.with_borrow_mut(|slot| {
                if let Some((worker, ..)) = slot.take() {
                    worker.terminate();
                }
            });
            let options = WorkerOptions::new();
            options.set_type(WorkerType::Module);
            let url = format!("star-worker.js?magnitude={magnitude}");
            let Ok(worker) = Worker::new_with_options(&url, &options) else {
                warn!("Cannot start star worker; using the frame-sliced loader");
                return Self::Inline(Loader::new(Sky::default(), magnitude));
            };
            let (sender, receiver) = mpsc::channel();
            let failed = Arc::new(AtomicBool::new(false));
            let invalid = failed.clone();
            let onmessage = Closure::<dyn FnMut(MessageEvent)>::new(move |event: MessageEvent| {
                let Ok(buffer) = event.data().dyn_into::<js_sys::ArrayBuffer>() else {
                    warn!("Star worker returned a non-binary message");
                    invalid.store(true, Ordering::Relaxed);
                    return;
                };
                let bytes = js_sys::Uint8Array::new(&buffer).to_vec();
                if let Some(batch) = space_simulation::stream_wire::decode(&bytes) {
                    let _ = sender.send(batch);
                } else {
                    warn!("Star worker returned an invalid batch");
                    invalid.store(true, Ordering::Relaxed);
                }
            });
            let error = failed.clone();
            let onerror = Closure::<dyn FnMut(ErrorEvent)>::new(move |event: ErrorEvent| {
                bevy::log::error!("Star worker failed: {}", event.message());
                error.store(true, Ordering::Relaxed);
            });
            worker.set_onmessage(Some(onmessage.as_ref().unchecked_ref()));
            worker.set_onerror(Some(onerror.as_ref().unchecked_ref()));
            WORKER.with_borrow_mut(|slot| *slot = Some((worker, onmessage, onerror)));
            Self::Worker {
                magnitude,
                batches: Mutex::new(receiver),
                failed,
                next_aim: None,
                camera: None,
                awaiting_response: false,
            }
        }

        pub fn exchange(&mut self, aim: Option<[f64; 3]>) -> Option<Batch> {
            if let Self::Worker {
                failed,
                camera,
                magnitude,
                ..
            } = self
            {
                if failed.load(Ordering::Relaxed) {
                    warn!("Falling back to frame-sliced galaxy loading");
                    WORKER.with_borrow_mut(|slot| {
                        if let Some((worker, ..)) = slot.take() {
                            worker.terminate();
                        }
                    });
                    let mut loader = Loader::new(Sky::default(), *magnitude);
                    if let Some(camera) = *camera {
                        loader.aim(camera);
                    }
                    *self = Self::Inline(loader);
                }
            }
            match self {
                Self::Worker {
                    batches,
                    failed,
                    next_aim,
                    camera,
                    awaiting_response,
                    ..
                } => {
                    // Keep only the latest camera position while the worker is
                    // busy, as the native channel's try_iter().last() does.
                    if let Some(position) = aim {
                        *camera = Some(position);
                        *next_aim = Some(position);
                    }
                    let batches = batches.get_mut().ok()?;
                    let mut merged: Option<Batch> = None;
                    // Avoid draining an unlimited number of messages in a frame.
                    let start = Instant::now();
                    for (events, pending, duration, cancelled) in batches.try_iter() {
                        let batch = merged.get_or_insert_with(Default::default);
                        batch.0.extend(events);
                        batch.1 = pending;
                        batch.2 += duration;
                        batch.3 += cancelled;
                        if start.elapsed() >= Duration::from_millis(2) {
                            break;
                        }
                    }
                    if merged.is_some() {
                        *awaiting_response = false;
                    }
                    if !*awaiting_response && let Some(camera) = next_aim.take() {
                        let array = js_sys::Array::new();
                        for value in camera {
                            array.push(&value.into());
                        }
                        WORKER.with_borrow(|slot| {
                            if let Some((worker, ..)) = &*slot {
                                if let Err(err) = worker.post_message(&array) {
                                    warn!("Cannot aim star worker: {err:?}");
                                    failed.store(true, Ordering::Relaxed);
                                }
                            }
                        });
                        *awaiting_response = true;
                    }
                    merged
                }
                Self::Inline(loader) => {
                    if let Some(camera) = aim {
                        loader.aim(camera);
                    } else if loader.pending() == 0 {
                        return None;
                    }
                    let start = Instant::now();
                    let events = loader.step(&mut || start.elapsed() < SLICE);
                    Some((
                        events,
                        loader.pending(),
                        start.elapsed(),
                        loader.take_cancelled_jobs(),
                    ))
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use space_simulation::galaxy::Component;

    /// Scattered stars in cells of two levels, by a fixed linear congruence.
    fn field() -> Starfield {
        let home = DVec3::from_array(GalaxyModel::milky_way().home_pc);
        let mut seed = 12345_u64;
        let mut next = || {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (seed >> 11) as f64 / (1u64 << 53) as f64 * 2.0 - 1.0
        };
        let mut stars: HashMap<Cell, Vec<StarPoint>> = HashMap::new();
        for index in 0..2000_u32 {
            let position = home + DVec3::new(next(), next(), next()) * 30.0;
            let cell = Cell::containing(8 + (index % 2) as usize, position.to_array()).unwrap();
            stars.entry(cell).or_default().push(StarPoint {
                position_pc: position.to_array(),
                luminosity_solar: 1.0,
                temperature_k: 5772.0,
                radius_solar: 1.0,
                component: Component::ThinDisc,
                index,
            });
        }
        Starfield {
            cells: stars
                .into_iter()
                .map(|(cell, stars)| (cell, StarCell::new(cell, stars)))
                .collect(),
            ..default()
        }
    }

    #[test]
    fn nearest_star_matches_a_full_scan_across_cells() {
        let field = field();
        let mut metrics = StreamMetrics::default();
        let home = DVec3::from_array(GalaxyModel::milky_way().home_pc);
        for step in 0..200 {
            let eye = home + DVec3::new(0.37, -0.11, 0.23) * step as f64 * 0.4;
            for cap in [6.0, 0.8] {
                let expected = field
                    .cells
                    .values()
                    .flat_map(|cell| &cell.stars)
                    .map(|star| DVec3::from_array(star.position_pc).distance(eye))
                    .filter(|&distance| distance < cap)
                    .min_by(f64::total_cmp);
                let found = field
                    .nearest(eye, cap, &mut metrics)
                    .map(|(distance, ..)| distance);
                assert_eq!(found, expected, "step {step}, cap {cap}");
            }
        }
    }
}
