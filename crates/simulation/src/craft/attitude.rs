//! Attitude guidance: thruster-limited rotation fractions in ordinary flight,
//! bubble turn rates in warp. It never edits orientation or spin. All vectors
//! and roll references use system axes.
use super::*;
use glam::DMat3;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "target", content = "value", rename_all = "snake_case")]
pub enum PointTarget {
    /// Along the velocity relative to the orbited body.
    Prograde,
    /// Against the velocity relative to the orbited body.
    Retrograde,
    /// Along the orbit's angular momentum.
    Normal,
    /// Against the orbit's angular momentum.
    Antinormal,
    /// Towards the orbited body.
    RadialIn,
    /// Away from the orbited body.
    RadialOut,
    /// Towards a body in the craft's system, by ID.
    Body(String),
    /// A fixed direction in system axes.
    Vector(#[schemars(extend("x-unit" = ""))] [f64; 3]),
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum AttitudePilot {
    /// No pilot: rotation inputs act directly.
    #[default]
    Manual,
    /// Braking spin to zero.
    Stop,
    /// Turning to and tracking a target.
    Point {
        target: PointTarget,
        #[schemars(extend("x-unit" = ""))]
        up: Option<[f64; 3]>,
        /// Parallel-transported desired attitude retains roll without an
        /// arbitrary world-up discontinuity as a target completes an orbit.
        #[schemars(extend("x-unit" = ""))]
        attitude: [f64; 4],
    },
}

impl Craft {
    /// Engage pointing. An optional up vector fixes roll (projected onto the
    /// plane normal to the target); without one the initial roll is retained.
    pub fn point(&mut self, target: PointTarget, up: Option<[f64; 3]>) -> Result<(), &'static str> {
        if let PointTarget::Vector(v) = &target
            && unit(*v).is_none()
        {
            return Err("direction must be finite and nonzero");
        }
        if up.is_some_and(|v| unit(v).is_none()) {
            return Err("up must be finite and nonzero");
        }
        self.input.controls.rotation = [0.0; 3];
        self.autopilot = AttitudePilot::Point {
            target,
            up,
            attitude: self.orientation,
        };
        Ok(())
    }

    /// Brake angular velocity only. Linear velocity and throttle are untouched.
    pub fn stop_rotation(&mut self) {
        self.input.controls.rotation = [0.0; 3];
        self.autopilot = AttitudePilot::Stop;
    }

    /// Return to manual rotation, with no residual autopilot thruster input.
    pub fn release(&mut self) {
        self.autopilot = AttitudePilot::Manual;
    }

    pub(super) fn guidance(&mut self, field: &impl Gravity) -> [f64; 3] {
        if matches!(self.autopilot, AttitudePilot::Manual) {
            return self.input.controls.rotation;
        }
        let error = self.attitude_error(field);
        let spin = self.spin;
        let limits = self.thrusters.angular;
        std::array::from_fn(|i| {
            let limit = limits[i];
            if limit <= 0.0 {
                return 0.0;
            }
            // Slow down well before the ideal stopping boundary. The linear
            // near-target branch is overdamped; the last step cannot reverse
            // the remaining rate. No attitude or spin snapping is used.
            let desired = error[i].signum()
                * (0.8 * error[i].abs())
                    .min((limit * error[i].abs()).sqrt())
                    .min(0.5);
            let acceleration = ((desired - spin[i]) / 0.25).clamp(-limit, limit);
            acceleration / limit
        })
    }

    /// The warp bubble's target turn rate in rad/s, body axes: manual input
    /// scaled by the turn rate, or pointing at a gain that critically damps
    /// the bubble's rate response, so a turn from rest does not overshoot.
    /// The extra step allows for holding each target for a whole step.
    /// The total rate never exceeds the turn rate.
    pub(super) fn steering(&mut self, field: &impl Gravity) -> DVec3 {
        let most = self.warp.turn_rate_dps.to_radians();
        let rate = match self.autopilot {
            AttitudePilot::Manual => DVec3::from_array(self.input.controls.rotation) * most,
            _ => self.attitude_error(field) / (4.0 * (warp::RAMP_SECONDS + STEP_SECONDS)),
        };
        rate.clamp_length_max(most)
    }

    /// The shortest rotation from the current attitude to the pilot's goal,
    /// as a body-axis scaled axis. Zero to brake, or while the target is
    /// undefined. Advances the transported goal of a pointing pilot.
    fn attitude_error(&mut self, field: &impl Gravity) -> DVec3 {
        let direction = match &self.autopilot {
            AttitudePilot::Manual | AttitudePilot::Stop => None,
            AttitudePilot::Point { target, .. } => target.direction(
                self.state(),
                self.seconds(),
                field,
                self.reference.as_deref(),
            ),
        };
        let current = DQuat::from_array(self.orientation);
        let mut error = DVec3::ZERO;
        if let (Some(forward), AttitudePilot::Point { up, attitude, .. }) =
            (direction, &mut self.autopilot)
        {
            let previous = DQuat::from_array(*attitude);
            let mut goal = DQuat::from_rotation_arc(previous * -DVec3::Z, forward) * previous;
            if let Some(up) = up {
                // At the up-vector pole, roll is undefined: retain transported
                // roll instead of normalizing a zero cross product.
                if let Some(right) = unit(forward.cross(DVec3::from_array(*up)).to_array()) {
                    goal =
                        DQuat::from_mat3(&DMat3::from_cols(right, right.cross(forward), -forward));
                }
            }
            goal = goal.normalize();
            *attitude = goal.to_array();
            let mut difference = current.conjugate() * goal;
            if difference.w < 0.0 {
                difference = -difference;
            }
            error = difference.to_scaled_axis();
        }
        error
    }
}

impl PointTarget {
    /// Resolve against the body's instantaneous state, not its spawn-time
    /// state. Orbital directions are relative to `orbit`, the craft's orbit
    /// reference, or one chosen here when it has none yet.
    pub fn direction(
        &self,
        state: State,
        seconds: f64,
        field: &impl Gravity,
        orbit: Option<&str>,
    ) -> Option<DVec3> {
        if let Self::Vector(v) = self {
            return unit(*v);
        }
        let id = if let Self::Body(id) = self {
            Some(id.as_str())
        } else {
            orbit
        };
        let body = field.reference(id, state.position, seconds)?;
        let radial = DVec3::from_array(state.position) - DVec3::from_array(body.position);
        let velocity = DVec3::from_array(state.velocity) - DVec3::from_array(body.velocity);
        let vector = match self {
            Self::Prograde => velocity,
            Self::Retrograde => -velocity,
            Self::Normal => radial.cross(velocity),
            Self::Antinormal => -radial.cross(velocity),
            Self::RadialIn | Self::Body(_) => -radial,
            Self::RadialOut => radial,
            Self::Vector(_) => unreachable!(),
        };
        unit(vector.to_array())
    }
}

pub(super) fn unit(v: [f64; 3]) -> Option<DVec3> {
    let vector = DVec3::from_array(v);
    let magnitude = vector.abs().max_element();
    if !vector.is_finite() || magnitude < 1e-12 {
        return None;
    }
    Some((vector / magnitude).normalize())
}

#[cfg(test)]
mod tests;
