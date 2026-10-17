//! Generation-only WASM for the browser's workers, with no Bevy, DOM or
//! graphics context: `web/star-worker.js` streams stars with `StarWorker`
//! and `web/search-worker.js` runs searches with `SearchWorker`.
//! `web/craft-worker.js` answers what craft wait for with `craft_answer`:
//! nearby systems and their full bodies. Each worker has its own instance
//! and memory.
use space_simulation::search::{Order, Runner};
use space_simulation::{cells::Sky, stream::Loader, stream_wire};
use std::time::Duration;
use wasm_bindgen::prelude::*;

/// Search work per slice: under a second on one browser thread, so newer
/// orders are seen promptly.
const SEARCH_SLICE: u64 = 1_000_000;

thread_local! {
    /// Kept warm between requests: a region's first query integrates the
    /// coarse cells around it, which later ones reuse.
    static CRAFT_SKY: std::cell::RefCell<Sky> = std::cell::RefCell::new(Sky::default());
}

/// A craft's wait, as a JSON `voyage::Request`, answered as a JSON
/// `voyage::Reply`, or `null` when the request is not understood.
#[wasm_bindgen]
pub fn craft_answer(request: &str) -> String {
    use space_simulation::craft::voyage::Request;
    let Ok(request) = serde_json::from_str::<Request>(request) else {
        return "null".into();
    };
    let reply = CRAFT_SKY.with_borrow_mut(|sky| request.answer(sky, 1));
    serde_json::to_string(&reply).unwrap_or_else(|_| "null".into())
}

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

/// Every open search in turn, on one thread. Takes `Order`s and gives
/// `SearchUpdate`s, both as JSON.
#[wasm_bindgen]
#[derive(Default)]
pub struct SearchWorker(Runner);

#[wasm_bindgen]
impl SearchWorker {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self::default()
    }

    /// Takes in an order; a refusal or cancellation comes back at once.
    pub fn order(&mut self, json: &str) -> Option<String> {
        let update = match serde_json::from_str::<Order>(json) {
            Ok(order) => self.0.order(order, js_sys::Date::now())?,
            Err(err) => {
                let id = serde_json::from_str::<serde_json::Value>(json)
                    .ok()
                    .and_then(|v| v.pointer("/submit/id")?.as_u64())?;
                space_simulation::search::SearchUpdate::Refused {
                    id,
                    reason: format!("the search worker could not read the request: {err}"),
                }
            }
        };
        serde_json::to_string(&update).ok()
    }

    pub fn busy(&self) -> bool {
        self.0.busy()
    }

    /// A slice of the next open search; what it has found so far.
    pub fn step(&mut self) -> Option<String> {
        let update = self.0.step(SEARCH_SLICE, 1, js_sys::Date::now)?;
        serde_json::to_string(&update).ok()
    }
}
