//! The browser's search service: a dedicated module Web Worker
//! (`web/search-worker.js`), separate from the star worker, falling back to
//! a slice of each frame. Orders and updates cross as JSON.
use bevy::platform::time::Instant;
use bevy::prelude::*;
use space_simulation::search::{Order, Runner, SearchRequest, SearchUpdate, State};
use std::cell::RefCell;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc,
};
use std::time::Duration;
use wasm_bindgen::{JsCast, closure::Closure};
use web_sys::{ErrorEvent, MessageEvent, Worker, WorkerOptions, WorkerType};

/// Share of each frame spent searching without the worker.
const FRAME_SLICE: Duration = Duration::from_millis(4);
/// Work between checks of the frame's time; a few milliseconds at most.
const INLINE_BUDGET: u64 = 10_000;

/// The worker and its handlers, which must live as long as it does.
type Running = (
    Worker,
    Closure<dyn FnMut(MessageEvent)>,
    Closure<dyn FnMut(ErrorEvent)>,
);

thread_local! {
    static WORKER: RefCell<Option<Running>> = const { RefCell::new(None) };
}

enum Mode {
    Worker {
        updates: mpsc::Receiver<SearchUpdate>,
        failed: Arc<AtomicBool>,
    },
    Inline {
        runner: Runner,
        /// Refusals and cancellations, reported at the next poll.
        replies: Vec<SearchUpdate>,
    },
}

pub struct Service {
    mode: RefCell<Mode>,
    /// Requests not yet finished, handed to the fallback if the worker fails.
    open: RefCell<Vec<SearchRequest>>,
}

impl Service {
    /// The browser searches on one thread, whatever `threads` says.
    pub fn spawn(_threads: usize) -> Self {
        Self {
            mode: RefCell::new(start().unwrap_or_else(|| {
                warn!(target: super::TARGET, "Cannot start the search worker; searching in frame slices");
                inline()
            })),
            open: RefCell::new(Vec::new()),
        }
    }

    pub fn mode(&self) -> &'static str {
        match &*self.mode.borrow() {
            Mode::Worker { .. } => "WEB WORKER",
            Mode::Inline { .. } => "FRAME SLICE",
        }
    }

    pub fn submit(&self, request: SearchRequest) {
        self.open.borrow_mut().push(request.clone());
        self.send(Order::Submit(Box::new(request)));
    }

    pub fn cancel(&self, id: u64) {
        self.send(Order::Cancel(id));
    }

    pub fn set_threads(&self, _threads: usize) {}

    fn send(&self, order: Order) {
        match &mut *self.mode.borrow_mut() {
            Mode::Worker { failed, .. } => {
                let Ok(json) = serde_json::to_string(&order) else {
                    return;
                };
                WORKER.with_borrow(|slot| {
                    if let Some((worker, ..)) = slot
                        && let Err(err) = worker.post_message(&json.into())
                    {
                        warn!(target: super::TARGET, "Cannot reach the search worker: {err:?}");
                        failed.store(true, Ordering::Relaxed);
                    }
                });
            }
            Mode::Inline { runner, replies } => {
                replies.extend(runner.order(order, js_sys::Date::now()));
            }
        }
    }

    /// Updates since the last call; without the worker, a slice of
    /// searching first.
    pub fn updates(&self) -> Vec<SearchUpdate> {
        self.fall_back_if_failed();
        let updates: Vec<SearchUpdate> = match &mut *self.mode.borrow_mut() {
            Mode::Worker { updates, .. } => updates.try_iter().collect(),
            Mode::Inline { runner, replies } => {
                let mut out = std::mem::take(replies);
                let start = Instant::now();
                while runner.busy() && start.elapsed() < FRAME_SLICE {
                    out.extend(runner.step(INLINE_BUDGET, 1, js_sys::Date::now));
                }
                out
            }
        };
        let finished = |u: &SearchUpdate| match u {
            SearchUpdate::Refused { .. } => true,
            SearchUpdate::Progress { progress, .. } => progress.state != State::Running,
        };
        let mut open = self.open.borrow_mut();
        for update in updates.iter().filter(|u| finished(u)) {
            open.retain(|r| r.id != update.id());
        }
        updates
    }

    /// Runs the unfinished requests again in frame slices if the worker
    /// has failed; they start over.
    fn fall_back_if_failed(&self) {
        let failed = matches!(&*self.mode.borrow(), Mode::Worker { failed, .. } if failed.load(Ordering::Relaxed));
        if !failed {
            return;
        }
        warn!(target: super::TARGET, "Falling back to searching in frame slices");
        terminate();
        let mut mode = inline();
        if let Mode::Inline { runner, replies } = &mut mode {
            for request in self.open.borrow().iter().cloned() {
                replies.extend(runner.order(Order::Submit(Box::new(request)), js_sys::Date::now()));
            }
        }
        *self.mode.borrow_mut() = mode;
    }
}

fn inline() -> Mode {
    Mode::Inline {
        runner: Runner::default(),
        replies: Vec::new(),
    }
}

fn terminate() {
    WORKER.with_borrow_mut(|slot| {
        if let Some((worker, ..)) = slot.take() {
            worker.terminate();
        }
    });
}

fn start() -> Option<Mode> {
    terminate();
    let options = WorkerOptions::new();
    options.set_type(WorkerType::Module);
    let worker = Worker::new_with_options("search-worker.js", &options).ok()?;
    let (sender, updates) = mpsc::channel();
    let failed = Arc::new(AtomicBool::new(false));
    let invalid = failed.clone();
    let onmessage = Closure::<dyn FnMut(MessageEvent)>::new(move |event: MessageEvent| match event
        .data()
        .as_string()
        .and_then(|json| serde_json::from_str::<SearchUpdate>(&json).ok())
    {
        Some(update) => {
            let _ = sender.send(update);
        }
        None => {
            warn!(target: super::TARGET, "The search worker sent an unreadable update");
            invalid.store(true, Ordering::Relaxed);
        }
    });
    let broken = failed.clone();
    let onerror = Closure::<dyn FnMut(ErrorEvent)>::new(move |event: ErrorEvent| {
        error!(target: super::TARGET, "Search worker failed: {}", event.message());
        broken.store(true, Ordering::Relaxed);
    });
    worker.set_onmessage(Some(onmessage.as_ref().unchecked_ref()));
    worker.set_onerror(Some(onerror.as_ref().unchecked_ref()));
    WORKER.with_borrow_mut(|slot| *slot = Some((worker, onmessage, onerror)));
    Some(Mode::Worker { updates, failed })
}
