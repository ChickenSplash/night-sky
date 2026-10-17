//! Demand-driven body loading. Never run generation in a frame update.
use space_shared::StarSystem;
use std::sync::{Mutex, mpsc};

pub struct Pending {
    result: Mutex<mpsc::Receiver<Option<StarSystem>>>,
}

impl Pending {
    pub fn start(id: String, position_ly: [f64; 3]) -> Option<Self> {
        let (send, receive) = mpsc::channel();
        #[cfg(not(target_arch = "wasm32"))]
        std::thread::Builder::new()
            .name("craft bodies".into())
            .spawn(move || {
                let _ = send.send(space_simulation::generated_system(&id, position_ly));
            })
            .ok()?;
        #[cfg(target_arch = "wasm32")]
        web::submit(id, position_ly, send)?;
        Some(Self {
            result: Mutex::new(receive),
        })
    }

    /// None while pending; Some(None) on failure. Never wait for a worker.
    pub fn poll(&self) -> Option<Option<StarSystem>> {
        match self
            .result
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .try_recv()
        {
            Ok(system) => Some(system),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => Some(None),
        }
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

    type Replies = Rc<RefCell<HashMap<u64, mpsc::Sender<Option<StarSystem>>>>>;
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
            let Ok((token, system)) = serde_json::from_str::<(u64, Option<StarSystem>)>(&text)
            else {
                return;
            };
            if let Some(send) = inbox.borrow_mut().remove(&token) {
                let _ = send.send(system);
            }
        });
        let inbox = replies.clone();
        let invalid = failed.clone();
        let error = Closure::<dyn FnMut(ErrorEvent)>::new(move |_| {
            invalid.set(true);
            inbox.borrow_mut().clear(); // Disconnect pending requests; no inline fallback.
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

    pub fn submit(
        id: String,
        position: [f64; 3],
        send: mpsc::Sender<Option<StarSystem>>,
    ) -> Option<()> {
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
            let text = serde_json::to_string(&(token, id, position)).ok()?;
            running.worker.post_message(&text.into()).ok()?;
            running.replies.borrow_mut().insert(token, send);
            Some(())
        })
    }
}
