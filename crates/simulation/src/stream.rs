//! Loading the galaxy's cells around a camera, a little at a time.
//!
//! - For each brightness tier, the cells of its level within its reach
//!   (`reach_pc`) of the camera hold every star of that tier bright enough to
//!   see. About three cells per axis per tier, whatever the magnitude limit.
//! - `Loader` builds wanted cells in small steps, most visible first, and stops
//!   whenever its caller runs out of time: a worker thread natively, a slice of
//!   each frame in the browser. Loaded cells are dropped once the camera is well
//!   beyond their reach.

use crate::cells::{Cell, ROOT_PC, Sky, cell_star_id};
use crate::galaxy::Component;
use crate::population::{TIERS, reach_pc};
use std::collections::HashSet;

/// Loaded cells are kept until the camera is this many times their reach away,
/// so moving back and forth across a boundary does not reload them.
const KEEP: f64 = 1.25;
/// Cached cells kept before those no wanted cell needs are dropped.
const CACHE: usize = 20_000;

/// A cell star, compact enough to hold millions. `id` rebuilds the rest.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StarPoint {
    /// Parsecs from the central black hole.
    pub position_pc: [f64; 3],
    pub luminosity_solar: f32,
    pub temperature_k: f32,
    pub radius_solar: f32,
    pub component: Component,
    pub index: u32,
}

impl StarPoint {
    pub fn id(&self, cell: Cell) -> String {
        cell_star_id(cell, self.component, u64::from(self.index))
    }
}

/// Stars a grid box holds on average, at most: fewer boxes for sparse cells.
const STARS_PER_BOX: f64 = 8.0;
/// Smallest grid box edge, in parsecs, so a search out to a few parsecs
/// visits a bounded number of boxes.
const MIN_BOX_PC: f64 = 0.5;

/// A cell's own stars: those of the tier matching its level. Build it with
/// `StarCell::new`, which sorts the stars into a grid of boxes so the stars
/// near a point are found without scanning the whole cell.
#[derive(Clone, Debug, PartialEq)]
pub struct StarCell {
    pub cell: Cell,
    /// Sorted by grid box. Changing them means building a new `StarCell`.
    pub stars: Vec<StarPoint>,
    grid: StarGrid,
}

/// A uniform grid over a cell: box `b` holds `stars[starts[b]..starts[b + 1]]`.
#[derive(Clone, Debug, PartialEq)]
struct StarGrid {
    min_pc: [f64; 3],
    box_pc: f64,
    per_axis: usize,
    starts: Vec<u32>,
}

impl StarGrid {
    fn new(cell: Cell, stars: usize) -> Self {
        let size = cell.size_pc();
        let per_axis = (stars as f64 / STARS_PER_BOX)
            .cbrt()
            .ceil()
            .min((size / MIN_BOX_PC).floor())
            .max(1.0) as usize;
        Self {
            min_pc: cell.min_pc(),
            box_pc: size / per_axis as f64,
            per_axis,
            starts: vec![0; per_axis.pow(3) + 1],
        }
    }

    /// The box containing `p` on each axis, clamped to the grid.
    fn coords(&self, p: [f64; 3]) -> [usize; 3] {
        std::array::from_fn(|axis| {
            ((p[axis] - self.min_pc[axis]) / self.box_pc)
                .floor()
                .clamp(0.0, (self.per_axis - 1) as f64) as usize
        })
    }

    fn index(&self, [x, y, z]: [usize; 3]) -> usize {
        (x * self.per_axis + y) * self.per_axis + z
    }

    /// Distance from `p` to the nearest point of a box.
    fn gap_pc(&self, coords: [usize; 3], p: [f64; 3]) -> f64 {
        (0..3)
            .map(|axis| {
                let min = self.min_pc[axis] + coords[axis] as f64 * self.box_pc;
                (min - p[axis])
                    .max(p[axis] - min - self.box_pc)
                    .max(0.0)
                    .powi(2)
            })
            .sum::<f64>()
            .sqrt()
    }
}

