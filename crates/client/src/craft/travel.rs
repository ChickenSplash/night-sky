//! The client's atlas: the system data its craft wait for, worked out in the
//! background and cached. It never decides a frame change, only supplies
//! data; `Voyage` decides from the craft's own state. Charted list pages take
//! the place of the generated systems they are assigned to.
use super::loading::Transport;
use super::*;
use bevy::math::DVec3;
use space_simulation::{
    LY_METRES,
    craft::voyage::{Arrival, Atlas, Rebase, Request, Wait, loading::Loader},
    galaxy::{GalaxyModel, PC_METRES},
    neighbourhood::{Candidate, Region},
};

pub struct Charts {
    loader: Loader<Transport>,
    /// Frames polled, each one cache round.
    frames: i64,
    /// Works out a spawned craft's seeded origin, and synchronous test waits.
    sky: Option<space_simulation::cells::Sky>,
    /// Off only in the synchronous test harness, never a failure fallback.
    pub background: bool,
}

impl Default for Charts {
    fn default() -> Self {
        Self {
            loader: Loader::default(),
            frames: 0,
            sky: None,
            background: !cfg!(test),
        }
    }
}

impl Atlas for Charts {
    fn region(&self, region: Region) -> Option<&[Candidate]> {
        self.loader.cache.region(region)
    }

    fn arrival(&self, star: &str) -> Option<&Arrival> {
        self.loader.cache.arrival(star)
    }
}

impl Charts {
    /// Start a frame's cache round and collect finished background work.
    pub fn poll(&mut self, galaxy: &Galaxy) {
        self.frames += 1;
        self.loader.begin_round(self.frames, galaxy);
    }

    /// Start working out what a craft waits for. Returns whether it is
    /// already known, so the craft can step again at once.
    pub fn offer(&mut self, wait: &Wait, galaxy: &Galaxy) -> bool {
        if self.loader.request(wait, true, galaxy) {
            return true;
        }
        if self.background {
            return false;
        }
        // Explicit synchronous test harness only, never a worker-failure fallback.
        let reply =
            Request::of(wait).answer(self.sky(), space_simulation::craft::voyage::threads());
        self.loader.cache.accept(wait, reply, galaxy);
        true
    }

    pub fn prefetch(&mut self, voyage: &Voyage, galaxy: &Galaxy) {
        if self.background {
            self.loader.prefetch(voyage, galaxy);
        }
    }

    pub fn dispatch(&mut self) {
        if self.background {
            self.loader.dispatch();
        }
    }

    pub fn error(&self) -> &Option<String> {
        &self.loader.error
    }

    pub fn loading_status(&self) -> LoadingStatus {
        let (queued, running) = self.loader.counts();
        let (cached_regions, cached_systems) = self.loader.cache.counts();
        LoadingStatus {
            queued,
            running,
            cached_regions,
            cached_systems,
        }
    }

    /// The sky this frame works seeded positions out on, kept warm.
    pub fn sky(&mut self) -> &mut space_simulation::cells::Sky {
        self.sky.get_or_insert_with(Default::default)
    }

    /// How many regions and systems are kept.
    #[cfg(test)]
    pub fn counts(&self) -> (usize, usize) {
        self.loader.cache.counts()
    }
}

/// Background system loading, for the developer readout.
#[derive(Clone, Copy, Debug, serde::Serialize, schemars::JsonSchema)]
pub struct LoadingStatus {
    pub queued: usize,
    pub running: usize,
    pub cached_regions: usize,
    pub cached_systems: usize,
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
