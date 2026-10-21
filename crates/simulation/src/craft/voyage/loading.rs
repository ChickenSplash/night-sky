//! Bounded, nonblocking preparation of voyage data. A round collects all ships'
//! demands before dispatch, so speculative work never jumps queued demands.
use super::{Atlas, Cache, Refused, Reply, Request, Voyage, Wait};
use crate::{galaxy::PC_METRES, neighbourhood::Region};
use glam::DVec3;
use space_shared::Galaxy;

#[cfg(not(target_arch = "wasm32"))]
mod native;
#[cfg(not(target_arch = "wasm32"))]
pub use native::Native;

pub const MAX_QUEUED: usize = 128;
pub const MAX_RUNNING: usize = 2;
/// Simulation-time lookahead. Never walks an arbitrarily long high-speed route.
const LOOKAHEAD_SECONDS: f64 = 5.0;
const MAX_PREFETCH: usize = 8;

/// Platform transport only. Neither operation may wait or generate on the caller.
pub trait Backend {
    type Job;
    fn start(&mut self, request: Request) -> Result<Self::Job, String>;
    fn poll(job: &mut Self::Job) -> Option<Result<Reply, String>>;
}

struct Queued {
    wait: Wait,
    required: bool,
}

pub struct Loader<B: Backend> {
    pub cache: Cache,
    backend: B,
    queued: Vec<Queued>,
    running: Vec<(Wait, B::Job)>,
    /// Last transport failure. Required work is retried, never computed inline.
    pub error: Option<String>,
}

impl<B: Backend + Default> Default for Loader<B> {
    fn default() -> Self {
        Self::new(B::default())
    }
}

fn same(a: &Wait, b: &Wait) -> bool {
    match (a, b) {
        (Wait::Region(a), Wait::Region(b)) => a == b,
        (Wait::System(a), Wait::System(b)) => a.id == b.id,
        _ => false,
    }
}

impl<B: Backend> Loader<B> {
    pub fn new(backend: B) -> Self {
        Self {
            cache: Cache::default(),
            backend,
            queued: Vec::new(),
            running: Vec::new(),
            error: None,
        }
    }

    /// Once per host update, not per craft. Queued interests expire each round:
    /// turning, despawning or changing speed cannot leave stale work behind.
    /// Already running jobs finish, with at most MAX_RUNNING obsolete jobs.
    pub fn begin_round(&mut self, round: i64, lists: &Galaxy) {
        self.queued.clear();
        self.cache.round(round);
        let mut i = 0;
        while i < self.running.len() {
            if let Some(result) = B::poll(&mut self.running[i].1) {
                let (wait, _) = self.running.remove(i);
                match result {
                    Ok(reply) => {
                        self.error = None;
                        self.cache.accept(&wait, reply, lists);
                    }
                    Err(error) => self.error = Some(error),
                }
            } else {
                i += 1;
            }
        }
    }

    /// True only when this dependency is ready. Full queues defer submission;
    /// the craft remains at its exact tick and offers the demand next round.
    pub fn request(&mut self, wait: &Wait, required: bool, lists: &Galaxy) -> bool {
        let known = match wait {
            Wait::Region(region) => self.cache.region(*region).is_some(),
            Wait::System(candidate) => self.cache.arrival(&candidate.id).is_some(),
        };
        if known || self.cache.chart(wait, lists) {
            return true;
        }
        if self.running.iter().any(|(w, _)| same(w, wait)) {
            return false;
        }
        if let Some(queued) = self.queued.iter_mut().find(|q| same(&q.wait, wait)) {
            queued.required |= required;
            return false;
        }
        if self.queued.len() == MAX_QUEUED {
            if required && let Some(i) = self.queued.iter().rposition(|q| !q.required) {
                self.queued.remove(i);
            } else {
                return false;
            }
        }
        self.queued.push(Queued {
            wait: wait.clone(),
            required,
        });
        false
    }

    /// Dispatch after all craft have offered demands and refreshed predictions.
    pub fn dispatch(&mut self) {
        while self.running.len() < MAX_RUNNING && !self.queued.is_empty() {
            let i = self.queued.iter().position(|q| q.required).unwrap_or(0);
            let queued = self.queued.remove(i);
            match self.backend.start(Request::of(&queued.wait)) {
                Ok(job) => self.running.push((queued.wait, job)),
                Err(error) => {
                    self.error = Some(error);
                    break;
                }
            }
        }
    }

    pub fn counts(&self) -> (usize, usize) {
        (self.queued.len(), self.running.len())
    }

