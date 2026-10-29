//! The native search service: one thread running every open request in
//! turn (`Runner`), sharing the background CPU budget for large cells.
use super::engine::SearchRequest;
use super::runner::{Order, Runner, SearchUpdate};
use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};
use std::time::{Duration, Instant};

/// Work per slice: a fraction of a second, so cancellations and new
/// requests are seen promptly.
const SLICE: u64 = 100_000;

enum Message {
    Order(Order),
}

pub struct SearchService {
    send: Sender<Message>,
    updates: Receiver<SearchUpdate>,
}

impl SearchService {
    /// Stops when dropped.
    pub fn spawn() -> Self {
        let (send, inbox) = channel();
        let (outbox, updates) = channel();
        std::thread::Builder::new()
            .name("search".into())
            .spawn(move || serve(&inbox, &outbox))
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

    /// Updates since the last call, without waiting.
    pub fn updates(&self) -> Vec<SearchUpdate> {
        self.updates.try_iter().collect()
    }

    /// The next update, waiting up to `timeout`.
    pub fn wait(&self, timeout: Duration) -> Option<SearchUpdate> {
        self.updates.recv_timeout(timeout).ok()
    }
}

fn serve(inbox: &Receiver<Message>, outbox: &Sender<SearchUpdate>) {
    let mut runner = Runner::default();
    let budget = crate::background::shared();
    let mut demand = None;
    let mut pending = None;
    let epoch = Instant::now();
    let clock = || epoch.elapsed().as_secs_f64() * 1e3;
    loop {
        if runner.busy() || pending.is_some() {
            demand.get_or_insert_with(|| budget.search_demand());
        } else {
            demand = None;
        }
        let message = if pending.is_some() {
            pending.take()
        } else if runner.busy() {
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
            Some(Message::Order(order @ Order::Submit(_))) => {
                demand.get_or_insert_with(|| budget.search_demand());
                let Some(_permit) = budget.try_search() else {
                    // Exact-ID and single-system requests can generate bodies
                    // during submission, so initialization needs a slot too.
                    pending = Some(Message::Order(order));
                    std::thread::sleep(Duration::from_millis(2));
                    continue;
                };
                runner.order(order, clock())
            }
            Some(Message::Order(order)) => runner.order(order, clock()),
            None => {
                let Some(coordinator) = budget.try_search() else {
                    // Poll commands again shortly, without spinning or blocking
                    // cancellation behind another subsystem's numerical solve.
                    std::thread::sleep(Duration::from_millis(2));
                    continue;
                };
                let mut permits = vec![coordinator];
                while let Some(permit) = budget.try_search_extra() {
                    permits.push(permit);
                }
                runner.step(SLICE, permits.len(), clock)
            }
        };
        if let Some(update) = update
            && outbox.send(update).is_err()
        {
            return;
        }
    }
}