impl StarCell {
    pub fn new(cell: Cell, stars: Vec<StarPoint>) -> Self {
        let mut grid = StarGrid::new(cell, stars.len());
        let boxes: Vec<usize> = stars
            .iter()
            .map(|star| grid.index(grid.coords(star.position_pc)))
            .collect();
        for &b in &boxes {
            grid.starts[b + 1] += 1;
        }
        for b in 1..grid.starts.len() {
            grid.starts[b] += grid.starts[b - 1];
        }
        let mut next = grid.starts.clone();
        let mut sorted = stars.clone();
        for (star, b) in stars.into_iter().zip(boxes) {
            sorted[next[b] as usize] = star;
            next[b] += 1;
        }
        Self {
            cell,
            stars: sorted,
            grid,
        }
    }

    fn box_stars(&self, coords: [usize; 3]) -> &[StarPoint] {
        let b = self.grid.index(coords);
        let (start, end) = (self.grid.starts[b], self.grid.starts[b + 1]);
        self.stars.get(start as usize..end as usize).unwrap_or(&[])
    }

    /// The star nearest `p` and closer than `cap_pc`, with its distance in
    /// parsecs; equal distances resolve by ID. Searches rings of boxes outward
    /// from `p` and stops once no farther ring could hold a nearer star.
    /// `checked` counts the stars measured.
    pub fn nearest(
        &self,
        p: [f64; 3],
        cap_pc: f64,
        checked: &mut usize,
    ) -> Option<(f64, StarPoint)> {
        let grid = &self.grid;
        let centre = grid.coords(p).map(|c| c as i64);
        let last = grid.per_axis as i64 - 1;
        let mut best: Option<(f64, StarPoint)> = None;
        for ring in 0..=last {
            let limit = best.map_or(cap_pc, |(distance, _)| distance);
            // `p` lies in or beyond the centre box: a ring's boxes are at
            // least one box fewer than the ring away.
            if (ring - 1).max(0) as f64 * grid.box_pc > limit {
                break;
            }
            for dx in -ring..=ring {
                for dy in -ring..=ring {
                    let edge = dx.abs() == ring || dy.abs() == ring;
                    let step = if edge { 1 } else { (2 * ring).max(1) };
                    for dz in (-ring..=ring).step_by(step as usize) {
                        let coords = [centre[0] + dx, centre[1] + dy, centre[2] + dz];
                        if coords.iter().any(|&c| c < 0 || c > last) {
                            continue;
                        }
                        let coords = coords.map(|c| c as usize);
                        let limit = best.map_or(cap_pc, |(distance, _)| distance);
                        if grid.gap_pc(coords, p) > limit {
                            continue;
                        }
                        for star in self.box_stars(coords) {
                            *checked += 1;
                            let distance = (0..3)
                                .map(|axis| (star.position_pc[axis] - p[axis]).powi(2))
                                .sum::<f64>()
                                .sqrt();
                            if distance >= cap_pc {
                                continue;
                            }
                            if best.is_none_or(|(old, old_star)| {
                                distance < old
                                    || (distance == old
                                        && star.id(self.cell) < old_star.id(self.cell))
                            }) {
                                best = Some((distance, *star));
                            }
                        }
                    }
                }
            }
        }
        best
    }

