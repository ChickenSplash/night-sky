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

mod metrics;

/// Camera moves shorter than this, in parsecs, do not re-aim the loader: about
/// 2,000 AU, so moving within a planetary system never does.
const AIM_PC: f64 = 0.01;

/// Stars loaded around the camera, by cell.
#[derive(Resource, Default)]
pub struct Starfield {
    /// Last resolved ID and point; invalidated whenever loaded cells change.
    pub picked: std::sync::Mutex<Option<(String, StarPoint)>>,
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

pub use metrics::StreamMetrics;
#[cfg(target_arch = "wasm32")]
pub use metrics::StreamSample;

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
            let changed_cell = match &event {
                Event::Loaded(cell) => cell.cell,
                Event::Dropped(cell) => *cell,
            };
            let cached = self
                .picked
                .get_mut()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if cached.as_ref().is_some_and(|(id, _)| {
                space_simulation::cells::parse_cell_star(id)
                    .is_some_and(|(cell, ..)| cell == changed_cell)
            }) {
                *cached = None;
            }
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
        *self
            .picked
            .get_mut()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
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
    (mut metrics, mut split): (ResMut<StreamMetrics>, ResMut<super::starfield::Split>),
    (rig, pose): (Res<CameraRig>, Res<CameraPose>),
    (debug, nav): (
        Res<crate::render::debug::RenderDebug>,
        Res<crate::Navigation>,
    ),
    mut restart: ResMut<StreamRestart>,
) {
    // A new magnitude limit changes which cells and stars load: start over.
    if std::mem::take(&mut restart.0) || backend.magnitude() != nav.magnitude {
        *backend = Backend::start(nav.magnitude);
        field.clear();
        *split = super::starfield::Split::default();
        metrics.split_examined = 0;
        metrics.split_systems = 0;
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
mod native;

#[cfg(target_arch = "wasm32")]
mod web;

#[cfg(test)]
mod tests {
    use super::*;
    use space_simulation::galaxy::Component;

    #[test]
    fn picked_cache_survives_unrelated_changes_but_not_removal_or_restart() {
        let mut field = field();
        field.stars = field.cells.values().map(|cell| cell.stars.len()).sum();
        let (&cell, stars) = field.cells.iter().next().unwrap();
        let star = stars.stars[0];
        let id = star.id(cell);
        *field.picked.lock().unwrap() = Some((id.clone(), star));
        let other = *field.cells.keys().find(|&&other| other != cell).unwrap();
        field.apply(vec![Event::Dropped(other)]);
        assert!(field.picked.lock().unwrap().is_some());
        field.apply(vec![Event::Dropped(cell)]);
        assert!(field.picked.lock().unwrap().is_none());
        *field.picked.lock().unwrap() = Some((id, star));
        field.clear();
        assert!(field.picked.lock().unwrap().is_none());
    }

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
                separation_au: 0.0,
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
