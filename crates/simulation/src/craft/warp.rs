//! Unsafe manual bubble transport, separate from ordinary momentum.
//! There are no collision checks or route-coverage prerequisites.
use super::{AttitudePilot, Craft, Gravity, Motion, STEP_SECONDS};
use crate::State;
use glam::{DQuat, DVec3};
use serde::{Deserialize, Serialize};

pub const MIN_SPEED: f64 = 1.0;
/// Shared upper travel speed for warp and the free camera, in metres/second.
pub const MAX_SPEED: f64 = 10_000.0 * crate::LY_METRES;
pub const SPEED_FACTOR: f64 = 3.162_277_660_168_379_5;
/// Exponential response time in simulated seconds, independent of frame rate.
pub const RAMP_SECONDS: f64 = 0.3;
pub const SPIN_TOLERANCE: f64 = 1e-6;
/// Bubble speed at and above which gravity is ignored under either policy.
/// A pass this fast changes ordinary velocity by at most metres per second
/// for a main-sequence star, while sampling gravity only at the ends of each
/// light-second-long step would give large, arbitrary kicks deep in a body.
pub const GRAVITY_CUTOFF_MS: f64 = 299_792_458.0;

/// A timestamped drive event within a straight fixed-step
/// segment. Keep it when interpolating that segment: its remaining transport
/// has been replaced by ordinary flight, without moving the step boundary.
#[derive(Clone, Copy, Debug)]
pub struct Cutoff {
    pub seconds: f64,
    pub state: State,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GravityPolicy {
    #[default]
    Active,
    Suspended,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Warp {
    pub engaged: bool,
    pub requested_ms: f64,
    pub actual_ms: f64,
    pub gravity: GravityPolicy,
    pub stop_reason: Option<String>,
    /// The displacement from bubble motion alone on the last physics step.
    pub bubble_delta_m: [f64; 3],
}

impl Default for Warp {
    fn default() -> Self {
        Self {
            engaged: false,
            requested_ms: MIN_SPEED,
            actual_ms: 0.0,
            gravity: GravityPolicy::Active,
            stop_reason: None,
            bubble_delta_m: [0.0; 3],
        }
    }
}

impl Warp {
    pub fn set_speed(&mut self, speed: f64) -> Result<(), &'static str> {
        if !speed.is_finite() || !(MIN_SPEED..=MAX_SPEED).contains(&speed) {
            return Err("Warp speed must be finite, between 1 m/s and 10,000 ly/s");
        }
        self.requested_ms = speed;
        Ok(())
    }

    pub fn stop(&mut self, reason: &str) {
        self.engaged = false;
        self.actual_ms = 0.0;
        self.bubble_delta_m = [0.0; 3];
        self.stop_reason = Some(reason.into());
    }
}

impl Craft {
    /// Cancel only the not-yet-reached part of a warp segment. Physics steps
    /// ahead to bracket the clock for interpolation, so simply setting warp off
    /// would otherwise leave up to one enormous bubble step still to display.
    /// The prefix remains the original straight path; gravity resumes over
    /// the remaining fraction of this same fixed step, under either policy.
    pub fn truncate_warp_segment(
        &mut self,
        from: State,
        from_seconds: f64,
        at_seconds: f64,
        gravity: &impl Gravity,
    ) -> Option<Cutoff> {
        if !self.warp.engaged
            || at_seconds < from_seconds
            || at_seconds >= self.seconds
            || from_seconds >= self.seconds
        {
            return None;
        }
        let share = (at_seconds - from_seconds) / (self.seconds - from_seconds);
        let current = self.state();
        let blend =
            |a: [f64; 3], b: [f64; 3]| std::array::from_fn(|i| a[i] + (b[i] - a[i]) * share);
        let cutoff = Cutoff {
            seconds: at_seconds,
            state: State {
                position: blend(from.position, current.position),
                velocity: blend(from.velocity, current.velocity),
            },
        };
        let mut end = cutoff.state;
        let acceleration = gravity.acceleration(end.position, at_seconds);
        super::verlet(
            &mut end,
            gravity,
            at_seconds,
            self.seconds - at_seconds,
            acceleration,
            [[0.0; 3]; 2],
        );
        self.place(end, self.seconds);
        self.reference = None;
        Some(cutoff)
    }

    /// Validate without changing motion or silently cutting other drives.
    pub fn engage_warp(&mut self) -> Result<(), &'static str> {
        if self.warp.engaged {
            return Err("Warp is already engaged");
        }
        let c = self.input.controls;
        if self.input.thrust != [0.0; 3] || c.throttle != 0.0 || c.translation != [0.0; 3] {
            return Err("Cut sublight thrust before engaging warp");
        }
        if c.rotation != [0.0; 3]
            || self.spin.iter().any(|v| v.abs() > SPIN_TOLERANCE)
            || !matches!(self.autopilot, AttitudePilot::Manual)
        {
            return Err("Stop rotation and release the attitude pilot before engaging warp");
        }
        self.warp.engaged = true;
        self.warp.actual_ms = 0.0;
        self.warp.stop_reason = None;
        self.gravity = None;
        Ok(())
    }

    /// Candidate drift for a complete fixed step. This never changes live state.
    pub fn warp_candidate(&self, gravity: &impl Gravity) -> (State, f64, [f64; 3]) {
        let dt = STEP_SECONDS;
        let speed = self.warp.requested_ms
            + (self.warp.actual_ms - self.warp.requested_ms) * (-dt / RAMP_SECONDS).exp();
        // Integral of the exponential speed ramp, not endpoint speed times dt.
        let travel = self.warp.requested_ms * dt
            + (self.warp.actual_ms - self.warp.requested_ms)
                * RAMP_SECONDS
                * (1.0 - (-dt / RAMP_SECONDS).exp());
        let bubble = DQuat::from_array(self.orientation) * -DVec3::Z * travel;
        let mut state = self.state();
        let pulled = self.warp.gravity == GravityPolicy::Active && travel < GRAVITY_CUTOFF_MS * dt;
        let a = if pulled {
            DVec3::from_array(gravity.acceleration(state.position, self.seconds))
        } else {
            DVec3::ZERO
        };
        let half_velocity = DVec3::from_array(state.velocity) + a * (dt / 2.0);
        state.position =
            (DVec3::from_array(state.position) + half_velocity * dt + bubble).to_array();
        let end_a = if pulled {
            DVec3::from_array(gravity.acceleration(state.position, self.seconds + dt))
        } else {
            DVec3::ZERO
        };
        state.velocity = (half_velocity + end_a * (dt / 2.0)).to_array();
        (state, speed, bubble.to_array())
    }

    /// Apply manual bubble transport without collision or route checks.
    pub fn commit_warp(&mut self, state: State, speed: f64, bubble: [f64; 3]) {
        self.motion = Motion::Free(state);
        self.seconds += STEP_SECONDS;
        self.warp.actual_ms = speed;
        self.warp.bubble_delta_m = bubble;
        self.gravity = None;
        self.reference = None;
    }
}

#[cfg(test)]
mod tests;
