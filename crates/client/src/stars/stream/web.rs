//! The browser backend: a dedicated module Web Worker, falling back to a
//! slice of each frame.
use super::*;
use std::{
    cell::RefCell,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::Duration,
};
use wasm_bindgen::{JsCast, closure::Closure};
use web_sys::{ErrorEvent, MessageEvent, Worker, WorkerOptions, WorkerType};

/// Share of each frame spent loading.
const SLICE: Duration = Duration::from_millis(4);

/// The worker and its handlers, which must live as long as it does.
type Running = (
    Worker,
    Closure<dyn FnMut(MessageEvent)>,
    Closure<dyn FnMut(ErrorEvent)>,
);

thread_local! {
    static WORKER: RefCell<Option<Running>> = const { RefCell::new(None) };
}

#[derive(Resource)]
pub enum Backend {
    Worker {
        magnitude: f64,
        batches: Mutex<mpsc::Receiver<Batch>>,
        failed: Arc<AtomicBool>,
        next_aim: Option<[f64; 3]>,
        camera: Option<[f64; 3]>,
        awaiting_response: bool,
    },
    Inline(Box<Loader>),
}

type Batch = (Vec<Event>, usize, Duration, usize);

impl Backend {
    pub fn mode(&self) -> &'static str {
        match self {
            Self::Worker { .. } => "WEB WORKER",
            Self::Inline(_) => "FRAME SLICE",
        }
    }

    pub fn magnitude(&self) -> f64 {
        match self {
            Self::Worker { magnitude, .. } => *magnitude,
            Self::Inline(loader) => loader.magnitude(),
        }
    }

    /// One: the browser makes stars on one worker, or in frame slices.
    pub fn threads(&self) -> usize {
        1
    }

    pub fn take_worked(&self) -> Duration {
        Duration::ZERO
    }

    pub fn start(magnitude: f64) -> Self {
        // A restart replaces the worker; the old one would keep loading.
        WORKER.with_borrow_mut(|slot| {
            if let Some((worker, ..)) = slot.take() {
                worker.terminate();
            }
        });
        let options = WorkerOptions::new();
        options.set_type(WorkerType::Module);
        let url = format!("star-worker.js?magnitude={magnitude}");
        let Ok(worker) = Worker::new_with_options(&url, &options) else {
            warn!("Cannot start star worker; using the frame-sliced loader");
            return Self::Inline(Box::new(Loader::new(Sky::default(), magnitude)));
        };
        let (sender, receiver) = mpsc::channel();
        let failed = Arc::new(AtomicBool::new(false));
        let invalid = failed.clone();
        let onmessage = Closure::<dyn FnMut(MessageEvent)>::new(move |event: MessageEvent| {
            let Ok(buffer) = event.data().dyn_into::<js_sys::ArrayBuffer>() else {
                warn!("Star worker returned a non-binary message");
                invalid.store(true, Ordering::Relaxed);
                return;
            };
            let bytes = js_sys::Uint8Array::new(&buffer).to_vec();
            if let Some(batch) = space_simulation::stream_wire::decode(&bytes) {
                let _ = sender.send(batch);
            } else {
                warn!("Star worker returned an invalid batch");
                invalid.store(true, Ordering::Relaxed);
            }
        });
        let error = failed.clone();
        let onerror = Closure::<dyn FnMut(ErrorEvent)>::new(move |event: ErrorEvent| {
            bevy::log::error!("Star worker failed: {}", event.message());
            error.store(true, Ordering::Relaxed);
        });
        worker.set_onmessage(Some(onmessage.as_ref().unchecked_ref()));
        worker.set_onerror(Some(onerror.as_ref().unchecked_ref()));
        WORKER.with_borrow_mut(|slot| *slot = Some((worker, onmessage, onerror)));
        Self::Worker {
            magnitude,
            batches: Mutex::new(receiver),
            failed,
            next_aim: None,
            camera: None,
            awaiting_response: false,
        }
    }

    pub fn exchange(&mut self, aim: Option<[f64; 3]>) -> Option<Batch> {
        if let Self::Worker {
            failed,
            camera,
            magnitude,
            ..
        } = self
            && failed.load(Ordering::Relaxed)
        {
            warn!("Falling back to frame-sliced galaxy loading");
            WORKER.with_borrow_mut(|slot| {
                if let Some((worker, ..)) = slot.take() {
                    worker.terminate();
                }
            });
            let mut loader = Loader::new(Sky::default(), *magnitude);
            if let Some(camera) = *camera {
                loader.aim(camera);
            }
            *self = Self::Inline(Box::new(loader));
        }
        match self {
            Self::Worker {
                batches,
                failed,
                next_aim,
                camera,
                awaiting_response,
                ..
            } => {
                // Keep only the latest camera position while the worker is
                // busy, as the native channel's try_iter().last() does.
                if let Some(position) = aim {
                    *camera = Some(position);
                    *next_aim = Some(position);
                }
                let batches = batches.get_mut().ok()?;
                let mut merged: Option<Batch> = None;
                // Avoid draining an unlimited number of messages in a frame.
                let start = Instant::now();
                for (events, pending, duration, cancelled) in batches.try_iter() {
                    let batch = merged.get_or_insert_with(Default::default);
                    batch.0.extend(events);
                    batch.1 = pending;
                    batch.2 += duration;
                    batch.3 += cancelled;
                    if start.elapsed() >= Duration::from_millis(2) {
                        break;
                    }
                }
                if merged.is_some() {
                    *awaiting_response = false;
                }
                if !*awaiting_response && let Some(camera) = next_aim.take() {
                    let array = js_sys::Array::new();
                    for value in camera {
                        array.push(&value.into());
                    }
                    WORKER.with_borrow(|slot| {
                        if let Some((worker, ..)) = slot
                            && let Err(err) = worker.post_message(&array)
                        {
                            warn!("Cannot aim star worker: {err:?}");
                            failed.store(true, Ordering::Relaxed);
                        }
                    });
                    *awaiting_response = true;
                }
                merged
            }
            Self::Inline(loader) => {
                if let Some(camera) = aim {
                    loader.aim(camera);
                } else if loader.pending() == 0 {
                    return None;
                }
                let start = Instant::now();
                let events = loader.step(&mut || start.elapsed() < SLICE);
                Some((
                    events,
                    loader.pending(),
                    start.elapsed(),
                    loader.take_cancelled_jobs(),
                ))
            }
        }
    }
}
