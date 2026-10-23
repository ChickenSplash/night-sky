//! Orders a pilot, autopilot or remote player gives a craft. Each patches only
//! the input it names, leaving other drives alone. Checks and application
//! share this one path, so any client or server refuses the same orders.
use super::route::{ArrivalMode, MAX_TRIP_S, MIN_TRIP_S, TAKEOVER, Target};
use super::warp::{
    GravityPolicy, MAX_SPEED, MAX_TURN_RATE_DPS, MIN_SPEED, MIN_TURN_RATE_DPS, SPEED_FACTOR,
};
use super::{Craft, Field, PointTarget, Thrusters};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Most legacy frame-axis thrust an order may hold, in m/s².
pub const MAX_THRUST: f64 = 100.0;
/// Longest body ID an order may name, so queued inputs stay small.
pub const MAX_BODY_ID: usize = 256;

fn body_id(id: &str) -> Result<(), String> {
    if id.is_empty() || id.len() > MAX_BODY_ID {
        return Err(format!("body ID must be 1 to {MAX_BODY_ID} bytes"));
    }
    Ok(())
}

/// Doc comments here are the CLI and TUI help: the first paragraph is the
/// short help, the rest its description. `x-group` sorts orders for display.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum Order {
    /// Main drive fraction [-1, 1]; negative uses reverse, 0 cuts main only.
    #[schemars(extend("x-group" = "sublight"))]
    Throttle {
        #[schemars(range(min = -1.0, max = 1.0), extend("x-unit" = "fraction"))]
        value: f64,
    },
    /// Fine translation fractions [-1, 1]; omitted axes reset to zero.
    #[schemars(extend("x-group" = "sublight"))]
    Translate {
        #[serde(default)]
        #[schemars(range(min = -1.0, max = 1.0), extend("x-unit" = "fraction"))]
        right: f64,
        #[serde(default)]
        #[schemars(range(min = -1.0, max = 1.0), extend("x-unit" = "fraction"))]
        up: f64,
        #[serde(default)]
        #[schemars(range(min = -1.0, max = 1.0), extend("x-unit" = "fraction"))]
        forward: f64,
    },
    /// Manual turning fractions [-1, 1] (in warp, bubble steering); releases
    /// pointing, omitted axes reset to zero.
    #[schemars(extend("x-group" = "turning"))]
    Rotate {
        #[serde(default)]
        #[schemars(range(min = -1.0, max = 1.0), extend("x-unit" = "fraction"))]
        pitch: f64,
        #[serde(default)]
        #[schemars(range(min = -1.0, max = 1.0), extend("x-unit" = "fraction"))]
        yaw: f64,
        #[serde(default)]
        #[schemars(range(min = -1.0, max = 1.0), extend("x-unit" = "fraction"))]
        roll: f64,
    },
    /// Turn to and track an orbital direction, body or fixed system vector.
    #[schemars(extend("x-group" = "turning"))]
    Point {
        target: PointTarget,
        /// System up direction to align roll; otherwise roll is preserved.
        #[serde(default)]
        #[schemars(extend("x-unit" = ""))]
        up: Option<[f64; 3]>,
    },
    /// Brake rotation only, leaving translation and throttle unchanged.
    #[schemars(extend("x-group" = "turning"))]
    Stop,
    /// Disable the attitude pilot without braking or changing drives.
    #[schemars(extend("x-group" = "turning"))]
    Release,
    /// Cut all drives and engage spin braking; does not stop linear motion.
    #[schemars(extend("x-group" = "turning"))]
    Cut,
    /// Replace thruster limits; reverse 0 disables the reverse drive.
    ///
    /// A server should accept this only from an administrator, never from a
    /// player.
    #[schemars(extend("x-group" = "sublight"))]
    Configure { thrusters: Thrusters },
    /// Legacy debug acceleration, in the frame's axes.
    #[schemars(extend("x-group" = "sublight"))]
    Thrust {
        #[schemars(extend("x-unit" = "m/s²"))]
        thrust: [f64; 3],
    },
    /// Bubble transport along the nose, separate from sublight thrust.
    ///
    /// While engaged, rotate, point, stop and cut steer the bubble instead of
    /// the turning thrusters: pitch and yaw bend the path, roll turns the ship
    /// about it. Steering sets a turn rate (fraction × turn rate) that eases
    /// in and out over about 0.3 s; dropout leaves no spin.
    #[schemars(extend("x-group" = "warp"))]
    Warp { command: WarpOrder },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "command", rename_all = "snake_case", deny_unknown_fields)]
