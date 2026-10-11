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
use crate::galaxy::{COMPONENTS, Component};
use crate::population::{TIERS, reach_pc};
use std::collections::{HashSet, VecDeque};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

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

/// Stars a grid box holds on average, at least: fewer boxes for sparse cells.
const STARS_PER_BOX: f64 = 8.0;
/// Smallest grid box edge, in parsecs.
const MIN_BOX_PC: f64 = 0.1;
/// Grid blocks holding this few stars are measured star by star rather than
/// divided further.
const LEAF_STARS: u32 = 32;

/// A cell's own stars: those of the tier matching its level. Build it with
/// `StarCell::new`, which sorts the stars into a grid of boxes so the stars
/// near a point are found without scanning the whole cell.
#[derive(Clone, Debug, PartialEq)]
pub struct StarCell {
    pub cell: Cell,
    /// Sorted by grid box, boxes in Morton order: any run of stars lies in a
    /// compact region. Changing them means building a new `StarCell`.
    pub stars: Vec<StarPoint>,
    grid: StarGrid,
}

/// A uniform grid over a cell, a power of two boxes per axis. Boxes are
/// numbered in Morton order, so every aligned block of boxes, down from the
/// whole grid, holds one contiguous run of stars: box `b` holds
/// `stars[starts[b]..starts[b + 1]]`, and an empty block is skipped at once.
#[derive(Clone, Debug, PartialEq)]
struct StarGrid {
    min_pc: [f64; 3],
    box_pc: f64,
    per_axis: usize,
    starts: Vec<u32>,
}

/// Spreads the low 10 bits of `v` to every third bit.
fn spread(v: usize) -> usize {
    let mut v = v & 0x3ff;
    v = (v | (v << 16)) & 0x0300_00ff;
    v = (v | (v << 8)) & 0x0300_f00f;
    v = (v | (v << 4)) & 0x030c_30c3;
    (v | (v << 2)) & 0x0924_9249
}

impl StarGrid {
    fn new(cell: Cell, stars: usize) -> Self {
        let size = cell.size_pc();
        let wanted = (stars as f64 / STARS_PER_BOX)
            .cbrt()
            .ceil()
            .min((size / MIN_BOX_PC).floor())
            .clamp(1.0, 1024.0) as usize;
        // The largest power of two not above it, for Morton order.
        let per_axis = 1 << wanted.ilog2();
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
        (spread(x) << 2) | (spread(y) << 1) | spread(z)
    }

    /// The stars of the aligned block of `size` boxes per axis at `origin`.
    fn block(&self, origin: [usize; 3], size: usize) -> std::ops::Range<usize> {
        let first = self.index(origin);
        self.starts[first] as usize..self.starts[first + size.pow(3)] as usize
    }

    /// Distance from `p` to the nearest point of a block.
    fn gap_pc(&self, origin: [usize; 3], size: usize, p: [f64; 3]) -> f64 {
        let edge = size as f64 * self.box_pc;
        (0..3)
            .map(|axis| {
                let min = self.min_pc[axis] + origin[axis] as f64 * self.box_pc;
                (min - p[axis]).max(p[axis] - min - edge).max(0.0).powi(2)
            })
            .sum::<f64>()
            .sqrt()
    }

    /// The eight blocks within a block, nearest `p` first.
    fn children(&self, origin: [usize; 3], size: usize, p: [f64; 3]) -> [(f64, [usize; 3]); 8] {
        let half = size / 2;
        let mut children = std::array::from_fn(|i| {
            let child = [
                origin[0] + half * (i >> 2 & 1),
                origin[1] + half * (i >> 1 & 1),
                origin[2] + half * (i & 1),
            ];
            (self.gap_pc(child, half, p), child)
        });
        children.sort_by(|a, b| a.0.total_cmp(&b.0));
        children
    }
}

/// An aligned cube of grid boxes and the contiguous run of stars inside it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Block {
    origin: [usize; 3],
    size: usize,
    pub start: usize,
    pub end: usize,
    pub centre_pc: [f64; 3],
    /// Half the cube's edge, in parsecs.
    pub half_pc: f64,
}

