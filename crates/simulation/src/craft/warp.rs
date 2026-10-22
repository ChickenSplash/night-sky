//! Unsafe manual bubble transport, separate from ordinary momentum.
//! There are no collision checks or route-coverage prerequisites.
//!
//! The drive also turns the bubble and the ship inside it together, without
//! turning thrusters: a game rule. Steering sets a turn rate, not an angular
//! acceleration, so releasing it stops the turn, and dropout leaves no spin.
use super::{
    Craft, Field, Frame, Gravity, Motion, STEP_SECONDS, Target, circular_speed, is_craft_id,
};
use glam::{DQuat, DVec3};
use serde::{Deserialize, Serialize};

pub const MIN_SPEED: f64 = 1.0;
/// Shared upper travel speed for warp and the free camera, in metres/second.
pub const MAX_SPEED: f64 = 10_000.0 * crate::LY_METRES;
pub const SPEED_FACTOR: f64 = 3.162_277_660_168_379_5;
/// Exponential response time of speed and turn rate in simulated seconds,
/// independent of frame rate.
pub const RAMP_SECONDS: f64 = 0.3;
pub const SPIN_TOLERANCE: f64 = 1e-6;
/// Range and default of the bubble's turn rate at full steering, in °/s.
pub const MIN_TURN_RATE_DPS: f64 = 1.0;
pub const MAX_TURN_RATE_DPS: f64 = 360.0;
pub const DEFAULT_TURN_RATE_DPS: f64 = 30.0;
/// Bubble speed at and above which gravity is ignored under either policy.
/// A pass this fast changes ordinary velocity by at most metres per second
/// for a main-sequence star, while sampling gravity only at the ends of each
/// light-second-long step would give large, arbitrary kicks deep in a body.
pub const GRAVITY_CUTOFF_MS: f64 = crate::LIGHT_SPEED_MS;

/// Whether gravity changes ordinary velocity during warp below the cutoff.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum GravityPolicy {
    #[default]
    Active,
    Suspended,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Warp {
    pub engaged: bool,
    /// The speed warp ramps towards while engaged.
    #[schemars(extend("x-unit" = "m/s"))]
    pub requested_ms: f64,
    /// The bubble's current speed.
    #[schemars(extend("x-unit" = "m/s"))]
    pub actual_ms: f64,
    pub gravity: GravityPolicy,
    pub stop_reason: Option<String>,
    /// Turn rate at full steering, in degrees per second. Steering on several
    /// axes together shares it, and pointing never exceeds it.
    #[serde(default = "default_turn_rate")]
    #[schemars(extend("x-unit" = "°/s"))]
    pub turn_rate_dps: f64,
    /// The displacement from bubble motion alone on the last physics step.
    #[schemars(extend("x-unit" = "m"))]
    pub bubble_delta_m: [f64; 3],
    /// Last successful matched exit, retained for inspection, not a standing
    /// order. Subsequent ordinary motion changes velocity normally.
    #[serde(default)]
    pub last_match: Option<MatchedExit>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct MatchedExit {
    pub body: String,
    pub orbit: bool,
    pub tick: i64,
    /// Velocity in the system frame at dropout, before ordinary integration.
    #[schemars(extend("x-unit" = "m/s"))]
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
            turn_rate_dps: DEFAULT_TURN_RATE_DPS,
            bubble_delta_m: [0.0; 3],
            last_match: None,
        }
    }
}

