//! Object searches (`space_simulation::search`) for the client: the request
//! the object browser shows (`UiCommand::Search`), the developer API's own
//! requests, their log lines, and the debug window's settings and counters.
//! Requests never run on the app's thread: natively they run on
//! `SearchService`'s, in the browser on the search worker (`web.rs`).
use crate::actions::UiCommand;
use crate::scene::{CameraPose, CameraRig};
use bevy::prelude::*;
use serde::Serialize;
use space_simulation::search::live::{self, LiveHit, LiveObject};
use space_simulation::search::{
    DEFAULT_WORK_CAP, Origin, Progress, Query, SearchRequest, SearchResult, SearchUpdate, State,
};
use std::collections::VecDeque;
use std::ops::RangeInclusive;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

#[cfg(not(target_arch = "wasm32"))]
use space_simulation::search::SearchService as Service;
#[cfg(target_arch = "wasm32")]
mod web;
#[cfg(target_arch = "wasm32")]
use web::Service;
#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests;

/// The log target of search lines.
pub const TARGET: &str = "search";
pub const THREADS: RangeInclusive<usize> = 1..=64;
/// Work cap, in millions of units.
pub const WORK_CAP_MILLIONS: RangeInclusive<f64> = 1.0..=10_000.0;
/// Requests kept, finished or not; the oldest finished goes first.
pub const KEPT: usize = 32;

/// Who asked: the in-game browser's request is separate from the CLI's.
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Asker {
    Game,
    Api,
}

/// One request and what it has found so far.
#[derive(Clone, Debug, Serialize)]
pub struct Entry {
    pub id: u64,
    pub asker: Asker,
    /// In `ns search` text.
    pub query: String,
    pub origin_pc: [f64; 3],
    pub work_cap: u64,
    pub progress: Progress,
    /// Why it was refused, such as an unknown designation.
    pub error: Option<String>,
    pub result_count: usize,
    /// Left out of summaries.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub results: Option<Vec<SearchResult>>,
    /// Counts the updates taken in, so a copy can tell it is out of date.
    #[serde(skip)]
    pub revision: u64,
    /// Live objects it matched when sent, merged into every update.
    #[serde(skip)]
    pub live: Option<(Query, Vec<LiveHit>)>,
}

impl Entry {
    pub fn state(&self) -> &'static str {
        if self.error.is_some() {
            return "refused";
        }
        match self.progress.state {
            State::Running => "running",
            State::Done => "done",
            State::Capped => "capped",
            State::Cancelled => "cancelled",
        }
    }
}

/// Where the camera is, in parsecs from the galactic centre: a search's
/// origin, fixed when it is sent.
pub fn camera_pc(rig: &CameraRig, pose: &CameraPose) -> [f64; 3] {
    let home = space_simulation::galaxy::GalaxyModel::milky_way().home_pc;
    (bevy::math::DVec3::from_array(home)
        + rig.galactic_ly(pose)
            * (space_simulation::LY_METRES / space_simulation::galaxy::PC_METRES))
        .to_array()
}

#[derive(Resource)]
pub struct Searches {
    pub hub: Arc<Hub>,
    /// The request the object browser shows, and a copy of it as of its
    /// last update.
    pub shown: Option<u64>,
    pub shown_entry: Option<Entry>,
    /// Debug settings, reset on restart (`set_search`). The browser always
    /// searches on one thread.
    pub threads: usize,
    pub work_cap: u64,
}

impl Default for Searches {
    fn default() -> Self {
        // The star loader takes three quarters of the machine's threads.
        let available = std::thread::available_parallelism().map_or(1, |n| n.get());
        let threads = (available / 4).max(1);
        Self {
            hub: Arc::new(Hub::new(threads)),
            shown: None,
            shown_entry: None,
            threads,
            work_cap: DEFAULT_WORK_CAP,
        }
    }
}

impl Searches {
    /// Recent requests, newest first, without their results. Empty if the
    /// store is busy this frame.
    pub fn recent(&self) -> Vec<Entry> {
        self.hub.summaries(false).unwrap_or_default()
    }

    /// How searches run: natively on threads, in the browser on the search
    /// worker or, if that failed, in frame slices.
    pub fn mode(&self) -> &'static str {
        #[cfg(not(target_arch = "wasm32"))]
        return "NATIVE THREAD";
        #[cfg(target_arch = "wasm32")]
        self.hub.service.try_lock().map_or("BUSY", |s| s.mode())
    }
}

pub struct SearchPlugin;

impl Plugin for SearchPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Searches>().add_systems(
            Update,
            (
                poll,
                apply.after(crate::actions::apply),
                publish_live.after(crate::craft::step),
            ),
        );
    }
}

