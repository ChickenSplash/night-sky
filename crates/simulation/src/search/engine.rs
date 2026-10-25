//! Runs a query over the generated galaxy: a cell walk nearest first, from
//! the engine's own `Sky`, so results never depend on what is streamed.
//!
//! - Coarse cells come off a queue by their nearest gap to the origin, and
//!   each one's finer subtree is walked depth first. A cell's own stars lie
//!   inside it, so once `limit` results are nearer than the next queued
//!   cell's gap they are final.
//! - Subtrees that miss the sphere, or hold no stars of their tier or finer,
//!   are never queued.
//! - A star is placed by its descent through child octants and dropped as
//!   soon as the descent leaves the sphere, before its light is worked out.
//!   Only stars inside the sphere, and nearer than the current last result,
//!   have their system generated and tested.
//! - Work is counted in stars tried (and a fixed amount per cell), never in
//!   time, so a search gives the same results however it is sliced or
//!   threaded.
use super::query::{Plan, Query, RelationPlan, Scope, Test};
use super::registry::{Attr, KINDS, KindSet, Link, PROPERTIES, RELATIONS, Source, Values};
use crate::galaxy::cells::{
    Cell, Need, Sky, StarKey, catalogue_name, cell_star_id, parse_cell_star,
};
use crate::galaxy::population::{TIERS, tiered_bodies, tiered_stars};
use crate::galaxy::{COMPONENTS, Component, PC_METRES};
use crate::objects::ObjectRef;
use crate::star::multiple::{Bodies, Hosted, member_id, system_key};
use crate::system::Class;
use serde::{Deserialize, Serialize};
use space_shared::Star;
use std::cmp::{Ordering, Reverse};
use std::collections::{BinaryHeap, HashSet};

pub const PC_PER_LY: f64 = crate::LY_METRES / PC_METRES;
/// Work a request may do unless it asks otherwise: about 100 million stars
/// tried, a second or two on several native threads.
pub const DEFAULT_WORK_CAP: u64 = 100_000_000;
/// Work counted for examining one cell and queueing its children.
const CELL_WORK: u64 = 32;
/// Stars each thread takes at a time.
const PART: usize = 4_096;
/// Stars tried per batch between checks of the queue and budget.
const BATCH: u64 = 262_144;
/// Cells of this level and coarser (32 pc) are visited strictly nearest
/// first; below it, each such cell's subtree is walked depth first. Only a
/// few thousand coarse cells are ever queued, so memory stays bounded.
const SUBTREE_LEVEL: usize = 12;
/// Cache entries (about 1 KB each at most) above which cells the walk has
/// passed are dropped.
const TRIM_AT: usize = 100_000;

/// Where a search is centred. Fixed when the request is sent; later also an
/// entity, such as the ship.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Origin {
    /// Parsecs from the galactic centre.
    Position { pc: [f64; 3] },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SearchRequest {
    pub id: u64,
    pub query: Query,
    pub origin: Origin,
    #[serde(default = "work_cap")]
    pub work_cap: u64,
}

fn work_cap() -> u64 {
    DEFAULT_WORK_CAP
}

/// One object found: no UI types, so any front end can show it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SearchResult {
    pub object: ObjectRef,
    /// Registry kind ID.
    pub kind: String,
    pub name: String,
    /// Plain type, such as "Red dwarf", "Binary system" or "Sub-Neptune".
    pub class: String,
    /// Its system's name.
    pub system: String,
    /// From the request's origin to its system.
    pub distance_ly: f64,
    /// Its system's position, in parsecs from the galactic centre, so it can
    /// be visited without being found again.
    pub position_pc: [f64; 3],
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    #[default]
    Running,
    /// Every result is final.
    Done,
    /// Stopped by the work cap: results within `complete_to_ly` are final.
    Capped,
    Cancelled,
}

