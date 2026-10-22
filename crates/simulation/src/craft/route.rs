//! The route pilot: a plotted warp trip to a target. Every step it gives the
//! orders a player would: it points the bubble, sets the requested speed and,
//! at the standoff, drops out as the route asks. It never moves the ship
//! itself, so takeover and cancelling leave nothing to jump.
//!
//! The speed profile is the remaining distance over a time constant τ, so the
//! ship covers the same share of what is left every second: a steady zoom
//! that reaches the standoff after τ × ln(distance ÷ standoff).
use super::*;
use crate::AU_METRES;
use warp::{MAX_SPEED, MIN_SPEED, RAMP_SECONDS};

/// Trip times an order may ask for, in seconds.
pub const MIN_TRIP_S: f64 = 10.0;
pub const MAX_TRIP_S: f64 = 3_600.0;
/// Default trip times: a short hop, and the longest, between systems.
pub const SHORT_TRIP_S: f64 = 20.0;
pub const LONG_TRIP_S: f64 = 60.0;
/// Standoff from a target's centre, in multiples of its size.
pub const STANDOFF_SIZES: f64 = 4.0;
/// Keep-clear distance from a body's centre, in multiples of its radius.
pub const KEEP_CLEAR: f64 = 1.5;
/// Share of a target's orbit reach the standoff stays within, so a rough
/// orbit on arrival is still around it.
pub const REACH_SHARE: f64 = 0.9;
/// Nose angle within which the ship is lined up, and beyond which speed is
/// held at the minimum. Speed ramps up between them.
pub const ALIGNED_DEG: f64 = 2.0;
pub const ALIGNING_DEG: f64 = 10.0;
/// Most the pilot aims ahead of a turning line of sight.
pub const MAX_LEAD_DEG: f64 = 45.0;
/// The approach is the last part of a trip, within this many standoffs.
pub const APPROACH_STANDOFFS: f64 = 10.0;
/// Shortest time constant: the speed ramp must keep up with the profile.
pub const MIN_TAU_S: f64 = 2.0 * RAMP_SECONDS;
/// Why a turning or warp speed order ends a route.
pub(super) const TAKEOVER: &str = "Manual takeover";

/// How the ship leaves warp at the standoff.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ArrivalMode {
    /// Keep the ship's ordinary velocity.
    #[default]
    Keep,
    /// Match the target's velocity.
    Match,
    /// Match it, plus the sideways speed for a rough orbit.
    Orbit,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    /// Turning to the target, at the lowest speed.
    Align,
    /// Lined up, slowing all the way in.
    Cruise,
    /// Within ten standoffs.
    Approach,
    /// Dropped out at the standoff.
    Arrived,
    /// Cancelled or taken over; see `end`.
    Released,
}

/// A plotted trip, kept after it ends for inspection.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Route {
    /// The target's ID: a system's key resolves to its star or outermost
    /// barycentre.
    pub target: String,
    pub arrival: ArrivalMode,
    pub stage: Stage,
    /// Trip time asked for; none for the distance-scaled default.
    #[schemars(extend("x-unit" = "s"))]
    pub requested_s: Option<f64>,
    /// Planned trip time, from the order's step.
    #[schemars(extend("x-unit" = "s"))]
    pub trip_s: f64,
    /// The step the order applied at.
    pub start_tick: i64,
    /// Distance from the target's centre where the ship drops out.
    #[schemars(extend("x-unit" = "m"))]
    pub standoff_m: f64,
    /// Speed is the remaining distance over this.
    #[schemars(extend("x-unit" = "s"))]
    pub tau_s: f64,
    /// From the target's centre, at the last step.
    #[schemars(extend("x-unit" = "m"))]
    pub distance_m: f64,
    /// Estimated time to arrival.
    #[schemars(extend("x-unit" = "s"))]
    pub eta_s: f64,
    /// Expected arrival after the planned trip time; negative when early.
    #[schemars(extend("x-unit" = "s"))]
    pub late_s: f64,
    /// The top warp speed stretches the trip.
    pub capped: bool,
    /// The nose has lined up once.
    pub aligned: bool,
    /// How the route ended.
    pub end: Option<String>,
}

impl Route {
    pub fn active(&self) -> bool {
        matches!(self.stage, Stage::Align | Stage::Cruise | Stage::Approach)
    }
}

