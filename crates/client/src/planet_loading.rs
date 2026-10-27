//! Step 8: cache headless refinements, without changing live planet consumers.
use crate::{Navigation, Selection, Universe};
use bevy::prelude::*;
use space_simulation::{
    background,
    system::{
        generated::{self, Mode, PlanetJob},
        structure::{coupled, prototype},
    },
};
use std::{
    collections::{BTreeMap, VecDeque},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread::JoinHandle,
};

const CACHE_SYSTEMS: usize = 4;
const RESULTS_PER_FRAME: usize = 2;

pub enum PlanetState {
    Queued,
    Running,
    Solved(Box<prototype::Solved>),
    Unsupported(coupled::Error),
    Failed(String),
}

#[derive(Default)]
pub struct CachedSystem {
    pub planets: BTreeMap<String, PlanetState>,
    /// Planet IDs and the designations the scene shows, in orbit order.
    pub names: Vec<(String, String)>,
    pub initial_ms: Option<f64>,
    pub error: Option<String>,
    prepared: bool,
    expected: usize,
}

enum Message {
    Prepared {
        epoch: u64,
        result: Result<(Vec<PlanetJob>, f64), String>,
    },
    Planet {
        epoch: u64,
        id: String,
        result: Result<Result<Box<prototype::Solved>, coupled::Error>, String>,
    },
}

#[derive(Resource)]
pub struct PlanetLoading {
    pub enabled: bool,
    active: Option<String>,
    epoch: u64,
    cancel: Arc<AtomicBool>,
    preparing: bool,
    pending_prepare: bool,
    queue: VecDeque<PlanetJob>,
    pub cache: BTreeMap<String, CachedSystem>,
    recent: VecDeque<String>,
    sender: mpsc::Sender<Message>,
    receiver: Mutex<mpsc::Receiver<Message>>,
    workers: Vec<JoinHandle<()>>,
    last_visit: Option<String>,
    departed: Option<String>,
    budget: Arc<background::Budget>,
}

impl Default for PlanetLoading {
    fn default() -> Self {
        let (sender, receiver) = mpsc::channel();
        Self {
            enabled: false,
            active: None,
            epoch: 0,
            cancel: Arc::new(AtomicBool::new(false)),
            preparing: false,
            pending_prepare: false,
            queue: VecDeque::new(),
            cache: BTreeMap::new(),
            recent: VecDeque::new(),
            sender,
            receiver: Mutex::new(receiver),
            workers: Vec::new(),
            last_visit: None,
            departed: None,
            budget: background::shared(),
        }
    }
}

impl PlanetLoading {
    fn aim(&mut self, visited: Option<String>, selected: Option<String>) {
        if !self.enabled {
            self.last_visit = None;
            self.departed = None;
            self.target(None);
            return;
        }
        if visited.is_none() && self.last_visit.is_some() && selected == self.last_visit {
            self.departed = selected.clone();
        }
        if selected != self.departed {
            self.departed = None;
        }
        self.last_visit = visited.clone();
        // A lingering selection of the star just left must not restart its jobs.
        let selected = selected.filter(|s| Some(s) != self.departed.as_ref());
        self.target(visited.or(selected));
    }

    pub fn active(&self) -> Option<(&str, &CachedSystem)> {
        let id = self.active.as_deref()?;
        Some((id, self.cache.get(id)?))
    }

    fn target(&mut self, id: Option<String>) {
        if self.active == id {
            return;
        }
        self.cancel.store(true, Ordering::Relaxed);
        self.cancel = Arc::new(AtomicBool::new(false));
        self.epoch += 1;
        self.queue.clear();
        self.preparing = false;
        self.pending_prepare = id.is_some();
        // Cancelled work is never mistaken for an in-flight job on a revisit.
        if let Some(old) = &self.active
            && let Some(c) = self.cache.get_mut(old)
        {
            c.planets
                .retain(|_, s| !matches!(s, PlanetState::Queued | PlanetState::Running));
        }
        self.active = id.clone();
        if let Some(id) = id {
            let c = self.cache.entry(id.clone()).or_default();
            self.pending_prepare = !c.prepared || c.planets.len() != c.expected;
            self.recent.retain(|s| s != &id);
            self.recent.push_back(id);
            while self.recent.len() > CACHE_SYSTEMS {
                if let Some(old) = self.recent.pop_front() {
                    self.cache.remove(&old);
                }
            }
        }
    }

