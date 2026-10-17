//! The client's atlas: the system data its craft wait for, worked out in the
//! background and cached. It never decides a frame change, only supplies
//! data; `Voyage` decides from the craft's own state. Charted list pages take
//! the place of the generated systems they are assigned to.
use super::loading::{Pending, Reply, Request};
use super::*;
use bevy::math::DVec3;
use space_simulation::{
    LY_METRES,
    craft::voyage::{Arrival, Atlas, Rebase, Wait, listed_origin_pc, page_star_id, seeded_arrival},
    galaxy::{GalaxyModel, PC_METRES},
    neighbourhood::{Candidate, REGION_PC, Region},
};
use std::collections::{HashMap, VecDeque};

/// Regions and systems kept once worked out, oldest dropped first.
const REGIONS: usize = 8;
const ARRIVALS: usize = 64;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum Key {
    Region(Region),
    System(String),
}

impl Key {
    fn of(wait: &Wait) -> Self {
        match wait {
            Wait::Region(region) => Self::Region(*region),
            Wait::System(candidate) => Self::System(candidate.id.clone()),
        }
    }
}

pub struct Charts {
    regions: HashMap<Region, Vec<Candidate>>,
    arrivals: HashMap<String, Arrival>,
    kept: VecDeque<Key>,
    pending: HashMap<Key, (Wait, Pending)>,
    /// Works out on the frame itself a spawned craft's seeded origin, and
    /// waits without a background worker or when none can be started.
    sky: Option<space_simulation::cells::Sky>,
    pub error: Option<String>,
    /// Work waits out on a background worker. Off in tests, which then step
    /// without waiting frames for one.
    pub background: bool,
}

impl Default for Charts {
    fn default() -> Self {
        Self {
            regions: HashMap::new(),
            arrivals: HashMap::new(),
            kept: VecDeque::new(),
            pending: HashMap::new(),
            sky: None,
            error: None,
            background: !cfg!(test),
        }
    }
}

impl Atlas for Charts {
    fn region(&self, region: Region) -> Option<&[Candidate]> {
        self.regions.get(&region).map(Vec::as_slice)
    }

    fn arrival(&self, star: &str) -> Option<&Arrival> {
        self.arrivals.get(star)
    }
}

impl Charts {
    /// Collect finished background work.
    pub fn poll(&mut self, galaxy: &Galaxy) {
        let done: Vec<(Key, Result<Reply, ()>)> = self
            .pending
            .iter()
            .filter_map(|(key, (_, job))| Some((key.clone(), job.poll()?)))
            .collect();
        for (key, result) in done {
            let (wait, _) = self.pending.remove(&key).unwrap();
            match result {
                Ok(reply) => self.accept(&wait, reply, galaxy),
                // Asked again on the next wait, never silently skipped.
                Err(()) => {
                    self.error = Some(format!("Background system loading failed for {key:?}"));
                }
            }
        }
    }

    /// Start working out what a craft waits for. Returns whether it is
    /// already known, so the craft can step again at once.
    pub fn offer(&mut self, wait: &Wait, galaxy: &Galaxy) -> bool {
        if let Wait::System(candidate) = wait
            && let Some((system, page)) = crate::objects::known_system(galaxy, &candidate.id)
        {
            let arrival = Arrival::new(Field::new(system.clone(), page), candidate.position_pc);
            self.keep(Key::System(candidate.id.clone()), |charts| {
                charts.arrivals.insert(candidate.id.clone(), arrival);
            });
            return true;
        }
        let key = Key::of(wait);
        if self.pending.contains_key(&key) {
            return false;
        }
        let request = Request::of(wait);
        let job = if !self.background {
            None
        } else {
            let job = Pending::start(request.clone());
            if job.is_none() {
                self.error = Some("Cannot start the system loader; loading on the frame".into());
            }
            job
        };
        if let Some(job) = job {
            self.pending.insert(key, (wait.clone(), job));
            return false;
        }
        let reply = request.answer(self.sky(), space_simulation::craft::voyage::threads());
        self.accept(wait, reply, galaxy);
        true
    }

    /// The sky this frame works seeded positions out on, kept warm.
    pub fn sky(&mut self) -> &mut space_simulation::cells::Sky {
        self.sky.get_or_insert_with(Default::default)
    }