fn default_turn_rate() -> f64 {
    DEFAULT_TURN_RATE_DPS
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

    pub fn set_turn_rate(&mut self, degrees_per_second: f64) -> Result<(), &'static str> {
        Self::validate_turn_rate(degrees_per_second)?;
        self.turn_rate_dps = degrees_per_second;
        Ok(())
    }

    pub(super) fn validate_turn_rate(degrees_per_second: f64) -> Result<(), &'static str> {
        if !degrees_per_second.is_finite()
            || !(MIN_TURN_RATE_DPS..=MAX_TURN_RATE_DPS).contains(&degrees_per_second)
        {
            return Err("Warp turn rate must be finite, between 1 and 360 °/s");
        }
        Ok(())
    }

    fn stop(&mut self, reason: &str) {
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
        self.matched_dropout_among(field, &[], body, orbit)
    }

    /// A matched dropout that may also match one of the other craft in
    /// `ships`, anywhere: formation flying, leaving its warp bubble behind.
    pub fn matched_dropout_among(
        &mut self,
        field: Option<&Field>,
        ships: &[Target],
        body: &str,
        orbit: bool,
    ) -> Result<(), String> {
        if !self.warp.engaged {
            return Err("Engage warp before requesting a matched dropout".into());
        }
        if body == self.id {
            return Err("A ship cannot match itself".into());
        }
        if let Some(ship) = ships.iter().find(|ship| ship.id == body) {
            if orbit {
                return Err(format!("`{body}` cannot be orbited"));
            }
            self.drop_matched(DVec3::from_array(ship.state.velocity), body, false);
            return Ok(());
        }
        if is_craft_id(body) {
            return Err(format!("No craft `{body}`"));
        }
        if !matches!(self.frame, Frame::System { .. }) {
            return Err("Matched dropout requires a body in the ship's current system".into());
        }
        let target = field
            .and_then(|field| field.target(body, self.seconds()))
            .ok_or_else(|| format!("No body `{body}` in this ship's frame"))?;
        let velocity = self.matched_velocity(&target, orbit)?;
        self.drop_matched(velocity, &target.id, orbit);
        Ok(())
    }

    /// The velocity a matched dropout gives: the target's, plus for a rough
    /// orbit sqrt(GM/r) sideways.
    pub(super) fn matched_velocity(&self, target: &Target, orbit: bool) -> Result<DVec3, String> {
        let state = self.state();
        let mut velocity = DVec3::from_array(target.state.velocity);
        if orbit {
            let radial =
                DVec3::from_array(state.position) - DVec3::from_array(target.state.position);
            let r = radial.length();
            if !r.is_finite() || r <= target.radius_m || r <= 0.0 {
                return Err(
                    "Rough-orbit dropout requires a position outside the reference body".into(),
                );
            }
            // Beyond its Hill sphere or stable zone another body dominates,
            // so a two-body circular speed would not describe an orbit.
            if r >= target.reach_m.unwrap_or(0.0) {
                return Err(format!(
                    "Rough-orbit dropout requires a position within `{}`'s sphere of influence",
                    target.id
                ));
            }
            let speed = circular_speed(target.gm, r);
            if !speed.is_finite() || speed <= 0.0 {
                return Err("Reference has no valid orbital speed".into());
            }
            let radial = radial / r;
            let attitude = DQuat::from_array(self.orientation);
            // Preserve the current orbital sense when clearly defined. At rest
            // or on a (near) radial course use the nose, then ship-up, without
            // turning. A tangential share under about 0.06° is treated as noise.
            let tangent = [
                DVec3::from_array(state.velocity) - velocity,
                attitude * -DVec3::Z,
                attitude * DVec3::Y,
            ]
            .into_iter()
            .find_map(|v| {
                let tangent = v - radial * v.dot(radial);
                (tangent.length() > 1e-3 * v.length()).then(|| tangent.normalize())
            })
            .ok_or("Cannot determine an orbital direction")?;
            velocity += tangent * speed;
        }
        if !velocity.is_finite() {
            return Err("Reference velocity is not finite".into());
        }
        Ok(velocity)
    }

    /// Leave warp at `velocity`, recording the match.
    pub(super) fn drop_matched(&mut self, velocity: DVec3, body: &str, orbit: bool) {
        let mut state = self.state();
        state.velocity = velocity.to_array();
        self.motion = Motion::Free(state);
        self.gravity = None;
        self.reference = None;
        self.disengage_warp(if orbit {
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
    }

    /// Validate without changing motion or silently cutting other drives.
    /// Turning carries over: the bubble takes over any spin and eases it to
    /// the rotation input or attitude pilot, which now steer the bubble.
    pub fn engage_warp(&mut self) -> Result<(), &'static str> {
        if self.warp.engaged {
            return Err("Warp is already engaged");
        }
        self.check_engage()?;
        self.warp.engaged = true;
        self.warp.actual_ms = 0.0;
        self.warp.stop_reason = None;
        self.warp.last_match = None;
        self.gravity = None;
        Ok(())
    }

    /// Whether warp could engage now, interlocked with sublight thrust.
    pub fn check_engage(&self) -> Result<(), &'static str> {
        let c = self.input.controls;
        if self.input.thrust != [0.0; 3] || c.throttle != 0.0 || c.translation != [0.0; 3] {
            return Err("Cut sublight thrust before engaging warp");
        }
        Ok(())
    }

    /// End bubble travel, keeping ordinary velocity. The bubble's turn ends
    /// with it, so the ship drops out without spin; the rotation input and
    /// attitude pilot pass back to the turning thrusters unchanged.
    pub fn disengage_warp(&mut self, reason: &str) {
        self.warp.stop(reason);
        self.spin = [0.0; 3];
    }

    /// How far the bubble carries the ship in the next step: the integral of
    /// the exponential speed ramp, not endpoint speed times the step.
    pub(super) fn warp_travel(&self) -> f64 {
        let decay = (-STEP_SECONDS / RAMP_SECONDS).exp();
        self.warp.requested_ms * STEP_SECONDS
            + (self.warp.actual_ms - self.warp.requested_ms) * RAMP_SECONDS * (1.0 - decay)
    }

    /// One fixed step of bubble transport, with ordinary drift and, under
    /// the active policy, gravity. No collision or route checks.
    pub(super) fn warp_step(&mut self, gravity: &impl Gravity) {
        let dt = STEP_SECONDS;
        let decay = (-dt / RAMP_SECONDS).exp();
        let speed = self.warp.requested_ms + (self.warp.actual_ms - self.warp.requested_ms) * decay;
        let travel = self.warp_travel();
        // The turn rate follows the same ramp towards the steering target,
        // and the path bends with the nose at the step's midpoint attitude.
        let target = self.steering(gravity);
        let spin = DVec3::from_array(self.spin);
        let turn = target * dt + (spin - target) * RAMP_SECONDS * (1.0 - decay);
        let start = DQuat::from_array(self.orientation);
        let heading = if turn == DVec3::ZERO {
            start
        } else {
            self.orientation = (start * DQuat::from_scaled_axis(turn))
                .normalize()
                .to_array();
            start * DQuat::from_scaled_axis(turn / 2.0)
        };
        self.spin = (target + (spin - target) * decay).to_array();
        let bubble = heading * -DVec3::Z * travel;
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