    /// Calls `f` with every star in the boxes within `radius_pc` of `p`: a
    /// superset of the stars within that distance, for callers to filter.
    pub fn near(&self, p: [f64; 3], radius_pc: f64, mut f: impl FnMut(&StarPoint)) {
        if gap_pc(self.cell, p) > radius_pc {
            return;
        }
        let grid = &self.grid;
        let low = grid.coords(std::array::from_fn(|axis| p[axis] - radius_pc));
        let high = grid.coords(std::array::from_fn(|axis| p[axis] + radius_pc));
        for x in low[0]..=high[0] {
            for y in low[1]..=high[1] {
                for z in low[2]..=high[2] {
                    if grid.gap_pc([x, y, z], p) <= radius_pc {
                        self.box_stars([x, y, z]).iter().for_each(&mut f);
                    }
                }
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    Loaded(StarCell),
    Dropped(Cell),
}

/// Nearest distance from a point to a cell, in parsecs.
pub fn gap_pc(cell: Cell, p: [f64; 3]) -> f64 {
    let (min, size) = (cell.min_pc(), cell.size_pc());
    (0..3)
        .map(|axis| {
            (min[axis] - p[axis])
                .max(p[axis] - min[axis] - size)
                .max(0.0)
                .powi(2)
        })
        .sum::<f64>()
        .sqrt()
}

/// Cells of a level within a distance of a point.
fn near(level: usize, p: [f64; 3], radius: f64) -> Vec<Cell> {
    let size = ROOT_PC / (1u64 << level) as f64;
    let last = ((1u64 << level) - 1) as f64;
    let range = |axis: usize| {
        let index = |x: f64| ((x + ROOT_PC / 2.0) / size).floor().clamp(0.0, last) as u32;
        index(p[axis] - radius)..=index(p[axis] + radius)
    };
    let mut cells = Vec::new();
    for x in range(0) {
        for y in range(1) {
            for z in range(2) {
                if let Some(cell) = Cell::from_coords(level, [x, y, z])
                    && gap_pc(cell, p) <= radius
                {
                    cells.push(cell);
                }
            }
        }
    }
    cells
}

/// Cells holding every star that reaches `magnitude` at `camera`, most visible
/// first: nearest relative to their tier's reach, then brightest tier.
pub fn wanted(camera: [f64; 3], magnitude: f64) -> Vec<Cell> {
    let mut cells: Vec<(f64, Cell)> = (0..TIERS)
        .flat_map(|tier| {
            let reach = reach_pc(tier, magnitude);
            near(tier, camera, reach)
                .into_iter()
                .map(move |cell| (gap_pc(cell, camera) / reach, cell))
        })
        .collect();
    cells.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.level().cmp(&b.1.level())));
    cells.into_iter().map(|(_, cell)| cell).collect()
}

/// A cell being built.
struct Job {
    cell: Cell,
    /// Its own stars of each component, once worked out.
    counts: Option<[u64; 6]>,
    component: usize,
    index: u64,
    stars: Vec<StarPoint>,
}

/// Loads the cells around a camera and drops those left behind.
pub struct Loader {
    sky: Sky,
    magnitude: f64,
    camera: Option<[f64; 3]>,
    /// Cells delivered and not yet dropped.
    loaded: HashSet<Cell>,
    /// Wanted cells not yet loaded, most visible last.
    queue: Vec<Cell>,
    job: Option<Job>,
    events: Vec<Event>,
    cancelled_jobs: usize,
}

impl Loader {
    pub fn new(sky: Sky, magnitude: f64) -> Self {
        Self {
            sky,
            magnitude,
            camera: None,
            loaded: HashSet::new(),
            queue: Vec::new(),
            job: None,
            events: Vec::new(),
            cancelled_jobs: 0,
        }
    }

    fn keeps(&self, cell: Cell, camera: [f64; 3]) -> bool {
        gap_pc(cell, camera) <= KEEP * reach_pc(cell.level(), self.magnitude)
    }

    /// Moves the camera, in parsecs from the centre: drops cells left well
    /// behind and queues those newly in reach. Cheap enough to call every frame.
    pub fn aim(&mut self, camera: [f64; 3]) {
        if self.camera == Some(camera) {
            return;
        }
        self.camera = Some(camera);
        let mut dropped: Vec<Cell> = self
            .loaded
            .iter()
            .copied()
            .filter(|&cell| !self.keeps(cell, camera))
            .collect();
        dropped.sort_unstable();
        for cell in dropped {
            self.loaded.remove(&cell);
            self.events.push(Event::Dropped(cell));
        }
        if self
            .job
            .as_ref()
            .is_some_and(|job| !self.keeps(job.cell, camera))
        {
            self.job = None;
            self.cancelled_jobs += 1;
        }
        let building = self.job.as_ref().map(|job| job.cell);
        self.queue = wanted(camera, self.magnitude)
            .into_iter()
            .filter(|cell| !self.loaded.contains(cell) && Some(*cell) != building)
            .rev()
            .collect();
    }

