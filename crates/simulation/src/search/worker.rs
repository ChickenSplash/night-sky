//! The native search service: one thread running every open request in
//! turn, a slice of work each, using `threads` threads for large cells. The
//! browser runs the same `Search` in its own Web Worker instead.
use super::engine::{Progress, Search, SearchRequest, SearchResult, State};
use std::collections::VecDeque;
use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};
use std::time::{Duration, Instant};

/// Work per slice: a fraction of a second, so cancellations and new
/// requests are seen promptly.
const SLICE: u64 = 1_000_000;

enum Message {
    Submit(Box<SearchRequest>),
    Cancel(u64),
    Threads(usize),
}

/// What a request has found so far, tagged with its ID.
#[derive(Clone, Debug)]
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

pub struct SearchService {
    send: Sender<Message>,
    updates: Receiver<SearchUpdate>,
}

impl SearchService {
    /// Stops when dropped.
    pub fn spawn(threads: usize) -> Self {
        let (send, inbox) = channel();
        let (outbox, updates) = channel();
        std::thread::Builder::new()
            .name("search".into())
            .spawn(move || serve(&inbox, &outbox, threads.max(1)))
            .expect("could not start the search thread");
        Self { send, updates }
    }

    pub fn submit(&self, request: SearchRequest) {
        let _ = self.send.send(Message::Submit(Box::new(request)));
    }

    /// Stops a request at its next slice; it reports `Cancelled` once.
    pub fn cancel(&self, id: u64) {
        let _ = self.send.send(Message::Cancel(id));
    }

    /// Threads for the next slices of every request.
    pub fn set_threads(&self, threads: usize) {
        let _ = self.send.send(Message::Threads(threads.max(1)));
    }

    /// Updates since the last call, without waiting.
    pub fn updates(&self) -> Vec<SearchUpdate> {
        self.updates.try_iter().collect()
    }

    /// The next update, waiting up to `timeout`.
    pub fn wait(&self, timeout: Duration) -> Option<SearchUpdate> {
        self.updates.recv_timeout(timeout).ok()
    }
}

struct Running {
    search: Search,
    started: Instant,
}

fn serve(inbox: &Receiver<Message>, outbox: &Sender<SearchUpdate>, mut threads: usize) {
    let mut open: VecDeque<Running> = VecDeque::new();
    loop {
        let message = if open.is_empty() {
            match inbox.recv() {
                Ok(message) => Some(message),
                Err(_) => return,
            }
        } else {
            match inbox.try_recv() {
                Ok(message) => Some(message),
                Err(TryRecvError::Empty) => None,
                Err(TryRecvError::Disconnected) => return,
            }
        };
        if let Some(message) = message {
            match message {
                Message::Submit(request) => {
                    let id = request.id;
                    match Search::new(*request) {
                        Ok(search) => open.push_back(Running {
                            search,
                            started: Instant::now(),
                        }),
                        Err(reason) => {
                            let _ = outbox.send(SearchUpdate::Refused { id, reason });
                        }
                    }
                }
                Message::Threads(n) => threads = n,
                Message::Cancel(id) => {
                    if let Some(at) = open.iter().position(|r| r.search.id == id) {
                        let mut running = open.remove(at).expect("found above");
                        running.search.cancel();
                        let _ = report(outbox, &running);
                    }
                }
            }
            // Take in every waiting message before working.
            continue;
        }
        let Some(mut running) = open.pop_front() else {
            continue;
        };
        running.search.step(SLICE, threads);
        if report(outbox, &running).is_err() {
            return;
        }
        if running.search.progress().state == State::Running {
            open.push_back(running);
        }
    }
}

fn report(
    outbox: &Sender<SearchUpdate>,
    running: &Running,
) -> Result<(), std::sync::mpsc::SendError<SearchUpdate>> {
    let mut progress = running.search.progress();
    progress.elapsed_ms = running.started.elapsed().as_secs_f64() * 1e3;
    outbox.send(SearchUpdate::Progress {
        id: running.search.id,
        progress,
        results: running.search.results(),
    })
}
