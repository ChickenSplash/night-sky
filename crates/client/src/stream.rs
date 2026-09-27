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

/// Faintest apparent magnitude loaded: the naked-eye limit.
pub const MAGNITUDE: f64 = 6.5;
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
            .insert_resource(Backend::start())
            .add_systems(Update, stream.after(crate::scene::place_camera));
    }
}

fn stream(
    mut backend: ResMut<Backend>,
    mut field: ResMut<Starfield>,
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
    let Some((events, pending)) = backend.exchange(aim) else {
        return;
    };
    if field.pending == 0 && pending > 0 {
        field.started = Some(Instant::now());
    }
    field.apply(events);
    field.pending = pending;
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

    type Batch = (Vec<Event>, usize);

    #[derive(Resource)]
    pub struct Backend {
        aim: mpsc::Sender<[f64; 3]>,
        batches: Mutex<mpsc::Receiver<Batch>>,
    }

    impl Backend {
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
                    if sender.send((events, loader.pending())).is_err() {
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
            for (events, pending) in batches.try_iter() {
                let batch = merged.get_or_insert_with(Default::default);
                batch.0.extend(events);
                batch.1 = pending;
            }
            merged
        }
    }
}

#[cfg(target_arch = "wasm32")]
mod web {
    use super::*;
    use std::time::Duration;

    /// Share of each frame spent loading.
    const SLICE: Duration = Duration::from_millis(4);

    #[derive(Resource)]
    pub struct Backend(Loader);

    impl Backend {
        pub fn start() -> Self {
            Self(Loader::new(Sky::default(), MAGNITUDE))
        }

        pub fn exchange(&mut self, aim: Option<[f64; 3]>) -> Option<(Vec<Event>, usize)> {
            if let Some(camera) = aim {
                self.0.aim(camera);
            } else if self.0.pending() == 0 {
                return None;
            }
            let start = Instant::now();
            let events = self.0.step(&mut || start.elapsed() < SLICE);
            Some((events, self.0.pending()))
        }
    }
}
