//! Runs every open request in turn, a slice of work each, on whatever runs
//! it: the native search thread, the browser's search worker, or the
//! browser's frame-sliced fallback. Clocks are the caller's, in
//! milliseconds, so this builds for every target.
use super::engine::{Progress, Search, SearchRequest, SearchResult, State};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

/// What a request has found so far, tagged with its ID.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SearchUpdate {
    Progress {
        id: u64,
        progress: Progress,
        results: Vec<SearchResult>,
    },
    Refused {
        id: u64,
        reason: String,
    },
}

impl SearchUpdate {
    pub fn id(&self) -> u64 {
        match self {
            Self::Progress { id, .. } | Self::Refused { id, .. } => *id,
        }
    }
}

/// What a front end sends whoever runs its searches; the browser's search
/// worker takes these as JSON.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Order {
    Submit(Box<SearchRequest>),
    /// Stops a request at its next slice; it reports `Cancelled` once.
    Cancel(u64),
}

struct Running {
    search: Search,
    started_ms: f64,
}

#[derive(Default)]
pub struct Runner {
    open: VecDeque<Running>,
}

impl Runner {
    /// Whether any request still has work to do.
    pub fn busy(&self) -> bool {
        !self.open.is_empty()
    }

    /// Takes in an order at `now_ms`; a refusal or cancellation is
    /// reported at once.
    pub fn order(&mut self, order: Order, now_ms: f64) -> Option<SearchUpdate> {
        match order {
            Order::Submit(request) => {
                let id = request.id;
                match Search::new(*request) {
                    Ok(search) => {
                        self.open.push_back(Running {
                            search,
                            started_ms: now_ms,
                        });
                        None
                    }
                    Err(reason) => Some(SearchUpdate::Refused { id, reason }),
                }
            }
            Order::Cancel(id) => {
                let at = self.open.iter().position(|r| r.search.id == id)?;
                let mut running = self.open.remove(at).expect("found above");
                running.search.cancel();
                Some(report(&running, now_ms))
            }
        }
    }

    /// Up to `budget` work on the next open request, which goes to the back
    /// of the line unless it has finished; what it has found so far.
    pub fn step(
        &mut self,
        budget: u64,
        threads: usize,
        clock: impl Fn() -> f64,
    ) -> Option<SearchUpdate> {
        let mut running = self.open.pop_front()?;
        running.search.step(budget, threads);
        let update = report(&running, clock());
        if running.search.progress().state == State::Running {
            self.open.push_back(running);
        }
        Some(update)
    }
}

fn report(running: &Running, now_ms: f64) -> SearchUpdate {
    let mut progress = running.search.progress();
    progress.elapsed_ms = (now_ms - running.started_ms).max(0.0);
    SearchUpdate::Progress {
        id: running.search.id,
        progress,
        results: running.search.results(),
    }
}