/// Fixed-size counters, cheap to copy into metrics.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Progress {
    pub state: State,
    /// Everything nearer than this has been searched.
    pub complete_to_ly: f64,
    pub work: u64,
    pub cells: u64,
    pub stars: u64,
    pub rejected_early: u64,
    pub placed: u64,
    /// Systems generated and tested.
    pub evaluated: u64,
    pub cache_bytes: u64,
    /// Set by whoever runs the search; the engine never reads a clock.
    pub elapsed_ms: f64,
}

/// A result with what orders it.
#[derive(Clone, Debug)]
pub(crate) struct Hit {
    pub distance_pc: f64,
    pub id: String,
    /// The sort property's value, or NaN where it has none.
    pub sort: f64,
    pub result: SearchResult,
}

impl PartialEq for Hit {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl Eq for Hit {}

impl PartialOrd for Hit {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Hit {
    fn cmp(&self, other: &Self) -> Ordering {
        self.distance_pc
            .total_cmp(&other.distance_pc)
            .then_with(|| self.id.cmp(&other.id))
    }
}

/// The stars of a cell still to try: its own tier, by component then index.
struct Cursor {
    cell: Cell,
    counts: [u64; COMPONENTS],
    component: usize,
    index: u64,
}

impl Cursor {
    /// Up to `n` of the stars left, advancing past them.
    fn take(&mut self, n: u64) -> Vec<(Component, u64)> {
        let mut out = Vec::new();
        while (out.len() as u64) < n && self.component < COMPONENTS {
            let count = self.counts[self.component];
            if self.index >= count {
                self.component += 1;
                self.index = 0;
                continue;
            }
            let end = count.min(self.index + n - out.len() as u64);
            let component = Component::ALL[self.component];
            out.extend((self.index..end).map(|i| (component, i)));
            self.index = end;
        }
        out
    }

    fn finished(&self) -> bool {
        (self.component..COMPONENTS).all(|c| {
            let start = if c == self.component { self.index } else { 0 };
            start >= self.counts[c]
        })
    }
}

pub struct Search {
    pub id: u64,
    plan: Plan,
    query: Query,
    origin: [f64; 3],
    radius_pc: f64,
    limit: usize,
    work_cap: u64,
    sky: Sky,
    /// Cells to visit, by nearest gap (bits of a non-negative f64 order as
    /// the number does) and code, so ties go the same way every time.
    frontier: BinaryHeap<Reverse<(u64, u64)>>,
    /// Cells still to visit, depth first, below the last queued cell taken
    /// at `SUBTREE_LEVEL`.
    subtree: Vec<Cell>,
    /// Gap of the last queued cell taken: everything nearer is searched.
    floor_pc: f64,
    current: Option<Cursor>,
    best: BinaryHeap<Hit>,
    /// Cache entries at which to trim next: twice what the last trim kept,
    /// so trimming costs a fixed share of the work.
    trim_at: usize,
    progress: Progress,
}

impl Search {
    /// Checks the query; refuses unknown designations and systems.
    pub fn new(request: SearchRequest) -> Result<Self, String> {
        let plan = request.query.plan()?;
        let Origin::Position { pc: origin } = request.origin;
        if origin.iter().any(|x| !x.is_finite()) {
            return Err("the origin must be a finite position".into());
        }
        let radius_pc = match request.query.scope {
            Scope::Within { radius_ly } => radius_ly * PC_PER_LY,
            Scope::System { .. } => 0.0,
        };
        let mut search = Self {
            id: request.id,
            plan,
            limit: request.query.limit as usize,
            query: request.query,
            origin,
            radius_pc,
            work_cap: request.work_cap,
            sky: Sky::default(),
            frontier: BinaryHeap::new(),
            subtree: Vec::new(),
            floor_pc: 0.0,
            current: None,
            best: BinaryHeap::new(),
            trim_at: TRIM_AT,
            progress: Progress::default(),
        };
        let generated = KindSet::matching(|k| k.source == Source::Cells);
        if search.plan.kinds.and(generated).is_empty() {
            // Only live kinds, such as ships: the client has them all.
            search.progress.state = State::Done;
        } else if !search.query.text.is_empty() {
            let (key, target) = resolve(&search.query.text)?;
            search.only(&key, Some(&target))?;
        } else if let Scope::System { id } = &search.query.scope {
            let key = id.clone();
            search.only(&key, None)?;
        } else {
            if let Some(gap) = search.wanted(Cell::ROOT) {
                search.frontier.push(Reverse((gap.to_bits(), Cell::ROOT.0)));
            }
        }
        Ok(search)
    }