/// Takes in the service's updates, and refreshes the shown request's copy
/// and the systems it charts for navigation when it has changed.
fn poll(mut searches: ResMut<Searches>, mut nav: ResMut<crate::Navigation>) {
    searches.hub.poll(false);
    let Some(id) = searches.shown else {
        searches.shown_entry = None;
        return;
    };
    let since = searches
        .shown_entry
        .as_ref()
        .filter(|e| e.id == id)
        .map_or(0, |e| e.revision);
    if let Some(entry) = searches.hub.newer(id, since) {
        chart(&mut nav, &entry);
        searches.shown_entry = Some(entry);
    }
}

/// Hands the hub this frame's craft, which new requests match against.
fn publish_live(
    searches: Res<Searches>,
    fleet: Res<crate::craft::Fleet>,
    universe: Res<crate::Universe>,
    clock: Res<crate::Clock>,
) {
    if let Ok(mut objects) = searches.hub.live.try_lock() {
        *objects = crate::craft::live_objects(&fleet, &universe.0, clock.0.seconds);
    }
}

/// Every system a request found, so its results can be visited.
fn chart(nav: &mut crate::Navigation, entry: &Entry) {
    use space_simulation::objects::ObjectRef;
    nav.charted.clear();
    for result in entry.results.iter().flatten() {
        if let ObjectRef::Star { id } | ObjectRef::Barycentre { id } | ObjectRef::Planet { id } =
            &result.object
        {
            let key = space_simulation::multiple::system_key(id);
            nav.charted.insert(key.to_owned(), result.position_pc);
        }
    }
}

/// The search commands; `actions::apply` leaves them to this.
fn apply(
    mut commands: MessageReader<UiCommand>,
    mut searches: ResMut<Searches>,
    rig: Res<CameraRig>,
    pose: Res<CameraPose>,
) {
    for command in commands.read() {
        match command {
            UiCommand::Search { query } => {
                let Ok(query) = Query::parse(query) else {
                    continue;
                };
                if let Some(old) = searches.shown.take() {
                    searches.hub.cancel(old);
                }
                let origin = camera_pc(&rig, &pose);
                let cap = searches.work_cap;
                searches.shown = searches.hub.submit(Asker::Game, query, origin, cap).ok();
            }
            UiCommand::ClearSearch => {
                if let Some(old) = searches.shown {
                    searches.hub.cancel(old);
                }
                searches.shown = None;
            }
            UiCommand::SetSearch {
                threads,
                work_cap_millions,
            } => {
                if let Some(threads) = threads {
                    searches.threads = (*threads).clamp(*THREADS.start(), *THREADS.end());
                    searches.hub.set_threads(searches.threads);
                }
                if let Some(millions) = work_cap_millions {
                    let millions =
                        millions.clamp(*WORK_CAP_MILLIONS.start(), *WORK_CAP_MILLIONS.end());
                    searches.work_cap = (millions * 1e6) as u64;
                    searches.hub.set_work_cap(searches.work_cap);
                }
                info!(
                    target: TARGET,
                    "search threads {}, work cap {} M",
                    searches.threads,
                    searches.work_cap / 1_000_000
                );
            }
            _ => {}
        }
    }
}

/// One line about a finished request, for the log.
fn summary(entry: &Entry) -> String {
    let p = &entry.progress;
    let reach = if p.state == State::Capped {
        format!(", searched out to {:.1} ly", p.complete_to_ly)
    } else {
        String::new()
    };
    format!(
        "#{} {} · {} results{reach} · {:.2} s · {:.1} M stars tried, {:.1} M rejected early, {} systems tested, {} cells · cache {:.0} MB",
        entry.id,
        entry.state(),
        entry.result_count,
        p.elapsed_ms / 1e3,
        p.stars as f64 / 1e6,
        p.rejected_early as f64 / 1e6,
        p.evaluated,
        p.cells,
        p.cache_bytes as f64 / 1e6,
    )
}

/// The requests kept by ID, and the service running them. Natively it is
/// shared by the app and the developer API's thread; each lock is held
/// briefly and never with the other.
pub struct Hub {
    service: Mutex<Service>,
    store: Mutex<VecDeque<Entry>>,
    /// The app's live objects, such as craft, as of its last frame.
    pub live: Mutex<Vec<LiveObject>>,
    next: AtomicU64,
    /// For requests that do not give one (`set_search`).
    work_cap: AtomicU64,
}

impl Hub {
    pub fn new(threads: usize) -> Self {
        Self {
            service: Mutex::new(Service::spawn(threads)),
            store: Mutex::new(VecDeque::new()),
            live: Mutex::new(Vec::new()),
            next: AtomicU64::new(1),
            work_cap: AtomicU64::new(DEFAULT_WORK_CAP),
        }
    }

    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    pub fn work_cap(&self) -> u64 {
        self.work_cap.load(Ordering::Relaxed)
    }

