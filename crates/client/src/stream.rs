//! Streams the galaxy's stars around the camera: on a worker thread natively,
//! in a slice of each frame in the browser.

use crate::scene::{CameraPose, CameraRig};
use bevy::{
    log::{debug, info, warn},
    platform::time::Instant,
    prelude::*,
};
use space_simulation::{
    LY_METRES,
    cells::{Cell, Sky},
    galaxy::{GalaxyModel, PC_METRES},
    stream::{Event, Loader, StarCell},
};
use std::collections::{HashMap, VecDeque};
use std::time::Duration;

/// Faintest apparent magnitude loaded: the naked-eye limit.
pub const MAGNITUDE: f64 = space_simulation::population::VISIBLE_MAGNITUDE;
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
    pub built_batches: usize,
    pub mesh_wait_ms: f64,
    pub mesh_wait_samples: usize,
    pub generated: usize,
    pub cancelled: usize,
    pub generation: Duration,
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
            built_batches: 0,
            mesh_wait_ms: 0.0,
            mesh_wait_samples: 0,
            generated: 0,
            cancelled: 0,
            generation: Duration::ZERO,
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
    pub generation_ms: f64,
    pub generated_cells: usize,
    pub cancelled: usize,
    pub render_sync_ms: f64,
    pub built_chunks: usize,
    pub mesh_wait_avg_ms: f64,
    pub mesh_wait_oldest_ms: f64,
    pub frame_p95_ms: f64,
    pub frame_p99_ms: f64,
}

impl StreamMetrics {
    fn report(&mut self, field: &Starfield) {
        let elapsed = self.started.elapsed();
        if elapsed < Duration::from_secs(1) {
            return;
        }
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
            generation_ms: self.generation.as_secs_f64() * 1000.0,
            generated_cells: self.generated,
            cancelled: self.cancelled,
            render_sync_ms: self.rendering.as_secs_f64() * 1000.0,
            built_chunks: self.built_batches,
            mesh_wait_avg_ms: self.mesh_wait_ms / self.mesh_wait_samples.max(1) as f64,
            mesh_wait_oldest_ms: self.oldest_mesh_ms,
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
        self.rendering = Duration::ZERO;
        self.built_batches = 0;
        self.mesh_wait_ms = 0.0;
        self.mesh_wait_samples = 0;
        self.frames.clear();
    }
}

impl Starfield {
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
            .insert_resource(Backend::start())
            .add_systems(Update, stream.after(crate::scene::place_camera));
    }
}

fn stream(
    time: Res<Time>,
    mut backend: ResMut<Backend>,
    mut field: ResMut<Starfield>,
    mut metrics: ResMut<StreamMetrics>,
    rig: Res<CameraRig>,
    pose: Res<CameraPose>,
) {
    let camera = camera_pc(&GalaxyModel::milky_way(), &rig, &pose);
    let moved = field.aimed_pc.is_none_or(|aimed| {
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
    use std::sync::{Mutex, mpsc};
    use std::time::Duration;

    /// Work between checks for a new camera position.
    const SLICE: Duration = Duration::from_millis(20);

    type Batch = (Vec<Event>, usize, Duration, usize);

    #[derive(Resource)]
    pub struct Backend {
        aim: mpsc::Sender<[f64; 3]>,
        batches: Mutex<mpsc::Receiver<Batch>>,
    }

    impl Backend {
        pub fn mode(&self) -> &'static str {
            "NATIVE THREAD"
        }

        pub fn start() -> Self {
            let (aim, aims) = mpsc::channel();
            let (sender, batches) = mpsc::channel();
            let work = move || {
                let mut loader = Loader::new(Sky::default(), MAGNITUDE);
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
                aim,
                batches: Mutex::new(batches),
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

        pub fn start() -> Self {
            let options = WorkerOptions::new();
            options.set_type(WorkerType::Module);
            let Ok(worker) = Worker::new_with_options("star-worker.js", &options) else {
                warn!("Cannot start star worker; using the frame-sliced loader");
                return Self::Inline(Loader::new(Sky::default(), MAGNITUDE));
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
                batches: Mutex::new(receiver),
                failed,
                next_aim: None,
                camera: None,
                awaiting_response: false,
            }
        }

        pub fn exchange(&mut self, aim: Option<[f64; 3]>) -> Option<Batch> {
            if let Self::Worker { failed, camera, .. } = self {
                if failed.load(Ordering::Relaxed) {
                    warn!("Falling back to frame-sliced galaxy loading");
                    WORKER.with_borrow_mut(|slot| {
                        if let Some((worker, ..)) = slot.take() {
                            worker.terminate();
                        }
                    });
                    let mut loader = Loader::new(Sky::default(), MAGNITUDE);
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