    pub fn progress(&self) -> Progress {
        let mut progress = self.progress;
        progress.complete_to_ly = self.complete_to_pc() / PC_PER_LY;
        progress.cache_bytes = self.sky.cache_bytes() as u64;
        progress
    }

    pub fn query(&self) -> &Query {
        &self.query
    }

    pub fn cancel(&mut self) {
        if self.progress.state == State::Running {
            self.progress.state = State::Cancelled;
        }
    }

    /// Results so far, in order. Those nearer than `complete_to_ly` are final.
    pub fn results(&self) -> Vec<SearchResult> {
        let mut hits: Vec<&Hit> = self.best.iter().collect();
        match self.plan.sort {
            Some((_, descending)) => hits.sort_by(|a, b| {
                let (a_none, b_none) = (a.sort.is_nan(), b.sort.is_nan());
                a_none
                    .cmp(&b_none)
                    .then_with(|| {
                        let order = a.sort.total_cmp(&b.sort);
                        if descending { order.reverse() } else { order }
                    })
                    .then_with(|| a.cmp(b))
            }),
            None => hits.sort(),
        }
        hits.into_iter()
            .take(self.limit)
            .map(|h| h.result.clone())
            .collect()
    }

    /// One system's objects, or the one object `target` names.
    fn only(&mut self, key: &str, target: Option<&ObjectRef>) -> Result<(), String> {
        let star = self
            .sky
            .star(key)
            .ok_or_else(|| format!("no system `{key}` in this galaxy"))?;
        let distance = distance(star.position_pc, self.origin);
        for hit in evaluate(
            &self.plan,
            key,
            (star.cell, star.component),
            star.position_pc,
            distance,
        ) {
            if target.is_none_or(|t| *t == hit.result.object) {
                self.best.push(hit);
            }
        }
        self.progress.evaluated = 1;
        self.progress.state = State::Done;
        Ok(())
    }

    fn complete_to_pc(&self) -> f64 {
        if self.progress.state == State::Done {
            self.radius_pc
        } else {
            self.floor_pc.min(self.radius_pc)
        }
    }

    /// Distance of the last result once `limit` are held; beyond it nothing
    /// can enter.
    fn kth_pc(&self) -> f64 {
        if self.best.len() >= self.limit {
            self.best.peek().map_or(f64::INFINITY, |h| h.distance_pc)
        } else {
            f64::INFINITY
        }
    }

    /// A cell's gap, if it overlaps the sphere and holds stars of its tier or
    /// finer.
    fn wanted(&mut self, cell: Cell) -> Option<f64> {
        let gap = gap_sq(cell.min_pc(), cell.size_pc(), self.origin).sqrt();
        if gap > self.radius_pc.min(self.kth_pc()) {
            return None;
        }
        let level = cell.level();
        let counts = self.sky.counts(cell);
        (!counts
            .iter()
            .all(|row| row[level..].iter().all(|&n| n == 0)))
        .then_some(gap)
    }

