//! Loading the galaxy's cells around a camera, a little at a time.
//!
//! - For each brightness tier, the cells of its level within its reach
//!   (`reach_pc`) of the camera hold every star of that tier bright enough to
//!   see. About three cells per axis per tier, whatever the magnitude limit.
//! - `Loader` builds wanted cells in small steps, most visible first, and stops
//!   whenever its caller, a worker thread, runs out of time. Loaded cells are dropped once the camera is well
//!   beyond their reach.

mod index;
pub use index::{AU_PC, Block, StarCell, StarPoint};

use crate::galaxy::cells::{Cell, ROOT_PC, Sky};
use crate::galaxy::population::{TIERS, reach_pc};
use crate::galaxy::{COMPONENTS, Component};
use std::collections::{HashSet, VecDeque};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// Loaded cells are kept until the camera is this many times their reach away,
/// so moving back and forth across a boundary does not reload them.
const KEEP: f64 = 1.25;
/// Cached cells kept before those no wanted cell needs are dropped.
const CACHE: usize = 20_000;

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
pub(crate) fn near(level: usize, p: [f64; 3], radius: f64) -> Vec<Cell> {
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
pub(crate) fn in_parallel<P: Send, T: Send>(
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
    separation_au: 0.0,
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
    /// The faintest apparent magnitude it loads.
    pub fn magnitude(&self) -> f64 {
        self.magnitude
    }

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

    /// Shares each cell's stars among this many threads. Stars are seeded alone, so the sky is the
    /// same with any number.
    pub fn with_threads(mut self, threads: usize) -> Self {
        self.threads = threads.max(1);
        self
    }

    fn keeps(&self, cell: Cell, camera: [f64; 3]) -> bool {
        gap_pc(cell, camera) <= KEEP * reach_pc(cell.level(), self.magnitude)
    }

    /// Moves the camera, in parsecs from the centre: drops cells left well
    /// behind and queues those newly in reach. Call from the loader thread:
    /// admission to the shared background budget can wait for ongoing work.
    pub fn aim(&mut self, camera: [f64; 3]) {
        if self.camera == Some(camera) {
            return;
        }
        let _permit = crate::background::shared().star();
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
        let budget = crate::background::shared();
        let _coordinator = budget.star();
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
                if self.threads > 1
                    && let Some(permit) = budget.try_star()
                {
                    if self.gridding.len() >= GRIDDING {
                        self.deliver(true);
                    }
                    let (cell, stars) = (job.cell, job.stars);
                    let grid = std::thread::spawn(move || {
                        let _permit = permit;
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
        let budget = crate::background::shared();
        let _permits: Vec<_> = (1..threads).map_while(|_| budget.try_star()).collect();
        let threads = _permits.len() + 1;
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
mod tests;