    /// Cells wanted but not loaded yet, including the one being built.
    pub fn pending(&self) -> usize {
        self.queue.len() + usize::from(self.job.is_some())
    }

    pub fn loaded(&self) -> usize {
        self.loaded.len()
    }

    /// Partly generated cells abandoned after the camera moved away.
    pub fn take_cancelled_jobs(&mut self) -> usize {
        std::mem::take(&mut self.cancelled_jobs)
    }

    /// Loads until `more` says to stop, checked after every star or child
    /// integrated, or until nothing is left. Returns cells loaded and dropped
    /// since the last step.
    pub fn step(&mut self, more: &mut dyn FnMut() -> bool) -> Vec<Event> {
        loop {
            if self.job.is_none() {
                let Some(cell) = self.queue.pop() else {
                    break;
                };
                self.job = Some(Job {
                    cell,
                    counts: None,
                    component: 0,
                    index: 0,
                    stars: Vec::new(),
                });
            }
            if self.advance() {
                let job = self.job.take().expect("a job is being built");
                self.loaded.insert(job.cell);
                self.events
                    .push(Event::Loaded(StarCell::new(job.cell, job.stars)));
                self.trim();
            }
            if !more() {
                break;
            }
        }
        std::mem::take(&mut self.events)
    }

    /// One unit of work on the cell being built: an integration, or a star.
    /// True once the cell is complete.
    fn advance(&mut self) -> bool {
        let Some(job) = self.job.as_mut() else {
            return false;
        };
        let cell = job.cell;
        let Some(counts) = job.counts else {
            match self.sky.counts_need(cell) {
                Some((cell, component)) => self.sky.integrate(cell, component),
                None => {
                    let counts = self.sky.counts(cell);
                    let own: [u64; 6] = std::array::from_fn(|c| counts[c][cell.level()]);
                    job.stars.reserve_exact(own.iter().sum::<u64>() as usize);
                    job.counts = Some(own);
                }
            }
            return false;
        };
        while job.component < 6 && job.index >= counts[job.component] {
            job.component += 1;
            job.index = 0;
        }
        let Some(&component) = Component::ALL.get(job.component) else {
            return true;
        };
        match self.sky.place_need(cell, component, job.index) {
            Some((cell, component)) => self.sky.integrate(cell, component),
            None => {
                job.stars.push(self.sky.point(cell, component, job.index));
                job.index += 1;
            }
        }
        false
    }

