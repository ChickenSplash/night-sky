//! Bounded background refinement and guarded, whole-result publication.
use crate::{Navigation, Selection, Universe};
use bevy::prelude::*;
use space_shared::LayeredPlanet;
use space_simulation::{
    background,
    system::{
        generated::{self, Mode, PlanetJob},
        physical,
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
    Solved(Box<LayeredPlanet>),
    /// The layered solver refused; the labelled fallback structure stands in.
    Fallback(Box<LayeredPlanet>),
    Failed(String),
}

impl PlanetState {
    fn from_record(record: Box<LayeredPlanet>) -> Self {
        if record.fallback_reason.is_some() {
            Self::Fallback(record)
        } else {
            Self::Solved(record)
        }
    }

    pub fn label(&self) -> String {
        match self {
            Self::Queued => "Queued".into(),
            Self::Running => "Calculating".into(),
            Self::Solved(_) => "Solved".into(),
            Self::Fallback(l) => format!(
                "Fallback (layered unsupported: {})",
                l.fallback_reason.as_deref().unwrap_or_default()
            ),
            Self::Failed(e) => format!("Worker failed: {e}"),
        }
    }
}

/// One planet's layered result as the inspector sees it.
pub enum Layered<'a> {
    /// Background work and publication are paused; adopted results remain active.
    Off,
    /// Its system is not the loader's target, or inputs are still being prepared.
    Waiting,
    State(&'a PlanetState),
}

impl Layered<'_> {
    /// Changes whenever what the inspector shows changes. A planet's result,
    /// once finished, is deterministic, so its kind is enough.
    pub fn key(&self) -> u8 {
        match self {
            Self::Off => 0,
            Self::Waiting => 1,
            Self::State(PlanetState::Queued) => 2,
            Self::State(PlanetState::Running) => 3,
            Self::State(PlanetState::Solved(_)) => 4,
            Self::State(PlanetState::Fallback(_)) => 5,
            Self::State(PlanetState::Failed(_)) => 6,
        }
    }
}

