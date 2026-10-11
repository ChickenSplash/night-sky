//! The native backend: a worker thread making stars with a share of the CPU's threads.
use super::*;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU64, Ordering},
    mpsc,
};
use std::time::Duration;

/// Work between checks for a new camera position.
const SLICE: Duration = Duration::from_millis(20);
/// Threads making each cell's stars, the stream thread included: three
/// quarters of the machine's, leaving the rest for the app.
fn star_threads() -> usize {
    let available = std::thread::available_parallelism().map_or(1, |n| n.get());
    (available * 3 / 4).max(1)
}

type Batch = (Vec<Event>, usize, Duration, usize);

#[derive(Resource)]
pub struct Backend {
    magnitude: f64,
    aim: mpsc::Sender<[f64; 3]>,
    batches: Mutex<mpsc::Receiver<Batch>>,
    threads: usize,
    /// Nanoseconds the loader's other threads have worked, not yet taken.
    worked: Arc<AtomicU64>,
}

impl Backend {
    pub fn mode(&self) -> &'static str {
        "NATIVE THREAD"
    }

    pub fn magnitude(&self) -> f64 {
        self.magnitude
    }

    pub fn threads(&self) -> usize {
        self.threads
    }

    pub fn take_worked(&self) -> Duration {
        Duration::from_nanos(self.worked.swap(0, Ordering::Relaxed))
    }

    pub fn start(magnitude: f64) -> Self {
        let (aim, aims) = mpsc::channel();
        let (sender, batches) = mpsc::channel();
        let threads = star_threads();
        bevy::log::info!("Making stars on {threads} threads");
        let worked = Arc::new(AtomicU64::new(0));
        let tally = worked.clone();
        let work = move || {
            let mut loader = Loader::new(Sky::default(), magnitude).with_threads(threads);
            loop {
                // Idle until the camera moves.
                let mut latest = None;
                if loader.pending() == 0 {
                    match aims.recv() {
                        Ok(camera) => latest = Some(camera),
                        Err(_) => return,
                    }
                }
                latest = aims.try_iter().last().or(latest);
                if let Some(camera) = latest {
                    loader.aim(camera);
                }
                let start = std::time::Instant::now();
                let events = loader.step(&mut || start.elapsed() < SLICE);
                let other = loader.take_worked().as_nanos() as u64;
                tally.fetch_add(other, Ordering::Relaxed);
                if sender
                    .send((
                        events,
                        loader.pending(),
                        start.elapsed(),
                        loader.take_cancelled_jobs(),
                    ))
                    .is_err()
                {
                    return;
                }
            }
        };
        if let Err(err) = std::thread::Builder::new()
            .name("galaxy-stream".into())
            .spawn(work)
        {
            bevy::log::error!("Cannot start the galaxy stream: {err}");
        }
        Self {
            magnitude,
            aim,
            batches: Mutex::new(batches),
            threads,
            worked,
        }
    }

    /// Sends a new camera position, and gathers what the worker has built.
    pub fn exchange(&mut self, aim: Option<[f64; 3]>) -> Option<Batch> {
        if let Some(camera) = aim {
            let _ = self.aim.send(camera);
        }
        let batches = self.batches.get_mut().ok()?;
        let mut merged: Option<Batch> = None;
        for (events, pending, duration, cancelled) in batches.try_iter() {
            let batch = merged.get_or_insert_with(Default::default);
            batch.0.extend(events);
            batch.1 = pending;
            batch.2 += duration;
            batch.3 += cancelled;
        }
        merged
    }
}
