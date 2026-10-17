//! Each craft's coast prediction, worked on a slice at a time so no frame
//! pays for a whole one. The last finished prediction stays on show, with
//! its age, while a newer one is worked on.
use super::*;
use space_simulation::craft::{
    Input,
    predict::{Prediction, Predictor},
};

/// Prediction steps all craft may take in one frame together, about a
/// millisecond natively. WebAssembly's steps are slower, so it takes fewer.
#[cfg(not(target_arch = "wasm32"))]
pub const STEPS_PER_FRAME: usize = 250;
#[cfg(target_arch = "wasm32")]
pub const STEPS_PER_FRAME: usize = 120;
/// While coasting, start a new prediction once the craft is this share of
/// the shown one's period, or horizon when unbound, past its start. Below
/// `predict::LEAD`, so a bound path is still a whole lap ahead meanwhile.
const REFRESH_SHARE: f64 = 0.25;

#[derive(Default)]
pub struct Forecast {
    /// The last finished prediction, until a newer one finishes.
    pub shown: Option<Prediction>,
    shown_key: Option<Key>,
    working: Option<(Predictor, Key)>,
}

/// What a prediction was made under: different inputs or a different
/// reference make it obsolete.
#[derive(Clone, PartialEq)]
struct Key {
    input: Input,
    reference: Option<String>,
}

impl Key {
    fn of(craft: &Craft) -> Self {
        Self {
            input: craft.input,
            reference: craft.reference.clone(),
        }
    }
}

impl Forecast {
    /// Whether a newer prediction is being worked on.
    pub fn updating(&self) -> bool {
        self.working.is_some()
    }

    /// Whether the shown prediction was made under the craft's current
    /// inputs and reference.
    pub fn current(&self, craft: &Craft) -> bool {
        self.shown_key.as_ref() == Some(&Key::of(craft))
    }

    /// Take at most `steps` prediction steps for this craft.
    fn work(&mut self, craft: &Craft, field: &Field, steps: usize) {
        if craft.warp.engaged || matches!(craft.frame, Frame::Galaxy { .. }) {
            *self = Self::default();
            return;
        }
        let key = Key::of(craft);
        // Obsolete work is dropped rather than finished.
        if self.working.as_ref().is_some_and(|(_, k)| *k != key) {
            self.working = None;
        }
        if self.working.is_none() && self.due(craft, &key) {
            self.working = Predictor::new(craft, field).map(|p| (p, key));
        }
        let Some((predictor, _)) = &mut self.working else {
            return;
        };
        if predictor.run(field, steps) {
            let (predictor, key) = self.working.take().unwrap();
            self.shown = Some(predictor.into_prediction());
            self.shown_key = Some(key);
        }
    }

    /// Whether to start a new prediction. One in progress always finishes
    /// first, so a burn refreshing constantly never starves it.
    fn due(&self, craft: &Craft, key: &Key) -> bool {
        let Some(shown) = &self.shown else {
            return true;
        };
        if self.shown_key.as_ref() != Some(key) || thrusting(&craft.input) {
            return true;
        }
        let length = shown.period_seconds.unwrap_or(shown.horizon_seconds);
        craft.seconds() - shown.start_seconds > REFRESH_SHARE * length
    }
}

fn thrusting(input: &Input) -> bool {
    input.thrust != [0.0; 3]
        || input.controls.throttle != 0.0
        || input.controls.translation != [0.0; 3]
}

/// Works on every craft's prediction, sharing `STEPS_PER_FRAME` evenly so
/// none waits behind the others.
pub fn forecast(mut fleet: ResMut<Fleet>) {
    fleet.forecast(STEPS_PER_FRAME);
}

impl Fleet {
    pub fn forecast(&mut self, budget: usize) {
        let share = (budget / self.craft.len().max(1)).max(1);
        for live in &mut self.craft {
            live.forecast
                .work(&live.voyage.craft, &live.voyage.field, share);
        }
    }
}