pub enum WarpOrder {
    /// Start bubble travel along the nose; needs no thrust, takes over any spin.
    Engage,
    /// Drop out keeping ordinary velocity.
    Disengage,
    /// Drop out matching a body in the ship's current system, or another
    /// ship anywhere.
    ///
    /// Resolved in the craft's current system at the application tick, or
    /// as the other ship was at the start of the step. This never changes
    /// heading or position.
    Match {
        /// Existing planet, star or barycentre ID, or another craft's ID.
        body: String,
        /// Add tangential speed for a rough orbit.
        #[serde(default)]
        orbit: bool,
    },
    /// Requested speed, from 1 m/s to 10,000 ly/s; does not engage.
    Set {
        #[schemars(range(min = MIN_SPEED, max = MAX_SPEED), extend("x-unit" = "m/s"))]
        speed_ms: f64,
    },
    /// Bubble turn rate at full steering (default 30 °/s).
    TurnRate {
        #[schemars(
            range(min = MIN_TURN_RATE_DPS, max = MAX_TURN_RATE_DPS),
            extend("x-unit" = "°/s")
        )]
        degrees_per_second: f64,
    },
    /// Multiply the requested speed by √10.
    Up,
    /// Divide the requested speed by √10.
    Down,
    /// Hold a sweep of the requested speed [-1, 1]; 1 is up 2 decades/s.
    ///
    /// Every factor of ten takes as long, so a short sweep nudges the speed.
    /// It continues until changed or dropout and needs warp engaged, except
    /// to end it. A sweep takes over a route.
    Sweep {
        #[schemars(range(min = -1.0, max = 1.0), extend("x-unit" = "fraction"))]
        rate: f64,
    },
    /// Developer gravity policy; change only while disengaged.
    Gravity { policy: GravityPolicy },
    /// Fly to a body, here or in another system, or to another ship, and
    /// drop out near it.
    ///
    /// Engages warp, turns to the target, curves round bodies in the way,
    /// then slows all the way in, dropping out at a standoff of a few times
    /// the target's size. Another system is loaded first and entered on the
    /// slowdown. Moving targets are tracked every step. Turning, warp speed,
    /// disengage and match orders take over; warp is left as it was.
    To {
        /// Planet, star or barycentre ID, a system's key, or another
        /// craft's ID.
        target: String,
        /// Drop out at the target's velocity.
        #[serde(default, rename = "match")]
        matched: bool,
        /// Drop out into a rough orbit around the target; matches too.
        #[serde(default)]
        orbit: bool,
        /// Trip time, including the first turn (default 20 to 60 s by
        /// distance).
        #[serde(default)]
        #[schemars(range(min = MIN_TRIP_S, max = MAX_TRIP_S), extend("x-unit" = "s"))]
        time: Option<f64>,
    },
    /// End the route, leaving warp engaged at its current speed.
    Cancel,
}

/// Controls an order sets, as bits of `Order::controls`.
const THROTTLE: u8 = 1;
const TRANSLATION: u8 = 2;
const TURNING: u8 = 4;
const THRUST: u8 = 8;
const WARP: u8 = 16;
const SWEEP: u8 = 32;

impl Order {
    /// The order that ends this one when it is given for a time: its axes
    /// back to zero, or out of warp. Motion and spin are kept, as when the
    /// controls are let go. Orders with no such rest state, like `stop` or
    /// `warp set`, cannot be timed.
    pub fn off(&self) -> Option<Order> {
        Some(match self {
            Self::Throttle { .. } => Self::Throttle { value: 0.0 },
            Self::Translate { .. } => Self::Translate {
                right: 0.0,
                up: 0.0,
                forward: 0.0,
            },
            Self::Rotate { .. } | Self::Point { .. } => Self::Rotate {
                pitch: 0.0,
                yaw: 0.0,
                roll: 0.0,
            },
            Self::Thrust { .. } => Self::Thrust { thrust: [0.0; 3] },
            Self::Warp {
                command: WarpOrder::Engage,
            } => Self::Warp {
                command: WarpOrder::Disengage,
            },
            Self::Warp {
                command: WarpOrder::Sweep { .. },
            } => Self::Warp {
                command: WarpOrder::Sweep { rate: 0.0 },
            },
            _ => return None,
        })
    }

    /// Whether this order sets any control `other` does, so applying it
    /// replaces `other`'s effect.
    pub fn overlaps(&self, other: &Order) -> bool {
        self.controls() & other.controls() != 0
    }

