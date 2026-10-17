//! System data craft wait for, worked out off the frame update: on one
//! long-lived native thread, or the browser's dedicated craft worker, each
//! keeping its own warm `Sky`. Results are pure functions of seeds, so where
//! they are computed never changes a craft's path.
pub use space_simulation::craft::voyage::{Reply, Request};
use std::sync::{Mutex, mpsc};

pub struct Pending {
    result: Mutex<mpsc::Receiver<Reply>>,
}

impl Pending {
    pub fn start(request: Request) -> Option<Self> {
        let (send, receive) = mpsc::channel();
        #[cfg(not(target_arch = "wasm32"))]
        native::submit(request, send)?;
        #[cfg(target_arch = "wasm32")]
        web::submit(request, send)?;
        Some(Self {
            result: Mutex::new(receive),
        })
    }

    /// `None` while pending; `Some(Err)` on failure. Never waits.
    pub fn poll(&self) -> Option<Result<Reply, ()>> {
        match self.receiver().try_recv() {
            Ok(reply) => Some(Ok(reply)),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => Some(Err(())),
        }
    }

    fn receiver(&self) -> std::sync::MutexGuard<'_, mpsc::Receiver<Reply>> {
        self.result
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

#[cfg(not(target_arch = "wasm32"))]
mod native {
    use super::*;
    use std::sync::OnceLock;

    type Job = (Request, mpsc::Sender<Reply>);
    static WORKER: OnceLock<Mutex<mpsc::Sender<Job>>> = OnceLock::new();

    fn start() -> Option<Mutex<mpsc::Sender<Job>>> {
        let (send, receive) = mpsc::channel::<Job>();
        std::thread::Builder::new()
            .name("craft systems".into())
            .spawn(move || {
                let mut sky = space_simulation::cells::Sky::default();
                let threads = space_simulation::craft::voyage::threads();
                for (request, reply) in receive {
                    let _ = reply.send(request.answer(&mut sky, threads));
                }
            })
            .ok()?;
        Some(Mutex::new(send))
    }

    pub fn submit(request: Request, reply: mpsc::Sender<Reply>) -> Option<()> {
        if WORKER.get().is_none() {
            let worker = start()?;
            let _ = WORKER.set(worker);
        }
        WORKER
            .get()?
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .send((request, reply))
            .ok()
    }
}

#[cfg(target_arch = "wasm32")]
mod web {
    use super::*;
    use std::{
        cell::{Cell, RefCell},
        collections::HashMap,
        rc::Rc,
    };
    use wasm_bindgen::{JsCast, closure::Closure};
    use web_sys::{ErrorEvent, MessageEvent, Worker, WorkerOptions, WorkerType};

    type Replies = Rc<RefCell<HashMap<u64, mpsc::Sender<Reply>>>>;
    struct Running {
        worker: Worker,
        replies: Replies,
        next: u64,
        failed: Rc<Cell<bool>>,
        _message: Closure<dyn FnMut(MessageEvent)>,
        _error: Closure<dyn FnMut(ErrorEvent)>,
    }
    thread_local! {
        static WORKER: RefCell<Option<Running>> = const { RefCell::new(None) };
    }

    fn start() -> Option<Running> {
        let options = WorkerOptions::new();
        options.set_type(WorkerType::Module);
        let worker = Worker::new_with_options("craft-worker.js", &options).ok()?;
        let replies = Replies::default();
        let failed = Rc::new(Cell::new(false));
        let inbox = replies.clone();
        let message = Closure::<dyn FnMut(MessageEvent)>::new(move |event: MessageEvent| {
            let Some(text) = event.data().as_string() else {
                return;
            };
            let Ok((token, reply)) = serde_json::from_str::<(u64, Option<Reply>)>(&text) else {
                return;
            };
            // A failed request drops its sender, which the poller sees.
            if let Some(send) = inbox.borrow_mut().remove(&token)
                && let Some(reply) = reply
            {
                let _ = send.send(reply);
            }
        });
        let inbox = replies.clone();
        let invalid = failed.clone();
        let error = Closure::<dyn FnMut(ErrorEvent)>::new(move |_| {
            invalid.set(true);
            inbox.borrow_mut().clear(); // Disconnect pending requests.
        });
        worker.set_onmessage(Some(message.as_ref().unchecked_ref()));
        worker.set_onerror(Some(error.as_ref().unchecked_ref()));
        Some(Running {
            worker,
            replies,
            next: 0,
            failed,
            _message: message,
            _error: error,
        })
    }

    pub fn submit(request: Request, send: mpsc::Sender<Reply>) -> Option<()> {
        WORKER.with_borrow_mut(|slot| {
            if slot.is_none() {
                *slot = start();
            }
            let running = slot.as_mut()?;
            if running.failed.get() {
                return None;
            }
            running.next += 1;
            let token = running.next;
            let text = serde_json::to_string(&(token, request)).ok()?;
            running.worker.post_message(&text.into()).ok()?;
            running.replies.borrow_mut().insert(token, send);
            Some(())
        })
    }
}
