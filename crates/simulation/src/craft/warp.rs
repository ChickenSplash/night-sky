//! Unsafe manual bubble transport, separate from ordinary momentum.
//! There are no collision checks or route-coverage prerequisites.
use super::{AttitudePilot, Craft, Field, Frame, Gravity, Motion, STEP_SECONDS, circular_speed};
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
pub const GRAVITY_CUTOFF_MS: f64 = crate::LIGHT_SPEED_MS;

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
    /// Last successful matched exit, retained for inspection, not a standing
    /// order. Subsequent ordinary motion changes velocity normally.
    #[serde(default)]
    pub last_match: Option<MatchedExit>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MatchedExit {
    pub body: String,
    pub orbit: bool,
    pub tick: i64,
    /// Velocity in the system frame at dropout, before ordinary integration.
    pub velocity_ms: [f64; 3],
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
            last_match: None,
        }
    }
}

impl Warp {
    pub fn set_speed(&mut self, speed: f64) -> Result<(), &'static str> {
        Self::validate_speed(speed)?;
        self.requested_ms = speed;
        Ok(())
    }

    pub(super) fn validate_speed(speed: f64) -> Result<(), &'static str> {
        if !speed.is_finite() || !(MIN_SPEED..=MAX_SPEED).contains(&speed) {
            return Err("Warp speed must be finite, between 1 m/s and 10,000 ly/s");
        }
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
    /// Match a local body's analytic velocity, optionally adding sqrt(GM/r)
    /// tangential speed. This is a two-body estimate, not an orbit autopilot.
    /// All checks precede mutation, so a refused exit leaves warp running.
    pub fn matched_dropout(
        &mut self,
        field: Option<&Field>,
        body: &str,
        orbit: bool,
    ) -> Result<(), String> {
        if !self.warp.engaged {
            return Err("Engage warp before requesting a matched dropout".into());
        }
        if !matches!(self.frame, Frame::System { .. }) {
            return Err("Matched dropout requires a body in the ship's current system".into());
        }
        let reference = field
            .and_then(|field| field.reference_by_id(body, self.seconds()))
            .ok_or_else(|| format!("No body `{body}` in this ship's frame"))?;
        let mut state = self.state();
        let mut velocity = DVec3::from_array(reference.state.velocity);
        if orbit {
            let radial =
                DVec3::from_array(state.position) - DVec3::from_array(reference.state.position);
            let r = radial.length();
            if !r.is_finite() || r <= reference.radius_m || r <= 0.0 {
                return Err(
                    "Rough-orbit dropout requires a position outside the reference body".into(),
                );
            }
            let speed = circular_speed(reference.gm, r);
            if !speed.is_finite() || speed <= 0.0 {
                return Err("Reference has no valid orbital speed".into());
            }
            let radial = radial / r;
            let attitude = DQuat::from_array(self.orientation);
            // Preserve the current orbital sense when defined. At rest or
            // on a radial course use the nose, then ship-up, without turning.
            let tangent = [
                DVec3::from_array(state.velocity) - velocity,
                attitude * -DVec3::Z,
                attitude * DVec3::Y,
            ]
            .into_iter()
            .find_map(|v| {
                let tangent = v - radial * v.dot(radial);
                (tangent.length() > 1e-9 * v.length().max(1.0)).then(|| tangent.normalize())
            })
            .ok_or("Cannot determine an orbital direction")?;
            velocity += tangent * speed;
        }
        if !velocity.is_finite() {
            return Err("Reference velocity is not finite".into());
        }
        state.velocity = velocity.to_array();
        self.motion = Motion::Free(state);
        self.gravity = None;
        self.reference = None;
        self.warp.stop(if orbit {
            "Rough-orbit matched dropout"
        } else {
            "Velocity-matched dropout"
        });
        self.warp.last_match = Some(MatchedExit {
            body: body.into(),
            orbit,
            tick: self.tick,
            velocity_ms: state.velocity,
        });
        Ok(())
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
        self.warp.last_match = None;
        self.gravity = None;
        Ok(())
    }

    /// One fixed step of bubble transport, with ordinary drift and, under
    /// the active policy, gravity. No collision or route checks.
    pub(super) fn warp_step(&mut self, gravity: &impl Gravity) {
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
        let seconds = self.seconds();
        let pulled = self.warp.gravity == GravityPolicy::Active && travel < GRAVITY_CUTOFF_MS * dt;
        let a = if pulled {
            DVec3::from_array(
                self.gravity
                    .unwrap_or_else(|| gravity.acceleration(state.position, seconds)),
            )
        } else {
            DVec3::ZERO
        };
        let half_velocity = DVec3::from_array(state.velocity) + a * (dt / 2.0);
        state.position =
            (DVec3::from_array(state.position) + half_velocity * dt + bubble).to_array();
        let end_a = if pulled {
            DVec3::from_array(
                gravity.acceleration(state.position, super::tick_seconds(self.tick + 1)),
            )
        } else {
            DVec3::ZERO
        };
        state.velocity = (half_velocity + end_a * (dt / 2.0)).to_array();
        self.motion = Motion::Free(state);
        self.tick += 1;
        self.warp.actual_ms = speed;
        self.warp.bubble_delta_m = bubble.to_array();
        // A skipped sample must not survive a suspended/superluminal step.
        // A valid final sample is also the next ordinary-flight half kick.
        self.gravity = pulled.then_some(end_a.to_array());
        self.reference = None;
    }
}

#[cfg(test)]
mod tests;
