//! Platform transport for the shared bounded voyage loader. Native hosts use
//! its warm worker pool; the browser uses its dedicated generation worker.
//! Neither transport generates on the frame update or waits for a result.
#[cfg(target_arch = "wasm32")]
pub use browser::Transport;
#[cfg(not(target_arch = "wasm32"))]
pub use space_simulation::craft::voyage::loading::Native as Transport;

#[cfg(target_arch = "wasm32")]
mod browser {
    use space_simulation::craft::voyage::{Reply, Request, loading::Backend};
    use std::sync::{Mutex, mpsc};

    #[derive(Default)]
    pub struct Transport;

    impl Backend for Transport {
        type Job = Pending;
        fn start(&mut self, request: Request) -> Result<Pending, String> {
            Pending::start(request).ok_or_else(|| "Cannot start browser craft loader".into())
        }
        fn poll(job: &mut Pending) -> Option<Result<Reply, String>> {
            job.poll()
                .map(|result| result.map_err(|()| "Browser craft loader failed".into()))
        }
    }

    pub struct Pending {
        result: Mutex<mpsc::Receiver<Reply>>,
    }

    impl Pending {
        pub fn start(request: Request) -> Option<Self> {
            let (send, receive) = mpsc::channel();
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
                if slot.as_ref().is_some_and(|running| running.failed.get()) {
                    let running = slot.take().unwrap();
                    running.worker.terminate();
                }
                if slot.is_none() {
                    *slot = start();
                }
                let running = slot.as_mut()?;
                running.next += 1;
                let token = running.next;
                let text = serde_json::to_string(&(token, request)).ok()?;
                running.worker.post_message(&text.into()).ok()?;
                running.replies.borrow_mut().insert(token, send);
                Some(())
            })
        }
    }
}
