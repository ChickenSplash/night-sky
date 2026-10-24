//! Where a voyage's systems come from, and the rules for assembling them that
//! every client and server shares: seeded candidates per region, charted list
//! pages taking the place of the generated systems they are assigned to, and
//! legacy pages with no cell star added as candidates of their own.
use super::{Arrival, Field, Reply, Request, Wait, listed_origin_pc, page_star_id, seeded_arrival};
use crate::galaxy::PC_METRES;
use crate::galaxy::neighbourhood::{Candidate, REGION_PC, Region};
use space_shared::{Galaxy, StarSystem};
use std::collections::HashMap;
use std::sync::atomic::{AtomicI64, Ordering};

/// Regions and systems a `Cache` keeps once worked out, beyond those in use.
pub const REGIONS: usize = 8;
pub const ARRIVALS: usize = 64;

/// Where a voyage's systems come from. Live hosts fill a cache in the
/// background; tools and tests may resolve dependencies synchronously.
pub trait Atlas {
    /// Systems that may reach into a region, or `None` until known.
    fn region(&self, region: Region) -> Option<&[Candidate]>;
    /// A candidate system's bodies, or `None` until loaded.
    fn arrival(&self, star: &str) -> Option<&Arrival>;
    /// Where a system is, by its page star's ID: `None` until known, then
    /// `Some(None)` for an ID that names no system.
    fn locate(&self, star: &str) -> Option<Option<&Candidate>>;
}

/// An atlas that knows nothing yet, for steps that need no system data.
pub struct Blank;

impl Atlas for Blank {
    fn region(&self, _: Region) -> Option<&[Candidate]> {
        None
    }
    fn arrival(&self, _: &str) -> Option<&Arrival> {
        None
    }
    fn locate(&self, _: &str) -> Option<Option<&Candidate>> {
        None
    }
}

/// The list page a star ID names: a page assigned to that cell star, else a
/// legacy page by its list key.
pub fn listed_page<'a>(lists: &'a Galaxy, star: &str) -> Option<(&'a StarSystem, u32)> {
    let assigned = lists.systems.iter().find_map(|system| {
        let page = system.page_stars.iter().find(|s| s.id == star)?.page;
        Some((system, page))
    });
    assigned.or_else(|| {
        lists.systems.iter().find_map(|system| {
            let page = system
                .pages()
                .into_iter()
                .find(|page| page_star_id(system, *page) == star)?;
            Some((system, page))
        })
    })
}

/// List pages with no cell star of their own, which seeds cannot find,
/// whose entry range reaches into a region.
pub fn legacy_candidates(region: Region, lists: &Galaxy) -> Vec<Candidate> {
    let min = region.min_pc();
    let mut found = Vec::new();
    for system in &lists.systems {
        for page in system.pages() {
            if system.page_star(page).is_some() {
                continue;
            }
            let candidate = legacy_candidate(system, page);
            let (origin_pc, range_pc) = (candidate.position_pc, candidate.reach_pc);
            if crate::search::engine::gap_sq(min, REGION_PC, origin_pc) < range_pc * range_pc {
                found.push(candidate);
            }
        }
    }
    found
}

/// A list page with no cell star, as a candidate: where its list puts it,
/// reaching as far as its exact entry range.
fn legacy_candidate(system: &StarSystem, page: u32) -> Candidate {
    Candidate {
        id: page_star_id(system, page),
        position_pc: listed_origin_pc(system, page),
        reach_pc: super::range_m(&Field::new(system.clone(), page)) / PC_METRES,
    }
}

/// A value kept in a `Cache`, with the round it was last used in.
#[derive(Debug)]
struct Kept<T> {
    value: T,
    used: AtomicI64,
}

/// System data worked out for voyages, shared by every craft. It keeps at
/// normally `REGIONS` regions and `ARRIVALS` systems, dropping the least
/// recently used first, but never anything used in the current or previous
/// round: however many craft are flying, none loses data it is using to
/// another, so none waits for the same data again and again. Results are
/// pure functions of seeds and lists, so what is dropped never changes a path.
#[derive(Debug, Default)]
pub struct Cache {
    regions: HashMap<Region, Kept<Vec<Candidate>>>,
    arrivals: HashMap<String, Kept<Arrival>>,
    stars: HashMap<String, Kept<Option<Candidate>>>,
    round: i64,
}

impl Atlas for Cache {
    fn region(&self, region: Region) -> Option<&[Candidate]> {
        self.regions
            .get(&region)
            .map(|kept| self.touch(kept).as_slice())
    }
    fn arrival(&self, star: &str) -> Option<&Arrival> {
        self.arrivals.get(star).map(|kept| self.touch(kept))
    }
    fn locate(&self, star: &str) -> Option<Option<&Candidate>> {
        self.stars.get(star).map(|kept| self.touch(kept).as_ref())
    }
}

impl Cache {
    /// Start a round, at least the last one: a world step on a server, a
    /// frame on a client.
    pub fn round(&mut self, round: i64) {
        self.round = self.round.max(round);
        evict(&mut self.regions, REGIONS, self.round);
        evict(&mut self.arrivals, ARRIVALS, self.round);
        evict(&mut self.stars, ARRIVALS, self.round);
    }