    pub fn set_work_cap(&self, cap: u64) {
        self.work_cap.store(cap, Ordering::Relaxed);
    }

    /// Checks and sends a query; its ID, or why it was refused.
    pub fn submit(
        &self,
        asker: Asker,
        query: Query,
        origin_pc: [f64; 3],
        work_cap: u64,
    ) -> Result<u64, String> {
        let text = query.to_text();
        if let Err(reason) = query.check() {
            warn!(target: TARGET, "{asker:?} search refused: {text}: {reason}");
            return Err(reason);
        }
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        let [x, y, z] = origin_pc;
        info!(target: TARGET, "#{id} {asker:?}: {text} · from {x:.1} {y:.1} {z:.1} pc");
        let hits = self
            .live
            .lock()
            .map(|objects| live::matching(&query, origin_pc, &objects))
            .unwrap_or_default();
        // A live object's name or ID is no catalogue designation: it is
        // found here, without the engine.
        let found_live = !query.text.is_empty() && !hits.is_empty();
        let results = live::merge(&query, Vec::new(), hits.clone());
        let mut cancel = None;
        if let Ok(mut store) = self.store.lock() {
            store.push_front(Entry {
                id,
                asker,
                query: text,
                origin_pc,
                work_cap,
                progress: Progress {
                    state: if found_live {
                        State::Done
                    } else {
                        State::Running
                    },
                    ..Progress::default()
                },
                error: None,
                result_count: results.len(),
                results: Some(results),
                revision: 1,
                live: (!hits.is_empty()).then(|| (query.clone(), hits)),
            });
            if store.len() > KEPT {
                let at = store
                    .iter()
                    .rposition(|e| e.progress.state != State::Running)
                    .unwrap_or(store.len() - 1);
                let dropped = store.remove(at).expect("in range");
                if dropped.progress.state == State::Running {
                    cancel = Some(dropped.id);
                }
            }
        }
        if let Ok(service) = self.service.lock() {
            if let Some(old) = cancel {
                service.cancel(old);
            }
            if found_live {
                return Ok(id);
            }
            service.submit(SearchRequest {
                id,
                query,
                origin: Origin::Position { pc: origin_pc },
                work_cap,
            });
        }
        Ok(id)
    }

    pub fn cancel(&self, id: u64) {
        if let Ok(service) = self.service.lock() {
            service.cancel(id);
        }
    }

    pub fn set_threads(&self, threads: usize) {
        if let Ok(service) = self.service.lock() {
            service.set_threads(threads);
        }
    }

    /// Takes in what the service has found. `wait` blocks for the locks;
    /// the app never does.
    pub fn poll(&self, wait: bool) {
        let updates = match if wait {
            self.service.lock().ok()
        } else {
            self.service.try_lock().ok()
        } {
            Some(service) => service.updates(),
            None => return,
        };
        if updates.is_empty() {
            return;
        }
        let Ok(mut store) = self.store.lock() else {
            return;
        };
        for update in updates {
            let Some(entry) = store.iter_mut().find(|e| e.id == update.id()) else {
                continue;
            };
            entry.revision += 1;
            match update {
                SearchUpdate::Refused { reason, .. } => {
                    warn!(target: TARGET, "#{} refused: {reason}", entry.id);
                    entry.error = Some(reason);
                    entry.progress.state = State::Done;
                }
                SearchUpdate::Progress {
                    progress, results, ..
                } => {
                    let finished =
                        entry.progress.state == State::Running && progress.state != State::Running;
                    entry.progress = progress;
                    let results = match &entry.live {
                        Some((query, hits)) => live::merge(query, results, hits.clone()),
                        None => results,
                    };
                    entry.result_count = results.len();
                    entry.results = Some(results);
                    if finished {
                        info!(target: TARGET, "{}", summary(entry));
                    }
                }
            }
        }
    }

    /// Recent requests, newest first; `None` if the store is busy and
    /// `wait` is false.
    pub fn summaries(&self, wait: bool) -> Option<Vec<Entry>> {
        let store = if wait {
            self.store.lock().ok()?
        } else {
            self.store.try_lock().ok()?
        };
        Some(
            store
                .iter()
                .map(|e| Entry {
                    results: None,
                    ..e.clone()
                })
                .collect(),
        )
    }

    /// A copy of a request if it has changed since `revision`; `None`
    /// too if the store is busy, so the app never waits.
    pub fn newer(&self, id: u64, revision: u64) -> Option<Entry> {
        self.store
            .try_lock()
            .ok()?
            .iter()
            .find(|e| e.id == id && e.revision > revision)
            .cloned()
    }

    /// One request with its results so far.
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    pub fn get(&self, id: u64) -> Option<Entry> {
        self.poll(true);
        self.store.lock().ok()?.iter().find(|e| e.id == id).cloned()
    }
}