/// What the pilot knows about a target each step, whatever kind it is.
#[derive(Clone, Debug, PartialEq)]
pub struct Target {
    pub id: String,
    pub state: State,
    /// Sets the standoff: a body's radius, or a pair's separation.
    pub size_m: f64,
    /// Nearest the ship may stop to its centre: past its surface, or its
    /// stars.
    pub clear_m: f64,
    /// Furthest a rough orbit still goes round it; none when it cannot be
    /// orbited, infinite for a whole system.
    pub reach_m: Option<f64>,
    /// GM of everything inside it, in m³/s².
    pub gm: f64,
    /// Surface radius; zero for a barycentre.
    pub radius_m: f64,
    /// The page star of its system.
    pub system: String,
}

impl Target {
    /// A comfortable multiple of its size, never closer than its keep-clear
    /// distance and never further than its orbit reach.
    pub fn standoff(&self) -> Result<f64, String> {
        let mut standoff = (STANDOFF_SIZES * self.size_m).max(self.clear_m);
        if let Some(reach) = self.reach_m {
            standoff = standoff.min(REACH_SHARE * reach);
        }
        if !standoff.is_finite() || standoff < self.clear_m || standoff <= 0.0 {
            return Err(format!(
                "`{}` has no safe standoff: its orbit reach is barely beyond what the ship must keep clear of",
                self.id
            ));
        }
        Ok(standoff)
    }
}

impl Field {
    /// A planet, star or barycentre of this field as a route target. A
    /// system's key names its star or outermost barycentre.
    pub fn target(&self, id: &str, seconds: f64) -> Option<Target> {
        let root = self.system.root(self.page)?;
        let id = if crate::multiple::system_key(root) == id
            && self.reference_by_id(id, seconds).is_none()
        {
            root
        } else {
            id
        };
        let reference = self.reference_by_id(id, seconds)?;
        let (size_m, clear_m) = match reference.kind {
            ReferenceKind::Planet | ReferenceKind::Star => {
                (reference.radius_m, KEEP_CLEAR * reference.radius_m)
            }
            ReferenceKind::Barycentre => {
                let pair = self.system.barycentre(id)?;
                let at = |id: &str| crate::member_state(&self.system, self.page, id, seconds);
                let size = distance(at(&pair.members[0]).position, at(&pair.members[1]).position);
                let clear = self
                    .stars_within(id)
                    .map(|(star, radius)| {
                        distance(at(star).position, reference.state.position) + KEEP_CLEAR * radius
                    })
                    .fold(0.0, f64::max);
                (size, clear)
            }
        };
        Some(Target {
            id: reference.id,
            state: reference.state,
            size_m,
            clear_m,
            reach_m: Some(reference.reach_m),
            gm: reference.gm,
            radius_m: reference.radius_m,
            system: voyage::page_star_id(&self.system, self.page),
        })
    }

    /// The stars inside a star or barycentre, with their radii.
    fn stars_within<'a>(&'a self, id: &'a str) -> impl Iterator<Item = (&'a str, f64)> + 'a {
        self.stars
            .iter()
            .filter(move |(star, ..)| self.system.lineage(self.page, star).iter().any(|m| m == id))
            .map(|(star, _, radius)| (star.as_str(), *radius))
    }
}

/// Requested bubble speed `remaining_m` from the target's centre: what is
/// left over τ, lowered so that the bubble's speed ramp, lagging behind,
/// still flies the profile. τ must be at least `MIN_TAU_S`.
pub fn profile_speed(remaining_m: f64, tau_s: f64) -> f64 {
    remaining_m / tau_s * (1.0 - RAMP_SECONDS / tau_s)
}

/// The time constant that covers `distance_m` down to `standoff_m` in
/// `seconds`, no shorter than `MIN_TAU_S`.
pub fn fit_tau(distance_m: f64, standoff_m: f64, seconds: f64) -> f64 {
    let fall = (distance_m / standoff_m).ln();
    if fall > 0.0 && seconds > 0.0 {
        (seconds / fall).max(MIN_TAU_S)
    } else {
        MIN_TAU_S
    }
}

/// Seconds the profile takes from `distance_m` to `standoff_m`, flying at
/// the top warp speed where the profile would be faster.
pub fn profile_seconds(distance_m: f64, standoff_m: f64, tau_s: f64) -> f64 {
    if distance_m <= standoff_m {
        return 0.0;
    }
    let fastest = MAX_SPEED * tau_s;
    if fastest <= standoff_m {
        (distance_m - standoff_m) / MAX_SPEED
    } else if distance_m > fastest {
        (distance_m - fastest) / MAX_SPEED + tau_s * (fastest / standoff_m).ln()
    } else {
        tau_s * (distance_m / standoff_m).ln()
    }
}