    fn accept(&mut self, wait: &Wait, reply: Reply, galaxy: &Galaxy) {
        match (wait, reply) {
            (Wait::Region(region), Reply::Region { mut candidates }) => {
                candidates.extend(legacy_candidates(*region, galaxy));
                candidates.sort_by(|a, b| a.id.cmp(&b.id));
                self.keep(Key::Region(*region), |charts| {
                    charts.regions.insert(*region, candidates);
                });
            }
            (Wait::System(candidate), Reply::System { system }) => {
                let arrival = seeded_arrival(candidate, system);
                self.keep(Key::System(candidate.id.clone()), |charts| {
                    charts.arrivals.insert(candidate.id.clone(), arrival);
                });
            }
            _ => {}
        }
    }

    fn keep(&mut self, key: Key, insert: impl FnOnce(&mut Self)) {
        insert(self);
        self.kept.retain(|k| *k != key);
        self.kept.push_back(key);
        let count = |kept: &VecDeque<Key>, region: bool| {
            kept.iter()
                .filter(|k| matches!(k, Key::Region(_)) == region)
                .count()
        };
        for (region, most) in [(true, REGIONS), (false, ARRIVALS)] {
            while count(&self.kept, region) > most {
                let index = self
                    .kept
                    .iter()
                    .position(|k| matches!(k, Key::Region(_)) == region)
                    .unwrap();
                match self.kept.remove(index) {
                    Some(Key::Region(r)) => {
                        self.regions.remove(&r);
                    }
                    Some(Key::System(id)) => {
                        self.arrivals.remove(&id);
                    }
                    None => {}
                }
            }
        }
    }
}

/// List pages with no cell star of their own, which seeds cannot find.
fn legacy_candidates(region: Region, galaxy: &Galaxy) -> Vec<Candidate> {
    let min = region.min_pc();
    let mut found = Vec::new();
    for system in &galaxy.systems {
        for page in system.pages() {
            if system.page_star(page).is_some() {
                continue;
            }
            let origin_pc = listed_origin_pc(system, page);
            let range_pc =
                space_simulation::craft::voyage::range_m(&Field::new(system.clone(), page))
                    / PC_METRES;
            let gap: f64 = (0..3)
                .map(|i| {
                    (min[i] - origin_pc[i])
                        .max(origin_pc[i] - min[i] - REGION_PC)
                        .max(0.0)
                        .powi(2)
                })
                .sum::<f64>()
                .sqrt();
            if gap < range_pc {
                found.push(Candidate {
                    id: page_star_id(system, page),
                    position_pc: origin_pc,
                    reach_pc: range_pc,
                });
            }
        }
    }
    found
}

impl Live {
    /// Re-express everything kept in the old frame after a frame change.
    pub(super) fn rebase(&mut self, rebase: &Rebase) {
        self.previous = rebase.state(self.previous);
        self.previous_orientation = rebase.orientation(self.previous_orientation);
        self.displayed = self.displayed.map(|state| rebase.state(state));
        if rebase.frame_changed {
            self.forecast = Forecast::default();
        }
    }

    /// Where the craft is at `seconds`, in parsecs from the galactic centre.
    pub fn position_pc(&self, seconds: f64) -> [f64; 3] {
        self.voyage.galactic_pc(self.state_at(seconds).position)
    }

    /// The frame's origin in light years from home, as drawing places it.
    pub fn origin_ly(&self) -> DVec3 {
        if !self.voyage.in_galaxy() {
            return crate::scene::page_origin_ly(self.field.system(), self.field.page());
        }
        (DVec3::from_array(self.origin_pc) - DVec3::from_array(GalaxyModel::milky_way().home_pc))
            * (PC_METRES / LY_METRES)
    }

    /// What the craft's next step is waiting for, if anything.
    pub fn arrival_status(&self) -> String {
        match &self.waiting {
            None if self.voyage.arriving() => "entering systems".into(),
            None if self.voyage.in_galaxy() => "too fast to enter systems".into(),
            None => format!("in {}", self.star()),
            Some(Wait::Region(_)) => "waiting for nearby systems (clock held)".into(),
            Some(Wait::System(candidate)) => format!("loading {} (clock held)", candidate.id),
        }
    }
}

#[cfg(test)]
mod tests;
