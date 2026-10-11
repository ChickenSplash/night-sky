//! Generation-only WASM. No Bevy, DOM or graphics context in this worker.
use space_simulation::{cells::Sky, stream::Loader, stream_wire};
use std::time::Duration;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub struct StarWorker(Loader);

#[wasm_bindgen]
impl StarWorker {
    /// Loads stars down to `magnitude`, the naked-eye limit if not a number.
    #[wasm_bindgen(constructor)]
    pub fn new(magnitude: f64) -> Self {
        let magnitude = if magnitude.is_finite() {
            magnitude
        } else {
            space_simulation::population::VISIBLE_MAGNITUDE
        };
        Self(Loader::new(Sky::default(), magnitude))
    }

    pub fn aim(&mut self, x: f64, y: f64, z: f64) {
        self.0.aim([x, y, z]);
    }

    pub fn pending(&self) -> usize {
        self.0.pending()
    }

    /// Check for a newer aim between batches, without blocking the worker's
    /// event loop for an entire large cell.
    pub fn step(&mut self) -> Vec<u8> {
        let start = js_sys::Date::now();
        let events = self.0.step(&mut || js_sys::Date::now() - start < 20.0);
        let elapsed = Duration::from_secs_f64(((js_sys::Date::now() - start) / 1000.0).max(0.0));
        stream_wire::encode(
            &events,
            self.0.pending(),
            elapsed,
            self.0.take_cancelled_jobs(),
        )
    }
}

impl Default for StarWorker {
    fn default() -> Self {
        Self::new(f64::NAN)
    }
}
