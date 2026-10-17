//! Orders a pilot, autopilot or remote player gives a craft. Each patches only
//! the input it names, leaving other drives alone. Checks and application
//! share this one path, so any client or server refuses the same orders.
use super::warp::{GravityPolicy, MAX_SPEED, MIN_SPEED, SPEED_FACTOR};
use super::{Craft, Field, PointTarget, Thrusters};
use serde::{Deserialize, Serialize};

/// Most legacy frame-axis thrust an order may hold, in m/s².
pub const MAX_THRUST: f64 = 100.0;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum Order {
    Throttle {
        value: f64,
    },
    Translate {
        right: f64,
        up: f64,
        forward: f64,
    },
    Rotate {
        pitch: f64,
        yaw: f64,
        roll: f64,
    },
    Point {
        target: PointTarget,
        #[serde(default)]
        up: Option<[f64; 3]>,
    },
    Stop,
    Release,
    Cut,
    /// Change the craft's own thruster limits. A server should accept this
    /// only from an administrator, never from a player.
    Configure {
        thrusters: Thrusters,
    },
    /// Legacy debug acceleration in m/s², in the frame's axes.
    Thrust {
        thrust: [f64; 3],
    },
    Warp {
        command: WarpOrder,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "command", rename_all = "snake_case", deny_unknown_fields)]
pub enum WarpOrder {
    Engage,
    Disengage,
    Set { speed_ms: f64 },
    Up,
    Down,
    Gravity { policy: GravityPolicy },
}

impl Order {
    /// State-independent validation, also run when future inputs are submitted.
    /// Drive interlocks and body/frame checks remain application-time rules.
    pub fn validate(&self) -> Result<(), String> {
        let fractions: &[f64] = match self {
            Self::Throttle { value } => std::slice::from_ref(value),
            Self::Translate { right, up, forward } => &[*right, *up, *forward],
            Self::Rotate { pitch, yaw, roll } => &[*pitch, *yaw, *roll],
            _ => &[],
        };
        if !fractions
            .iter()
            .all(|v| v.is_finite() && (-1.0..=1.0).contains(v))
        {
            return Err("controls must be finite fractions in [-1, 1]".into());
        }
        match self {
            Self::Point { target, up } => {
                if let PointTarget::Vector(v) = target
                    && super::attitude::unit(*v).is_none()
                {
                    return Err("direction must be finite and nonzero".into());
                }
                if up.is_some_and(|v| super::attitude::unit(v).is_none()) {
                    return Err("up must be finite and nonzero".into());
                }
            }
            Self::Configure { thrusters } => {
                if ![thrusters.forward, thrusters.reverse, thrusters.fine]
                    .iter()
                    .chain(thrusters.angular.iter())
                    .all(|v| v.is_finite() && *v >= 0.0)
                {
                    return Err("thruster limits must be finite and nonnegative".into());
                }
            }
            Self::Thrust { thrust } => {
                let size = thrust.iter().map(|v| v * v).sum::<f64>().sqrt();
                if !size.is_finite() {
                    return Err("thrust must be three finite numbers".into());
                }
                if size > MAX_THRUST {
                    return Err(format!("thrust must be at most {MAX_THRUST} m/s²"));
                }
            }
            Self::Warp {
                command: WarpOrder::Set { speed_ms },
            } => {
                super::warp::Warp::validate_speed(*speed_ms)?;
            }
            _ => {}
        }
        Ok(())
    }

    /// Every order, warp included, takes effect from the craft's next fixed
    /// step (`Craft::tick`), so a server can key orders by step number.
    /// `field` is the craft's system, when it is in one, to check bodies.
    pub fn apply(&self, craft: &mut Craft, field: Option<&Field>) -> Result<(), String> {
        self.validate()?;
        if craft.warp.engaged && matches!(self, Self::Point { .. } | Self::Stop | Self::Cut) {
            return Err("Disengage warp before engaging turning thrusters or pointing".into());
        }
        let mut controls = craft.input.controls;
        match self {
            Self::Warp { command } => {
                match command {
                    WarpOrder::Engage => craft.engage_warp()?,
                    WarpOrder::Disengage => craft.warp.stop("Manual disengagement"),
                    WarpOrder::Set { speed_ms } => craft.warp.set_speed(*speed_ms)?,
                    WarpOrder::Up => craft
                        .warp
                        .set_speed((craft.warp.requested_ms * SPEED_FACTOR).min(MAX_SPEED))?,
                    WarpOrder::Down => craft
                        .warp
                        .set_speed((craft.warp.requested_ms / SPEED_FACTOR).max(MIN_SPEED))?,
                    WarpOrder::Gravity { policy } => {
                        if craft.warp.engaged {
                            return Err("Disengage warp before changing its gravity policy".into());
                        }
                        craft.warp.gravity = *policy;
                    }
                }
                return Ok(());
            }
            Self::Throttle { value } => controls.throttle = *value,
            Self::Translate { right, up, forward } => {
                controls.translation = [*right, *up, -*forward]
            }
            Self::Rotate { pitch, yaw, roll } => controls.rotation = [*pitch, *yaw, *roll],
            Self::Point { target, up } => {
                if let PointTarget::Body(id) = target
                    && let Some(field) = field
                    && !field
                        .bodies(craft.seconds())
                        .iter()
                        .any(|body| body.id == *id)
                {
                    return Err(format!("No body `{id}` in this ship's frame"));
                }
                return craft.point(target.clone(), *up).map_err(String::from);
            }
            Self::Stop => {
                craft.stop_rotation();
                return Ok(());
            }
            Self::Release => {
                craft.release();
                return Ok(());
            }
            Self::Cut => {
                craft.input = Default::default();
                craft.stop_rotation();
                return Ok(());
            }
            Self::Configure { thrusters } => {
                if thrusters.reverse == 0.0 && controls.throttle < 0.0 {
                    return Err("Cut reverse throttle before disabling the reverse drive".into());
                }
                craft.thrusters = *thrusters;
                return Ok(());
            }
            Self::Thrust { thrust } => {
                if craft.warp.engaged && *thrust != [0.0; 3] {
                    return Err("Disengage warp before using thrusters".into());
                }
                craft.input.thrust = *thrust;
                return Ok(());
            }
        }
        craft.control(controls)?;
        if matches!(self, Self::Rotate { .. }) {
            craft.release();
        }
        if matches!(self, Self::Throttle { .. } | Self::Translate { .. }) {
            craft.input.thrust = [0.0; 3];
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
