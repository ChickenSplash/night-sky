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
        let a = if self.warp.gravity == GravityPolicy::Active {
            DVec3::from_array(gravity.acceleration(state.position, self.seconds))
        } else {
            DVec3::ZERO
        };
        let half_velocity = DVec3::from_array(state.velocity) + a * (dt / 2.0);
        state.position =
            (DVec3::from_array(state.position) + half_velocity * dt + bubble).to_array();
        let end_a = if self.warp.gravity == GravityPolicy::Active {
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
