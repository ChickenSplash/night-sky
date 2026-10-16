//! Where a craft would go if it coasted from now: its current translational
//! state, every propulsion input off, under the same summed gravity and
//! Verlet steps as real flight, but with a prediction-only adaptive step.
//! The real craft and clock are never touched.
//!
//! - The whole path is relative to the orbit reference at its start, so an
//!   orbit closes rather than smearing along the reference's own motion.
//!   Later reference changes are reported as events, not spliced in.
//! - Work is bounded: a horizon of about one period when bound, a finite
//!   one otherwise, and at most `MAX_STEPS` steps, each kept as a sample.
//!   `Predictor::run` does a slice of the work at a time.
use super::*;

/// Step length as a share of the shortest free-fall time sqrt(r³/GM) to any
/// body: about 1,250 steps per circular orbit.
const ETA: f64 = 0.005;
/// Steps, and so samples, a prediction may take.
pub const MAX_STEPS: usize = 6_000;
/// The longest horizon, for very wide orbits.
const MAX_HORIZON_SECONDS: f64 = 100.0 * crate::YEAR_SECONDS;
/// Bound paths run this many periods, so the end visibly meets the start.
const PERIODS: f64 = 1.02;
/// Unbound paths run until they have gone this many times the reference's
/// reach, or its distance when the reach is infinite, at the current speed.
const ESCAPE_REACHES: f64 = 2.0;

/// A predicted state, relative to the prediction's reference at that time.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sample {
    pub seconds: f64,
    pub position: [f64; 3],
    pub velocity: [f64; 3],
}