/// The default trip time for a straight distance: about 20 s for a hop
/// within a system, rising 10 s for each tenfold distance beyond 1 AU, up to
/// 60 s.
pub fn default_trip_s(distance_m: f64) -> f64 {
    (SHORT_TRIP_S + 10.0 * (distance_m / AU_METRES).log10()).clamp(SHORT_TRIP_S, LONG_TRIP_S)
}

/// Seconds the bubble takes to turn the nose `angle` radians and settle
/// within `ALIGNED_DEG`, at `turn_rate` rad/s: flat out, then easing in as
/// pointing does.
pub fn align_seconds(angle: f64, turn_rate: f64) -> f64 {
    let aligned = ALIGNED_DEG.to_radians();
    if angle <= aligned {
        return 0.0;
    }
    let easing = attitude::WARP_POINT_SECONDS;
    let eased = (turn_rate * easing).max(aligned);
    (angle - eased).max(0.0) / turn_rate + easing * (angle.min(eased) / aligned).ln()
}

impl Craft {
    /// Plot a route to `id` in the ship's system and engage warp. All
    /// checks come first, so a refused order leaves the ship unchanged.
    pub fn fly_to(
        &mut self,
        field: Option<&Field>,
        id: &str,
        arrival: ArrivalMode,
        requested_s: Option<f64>,
    ) -> Result<(), String> {
        if id == self.id {
            return Err("A ship cannot fly to itself".into());
        }
        let field = field.ok_or("Routes start inside a system; galactic routes come later")?;
        let seconds = self.seconds();
        let target = field
            .target(id, seconds)
            .ok_or_else(|| format!("No body `{id}` in this ship's system"))?;
        if arrival == ArrivalMode::Orbit && target.reach_m.is_none() {
            return Err(format!("`{id}` cannot be orbited"));
        }
        let standoff = target.standoff()?;
        let offset =
            DVec3::from_array(target.state.position) - DVec3::from_array(self.state().position);
        let distance = offset.length();
        if distance <= standoff {
            return Err(format!(
                "Already within the standoff of `{id}`; use warp match to drop out here"
            ));
        }
        let nose = DQuat::from_array(self.orientation) * -DVec3::Z;
        let align = align_seconds(nose.angle_between(offset), self.turn_rate());
        let trip = requested_s.unwrap_or_else(|| default_trip_s(distance));
        let tau = fit_tau(distance, standoff, trip - align);
        if !self.warp.engaged {
            self.engage_warp()?;
        }
        self.input.controls.rotation = [0.0; 3];
        self.autopilot = AttitudePilot::Point {
            target: PointTarget::Vector((offset / distance).to_array()),
            up: None,
            attitude: self.orientation,
        };
        let eta = align + profile_seconds(distance, standoff, tau);
        self.route = Some(Route {
            target: target.id,
            arrival,
            stage: Stage::Align,
            requested_s,
            trip_s: trip,
            start_tick: self.tick,
            standoff_m: standoff,
            tau_s: tau,
            distance_m: distance,
            eta_s: eta,
            late_s: eta - trip,
            capped: distance / tau > MAX_SPEED,
            aligned: false,
            end: None,
        });
        Ok(())
    }

    /// End an active route, handing turning back to manual. Warp is left as
    /// it is. Returns whether there was one.
    pub fn end_route(&mut self, reason: &str) -> bool {
        let Some(route) = self.route.as_mut().filter(|route| route.active()) else {
            return false;
        };
        route.stage = Stage::Released;
        route.end = Some(reason.into());
        self.autopilot = AttitudePilot::Manual;
        true
    }

    fn turn_rate(&self) -> f64 {
        self.warp.turn_rate_dps.to_radians()
    }