impl Block {
    /// Distance from the block's centre to its farthest corner, in parsecs.
    pub fn half_diagonal_pc(&self) -> f64 {
        self.half_pc * 3.0_f64.sqrt()
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

    /// The star nearest `p` and closer than `cap_pc`, with its distance in
    /// parsecs; equal distances resolve by ID. Descends the grid's blocks
    /// nearest first, skipping empty ones and those beyond the best so far.
    /// `checked` counts the stars measured.
    pub fn nearest(
        &self,
        p: [f64; 3],
        cap_pc: f64,
        checked: &mut usize,
    ) -> Option<(f64, StarPoint)> {
        let mut best = None;
        self.search([0; 3], self.grid.per_axis, p, cap_pc, &mut best, checked);
        best
    }

    fn search(
        &self,
        origin: [usize; 3],
        size: usize,
        p: [f64; 3],
        cap_pc: f64,
        best: &mut Option<(f64, StarPoint)>,
        checked: &mut usize,
    ) {
        let stars = self.grid.block(origin, size);
        let limit = best.map_or(cap_pc, |(distance, _)| distance);
        if stars.is_empty() || self.grid.gap_pc(origin, size, p) > limit {
            return;
        }
        if size == 1 || stars.len() <= LEAF_STARS as usize {
            for star in &self.stars[stars] {
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
                        || (distance == old && star.id(self.cell) < old_star.id(self.cell))
                }) {
                    *best = Some((distance, *star));
                }
            }
            return;
        }
        for (_, child) in self.grid.children(origin, size, p) {
            self.search(child, size / 2, p, cap_pc, best, checked);
        }
    }

    /// Calls `f` with the index and star of every star in the grid blocks
    /// within `radius_pc` of `p`: a superset of the stars within that
    /// distance, for callers to filter.
    pub fn near(&self, p: [f64; 3], radius_pc: f64, mut f: impl FnMut(usize, &StarPoint)) {
        self.within([0; 3], self.grid.per_axis, p, radius_pc, &mut f);
    }

    /// As `near`, within one of this cell's `blocks`.
    pub fn near_in(
        &self,
        block: &Block,
        p: [f64; 3],
        radius_pc: f64,
        mut f: impl FnMut(usize, &StarPoint),
    ) {
        self.within(block.origin, block.size, p, radius_pc, &mut f);
    }

    /// Boxes in the grid, empty ones included.
    pub fn boxes(&self) -> usize {
        self.grid.starts.len() - 1
    }

    /// Non-empty aligned cubes of the grid holding at most `most` stars each,
    /// unless a single box holds more, in star order: together they cover
    /// `stars` from start to end. Each is a compact region with its own centre.
    pub fn blocks(&self, most: usize) -> Vec<Block> {
        let mut out = Vec::new();
        self.split([0; 3], self.grid.per_axis, most, &mut out);
        out
    }

    fn split(&self, origin: [usize; 3], size: usize, most: usize, out: &mut Vec<Block>) {
        let stars = self.grid.block(origin, size);
        if stars.is_empty() {
            return;
        }
        if size == 1 || stars.len() <= most {
            let half_pc = size as f64 * self.grid.box_pc / 2.0;
            out.push(Block {
                origin,
                size,
                start: stars.start,
                end: stars.end,
                centre_pc: std::array::from_fn(|axis| {
                    self.grid.min_pc[axis] + origin[axis] as f64 * self.grid.box_pc + half_pc
                }),
                half_pc,
            });
            return;
        }
        // Children in Morton order, so blocks come out in star order.
        let half = size / 2;
        for i in 0..8 {
            let child = [
                origin[0] + half * (i >> 2 & 1),
                origin[1] + half * (i >> 1 & 1),
                origin[2] + half * (i & 1),
            ];
            self.split(child, half, most, out);
        }
    }

    fn within(
        &self,
        origin: [usize; 3],
        size: usize,
        p: [f64; 3],
        radius_pc: f64,
        f: &mut impl FnMut(usize, &StarPoint),
    ) {
        let stars = self.grid.block(origin, size);
        if stars.is_empty() || self.grid.gap_pc(origin, size, p) > radius_pc {
            return;
        }
        if size == 1 || stars.len() <= LEAF_STARS as usize {
            for index in stars {
                f(index, &self.stars[index]);
            }
            return;
        }
        for (_, child) in self.grid.children(origin, size, p) {
            self.within(child, size / 2, p, radius_pc, f);
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

/// `work` on every part, by this thread and `threads - 1` more, each taking
/// the next part as it finishes one. Results come in no particular order,
/// with the time the other threads worked.
fn in_parallel<P: Send, T: Send>(
    threads: usize,
    parts: impl Iterator<Item = P> + Send,
    work: impl Fn(P) -> T + Sync,
) -> (Vec<T>, Duration) {
    let parts = std::sync::Mutex::new(parts);
    let take = || {
        let mut done = Vec::new();
        loop {
            let part = parts.lock().expect("a star thread panicked").next();
            let Some(part) = part else {
                return done;
            };
            done.push(work(part));
        }
    };
    let timed = || {
        let start = Instant::now();
        (take(), start.elapsed())
    };
    std::thread::scope(|scope| {
        let others: Vec<_> = (1..threads).map(|_| scope.spawn(timed)).collect();
        let mut done = take();
        let mut worked = Duration::ZERO;
        for other in others {
            let (more, time) = other.join().expect("a star thread panicked");
            done.extend(more);
            worked += time;
        }
        (done, worked)
    })
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
    counts: Option<[u64; COMPONENTS]>,
    component: usize,
    /// The next star of `component` to try.
    index: u64,
    stars: Vec<StarPoint>,
    /// With several threads: stars tried that needed something worked out
    /// first. Their places in `stars` hold `UNMADE` until then.
    holes: Vec<u64>,
}

/// Stars each thread makes per step when threads share a cell: a few
/// milliseconds' work, so the caller's `more()` is still asked often.
const RUN: u64 = 8_192;
/// Stars threads take at a time: small, so a slower core (an E-core) just
/// takes fewer, and the others never wait long for it.
const PIECE: usize = 512;

/// A place in a cell's stars not made yet.
const UNMADE: StarPoint = StarPoint {
    position_pc: [0.0; 3],
    luminosity_solar: 0.0,
    temperature_k: 0.0,
    radius_solar: 0.0,
    component: Component::Nucleus,
    index: 0,
};

/// Loads the cells around a camera and drops those left behind.
pub struct Loader {
    sky: Sky,
    magnitude: f64,
    /// Threads making each cell's stars, this one included.
    threads: usize,
    camera: Option<[f64; 3]>,
    /// Cells delivered and not yet dropped.
    loaded: HashSet<Cell>,
    /// Wanted cells not yet loaded, most visible last.
    queue: Vec<Cell>,
    job: Option<Job>,
    /// With several threads: cells made, their grids building on threads of
    /// their own while the next cell's stars are made. Delivered in order.
    gridding: VecDeque<(Cell, JoinHandle<(StarCell, Duration)>)>,
    events: Vec<Event>,
    cancelled_jobs: usize,
    /// Time threads other than the caller's worked, for `take_worked`.
    worked: Duration,
}

/// Most grids building at once, so made cells cannot pile up in memory.
const GRIDDING: usize = 2;

impl Loader {
    pub fn new(sky: Sky, magnitude: f64) -> Self {
        Self {
            sky,
            magnitude,
            threads: 1,
            camera: None,
            loaded: HashSet::new(),
            queue: Vec::new(),
            job: None,
            gridding: VecDeque::new(),
            events: Vec::new(),
            cancelled_jobs: 0,
            worked: Duration::ZERO,
        }
    }

    /// Shares each cell's stars among this many threads (native only; the
    /// browser has none to spare). Stars are seeded alone, so the sky is the
    /// same with any number.
    pub fn with_threads(mut self, threads: usize) -> Self {
        self.threads = threads.max(1);
        self
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
        // A grid still building is never delivered, so it needs no drop.
        let before = self.gridding.len();
        let gridding = std::mem::take(&mut self.gridding);
        self.gridding = gridding
            .into_iter()
            .filter(|(cell, _)| self.keeps(*cell, camera))
            .collect();
        self.cancelled_jobs += before - self.gridding.len();
        let building = self.job.as_ref().map(|job| job.cell);
        let gridding: HashSet<Cell> = self.gridding.iter().map(|(cell, _)| *cell).collect();
        self.queue = wanted(camera, self.magnitude)
            .into_iter()
            .filter(|cell| {
                !self.loaded.contains(cell) && Some(*cell) != building && !gridding.contains(cell)
            })
            .rev()
            .collect();
    }

    /// Cells wanted but not loaded yet, including the one being built and
    /// those whose grids are.
    pub fn pending(&self) -> usize {
        self.queue.len() + usize::from(self.job.is_some()) + self.gridding.len()
    }

    pub fn loaded(&self) -> usize {
        self.loaded.len()
    }

    /// Partly generated cells abandoned after the camera moved away.
    pub fn take_cancelled_jobs(&mut self) -> usize {
        std::mem::take(&mut self.cancelled_jobs)
    }

    /// Threads this loader starts.
    pub fn threads(&self) -> usize {
        self.threads
    }

    /// Time this loader's other threads (star making and grids) have worked
    /// since last asked, besides the caller's own in `step`. Always zero with
    /// one thread.
    pub fn take_worked(&mut self) -> Duration {
        std::mem::take(&mut self.worked)
    }

    /// Loads until `more` says to stop, checked after every star or child
    /// integrated, or until nothing is left. Returns cells loaded and dropped
    /// since the last step.
    pub fn step(&mut self, more: &mut dyn FnMut() -> bool) -> Vec<Event> {
        loop {
            self.deliver(false);
            if self.job.is_none() {
                let Some(cell) = self.queue.pop() else {
                    // Nothing left to make: wait for the grids.
                    if self.gridding.is_empty() {
                        break;
                    }
                    self.deliver(true);
                    if !more() {
                        break;
                    }
                    continue;
                };
                self.job = Some(Job {
                    cell,
                    counts: None,
                    component: 0,
                    index: 0,
                    stars: Vec::new(),
                    holes: Vec::new(),
                });
            }
            if self.advance() {
                let job = self.job.take().expect("a job is being built");
                if self.threads > 1 {
                    if self.gridding.len() >= GRIDDING {
                        self.deliver(true);
                    }
                    let (cell, stars) = (job.cell, job.stars);
                    let grid = std::thread::spawn(move || {
                        let start = Instant::now();
                        (StarCell::new(cell, stars), start.elapsed())
                    });
                    self.gridding.push_back((cell, grid));
                } else {
                    self.load(StarCell::new(job.cell, job.stars));
                }
            }
            if !more() {
                break;
            }
        }
        std::mem::take(&mut self.events)
    }

    /// Delivers grids built so far, in order, first waiting for the oldest if
    /// asked to.
    fn deliver(&mut self, wait: bool) {
        let mut wait = wait;
        while let Some((_, grid)) = self.gridding.front()
            && (wait || grid.is_finished())
        {
            let (_, grid) = self.gridding.pop_front().expect("a grid is building");
            let (stars, worked) = grid.join().expect("a grid thread panicked");
            self.worked += worked;
            self.load(stars);
            wait = false;
        }
    }

    fn load(&mut self, stars: StarCell) {
        self.loaded.insert(stars.cell);
        self.events.push(Event::Loaded(stars));
        self.trim();
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
                    let own: [u64; COMPONENTS] = std::array::from_fn(|c| counts[c][cell.level()]);
                    job.stars.reserve_exact(own.iter().sum::<u64>() as usize);
                    job.counts = Some(own);
                }
            }
            return false;
        };
        while job.component < COMPONENTS
            && job.index >= counts[job.component]
            && job.holes.is_empty()
        {
            job.component += 1;
            job.index = 0;
        }
        let Some(&component) = Component::ALL.get(job.component) else {
            return true;
        };
        if self.threads > 1 {
            let count = counts[job.component];
            self.worked += Self::share(&mut self.sky, self.threads, job, component, count);
            return false;
        }
        match self.sky.try_point(cell, component, job.index) {
            Err(need) => self.sky.supply(need),
            Ok(star) => {
                job.stars.push(star);
                job.index += 1;
            }
        }
        false
    }

    /// The next run of stars per thread, in parallel, written into their
    /// places in `stars`, or else the holes left so far. Stars needing
    /// something become holes, tried again once it is worked out, so the cell
    /// is the same as one thread makes. Returns the other threads' time.
    fn share(
        sky: &mut Sky,
        threads: usize,
        job: &mut Job,
        component: Component,
        count: u64,
    ) -> Duration {
        let cell = job.cell;
        // Where the component's first star goes in `stars`.
        let base = job.stars.len() - job.index as usize;
        let shared: &Sky = sky;
        let make = |index| shared.try_point(cell, component, index);
        let mut needs = HashSet::new();
        let worked = if job.holes.is_empty() {
            let start = job.index;
            let end = count.min(start + RUN * threads as u64);
            job.stars.resize(base + end as usize, UNMADE);
            job.index = end;
            let fresh = &mut job.stars[base + start as usize..];
            let (missed, worked) = in_parallel(
                threads,
                fresh.chunks_mut(PIECE).enumerate(),
                |(n, places)| {
                    let first = start + (n * PIECE) as u64;
                    let mut missed = Vec::new();
                    for (index, place) in (first..).zip(places) {
                        match make(index) {
                            Ok(star) => *place = star,
                            Err(need) => missed.push((index, need)),
                        }
                    }
                    missed
                },
            );
            for (index, need) in missed.into_iter().flatten() {
                job.holes.push(index);
                needs.insert(need);
            }
            worked
        } else {
            let holes = std::mem::take(&mut job.holes);
            let (tried, worked) = in_parallel(threads, holes.chunks(PIECE), |holes| {
                holes
                    .iter()
                    .map(|&index| (index, make(index)))
                    .collect::<Vec<_>>()
            });
            for (index, star) in tried.into_iter().flatten() {
                match star {
                    Ok(star) => job.stars[base + index as usize] = star,
                    Err(need) => {
                        job.holes.push(index);
                        needs.insert(need);
                    }
                }
            }
            worked
        };
        for need in needs {
            sky.complete(need);
        }
        worked
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
                    cell.near(p, cap, |index, star| {
                        assert_eq!(&cell.stars[index], star);
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

    #[test]
    fn blocks_cover_the_stars_in_order_within_their_cubes() {
        let cell = scattered_cell();
        for most in [50, 700, 5_000, 100_000] {
            let blocks = cell.blocks(most);
            assert_eq!(blocks.first().map(|b| b.start), Some(0));
            assert_eq!(blocks.last().map(|b| b.end), Some(cell.stars.len()));
            for pair in blocks.windows(2) {
                assert_eq!(pair[0].end, pair[1].start);
            }
            for block in &blocks {
                assert!(block.end > block.start);
                assert!(block.end - block.start <= most || block.size == 1);
                for star in &cell.stars[block.start..block.end] {
                    for axis in 0..3 {
                        let offset = (star.position_pc[axis] - block.centre_pc[axis]).abs();
                        assert!(offset <= block.half_pc * (1.0 + 1e-12), "{most}");
                    }
                }
                let mut seen = 0;
                cell.near_in(
                    block,
                    block.centre_pc,
                    block.half_diagonal_pc(),
                    |index, _| {
                        assert!((block.start..block.end).contains(&index));
                        seen += 1;
                    },
                );
                assert_eq!(seen, block.end - block.start);
            }
        }
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
        // So do several threads sharing each cell.
        for more in [true, false] {
            let mut shared = Loader::new(Sky::default(), MAGNITUDE).with_threads(3);
            shared.aim(home);
            assert_eq!(run(&mut shared, &mut || more), cells);
        }
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
    fn moving_away_from_building_grids_drops_only_delivered_cells() {
        let home = GalaxyModel::milky_way().home_pc;
        let mut loader = Loader::new(Sky::default(), MAGNITUDE).with_threads(3);
        loader.aim(home);
        let mut cells = HashMap::new();
        while loader.gridding.is_empty() {
            for event in loader.step(&mut || false) {
                if let Event::Loaded(stars) = event {
                    cells.insert(stars.cell, stars);
                }
            }
        }
        loader.aim([1_000_000.0, home[1], home[2]]);
        assert!(loader.take_cancelled_jobs() > 0);
        assert!(loader.gridding.is_empty());
        for event in loader.step(&mut || true) {
            match event {
                Event::Loaded(stars) => assert!(cells.insert(stars.cell, stars).is_none()),
                Event::Dropped(cell) => assert!(cells.remove(&cell).is_some()),
            }
        }
        assert!(cells.is_empty());
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