#[derive(Default)]
pub struct CachedSystem {
    pub planets: BTreeMap<String, PlanetState>,
    /// Planet IDs and the designations the scene shows, in orbit order.
    pub names: Vec<(String, String)>,
    pub initial_ms: Option<f64>,
    pub error: Option<String>,
    pub adoption: BTreeMap<String, String>,
    ready: BTreeMap<String, physical::Refinement>,
    bases: BTreeMap<String, space_shared::PhysicalRevision>,
    check_cursor: Option<String>,
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
        result: Result<Box<LayeredPlanet>, String>,
    },
    Ready {
        epoch: u64,
        id: String,
        record: Box<LayeredPlanet>,
        refinement: Result<Box<physical::Refinement>, String>,
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
            enabled: true,
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
    fn reuse_adopted(&mut self, fleet: &crate::craft::Fleet) {
        let Some(cache) = self.active.as_ref().and_then(|id| self.cache.get_mut(id)) else {
            return;
        };
        for (id, base) in &cache.bases {
            let Some(refinement) = fleet.charts.refinement(id).filter(|r| r.base == *base) else {
                continue;
            };
            if cache
                .adoption
                .get(id)
                .is_some_and(|s| s.starts_with("Applied:"))
            {
                continue;
            }
            cache.planets.insert(
                id.clone(),
                PlanetState::from_record(Box::new(refinement.result.record.clone())),
            );
            cache.ready.remove(id);
            cache
                .adoption
                .insert(id.clone(), "Applied: active physical revision".into());
        }
        self.queue.retain(|job| {
            fleet
                .charts
                .refinement(&job.id)
                .is_none_or(|r| r.base != job.initial_result.revision)
        });
    }
    fn invalidate_changed_inputs(&mut self, nav: &Navigation, galaxy: &space_shared::Galaxy) {
        let Some(key) = self.active.clone() else {
            return;
        };
        let Some(cache) = self.cache.get(&key) else {
            return;
        };
        let changed = nav.system(galaxy).into_iter().chain(galaxy.systems.iter())
            .flat_map(|s| &s.planets).any(|p| {
                matches!((cache.bases.get(&p.id), p.initial_result.as_ref()), (Some(base), Some(active))
                    if base.inputs != active.revision.inputs || base.model != active.revision.model)
            });
        if changed {
            self.target(None);
            self.cache.remove(&key);
            self.recent.retain(|id| id != &key);
            self.target(Some(key));
        }
    }
    pub fn adoption(&self, planet: &str) -> Option<&str> {
        let system = crate::navigation::objects::parent_star(planet)?;
        self.cache
            .get(system)?
            .adoption
            .get(planet)
            .map(String::as_str)
    }
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

    pub fn status(&self, planet: &str) -> Layered<'_> {
        if !self.enabled {
            return Layered::Off;
        }
        crate::navigation::objects::parent_star(planet)
            .and_then(|system| self.cache.get(system)?.planets.get(planet))
            .map_or(Layered::Waiting, Layered::State)
    }

    /// For `GET /state` and `ns planets`: the target system and each planet's
    /// state, with its full layered record once solved.
    pub fn readout(&self) -> serde_json::Value {
        use serde_json::json;
        let system = self.active().map(|(id, c)| {
            json!({
                "id": id,
                "initial_generation_ms": c.initial_ms,
                "error": c.error,
                "planets": c.names.iter().filter_map(|(id, name)| {
                    let state = c.planets.get(id)?;
                    Some(json!({
                        "id": id, "designation": name,
                        "state": match state {
                            PlanetState::Queued => "queued",
                            PlanetState::Running => "calculating",
                            PlanetState::Solved(_) => "solved",
                            PlanetState::Fallback(_) => "fallback",
                            PlanetState::Failed(_) => "failed",
                        },
                        "status": state.label(),
                        "adoption": c.adoption.get(id),
                        "layered": match state {
                            PlanetState::Solved(l) | PlanetState::Fallback(l) => json!(l),
                            _ => json!(null),
                        },
                    }))
                }).collect::<Vec<_>>(),
            })
        });
        json!({
            "enabled": self.enabled,
            "shared_cpu_threads": self.budget.limit(),
            "cached_systems": self.recent,
            "system": system,
        })
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
                                c.bases
                                    .insert(j.id.clone(), j.initial_result.revision.clone());
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
                            Ok(record) => PlanetState::from_record(record),
                            Err(e) => PlanetState::Failed(e),
                        };
                        c.planets.insert(id, state);
                    }
                }
                Message::Ready {
                    epoch,
                    id,
                    record,
                    refinement,
                } if epoch == self.epoch => {
                    if let Some(c) = self.active.as_ref().and_then(|id| self.cache.get_mut(id)) {
                        c.planets
                            .insert(id.clone(), PlanetState::from_record(record));
                        match refinement {
                            Ok(refinement) => {
                                c.ready.insert(id.clone(), *refinement);
                                c.adoption.insert(id, "Ready for safety checks".into());
                            }
                            Err(reason) => {
                                c.adoption.insert(id, format!("Not applied: {reason}"));
                            }
                        }
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
                        generated::generated_system_with_mode(
                            &id,
                            [0.0; 3],
                            Mode::PrepareRefinement,
                        )
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
                    let result = std::panic::catch_unwind(|| {
                        let outcome = job.resolve();
                        let refinement = physical::prepare_refinement(
                            &job.initial_result,
                            &job.environment,
                            &outcome,
                        )
                        .map(Box::new)
                        .map_err(str::to_owned);
                        let record = Box::new(refinement.as_ref().map_or_else(
                            |_| physical::outcome(&outcome),
                            |r| r.result.record.clone(),
                        ));
                        (record, refinement)
                    });
                    if !cancel.load(Ordering::Relaxed) {
                        let message = match result {
                            Ok((record, refinement)) => Message::Ready {
                                epoch,
                                id: job.id,
                                record,
                                refinement,
                            },
                            Err(_) => Message::Planet {
                                epoch,
                                id: job.id,
                                result: Err("Planet solver worker panicked".into()),
                            },
                        };
                        let _ = sender.send(message);
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

pub fn update(
    mut loading: ResMut<PlanetLoading>,
    mut commands: MessageReader<crate::actions::UiCommand>,
    mut nav: ResMut<Navigation>,
    mut universe: ResMut<Universe>,
    mut fleet: ResMut<crate::craft::Fleet>,
    (rig, pose): (Res<crate::scene::CameraRig>, Res<crate::scene::CameraPose>),
    clock: Res<crate::Clock>,
) {
    for command in commands.read() {
        if let crate::actions::UiCommand::SetLayeredPlanets { on } = command {
            loading.enabled = *on;
        }
    }
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
    loading.invalidate_changed_inputs(&nav, &universe.0);
    loading.receive();
    loading.reuse_adopted(&fleet);
    let selected = match &nav.selection {
        Some(Selection::Planet(id)) => Some(id.as_str()),
        _ => None,
    };
    if loading.active.is_some() {
        loading.dispatch(selected);
    }
    let status_changed = adoption::refresh_status(
        &loading,
        nav.bypass_change_detection(),
        &mut universe.bypass_change_detection().0,
    );
    if adoption::publish(
        &mut loading,
        nav.bypass_change_detection(),
        &mut universe.bypass_change_detection().0,
        &mut fleet,
        &rig,
        &pose,
        clock.0.seconds,
    ) || status_changed
    {
        nav.set_changed();
        universe.set_changed();
    }
}

mod adoption;

/// Restore authoritative session facts before navigation/physics use a newly
/// generated visit or refreshed backend snapshot. This is replay, not adoption.
pub fn restore(
    mut nav: ResMut<Navigation>,
    mut universe: ResMut<Universe>,
    fleet: Res<crate::craft::Fleet>,
) {
    let mut changed = false;
    for system in &mut universe.bypass_change_detection().0.systems {
        changed |= fleet.charts.restore_system(system);
    }
    if changed {
        universe.set_changed();
    }
    if let Some(crate::Visit::Generated(system)) = &mut nav.bypass_change_detection().visit
        && fleet.charts.restore_system(system)
    {
        nav.set_changed();
    }
}

pub fn restore_visit(mut nav: ResMut<Navigation>, fleet: Res<crate::craft::Fleet>) {
    if let Some(crate::Visit::Generated(system)) = &mut nav.bypass_change_detection().visit
        && fleet.charts.restore_system(system)
    {
        nav.set_changed();
    }
}

#[cfg(test)]
mod tests;