    /// Does up to `budget` work with `threads` threads.
    pub fn step(&mut self, budget: u64, threads: usize) -> Progress {
        let mut spent = 0;
        while self.progress.state == State::Running {
            if self.progress.work >= self.work_cap {
                self.progress.state = State::Capped;
                break;
            }
            if spent >= budget {
                break;
            }
            let Some(cursor) = &mut self.current else {
                self.next_cell();
                spent += CELL_WORK;
                continue;
            };
            let n = BATCH
                .min(budget - spent)
                .min(self.work_cap - self.progress.work)
                .max(1);
            let cell = cursor.cell;
            let items = cursor.take(n);
            if cursor.finished() {
                self.current = None;
            }
            let tried = items.len() as u64;
            self.run(cell, items, threads);
            spent += tried;
            self.progress.work += tried;
            self.progress.stars += tried;
        }
        self.progress()
    }

    /// The next cell: depth first within the current subtree, else the
    /// nearest queued one.
    fn next_cell(&mut self) {
        let cell = loop {
            if let Some(cell) = self.subtree.pop() {
                // Its gap may now be beyond the last result.
                if self.wanted(cell).is_some() {
                    break cell;
                }
                continue;
            }
            let Some(&Reverse((gap, code))) = self.frontier.peek() else {
                self.progress.state = State::Done;
                return;
            };
            let gap = f64::from_bits(gap);
            if gap > self.kth_pc() {
                self.progress.state = State::Done;
                return;
            }
            self.frontier.pop();
            self.floor_pc = gap;
            break Cell(code);
        };
        self.progress.cells += 1;
        self.progress.work += CELL_WORK;
        if self.sky.cached() > self.trim_at {
            // Queued cells are coarse and at least `floor` away; finer cells
            // outside the current branch are worked out again if needed.
            let (origin, floor) = (self.origin, self.floor_pc);
            self.sky.retain(|c| {
                c.level() <= 4
                    || (c.level() <= SUBTREE_LEVEL
                        && far_sq(c.min_pc(), c.size_pc(), origin) >= floor * floor)
                    || holds(c, cell)
                    || holds(cell, c)
            });
            self.trim_at = TRIM_AT.max(2 * self.sky.cached());
        }
        let level = cell.level();
        if level + 1 < TIERS {
            let mut children: Vec<(f64, Cell)> = cell
                .children()
                .into_iter()
                .filter_map(|child| Some((self.wanted(child)?, child)))
                .collect();
            if level < SUBTREE_LEVEL {
                for (gap, child) in children {
                    self.frontier.push(Reverse((gap.to_bits(), child.0)));
                }
            } else {
                // Nearest popped first.
                children.sort_by(|a, b| b.0.total_cmp(&a.0).then(b.1.cmp(&a.1)));
                self.subtree.extend(children.into_iter().map(|(_, c)| c));
            }
        }
        let counts = self.sky.counts(cell);
        self.current = Some(Cursor {
            cell,
            counts: std::array::from_fn(|c| counts[c][level]),
            component: 0,
            index: 0,
        });
    }

    /// Places and tests these stars of `cell`, working out what their
    /// descents need between rounds, as the star loader does.
    fn run(&mut self, cell: Cell, mut pending: Vec<(Component, u64)>, threads: usize) {
        let (origin, r2) = (self.origin, self.radius_pc * self.radius_pc);
        while !pending.is_empty() {
            let (sky, plan, kth) = (&self.sky, &self.plan, self.kth_pc());
            let (batches, _) = crate::galaxy::stream::in_parallel(
                if pending.len() > PART { threads } else { 1 },
                pending.chunks(PART),
                |part| batch(sky, plan, cell, part, origin, r2, kth),
            );
            let mut needs = HashSet::new();
            pending = Vec::new();
            for batch in batches {
                self.progress.rejected_early += batch.rejected;
                self.progress.placed += batch.placed;
                self.progress.evaluated += batch.evaluated;
                for hit in batch.hits {
                    self.offer(hit);
                }
                for (component, index, need) in batch.needs {
                    needs.insert(need);
                    pending.push((component, index));
                }
            }
            // Caches only hold what follows from cell codes: completing needs
            // in any order gives the same sky.
            for need in needs {
                self.sky.complete(need);
            }
        }
    }