    pub fn prefetch(&mut self, voyage: &Voyage, lists: &Galaxy) {
        for wait in prefetch(voyage, &self.cache) {
            self.request(&wait, false, lists);
        }
    }

    /// A server's independent per-ship catch-up. The host clock is never changed;
    /// callers retain the ship and its queued orders while other ships advance.
    /// Call begin_round/dispatch once around the fleet, and impose a fleet-wide
    /// step budget as well as this per-ship budget in a production host.
    pub fn catch_up(
        &mut self,
        voyage: &mut Voyage,
        until_tick: i64,
        max_steps: usize,
        lists: &Galaxy,
    ) -> Progress {
        let mut progress = Progress::default();
        while voyage.craft.tick < until_tick && progress.steps < max_steps {
            match voyage.step(&self.cache) {
                Ok(step) => {
                    progress.steps += 1;
                    progress.refused.extend(step.refused);
                }
                Err(wait) => {
                    if self.request(&wait, true, lists) {
                        continue;
                    }
                    progress.waiting = Some(wait);
                    break;
                }
            }
        }
        self.prefetch(voyage, lists);
        progress
    }
}

#[derive(Default, Debug)]
pub struct Progress {
    pub steps: usize,
    pub waiting: Option<Wait>,
    pub refused: Vec<Refused>,
}

/// A few local dependencies, independent of camera or transport. High-speed
/// cruise does no speculative census. A requested slowdown prepares its local
/// endpoint using the integral of the same exponential ramp as the drive.
pub fn prefetch(voyage: &Voyage, atlas: &impl Atlas) -> Vec<Wait> {
    let warp = &voyage.craft.warp;
    if warp.engaged
        && warp.actual_ms >= super::ARRIVAL_MAX_SPEED
        && warp.requested_ms >= super::ARRIVAL_MAX_SPEED
    {
        return Vec::new();
    }
    let state = voyage.craft.state();
    let dt = LOOKAHEAD_SECONDS
        + if warp.engaged && warp.actual_ms >= super::ARRIVAL_MAX_SPEED {
            super::super::warp::RAMP_SECONDS
                * ((warp.actual_ms - warp.requested_ms)
                    / (super::ARRIVAL_MAX_SPEED - warp.requested_ms))
                    .ln()
        } else {
            0.0
        };
    let bubble = if warp.engaged {
        let travel = warp.requested_ms * dt
            + (warp.actual_ms - warp.requested_ms)
                * super::super::warp::RAMP_SECONDS
                * (1.0 - (-dt / super::super::warp::RAMP_SECONDS).exp());
        glam::DQuat::from_array(voyage.craft.orientation) * -DVec3::Z * travel
    } else {
        DVec3::ZERO
    };
    let start = DVec3::from_array(voyage.galactic_pc(state.position));
    let delta = (voyage.basis() * (DVec3::from_array(state.velocity) * dt + bubble)) / PC_METRES;
    if !voyage.in_galaxy()
        && DVec3::from_array(state.position).length() + delta.length() * PC_METRES
            < super::LEAVE_RANGES * voyage.range_m
    {
        return Vec::new();
    }
    let mut waits = Vec::new();
    // Endpoint first when decelerating from FTL: intermediate regions are crossed
    // too fast to enter. Otherwise include the present region and near boundary.
    let points = if warp.engaged && warp.actual_ms >= super::ARRIVAL_MAX_SPEED {
        vec![start + delta]
    } else {
        vec![start, start + delta]
    };
    for point in points {
        let region = Region::of(point.to_array());
        let Some(candidates) = atlas.region(region) else {
            let wait = Wait::Region(region);
            if !waits.iter().any(|w| same(w, &wait)) {
                waits.push(wait);
            }
            continue;
        };
        for candidate in candidates {
            // Distance to the short predicted segment, not only its endpoint.
            let centre = DVec3::from_array(candidate.position_pc);
            let share = if delta.length_squared() > 0.0 {
                ((centre - start).dot(delta) / delta.length_squared()).clamp(0.0, 1.0)
            } else {
                0.0
            };
            if centre.distance(start + delta * share) <= candidate.reach_pc
                && atlas.arrival(&candidate.id).is_none()
            {
                let wait = Wait::System(candidate.clone());
                if !waits.iter().any(|w| same(w, &wait)) {
                    waits.push(wait);
                }
                if waits.len() == MAX_PREFETCH {
                    return waits;
                }
            }
        }
    }
    waits
}

#[cfg(test)]
mod tests;