#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    /// Closest to the reference's centre, interpolated between samples.
    Periapsis {
        seconds: f64,
        distance_m: f64,
    },
    Apoapsis {
        seconds: f64,
        distance_m: f64,
    },
    /// The orbit reference would change here: leaving one region or
    /// entering another, past the switching margin.
    Reference {
        seconds: f64,
        id: String,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub enum End {
    /// The planned horizon was reached.
    Horizon,
    /// The step budget ran out first: the path is incomplete.
    Truncated,
    /// The path enters this body. There are no collisions yet, so the real
    /// craft would pass through; the prediction stops at the surface.
    Impact { body: String },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Prediction {
    /// The reference every sample is relative to.
    pub reference: String,
    pub start_seconds: f64,
    /// The planned length, in seconds from the start.
    pub horizon_seconds: f64,
    /// The planned length is about one period of a bound orbit.
    pub bound: bool,
    /// The start first.
    pub samples: Vec<Sample>,
    pub events: Vec<Event>,
    /// None while still running.
    pub end: Option<End>,
}

impl Event {
    pub fn seconds(&self) -> f64 {
        match self {
            Self::Periapsis { seconds, .. }
            | Self::Apoapsis { seconds, .. }
            | Self::Reference { seconds, .. } => *seconds,
        }
    }
}

impl Prediction {
    /// How far ahead the samples reach, in seconds.
    pub fn achieved_seconds(&self) -> f64 {
        self.samples
            .last()
            .map_or(0.0, |s| s.seconds - self.start_seconds)
    }

    /// The simulation time the samples reach.
    pub fn end_seconds(&self) -> f64 {
        self.start_seconds + self.achieved_seconds()
    }

    /// Events still ahead at `seconds`, soonest first.
    pub fn upcoming(&self, seconds: f64) -> impl Iterator<Item = &Event> {
        self.events.iter().filter(move |e| e.seconds() >= seconds)
    }

    /// The predicted position relative to the reference at `seconds`, on
    /// the cubic between the samples either side.
    pub fn position_at(&self, seconds: f64) -> Option<[f64; 3]> {
        let after = self.samples.partition_point(|s| s.seconds < seconds);
        let b = self.samples.get(after)?;
        let Some(a) = after.checked_sub(1).map(|i| &self.samples[i]) else {
            return Some(b.position);
        };
        Some(hermite(
            a,
            b,
            (seconds - a.seconds) / (b.seconds - a.seconds),
        ))
    }

    /// The samples still ahead at `seconds`, soonest first.
    pub fn ahead(&self, seconds: f64) -> &[Sample] {
        let first = self.samples.partition_point(|s| s.seconds < seconds);
        &self.samples[first..]
    }
}

/// A prediction in progress.
#[derive(Clone, Debug)]
pub struct Predictor {
    state: State,
    seconds: f64,
    gravity: [f64; 3],
    /// The shortest free-fall time from here, which sets the next step.
    pace: f64,
    /// The orbit reference as it would be now, for events.
    current: String,
    prediction: Prediction,
}

impl Predictor {
    /// Start from the craft's state now. None when the field has no orbit
    /// reference for it.
    pub fn new(craft: &Craft, gravity: &impl Gravity) -> Option<Self> {
        let (state, seconds) = (craft.state(), craft.seconds);
        let reference =
            gravity.orbit_reference(craft.reference.as_deref(), state.position, seconds)?;
        let relative = relative(state, reference.state);
        let orbit = Elements::of(relative, reference.gm);
        let (horizon_seconds, bound) = match orbit.period() {
            Some(period) => (period * PERIODS, true),
            None => {
                let r = distance(relative.position, [0.0; 3]);
                let reach = if reference.reach_m.is_finite() {
                    reference.reach_m
                } else {
                    r
                };
                let speed = distance(relative.velocity, [0.0; 3]).max(1e-9);
                (ESCAPE_REACHES * reach.max(r) / speed, false)
            }
        };
        let prediction = Prediction {
            reference: reference.id.clone(),
            start_seconds: seconds,
            horizon_seconds: horizon_seconds.min(MAX_HORIZON_SECONDS),
            bound,
            samples: vec![Sample {
                seconds,
                position: relative.position,
                velocity: relative.velocity,
            }],
            events: Vec::new(),
            end: None,
        };
        let mut predictor = Self {
            state,
            seconds,
            gravity: gravity.acceleration(state.position, seconds),
            pace: f64::INFINITY,
            current: reference.id,
            prediction,
        };
        predictor.survey(gravity);
        Some(predictor)
    }

    pub fn prediction(&self) -> &Prediction {
        &self.prediction
    }

    pub fn into_prediction(self) -> Prediction {
        self.prediction
    }

    pub fn done(&self) -> bool {
        self.prediction.end.is_some()
    }

    /// Take at most `steps` more steps. Returns whether it has finished.
    pub fn run(&mut self, gravity: &impl Gravity, steps: usize) -> bool {
        for _ in 0..steps {
            if self.done() {
                break;
            }
            self.step(gravity);
        }
        self.done()
    }

    fn step(&mut self, gravity: &impl Gravity) {
        let p = &mut self.prediction;
        let end = p.start_seconds + p.horizon_seconds;
        if self.seconds >= end {
            p.end = Some(End::Horizon);
            return;
        }
        if p.samples.len() > MAX_STEPS {
            p.end = Some(End::Truncated);
            return;
        }
        let dt = (ETA * self.pace).max(STEP_SECONDS).min(end - self.seconds);
        self.gravity = verlet(
            &mut self.state,
            gravity,
            self.seconds,
            dt,
            self.gravity,
            [[0.0; 3]; 2],
        );
        self.seconds += dt;
        let Some(reference) = gravity.reference(Some(&p.reference), [0.0; 3], self.seconds) else {
            p.end = Some(End::Truncated);
            return;
        };
        let relative = relative(self.state, reference);
        let sample = Sample {
            seconds: self.seconds,
            position: relative.position,
            velocity: relative.velocity,
        };
        if let Some(previous) = p.samples.last() {
            p.events.extend(apsis(previous, &sample));
        }
        p.samples.push(sample);
        if let Some(next) =
            gravity.orbit_reference(Some(&self.current), self.state.position, self.seconds)
            && next.id != self.current
        {
            p.events.push(Event::Reference {
                seconds: self.seconds,
                id: next.id.clone(),
            });
            self.current = next.id;
        }
        self.survey(gravity);
    }

    /// The pace of the next step, and whether the path has hit a body.
    fn survey(&mut self, gravity: &impl Gravity) {
        let closest = gravity.closest(self.state.position, self.seconds);
        self.pace = closest.as_ref().map_or(f64::INFINITY, |c| c.timescale);
        if let Some(body) = closest
            && body.inside
        {
            self.prediction.end = Some(End::Impact { body: body.id });
        }
    }
}

/// Predict to the end in one go.
pub fn predict(craft: &Craft, gravity: &impl Gravity) -> Option<Prediction> {
    let mut predictor = Predictor::new(craft, gravity)?;
    predictor.run(gravity, usize::MAX);
    Some(predictor.into_prediction())
}

fn relative(state: State, to: State) -> State {
    State {
        position: std::array::from_fn(|i| state.position[i] - to.position[i]),
        velocity: std::array::from_fn(|i| state.velocity[i] - to.velocity[i]),
    }
}

/// An apsis between two samples, where the radial speed changes sign.
fn apsis(a: &Sample, b: &Sample) -> Option<Event> {
    let radial = |s: &Sample| (0..3).map(|i| s.position[i] * s.velocity[i]).sum::<f64>();
    let (ra, rb) = (radial(a), radial(b));
    if ra.signum() == rb.signum() || ra == 0.0 {
        return None;
    }
    // Where it changes sign, on the cubic through both states.
    let share = ra / (ra - rb);
    let dt = b.seconds - a.seconds;
    let position = hermite(a, b, share);
    let seconds = a.seconds + share * dt;
    let distance_m = distance(position, [0.0; 3]);
    Some(if ra < 0.0 {
        Event::Periapsis {
            seconds,
            distance_m,
        }
    } else {
        Event::Apoapsis {
            seconds,
            distance_m,
        }
    })
}

/// Position on the cubic Hermite curve between two states, `s` of the way.
fn hermite(a: &Sample, b: &Sample, s: f64) -> [f64; 3] {
    let dt = b.seconds - a.seconds;
    let (s2, s3) = (s * s, s * s * s);
    let h00 = 2.0 * s3 - 3.0 * s2 + 1.0;
    let h10 = s3 - 2.0 * s2 + s;
    let h01 = -2.0 * s3 + 3.0 * s2;
    let h11 = s3 - s2;
    std::array::from_fn(|i| {
        h00 * a.position[i]
            + h10 * dt * a.velocity[i]
            + h01 * b.position[i]
            + h11 * dt * b.velocity[i]
    })
}

#[cfg(test)]
mod tests;