    fn offer(&mut self, hit: Hit) {
        if self.best.len() < self.limit {
            self.best.push(hit);
        } else if self.best.peek().is_some_and(|last| hit < *last) {
            self.best.pop();
            self.best.push(hit);
        }
    }
}

#[derive(Default)]
struct Batch {
    hits: Vec<Hit>,
    needs: Vec<(Component, u64, Need)>,
    rejected: u64,
    placed: u64,
    evaluated: u64,
}

fn batch(
    sky: &Sky,
    plan: &Plan,
    cell: Cell,
    part: &[(Component, u64)],
    origin: [f64; 3],
    r2: f64,
    kth: f64,
) -> Batch {
    let mut out = Batch::default();
    for &(component, index) in part {
        let key = StarKey::new(cell, component, index);
        match sky.try_place_if(key, cell, component, |min, size| {
            gap_sq(min, size, origin) <= r2
        }) {
            Err(need) => out.needs.push((component, index, need)),
            Ok(None) => out.rejected += 1,
            Ok(Some(p)) => {
                out.placed += 1;
                let d = distance(p, origin);
                if d * d <= r2 && d <= kth {
                    out.evaluated += 1;
                    let id = cell_star_id(cell, component, index);
                    out.hits
                        .extend(evaluate(plan, &id, (cell, component), p, d));
                }
            }
        }
    }
    out
}

pub(crate) fn distance(p: [f64; 3], origin: [f64; 3]) -> f64 {
    (0..3)
        .map(|a| (p[a] - origin[a]).powi(2))
        .sum::<f64>()
        .sqrt()
}

/// Squared distance from a point to the nearest point of a box.
pub(crate) fn gap_sq(min: [f64; 3], size: f64, p: [f64; 3]) -> f64 {
    (0..3)
        .map(|axis| {
            let d = (min[axis] - p[axis])
                .max(p[axis] - (min[axis] + size))
                .max(0.0);
            d * d
        })
        .sum()
}

/// Whether `a` is `b` or one of its ancestors.
fn holds(a: Cell, b: Cell) -> bool {
    a.level() <= b.level() && b.0 >> (3 * (b.level() - a.level())) == a.0
}

/// Squared distance from a point to the farthest corner of a box.
fn far_sq(min: [f64; 3], size: f64, p: [f64; 3]) -> f64 {
    (0..3)
        .map(|axis| {
            let d = (p[axis] - min[axis])
                .abs()
                .max((min[axis] + size - p[axis]).abs());
            d * d
        })
        .sum()
}

enum Value {
    Number(f64),
    Choice(&'static str),
    Flag(bool),
}

/// What a property can read about one candidate.
#[derive(Clone, Copy, Default)]
pub(super) struct Facts<'a> {
    pub star: Option<&'a Star>,
    pub planet: Option<(&'a Hosted, f64)>,
    pub stars: usize,
    pub planets: Option<usize>,
    /// A live craft's altitude in km and speed in km/s, both relative to
    /// the body it is around.
    pub craft: Option<[f64; 2]>,
}

fn value(attr: Attr, facts: Facts) -> Option<Value> {
    let star = || facts.star;
    let planet = || facts.planet.map(|(p, _)| &p.body);
    Some(match attr {
        Attr::StarClass => Value::Choice(star_class(star()?)),
        Attr::TemperatureK => Value::Number(star()?.temperature_k),
        Attr::MassSolar => Value::Number(star()?.mass_solar),
        Attr::RadiusSolar => Value::Number(star()?.radius_solar),
        Attr::LuminositySolar => Value::Number(star()?.luminosity_solar),
        Attr::Multiple => Value::Flag(facts.stars > 1),
        Attr::StarCount => Value::Number(facts.stars as f64),
        Attr::PlanetCount => Value::Number(facts.planets? as f64),
        Attr::PlanetClass => Value::Choice(planet_class(planet()?.class)),
        Attr::RadiusKm => Value::Number(planet()?.radius_km),
        Attr::MassEarth => Value::Number(planet()?.mass_earth),
        Attr::SemiMajorAu => Value::Number(planet()?.orbit.semi_major_au),
        Attr::Eccentricity => Value::Number(planet()?.orbit.eccentricity),
        Attr::PeriodDays => {
            let (planet, host_mass) = facts.planet?;
            Value::Number(crate::period_seconds(&planet.body.orbit, host_mass) / 86_400.0)
        }
        Attr::Resonant => Value::Flag(planet()?.resonance.is_some()),
        Attr::TidallyLocked => Value::Flag(planet()?.spin.locked),
        Attr::AltitudeKm => Value::Number(facts.craft?[0]),
        Attr::SpeedKms => Value::Number(facts.craft?[1]),
    })
}

/// What a relationship starts from.
#[derive(Clone, Copy)]
enum Subject<'a> {
    Star(&'a Star),
    System,
    Planet(&'a Hosted),
}

/// Whether an object of `kind` passes every filter. Every kind left in a
/// plan has every one of its filters' properties.
pub(super) fn all_pass(filters: &[(usize, Test)], kind: &str, facts: Facts) -> bool {
    filters.iter().all(|(i, test)| {
        let property = &PROPERTIES[*i];
        property.kinds.contains(&kind) && passes(test, value(property.attr, facts))
    })
}

fn passes(test: &Test, value: Option<Value>) -> bool {
    match (test, value) {
        (Test::Range { min, max }, Some(Value::Number(n))) => {
            min.is_none_or(|min| n >= min) && max.is_none_or(|max| n <= max)
        }
        (Test::OneOf(values), Some(Value::Choice(c))) => values.iter().any(|v| v == c),
        (Test::Is(want), Some(Value::Flag(f))) => *want == f,
        _ => false,
    }
}

pub(super) fn sort_value(plan: &Plan, facts: Facts) -> f64 {
    let Some((index, _)) = plan.sort else {
        return f64::NAN;
    };
    let property = &PROPERTIES[index];
    match value(property.attr, facts) {
        Some(Value::Number(n)) => n,
        Some(Value::Flag(f)) => f64::from(u8::from(f)),
        Some(Value::Choice(c)) => match property.values {
            Values::Choice { options } => options
                .iter()
                .position(|(o, _)| *o == c)
                .map_or(f64::NAN, |i| i as f64),
            _ => f64::NAN,
        },
        None => f64::NAN,
    }
}

pub(crate) fn star_class(star: &Star) -> &'static str {
    match star.kind.as_str() {
        "White dwarf" => "white_dwarf",
        kind if kind.contains("supergiant") => "supergiant",
        kind if kind.contains("giant") => "giant",
        _ => match star.spectral.chars().next() {
            Some('O') => "o",
            Some('B') => "b",
            Some('A') => "a",
            Some('F') => "f",
            Some('G') => "g",
            Some('K') => "k",
            _ => "m",
        },
    }
}

pub(crate) fn planet_class(class: Class) -> &'static str {
    match class {
        Class::Rocky => "rocky",
        Class::MetalRich => "metal_rich",
        Class::WaterRich => "water_rich",
        Class::SubNeptune => "sub_neptune",
        Class::EnvelopeWorld => "envelope_world",
        Class::GasGiant => "gas_giant",
    }
}

fn system_class(stars: usize) -> &'static str {
    match stars {
        1 => "Single star",
        2 => "Binary system",
        3 => "Triple system",
        _ => "Quadruple system",
    }
}

/// The objects of one system that match: its stars, the system itself, its
/// planets. Generates planets only when a wanted kind or filter needs them.
pub(crate) fn evaluate(
    plan: &Plan,
    key: &str,
    (cell, component): (Cell, Component),
    position_pc: [f64; 3],
    distance_pc: f64,
) -> Vec<Hit> {
    let wants = |id: &str| plan.kinds.has(id);
    let reads_planets = plan
        .filters
        .iter()
        .chain(plan.relations.iter().flat_map(|r| &r.filters))
        .map(|(i, _)| *i)
        .chain(plan.sort.map(|(i, _)| i))
        .any(|i| PROPERTIES[i].attr == Attr::PlanetCount)
        || plan.relations.iter().any(|r| r.kinds.has("planet"));
    let level = cell.level();
    let (stars, bodies) = if wants("planet") || reads_planets {
        let bodies = tiered_bodies(key, component, level);
        (bodies.stars.clone(), Some(bodies))
    } else {
        (tiered_stars(key, component, level).0, None)
    };
    let name = catalogue_name(key);
    let letters: String = stars.iter().map(|s| s.component.as_str()).collect();
    let system = if stars.len() > 1 {
        format!("{name} {letters}")
    } else {
        name.clone()
    };
    let base = Facts {
        stars: stars.len(),
        planets: bodies.as_ref().map(|b| b.planets.len()),
        ..Facts::default()
    };
    fn with_planet<'a>(base: Facts<'a>, bodies: &'a Bodies, planet: &'a Hosted) -> Facts<'a> {
        Facts {
            planet: Some((planet, bodies.mass(&planet.host))),
            ..base
        }
    }
    // Whether something `relation` leads to from `subject` matches it.
    let related = |relation: &RelationPlan, subject: Subject| {
        let matches = |kind: &str, facts: Facts| {
            relation.kinds.has(kind) && all_pass(&relation.filters, kind, facts)
        };
        let star = |star| Facts {
            star: Some(star),
            ..base
        };
        match RELATIONS[relation.relation].link {
            Link::Orbits => match subject {
                // A pair matches when any of its stars does.
                Subject::Planet(planet) => stars
                    .iter()
                    .filter(|s| planet.host_letters.contains(s.component.as_str()))
                    .any(|s| matches("star", star(s))),
                _ => false,
            },
            Link::System => {
                stars
                    .iter()
                    .filter(|s| !matches!(subject, Subject::Star(own) if own.id == s.id))
                    .any(|s| matches("star", star(s)))
                    || (!matches!(subject, Subject::System) && matches("system", base))
                    || bodies.as_ref().is_some_and(|b| {
                        b.planets
                            .iter()
                            .filter(
                                |p| !matches!(subject, Subject::Planet(own) if own.id() == p.id()),
                            )
                            .any(|p| matches("planet", with_planet(base, b, p)))
                    })
            }
        }
    };
    let accept = |kind: &str, facts: Facts, subject: Subject| {
        wants(kind)
            && all_pass(&plan.filters, kind, facts)
            && plan.relations.iter().all(|r| related(r, subject))
    };
    let hit = |object: ObjectRef, kind: &str, name: String, class: &str, facts: Facts| {
        let id = match &object {
            ObjectRef::Star { id } | ObjectRef::Planet { id } | ObjectRef::Barycentre { id } => {
                id.clone()
            }
            ObjectRef::List { id } | ObjectRef::Craft { id } => id.clone(),
            ObjectRef::Galaxy => String::new(),
        };
        Hit {
            distance_pc,
            id,
            sort: sort_value(plan, facts),
            result: SearchResult {
                object,
                kind: kind.into(),
                name,
                class: class.into(),
                system: system.clone(),
                distance_ly: distance_pc / PC_PER_LY,
                position_pc,
            },
        }
    };
    let mut hits = Vec::new();
    for star in &stars {
        let facts = Facts {
            star: Some(star),
            ..base
        };
        if accept("star", facts, Subject::Star(star)) {
            let label = if star.component.is_empty() {
                name.clone()
            } else {
                format!("{name} {}", star.component)
            };
            hits.push(hit(
                ObjectRef::Star {
                    id: star.id.clone(),
                },
                "star",
                label,
                &star.kind,
                facts,
            ));
        }
    }
    // A single star is its own system: list it once.
    let listed = stars.len() == 1 && !hits.is_empty();
    if !listed && accept("system", base, Subject::System) {
        hits.push(hit(
            ObjectRef::Star { id: key.into() },
            "system",
            system.clone(),
            system_class(stars.len()),
            base,
        ));
    }
    if let Some(bodies) = &bodies
        && wants("planet")
    {
        for planet in &bodies.planets {
            let facts = with_planet(base, bodies, planet);
            if accept("planet", facts, Subject::Planet(planet)) {
                hits.push(hit(
                    ObjectRef::Planet { id: planet.id() },
                    "planet",
                    crate::galaxy::cells::planet_designation(&planet.host, planet.index),
                    planet.body.class.name(),
                    facts,
                ));
            }
        }
    }
    debug_assert!(
        hits.iter()
            .all(|h| KINDS.iter().any(|k| k.id == h.result.kind))
    );
    hits
}