    /// Drops cached work no loaded or wanted cell needs to be worked out again.
    fn trim(&mut self) {
        if self.sky.cached() < CACHE {
            return;
        }
        let mut live = HashSet::new();
        for &cell in self.loaded.iter().chain(&self.queue) {
            let mut next = Some(cell);
            while let Some(cell) = next {
                if !live.insert(cell) {
                    break;
                }
                next = cell.parent();
            }
        }
        self.sky.retain(|cell| live.contains(&cell));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::galaxy::GalaxyModel;
    use crate::system::Rng;
    use std::collections::HashMap;

    /// A few bright tiers only, so the tests stay quick.
    const MAGNITUDE: f64 = -1.0;

    /// A cell with stars scattered through it and a dense cluster inside.
    fn scattered_cell() -> StarCell {
        let cell = Cell::containing(8, GalaxyModel::milky_way().home_pc).unwrap();
        let (min, size) = (cell.min_pc(), cell.size_pc());
        let mut seed = 12345_u64;
        let mut next = || {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (seed >> 11) as f64 / (1u64 << 53) as f64
        };
        let stars = (0..20_000_u32)
            .map(|index| {
                let position_pc = if index % 2 == 0 {
                    std::array::from_fn(|axis| min[axis] + next() * size)
                } else {
                    std::array::from_fn(|axis| min[axis] + size * 0.3 + next())
                };
                StarPoint {
                    position_pc,
                    luminosity_solar: 1.0,
                    temperature_k: 5772.0,
                    radius_solar: 1.0,
                    component: Component::ThinDisc,
                    index,
                }
            })
            .collect();
        StarCell::new(cell, stars)
    }

    fn distance(a: [f64; 3], b: [f64; 3]) -> f64 {
        (0..3).map(|i| (a[i] - b[i]).powi(2)).sum::<f64>().sqrt()
    }

    #[test]
    fn grid_finds_the_same_stars_as_a_full_scan() {
        let cell = scattered_cell();
        let (min, size) = (cell.cell.min_pc(), cell.cell.size_pc());
        let mut checked = 0;
        // Probes inside, in the cluster, and a little outside the cell.
        for step in 0..300 {
            let t = step as f64 / 300.0;
            let p: [f64; 3] = std::array::from_fn(|axis| {
                min[axis] - 3.0 + (size + 6.0) * t * [1.0, 0.7, 0.4][axis]
            });
            let cluster: [f64; 3] = std::array::from_fn(|axis| min[axis] + size * 0.3 + t);
            for (p, in_cluster) in [(p, false), (cluster, true)] {
                for cap in [0.006, 0.3, 6.1, 40.0] {
                    let expected = cell
                        .stars
                        .iter()
                        .map(|star| distance(star.position_pc, p))
                        .filter(|&d| d < cap)
                        .min_by(f64::total_cmp);
                    let mut counted = 0;
                    let found = cell.nearest(p, cap, &mut counted).map(|(d, _)| d);
                    if !in_cluster {
                        checked += counted;
                    }
                    assert_eq!(found, expected, "{p:?} within {cap}");
                    let mut near = 0;
                    cell.near(p, cap, |star| {
                        near += usize::from(distance(star.position_pc, p) <= cap);
                    });
                    let within = cell
                        .stars
                        .iter()
                        .filter(|star| distance(star.position_pc, p) <= cap)
                        .count();
                    assert_eq!(near, within, "{p:?} within {cap}");
                }
            }
        }
        // Outside the cluster, a few dozen stars per query, not 20,000.
        assert!(checked < 300 * 4 * 100, "{checked}");
    }

    fn run(loader: &mut Loader, more: &mut dyn FnMut() -> bool) -> HashMap<Cell, StarCell> {
        let mut cells = HashMap::new();
        while loader.pending() > 0 {
            for event in loader.step(more) {
                match event {
                    Event::Loaded(stars) => {
                        assert!(cells.insert(stars.cell, stars).is_none());
                    }
                    Event::Dropped(cell) => {
                        assert!(cells.remove(&cell).is_some());
                    }
                }
            }
        }
        cells
    }

    #[test]
    fn wanted_cells_cover_each_tier_reach() {
        let home = GalaxyModel::milky_way().home_pc;
        let cells: HashSet<Cell> = wanted(home, 6.5).into_iter().collect();
        let mut rng = Rng::seeded(3);
        for tier in 0..TIERS {
            let reach = reach_pc(tier, 6.5);
            let level: Vec<_> = cells.iter().filter(|c| c.level() == tier).collect();
            assert!((1..=27).contains(&level.len()), "tier {tier}");
            for _ in 0..200 {
                // A random point within reach, and inside the galaxy's cube, lies
                // in a wanted cell of the tier.
                let d = [rng.unit(), rng.unit(), rng.unit()].map(|u| 2.0 * u - 1.0);
                let length = d.iter().map(|x| x * x).sum::<f64>().sqrt();
                let r = reach * rng.unit().cbrt() / length;
                let p = std::array::from_fn(|axis| home[axis] + d[axis] * r);
                if Cell::ROOT.contains(p) {
                    assert!(level.iter().any(|c| c.contains(p)), "tier {tier}");
                }
            }
        }
        // The cells the camera touches come first, brightest first.
        let order = wanted(home, 6.5);
        let touching = order.iter().take_while(|c| gap_pc(**c, home) == 0.0);
        assert!(touching.clone().is_sorted_by_key(|c| c.level()));
        for tier in 0..TIERS {
            let own = Cell::containing(tier, home).unwrap();
            assert!(touching.clone().any(|&c| c == own));
        }
    }

    #[test]
    fn loads_the_same_stars_in_any_slices() {
        let home = GalaxyModel::milky_way().home_pc;
        let mut whole = Loader::new(Sky::default(), MAGNITUDE);
        whole.aim(home);
        let cells = run(&mut whole, &mut || true);
        assert_eq!(cells.len(), wanted(home, MAGNITUDE).len());
        assert_eq!(whole.loaded(), cells.len());
        // One chunk of stars per step gives the same cells.
        let mut sliced = Loader::new(Sky::default(), MAGNITUDE);
        sliced.aim(home);
        assert_eq!(run(&mut sliced, &mut || false), cells);
        // And they are the sky's own stars, in grid order rather than the sky's.
        let mut sky = Sky::default();
        for (cell, stars) in &cells {
            let own = sky.cell_stars(*cell);
            assert_eq!(own.len(), stars.stars.len());
            let mut points = stars.stars.clone();
            let mut own = own.to_vec();
            own.sort_by(|a, b| a.id.cmp(&b.id));
            points.sort_by_key(|point| point.id(*cell));
            for (star, point) in own.iter().zip(&points) {
                assert_eq!(point.id(*cell), star.id);
                assert_eq!(point.position_pc, star.position_pc);
                assert_eq!(point.temperature_k, star.star.temperature_k as f32);
            }
        }
        assert!(cells.values().map(|c| c.stars.len()).sum::<usize>() > 1_000);
    }

    #[test]
    fn moving_drops_cells_left_behind_and_loads_new_ones() {
        let home = GalaxyModel::milky_way().home_pc;
        let mut loader = Loader::new(Sky::default(), MAGNITUDE);
        loader.aim(home);
        let mut cells = run(&mut loader, &mut || true);
        // A small step back and forth drops nothing.
        let step = reach_pc(TIERS - 1, MAGNITUDE) * 0.1;
        for x in [home[0] + step, home[0]] {
            loader.aim([x, home[1], home[2]]);
            assert!(
                loader
                    .step(&mut || true)
                    .iter()
                    .all(|e| matches!(e, Event::Loaded(_)))
            );
        }
        // Across the disc, near cells go and new ones come, with nothing loaded
        // twice or dropped unloaded.
        let away = [home[0] - 3_000.0, home[1], home[2] + 2_000.0];
        loader.aim(away);
        let mut dropped = 0;
        loop {
            for event in loader.step(&mut || true) {
                match event {
                    Event::Loaded(stars) => assert!(cells.insert(stars.cell, stars).is_none()),
                    Event::Dropped(cell) => {
                        dropped += 1;
                        assert!(cells.remove(&cell).is_some());
                    }
                }
            }
            if loader.pending() == 0 {
                break;
            }
        }
        assert!(dropped > 0);
        let wanted: HashSet<Cell> = wanted(away, MAGNITUDE).into_iter().collect();
        assert!(wanted.iter().all(|cell| cells.contains_key(cell)));
        assert!(cells.keys().all(|&cell| loader.keeps(cell, away)));
        assert_eq!(loader.loaded(), cells.len());
    }

    #[test]
    fn abandoned_partial_cell_is_counted_once() {
        let home = GalaxyModel::milky_way().home_pc;
        let mut loader = Loader::new(Sky::default(), MAGNITUDE);
        loader.aim(home);
        loader.step(&mut || false);
        assert_eq!(loader.take_cancelled_jobs(), 0);
        loader.aim([1_000_000.0, home[1], home[2]]);
        assert_eq!(loader.take_cancelled_jobs(), 1);
        assert_eq!(loader.take_cancelled_jobs(), 0);
    }
}