    /// One step of the route pilot, before the ship moves: drop out at the
    /// standoff, else aim and set the speed for this step.
    pub(super) fn fly_route(&mut self, field: &impl Gravity) {
        let Some(route) = self.route.as_ref().filter(|route| route.active()) else {
            return;
        };
        if !self.warp.engaged {
            self.end_route("Warp disengaged");
            return;
        }
        let seconds = self.seconds();
        let Some(target) = field.target(&route.target, seconds) else {
            let reason = format!("Lost track of `{}`", route.target);
            self.end_route(&reason);
            return;
        };
        let state = self.state();
        let offset = DVec3::from_array(target.state.position) - DVec3::from_array(state.position);
        let distance = offset.length();
        if distance <= route.standoff_m {
            self.arrive(&target, distance);
            return;
        }
        let nose = DQuat::from_array(self.orientation) * -DVec3::Z;
        let turn_rate = self.turn_rate();
        let tick = self.tick;
        let route = self.route.as_mut().expect("active route");
        let approach = distance < APPROACH_STANDOFFS * route.standoff_m;
        // Fit τ again once lined up, then once a second: the turn and the
        // speed ramp rarely take exactly the time allowed for them.
        let first = !route.aligned && nose.angle_between(offset) <= ALIGNED_DEG.to_radians();
        if first || (route.aligned && !approach && (tick - route.start_tick) % REFERENCE_STEPS == 0)
        {
            let left = super::tick_seconds(route.start_tick) + route.trip_s - seconds;
            route.tau_s = fit_tau(distance, route.standoff_m, left);
        }
        route.aligned |= first;
        // Close on the target along the line to it, and cancel the drift of
        // the ship's ordinary velocity against the target's.
        let sight = offset / distance;
        let drift = DVec3::from_array(target.state.velocity) - DVec3::from_array(state.velocity);
        let bubble = sight * profile_speed(distance, route.tau_s) + drift;
        // Flying off the line turns the line of sight, faster than pointing
        // follows once τ is short. Aiming ahead by as much as it turns over
        // pointing's response keeps the nose on it.
        let closing = drift - nose * self.warp.actual_ms;
        let lead = (sight.cross(closing) / distance * attitude::WARP_POINT_SECONDS)
            .clamp_length_max(MAX_LEAD_DEG.to_radians());
        let direction = DQuat::from_scaled_axis(lead) * bubble.normalize_or(sight);
        let angle = nose.angle_between(direction);
        let share = ((ALIGNING_DEG.to_radians() - angle)
            / (ALIGNING_DEG - ALIGNED_DEG).to_radians())
        .clamp(0.0, 1.0);
        self.warp.requested_ms = (bubble.length() * share).clamp(MIN_SPEED, MAX_SPEED);
        let eta = if route.aligned {
            0.0
        } else {
            align_seconds(angle, turn_rate)
        } + profile_seconds(distance, route.standoff_m, route.tau_s);
        route.stage = match (route.aligned, approach) {
            (false, _) => Stage::Align,
            (true, false) => Stage::Cruise,
            (true, true) => Stage::Approach,
        };
        route.distance_m = distance;
        route.eta_s = eta;
        route.late_s = seconds + eta - super::tick_seconds(route.start_tick) - route.trip_s;
        route.capped = distance / route.tau_s > MAX_SPEED;
        match &mut self.autopilot {
            AttitudePilot::Point {
                target: PointTarget::Vector(vector),
                ..
            } => *vector = direction.to_array(),
            pilot => {
                *pilot = AttitudePilot::Point {
                    target: PointTarget::Vector(direction.to_array()),
                    up: None,
                    attitude: self.orientation,
                }
            }
        }
    }

    /// Drop out at the standoff as the route asks. A rough orbit that cannot
    /// be had here falls back to matching the target.
    fn arrive(&mut self, target: &Target, distance: f64) {
        let route = self.route.as_ref().expect("active route");
        let mode = route.arrival;
        let mut end = format!("Arrived at `{}`", target.id);
        match mode {
            ArrivalMode::Keep => self.disengage_warp(&end),
            ArrivalMode::Match | ArrivalMode::Orbit => {
                let mut orbit = mode == ArrivalMode::Orbit;
                let velocity = self.matched_velocity(target, orbit).or_else(|reason| {
                    end = format!("{end}, matched only: {reason}");
                    orbit = false;
                    self.matched_velocity(target, false)
                });
                match velocity {
                    Ok(velocity) => self.drop_matched(velocity, &target.id, orbit),
                    Err(reason) => {
                        end = format!("Arrived at `{}`, unmatched: {reason}", target.id);
                        self.disengage_warp(&end);
                    }
                }
            }
        }
        self.autopilot = AttitudePilot::Manual;
        let route = self.route.as_mut().expect("active route");
        route.stage = Stage::Arrived;
        route.distance_m = distance;
        route.eta_s = 0.0;
        route.late_s =
            super::tick_seconds(self.tick) - super::tick_seconds(route.start_tick) - route.trip_s;
        route.end = Some(end);
    }
}

#[cfg(test)]
mod tests;