    fn receive(&mut self) {
        for _ in 0..RESULTS_PER_FRAME {
            let Ok(message) = self.receiver.get_mut().unwrap().try_recv() else {
                break;
            };
            match message {
                Message::Prepared { epoch, result } if epoch == self.epoch => {
                    self.preparing = false;
                    let Some(c) = self.active.as_ref().and_then(|id| self.cache.get_mut(id)) else {
                        continue;
                    };
                    match result {
                        Ok((jobs, ms)) => {
                            c.error = None;
                            c.prepared = true;
                            c.expected = jobs.len();
                            c.initial_ms = Some(ms);
                            c.names = jobs
                                .iter()
                                .map(|j| (j.id.clone(), j.designation.clone()))
                                .collect();
                            for j in jobs {
                                if !c.planets.contains_key(&j.id) {
                                    c.planets.insert(j.id.clone(), PlanetState::Queued);
                                    self.queue.push_back(j);
                                }
                            }
                        }
                        Err(e) => c.error = Some(e),
                    }
                }
                Message::Planet { epoch, id, result } if epoch == self.epoch => {
                    if let Some(c) = self.active.as_ref().and_then(|id| self.cache.get_mut(id)) {
                        let state = match result {
                            Ok(Ok(s)) => PlanetState::Solved(s),
                            Ok(Err(e)) => PlanetState::Unsupported(e),
                            Err(e) => PlanetState::Failed(e),
                        };
                        c.planets.insert(id, state);
                    }
                }
                _ => {} // Old camera target or disabled mode: discard stale results.
            }
        }
        self.workers.retain(|h| !h.is_finished());
    }

    fn dispatch(&mut self, selected: Option<&str>) {
        if self.pending_prepare && !self.preparing {
            let Some(permit) = self.budget.try_planet() else {
                return;
            };
            let id = self.active.clone().unwrap();
            let (sender, cancel, epoch) = (self.sender.clone(), self.cancel.clone(), self.epoch);
            match std::thread::Builder::new()
                .name("planet-inputs".into())
                .spawn(move || {
                    let _permit = permit;
                    if cancel.load(Ordering::Relaxed) {
                        return;
                    }
                    let result = std::panic::catch_unwind(|| {
                        generated::generated_system_with_mode(&id, [0.0; 3], Mode::LayeredPrototype)
                            .map(|p| (p.jobs, p.initial_generation.as_secs_f64() * 1000.0))
                            .ok_or_else(|| "System generation unavailable".to_owned())
                    })
                    .unwrap_or_else(|_| Err("System generation worker panicked".into()));
                    if !cancel.load(Ordering::Relaxed) {
                        let _ = sender.send(Message::Prepared { epoch, result });
                    }
                }) {
                Ok(h) => {
                    self.workers.push(h);
                    self.preparing = true;
                    self.pending_prepare = false;
                }
                Err(e) => {
                    self.pending_prepare = false;
                    self.cache
                        .get_mut(self.active.as_ref().unwrap())
                        .unwrap()
                        .error = Some(e.to_string());
                }
            }
        }
        if let Some(selected) = selected
            && let Some(i) = self.queue.iter().position(|j| j.id == selected)
        {
            let j = self.queue.remove(i).unwrap();
            self.queue.push_front(j);
        }
        while !self.queue.is_empty() {
            let Some(permit) = self.budget.try_planet() else {
                break;
            };
            let job = self.queue.pop_front().unwrap();
            let id = job.id.clone();
            let (sender, cancel, epoch) = (self.sender.clone(), self.cancel.clone(), self.epoch);
            let state = match std::thread::Builder::new()
                .name("planet-structure".into())
                .spawn(move || {
                    let _permit = permit;
                    if cancel.load(Ordering::Relaxed) {
                        return;
                    }
                    let result = std::panic::catch_unwind(|| job.solve().map(Box::new))
                        .map_err(|_| "Planet solver worker panicked".into());
                    if !cancel.load(Ordering::Relaxed) {
                        let _ = sender.send(Message::Planet {
                            epoch,
                            id: job.id,
                            result,
                        });
                    }
                }) {
                Ok(h) => {
                    self.workers.push(h);
                    PlanetState::Running
                }
                Err(e) => PlanetState::Failed(e.to_string()),
            };
            self.cache
                .get_mut(self.active.as_ref().unwrap())
                .unwrap()
                .planets
                .insert(id, state);
        }
    }
}

impl Drop for PlanetLoading {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

pub fn update(mut loading: ResMut<PlanetLoading>, nav: Res<Navigation>, universe: Res<Universe>) {
    let visited = nav
        .system(&universe.0)
        .map(|s| crate::navigation::objects::star_id(s, nav.page));
    let selected = match &nav.selection {
        Some(Selection::Star(star)) => {
            Some(space_simulation::star::multiple::system_key(&star.id).to_owned())
        }
        Some(Selection::Planet(id)) => {
            crate::navigation::objects::parent_star(id).map(str::to_owned)
        }
        _ => None,
    };
    loading.aim(visited, selected);
    loading.receive();
    let selected = match &nav.selection {
        Some(Selection::Planet(id)) => Some(id.as_str()),
        _ => None,
    };
    if loading.active.is_some() {
        loading.dispatch(selected);
    }
}

#[cfg(test)]
mod tests;