    fn touch<'a, T>(&self, kept: &'a Kept<T>) -> &'a T {
        kept.used.store(self.round, Ordering::Relaxed);
        &kept.value
    }

    fn kept<T>(&self, value: T) -> Kept<T> {
        Kept {
            value,
            used: AtomicI64::new(self.round),
        }
    }

    /// How many regions and systems are kept.
    pub fn counts(&self) -> (usize, usize) {
        (self.regions.len(), self.arrivals.len())
    }

    /// Answer a system wait from the lists, when a charted page takes the
    /// generated system's place, or locate a legacy page, which seeds cannot.
    /// Returns whether it did.
    pub fn chart(&mut self, wait: &Wait, lists: &Galaxy) -> bool {
        if let Wait::Star(id) = wait {
            let Some((system, page)) =
                listed_page(lists, id).filter(|(system, page)| system.page_star(*page).is_none())
            else {
                return false;
            };
            self.insert_star(id.clone(), Some(legacy_candidate(system, page)));
            return true;
        }
        let Wait::System(candidate) = wait else {
            return false;
        };
        let Some((system, page)) = listed_page(lists, &candidate.id) else {
            return false;
        };
        let arrival = Arrival::new(Field::new(system.clone(), page), candidate.position_pc);
        self.insert_arrival(candidate.id.clone(), arrival);
        true
    }

    /// Store a worked-out reply to a wait, adding the lists' legacy pages to
    /// a region's seeded candidates.
    pub fn accept(&mut self, wait: &Wait, reply: Reply, lists: &Galaxy) {
        match (wait, reply) {
            (Wait::Region(region), Reply::Region { mut candidates }) => {
                candidates.extend(legacy_candidates(*region, lists));
                candidates.sort_by(|a, b| a.id.cmp(&b.id));
                let kept = self.kept(candidates);
                self.regions.insert(*region, kept);
                let round = self.round;
                evict(&mut self.regions, REGIONS, round);
            }
            (Wait::System(candidate), Reply::System { system }) if !self.chart(wait, lists) => {
                self.insert_arrival(candidate.id.clone(), seeded_arrival(candidate, system));
            }
            (Wait::Star(id), Reply::Star { candidate }) if !self.chart(wait, lists) => {
                self.insert_star(id.clone(), candidate);
            }
            _ => {}
        }
    }

    fn insert_star(&mut self, id: String, candidate: Option<Candidate>) {
        let kept = self.kept(candidate);
        self.stars.insert(id, kept);
        let round = self.round;
        evict(&mut self.stars, ARRIVALS, round);
    }

    fn insert_arrival(&mut self, id: String, arrival: Arrival) {
        let kept = self.kept(arrival);
        self.arrivals.insert(id, kept);
        let round = self.round;
        evict(&mut self.arrivals, ARRIVALS, round);
    }
}

/// Drop the least recently used entries beyond `most`, sparing any used in
/// this round or the one before.
fn evict<K: Clone + Ord + Eq + std::hash::Hash, T>(
    map: &mut HashMap<K, Kept<T>>,
    most: usize,
    round: i64,
) {
    while map.len() > most {
        let Some(oldest) = map
            .iter()
            .map(|(key, kept)| (key, kept.used.load(Ordering::Relaxed)))
            .filter(|(_, used)| *used < round - 1)
            .min_by(|(a, au), (b, bu)| au.cmp(bu).then_with(|| a.cmp(b)))
            .map(|(key, _)| key.clone())
        else {
            return;
        };
        map.remove(&oldest);
    }
}

/// A synchronous atlas for tools, tests and offline replay, with the same
/// lists and cache rules as a live host. Never use its blocking `step` in a
/// live server tick: use `loading::Loader` and independent per-ship catch-up.
pub struct Library {
    sky: crate::galaxy::cells::Sky,
    cache: Cache,
    lists: Galaxy,
    threads: usize,
}

impl Default for Library {
    fn default() -> Self {
        Self::new(Galaxy::default())
    }
}

impl Library {
    /// A library whose list pages take the place of generated systems.
    pub fn new(lists: Galaxy) -> Self {
        Self {
            sky: crate::galaxy::cells::Sky::default(),
            cache: Cache::default(),
            lists,
            threads: super::threads(),
        }
    }

    pub fn cache(&self) -> &Cache {
        &self.cache
    }

    /// Work out what a step waited for.
    pub fn supply(&mut self, wait: &Wait) {
        if self.cache.chart(wait, &self.lists) {
            return;
        }
        let reply = Request::of(wait).answer(&mut self.sky, self.threads);
        self.cache.accept(wait, reply, &self.lists);
    }

    /// Where a page's frame is anchored; see `frame_origin_pc`.
    pub fn origin_pc(&mut self, system: &StarSystem, page: u32) -> [f64; 3] {
        super::frame_origin_pc(&mut self.sky, system, page)
    }

    /// Step, supplying whatever the step waits for first. Each world step is
    /// one cache round, so craft stepped together keep each other's data.
    pub fn step(&mut self, voyage: &mut super::Voyage) -> super::Step {
        self.cache.round(voyage.craft.tick);
        loop {
            match voyage.step(&self.cache) {
                Ok(step) => return step,
                Err(wait) => self.supply(&wait),
            }
        }
    }
}

impl Atlas for Library {
    fn region(&self, region: Region) -> Option<&[Candidate]> {
        self.cache.region(region)
    }
    fn arrival(&self, star: &str) -> Option<&Arrival> {
        self.cache.arrival(star)
    }
    fn locate(&self, star: &str) -> Option<Option<&Candidate>> {
        self.cache.locate(star)
    }
}

#[cfg(test)]
mod tests;
