//! The client's atlas: the system data its craft wait for, worked out in the
//! background and cached. It never decides a frame change, only supplies
//! data; `Voyage` decides from the craft's own state. Charted list pages take
//! the place of the generated systems they are assigned to.
use super::loading::{Pending, Reply, Request};
use super::*;
use bevy::math::DVec3;
use space_simulation::{
    LY_METRES,
    craft::voyage::{Arrival, Atlas, Cache, Rebase, Wait},
    galaxy::{GalaxyModel, PC_METRES},
    neighbourhood::{Candidate, Region},
};
use std::collections::HashMap;

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
    cache: Cache,
    /// Frames polled, each one cache round.
    frames: i64,
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
            cache: Cache::default(),
            frames: 0,
            pending: HashMap::new(),
            sky: None,
            error: None,
            background: !cfg!(test),
        }
    }
}

impl Atlas for Charts {
    fn region(&self, region: Region) -> Option<&[Candidate]> {
        self.cache.region(region)
    }

    fn arrival(&self, star: &str) -> Option<&Arrival> {
        self.cache.arrival(star)
    }
}

impl Charts {
    /// Start a frame's cache round and collect finished background work.
    pub fn poll(&mut self, galaxy: &Galaxy) {
        self.frames += 1;
        self.cache.round(self.frames);
        let done: Vec<(Key, Result<Reply, ()>)> = self
            .pending
            .iter()
            .filter_map(|(key, (_, job))| Some((key.clone(), job.poll()?)))
            .collect();
        for (key, result) in done {
            let (wait, _) = self.pending.remove(&key).unwrap();
            match result {
                Ok(reply) => self.cache.accept(&wait, reply, galaxy),
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
        if self.cache.chart(wait, galaxy) {
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
        self.cache.accept(wait, reply, galaxy);
        true
    }

    /// The sky this frame works seeded positions out on, kept warm.
    pub fn sky(&mut self) -> &mut space_simulation::cells::Sky {
        self.sky.get_or_insert_with(Default::default)
    }

    /// How many regions and systems are kept.
    #[cfg(test)]
    pub fn counts(&self) -> (usize, usize) {
        self.cache.counts()
    }
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