/// The system key and object an exact designation (`TS 1abc-2-34 Ab`) or ID
/// (`g1abc.2.34#A/0`) names. Pairs within a system are not searchable.
pub(crate) fn resolve(text: &str) -> Result<(String, ObjectRef), String> {
    let text = text.trim();
    let unknown = || format!("`{text}` is not a catalogue designation or ID");
    if text.starts_with('g') && !text.contains(' ') {
        let key = system_key(text).to_owned();
        parse_cell_star(&key).ok_or_else(unknown)?;
        let object = match text.rsplit_once('/') {
            Some((_, index)) => {
                index.parse::<usize>().map_err(|_| unknown())?;
                ObjectRef::Planet { id: text.into() }
            }
            None => ObjectRef::Star { id: text.into() },
        };
        return Ok((key, object));
    }
    let rest = text.strip_prefix("TS ").ok_or_else(unknown)?;
    let mut words = rest.split(' ');
    let mut parts = words.next().ok_or_else(unknown)?.split('-');
    let key = format!(
        "g{}.{}.{}",
        parts.next().ok_or_else(unknown)?,
        parts.next().ok_or_else(unknown)?,
        parts.next().ok_or_else(unknown)?
    );
    if parts.next().is_some() || parse_cell_star(&key).is_none() {
        return Err(unknown());
    }
    let words: Vec<&str> = words.collect();
    let planet = |host: String, letter: &str| -> Result<ObjectRef, String> {
        let mut chars = letter.chars();
        match (chars.next(), chars.next()) {
            (Some(c @ 'b'..='z'), None) => Ok(ObjectRef::Planet {
                id: format!("{host}/{}", c as u32 - 'b' as u32),
            }),
            _ => Err(unknown()),
        }
    };
    let upper = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_ascii_uppercase());
    let object = match words.as_slice() {
        [] => ObjectRef::Star { id: key.clone() },
        [pair, letter] if pair.starts_with('(') && pair.ends_with(')') => {
            planet(member_id(&key, &pair[1..pair.len() - 1]), letter)?
        }
        [word] if word.len() == 1 && !upper(word) => planet(key.clone(), word)?,
        [word] if upper(word) && word.len() == 1 => ObjectRef::Star {
            id: member_id(&key, word),
        },
        [word] if upper(word) => {
            // A system is labelled with all its letters.
            let (cell, component, _) = parse_cell_star(&key).ok_or_else(unknown)?;
            let stars = tiered_stars(&key, component, cell.level()).0;
            if word.len() != stars.len() {
                return Err(format!(
                    "`{text}`: pairs within a system are not searchable"
                ));
            }
            ObjectRef::Star { id: key.clone() }
        }
        [word] => {
            let split = word
                .find(|c: char| c.is_ascii_lowercase())
                .ok_or_else(unknown)?;
            let (host, letter) = word.split_at(split);
            if !upper(host) || host.len() != 1 {
                return Err(unknown());
            }
            planet(member_id(&key, host), letter)?
        }
        _ => return Err(unknown()),
    };
    Ok((key, object))
}
