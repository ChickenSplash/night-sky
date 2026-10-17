//! CLI/API ship orders. Patch only the named input, leaving other drives
//! alone. Checks and application share this path so unsupported orders fail.
use super::*;
use serde::{Deserialize, Serialize};
use space_simulation::craft::{PointTarget, Thrusters};

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
    Configure {
        thrusters: Thrusters,
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
    Set {
        speed_ms: f64,
    },
    Up,
    Down,
    Gravity {
        policy: space_simulation::craft::warp::GravityPolicy,
    },
}

impl Order {
    /// Every order, warp included, takes effect from the next fixed step.
    /// Physics already brackets the clock, so up to one step of the previous
    /// motion (or warp travel) still plays out after the command.
    pub fn apply(&self, craft: &mut Craft, field: Option<&Field>) -> Result<(), String> {
        if craft.warp.engaged && matches!(self, Self::Point { .. } | Self::Stop | Self::Cut) {
            return Err("Disengage warp before engaging turning thrusters or pointing".into());
        }
        let mut controls = craft.input.controls;
        match self {
            Self::Warp { command } => {
                use space_simulation::craft::warp::{MAX_SPEED, MIN_SPEED, SPEED_FACTOR};
                match command {
                    WarpOrder::Engage => {
                        craft.engage_warp().map_err(String::from)?;
                    }
                    WarpOrder::Disengage => craft.warp.stop("Manual disengagement"),
                    WarpOrder::Set { speed_ms } => {
                        craft.warp.set_speed(*speed_ms).map_err(String::from)?
                    }
                    WarpOrder::Up => craft
                        .warp
                        .set_speed((craft.warp.requested_ms * SPEED_FACTOR).min(MAX_SPEED))
                        .map_err(String::from)?,
                    WarpOrder::Down => craft
                        .warp
                        .set_speed((craft.warp.requested_ms / SPEED_FACTOR).max(MIN_SPEED))
                        .map_err(String::from)?,
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
                if let space_simulation::craft::PointTarget::Body(id) = target
                    && let Some(field) = field
                    && !field
                        .bodies(craft.seconds)
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
                if ![thrusters.forward, thrusters.reverse, thrusters.fine]
                    .iter()
                    .chain(thrusters.angular.iter())
                    .all(|v| v.is_finite() && *v >= 0.0)
                {
                    return Err("thruster limits must be finite and nonnegative".into());
                }
                if thrusters.reverse == 0.0 && controls.throttle < 0.0 {
                    return Err("Cut reverse throttle before disabling the reverse drive".into());
                }
                craft.thrusters = *thrusters;
                return Ok(());
            }
        }
        craft.control(controls).map_err(String::from)?;
        if matches!(self, Self::Rotate { .. }) {
            craft.release();
        }
        if matches!(self, Self::Throttle { .. } | Self::Translate { .. }) {
            craft.input.thrust = [0.0; 3];
        }
        Ok(())
    }
}