    fn controls(&self) -> u8 {
        match self {
            Self::Throttle { .. } => THROTTLE,
            Self::Translate { .. } => TRANSLATION,
            Self::Rotate { .. } | Self::Point { .. } | Self::Stop | Self::Release => TURNING,
            Self::Cut => THROTTLE | TRANSLATION | TURNING | THRUST,
            Self::Thrust { .. } => THRUST,
            Self::Warp {
                command:
                    WarpOrder::Engage
                    | WarpOrder::Disengage
                    | WarpOrder::Match { .. }
                    | WarpOrder::To { .. }
                    | WarpOrder::Cancel,
            } => WARP,
            Self::Warp {
                command: WarpOrder::Sweep { .. },
            } => SWEEP,
            Self::Configure { .. } | Self::Warp { .. } => 0,
        }
    }

    /// State-independent validation, also run when future inputs are submitted.
    /// Drive interlocks and body/frame checks remain application-time rules.
    pub fn validate(&self) -> Result<(), String> {
        let fractions: &[f64] = match self {
            Self::Throttle { value } => std::slice::from_ref(value),
            Self::Translate { right, up, forward } => &[*right, *up, *forward],
            Self::Rotate { pitch, yaw, roll } => &[*pitch, *yaw, *roll],
            Self::Warp {
                command: WarpOrder::Sweep { rate },
            } => std::slice::from_ref(rate),
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
                if let PointTarget::Body(id) = target {
                    body_id(id)?;
                }
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
            Self::Warp {
                command: WarpOrder::TurnRate { degrees_per_second },
            } => {
                super::warp::Warp::validate_turn_rate(*degrees_per_second)?;
            }
            Self::Warp {
                command: WarpOrder::Match { body, .. },
            } => body_id(body)?,
            Self::Warp {
                command: WarpOrder::To { target, time, .. },
            } => {
                body_id(target)?;
                if time.is_some_and(|t| !(MIN_TRIP_S..=MAX_TRIP_S).contains(&t)) {
                    return Err(format!(
                        "Trip time must be {MIN_TRIP_S} to {MAX_TRIP_S} seconds"
                    ));
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// Every order, warp included, takes effect from the craft's next fixed
    /// step (`Craft::tick`), so a server can key orders by step number.
    /// `field` is the craft's system, when it is in one, to check bodies.
    pub fn apply(&self, craft: &mut Craft, field: Option<&Field>) -> Result<(), String> {
        self.apply_among(craft, field, &[])
    }

    /// As `apply`, with the other craft as targets, in the craft's frame.
    pub fn apply_among(
        &self,
        craft: &mut Craft,
        field: Option<&Field>,
        ships: &[Target],
    ) -> Result<(), String> {
        self.validate()?;
        let mut controls = craft.input.controls;
        match self {
            Self::Warp { command } => {
                match command {
                    WarpOrder::Engage => craft.engage_warp()?,
                    WarpOrder::Disengage => {
                        craft.end_route(TAKEOVER);
                        craft.disengage_warp("Manual disengagement");
                    }
                    WarpOrder::Match { body, orbit } => {
                        craft.matched_dropout_among(field, ships, body, *orbit)?;
                        craft.end_route(TAKEOVER);
                    }
                    WarpOrder::Set { speed_ms } => {
                        craft.warp.set_speed(*speed_ms)?;
                        craft.end_route(TAKEOVER);
                    }
                    WarpOrder::TurnRate { degrees_per_second } => {
                        craft.warp.set_turn_rate(*degrees_per_second)?
                    }
                    WarpOrder::Up => {
                        craft
                            .warp
                            .set_speed((craft.warp.requested_ms * SPEED_FACTOR).min(MAX_SPEED))?;
                        craft.end_route(TAKEOVER);
                    }
                    WarpOrder::Down => {
                        craft
                            .warp
                            .set_speed((craft.warp.requested_ms / SPEED_FACTOR).max(MIN_SPEED))?;
                        craft.end_route(TAKEOVER);
                    }
                    WarpOrder::Sweep { rate } => {
                        if *rate != 0.0 {
                            if !craft.warp.engaged {
                                return Err("Engage warp before sweeping its speed".into());
                            }
                            craft.end_route(TAKEOVER);
                        }
                        craft.warp.sweep = *rate;
                    }
                    WarpOrder::To {
                        target,
                        matched,
                        orbit,
                        time,
                    } => {
                        let arrival = match (*matched, *orbit) {
                            (_, true) => ArrivalMode::Orbit,
                            (true, false) => ArrivalMode::Match,
                            (false, false) => ArrivalMode::Keep,
                        };
                        craft.fly_to(field, ships, target, arrival, *time)?;
                        craft.warp.sweep = 0.0;
                    }
                    WarpOrder::Cancel => {
                        if !craft.end_route("Cancelled") {
                            return Err("No route is being flown".into());
                        }
                    }
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
