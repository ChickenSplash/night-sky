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
}

impl Order {
    pub fn apply(&self, craft: &mut Craft, field: Option<&Field>) -> Result<(), String> {
        let mut controls = craft.input.controls;
        match self {
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
