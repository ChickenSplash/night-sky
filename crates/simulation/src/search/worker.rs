//! The native search service: one thread running every open request in
//! turn (`Runner`), using `threads` threads for large cells. The browser
//! runs the same `Runner` in its own Web Worker instead.
use super::engine::SearchRequest;
use super::runner::{Order, Runner, SearchUpdate};
use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};
use std::time::{Duration, Instant};

/// Work per slice: a fraction of a second, so cancellations and new
/// requests are seen promptly.
const SLICE: u64 = 1_000_000;

enum Message {
    Order(Order),
    Threads(usize),
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
        let _ = self
            .send
            .send(Message::Order(Order::Submit(Box::new(request))));
    }

    /// Stops a request at its next slice; it reports `Cancelled` once.
    pub fn cancel(&self, id: u64) {
        let _ = self.send.send(Message::Order(Order::Cancel(id)));
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

fn serve(inbox: &Receiver<Message>, outbox: &Sender<SearchUpdate>, mut threads: usize) {
    let mut runner = Runner::default();
    let epoch = Instant::now();
    let clock = || epoch.elapsed().as_secs_f64() * 1e3;
    loop {
        let message = if runner.busy() {
            match inbox.try_recv() {
                Ok(message) => Some(message),
                Err(TryRecvError::Empty) => None,
                Err(TryRecvError::Disconnected) => return,
            }
        } else {
            match inbox.recv() {
                Ok(message) => Some(message),
                Err(_) => return,
            }
        };
        let update = match message {
            // Take in every waiting message before working.
            Some(Message::Order(order)) => runner.order(order, clock()),
            Some(Message::Threads(n)) => {
                threads = n;
                None
            }
            None => runner.step(SLICE, threads, clock),
        };
        if let Some(update) = update
            && outbox.send(update).is_err()
        {
            return;
        }
    }
}
