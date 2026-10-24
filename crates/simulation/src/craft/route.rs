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
use std::f64::consts::FRAC_PI_2;
use warp::{MAX_SPEED, MIN_SPEED, RAMP_SECONDS};
use wrap::Bend;
pub use wrap::Obstacle;

mod wrap;

/// Trip times an order may ask for, in seconds.
pub const MIN_TRIP_S: f64 = 10.0;
pub const MAX_TRIP_S: f64 = 3_600.0;
/// Default trip times: a short hop, and the longest, between systems.
pub const SHORT_TRIP_S: f64 = 20.0;
pub const LONG_TRIP_S: f64 = 60.0;
/// Standoff from a target's centre, in multiples of its size.
pub const STANDOFF_SIZES: f64 = 4.0;
/// Standoff from another craft: formation distance, well clear of its hull.
pub const CRAFT_STANDOFF_M: f64 = 1_000.0;
/// Keep-clear distance from a body's centre, in multiples of its radius.
pub use crate::system::clearance::KEEP_CLEAR;
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
/// Share of the turn rate a bend round a body may use, leaving the rest for
/// pointing to catch up.
pub const BEND_SHARE: f64 = 0.5;
/// Least time the ship takes to reach a sphere it is heading into: enough
/// for pointing to respond and the speed ramp to slow it.
pub const IMPACT_SECONDS: f64 = attitude::WARP_POINT_SECONDS + RAMP_SECONDS;
/// A trip this far behind its time says what is slowing it.
pub const LATE_S: f64 = 0.5;
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
    /// Waiting for the data of the target's system, when it is not the
    /// ship's own; plotted on the first step that has it.
    Plotting,
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
    /// The page star of the target's system; empty for another craft.
    #[serde(default)]
    pub system: String,
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
    /// How much longer the path round bodies in the way is than the straight
    /// line, as last measured.
    #[schemars(extend("x-unit" = "m"))]
    pub detour_m: f64,
    /// Time allowed for slowing round the bends still ahead.
    #[schemars(extend("x-unit" = "s"))]
    pub bends_s: f64,
    /// The body being steered round now.
    pub around: Option<String>,
    /// What keeps a late trip from its time, such as a detour.
    pub slowed_by: Option<String>,
    /// The nose has lined up once.
    pub aligned: bool,
    /// How the route ended.
    pub end: Option<String>,
    /// Approximations behind the clearances this route keeps, such as an
    /// extended atmosphere's constant-gravity estimate.
    #[serde(default)]
    pub clearance_warning: Option<String>,
}

impl Route {
    pub fn active(&self) -> bool {
        matches!(
            self.stage,
            Stage::Plotting | Stage::Align | Stage::Cruise | Stage::Approach
        )
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
    /// The page star of its system; empty for a craft.
    pub system: String,
    /// What a warp bubble adds to its motion: tracked, never matched.
    pub warp_velocity: [f64; 3],
    pub clearance_error: Option<String>,
}

impl Target {
    /// Another craft, as it was at the start of the step: flown to within
    /// formation distance, never orbited.
    pub fn craft(id: &str, state: State, warp_velocity: [f64; 3]) -> Self {
        Self {
            id: id.into(),
            state,
            size_m: SHIP_LENGTH_M,
            clear_m: CRAFT_STANDOFF_M,
            reach_m: None,
            gm: 0.0,
            radius_m: SHIP_LENGTH_M / 2.0,
            system: String::new(),
            warp_velocity,
            clearance_error: None,
        }
    }

    /// A comfortable multiple of its size, never closer than its keep-clear
    /// distance and never further than its orbit reach.
    pub fn standoff(&self) -> Result<f64, String> {
        if let Some(error) = &self.clearance_error {
            return Err(format!("Unsafe clearance for `{}`: {error}", self.id));
        }
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
        let id = if crate::star::multiple::system_key(root) == id
            && self.reference_by_id(id, seconds).is_none()
        {
            root
        } else {
            id
        };
        let reference = self.reference_by_id(id, seconds)?;
        let clearance = self.clearances.get(&reference.id);
        let clearance_error = clearance.and_then(|c| c.as_ref().err()).cloned();
        let (size_m, clear_m) = match reference.kind {
            ReferenceKind::Planet | ReferenceKind::Star => (
                reference.radius_m,
                clearance
                    .and_then(|c| c.as_ref().ok())
                    .map_or(0.0, |c| c.preferred_m),
            ),
            ReferenceKind::Barycentre => {
                let pair = self.system.barycentre(id)?;
                let at = |id: &str| crate::member_state(&self.system, self.page, id, seconds);
                let size = distance(at(&pair.members[0]).position, at(&pair.members[1]).position);
                let clear = self
                    .stars_within(id)
                    .map(|(star, radius)| {
                        distance(at(star).position, reference.state.position)
                            + self
                                .clearances
                                .get(star)
                                .and_then(|c| c.as_ref().ok())
                                .map_or(KEEP_CLEAR * radius, |c| c.preferred_m)
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
            warp_velocity: [0.0; 3],
            clearance_error,
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

/// Seconds allowed for slowing round bends at `turn_rate` rad/s. Round
/// each, speed is capped by how far the aim is pulled off the line, which
/// eases as the target comes into view: the last quarter turn takes
/// 1 − cos of what is left of it, any more is flown at the full cap. Then
/// pointing settles on the new line, for longer the larger the turn.
fn bend_seconds(bends: &[Bend], turn_rate: f64) -> f64 {
    bends
        .iter()
        .map(|bend| one_bend_seconds(bend.turn, turn_rate))
        .sum()
}

fn one_bend_seconds(turn: f64, turn_rate: f64) -> f64 {
    let eased = 1.0 - turn.min(FRAC_PI_2).cos();
    let settle = attitude::WARP_POINT_SECONDS * (turn / FRAC_PI_2).min(1.0);
    (eased + (turn - FRAC_PI_2).max(0.0)) / (BEND_SHARE * turn_rate) + settle
}

/// What keeps a trip from its time: the first turn, the slowest bend or the
/// top warp speed, whichever costs most.
fn slowed_by(align_s: f64, bends: &[Bend], turn_rate: f64, capped: bool) -> Option<String> {
    let bend = bends
        .iter()
        .map(|bend| (bend, one_bend_seconds(bend.turn, turn_rate)))
        .max_by(|a, b| a.1.total_cmp(&b.1));
    match bend {
        Some((bend, seconds)) if seconds >= align_s => {
            Some(format!("the detour round `{}`", bend.id))
        }
        _ if capped && align_s < LATE_S => Some("the top warp speed".into()),
        _ if align_s > 0.0 => Some("turning round first".into()),
        _ => None,
    }
}

/// Every obstacle but the target, which the route ends outside of anyway,
/// after refusing unknown clearances, a start inside an exclusion and an
/// arrival that would cross one. A preferred keep-clear sphere reaching the
/// arrival, as a planet's does a ship in low orbit, shrinks towards it, but
/// never below the body's exclusion.
fn others(
    field: &impl Gravity,
    target: &Target,
    seconds: f64,
    from: DVec3,
    standoff: f64,
) -> Result<Vec<Obstacle>, String> {
    let goal = DVec3::from_array(target.state.position);
    let mut obstacles = field.obstacles(seconds);
    // Validate all bodies before excluding the arrival target. Unknown data
    // must never quietly remove an obstacle from a certified route.
    for obstacle in &obstacles {
        if let Some(error) = &obstacle.error {
            return Err(format!("Unsafe clearance for `{}`: {error}", obstacle.id));
        }
        if obstacle.centre.distance(from) < obstacle.exclusion_m {
            return Err(format!(
                "Ship is inside the navigation exclusion of `{}`; leave it manually before plotting",
                obstacle.id
            ));
        }
        // The ship stops on the standoff sphere, so only an exclusion that
        // crosses that sphere is in the way: a barycentre's own stars lie
        // well inside it.
        if obstacle.id != target.id
            && (obstacle.centre.distance(goal) - standoff).abs() < obstacle.exclusion_m
        {
            return Err(format!(
                "Arrival region intersects the navigation exclusion of `{}`",
                obstacle.id
            ));
        }
    }
    obstacles.retain(|obstacle| obstacle.id != target.id);
    for obstacle in &mut obstacles {
        obstacle.clear_m = obstacle.clear_m.min(
            (obstacle.centre.distance(goal) - standoff)
                .abs()
                .max(obstacle.exclusion_m),
        );
    }
    Ok(obstacles)
}

/// Approximations behind the bodies' clearances, such as an extended
/// atmosphere's constant-gravity estimate, for the readout.
fn warnings(field: &impl Gravity, seconds: f64) -> Option<String> {
    let warnings: Vec<_> = field
        .obstacles(seconds)
        .into_iter()
        .filter_map(|o| o.warning.map(|warning| format!("`{}`: {warning}", o.id)))
        .collect();
    (!warnings.is_empty()).then(|| warnings.join("; "))
}

/// Whether moving by `travel` passes within `radius` of a point `offset`
/// away.
fn reaches(offset: DVec3, travel: DVec3, radius: f64) -> bool {
    let along = (offset.dot(travel) / travel.length_squared()).clamp(0.0, 1.0);
    along.is_finite() && (offset - travel * along).length() <= radius
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
    /// Last-line swept check of the actual midpoint warp step, including
    /// ordinary drift and linearly moving obstacles. Never applies to manual
    /// warp. A refusal stops the bubble, not ordinary momentum or gravity.
    pub(super) fn check_route_step(
        &self,
        field: &impl Gravity,
        travel: DVec3,
    ) -> Result<(), String> {
        if !self.route.as_ref().is_some_and(Route::active) {
            return Ok(());
        }
        let from = DVec3::from_array(self.state().position);
        for obstacle in field.obstacles(self.seconds()) {
            if let Some(error) = obstacle.error {
                return Err(format!("Unsafe clearance for `{}`: {error}", obstacle.id));
            }
            let relative = travel - obstacle.velocity * STEP_SECONDS;
            if from.distance(obstacle.centre) < obstacle.exclusion_m
                || reaches(obstacle.centre - from, relative, obstacle.exclusion_m)
            {
                return Err(format!(
                    "Route stopped before entering the navigation exclusion of `{}`",
                    obstacle.id
                ));
            }
        }
        Ok(())
    }

    /// Plot a route to `id` and engage warp. A target in the ship's system,
    /// or one of the other craft in `ships`, is plotted at once, with every
    /// check first, so a refused order leaves the ship unchanged. One in
    /// another system, or from galactic space, waits in `Stage::Plotting` for
    /// its system's data and is plotted on the first step that has it; a
    /// refusal then ends the route with its reason.
    pub fn fly_to(
        &mut self,
        field: Option<&Field>,
        ships: &[Target],
        id: &str,
        arrival: ArrivalMode,
        requested_s: Option<f64>,
    ) -> Result<(), String> {
        if id == self.id {
            return Err("A ship cannot fly to itself".into());
        }
        if let Some(ship) = ships.iter().find(|ship| ship.id == id) {
            let ship = ship.clone();
            return match field {
                Some(field) => self.plot(field, ship, arrival, requested_s),
                None => self.plot(&voyage::Empty, ship, arrival, requested_s),
            };
        }
        if is_craft_id(id) {
            return Err(format!("No craft `{id}`"));
        }
        let system = crate::star::multiple::system_key(id);
        if let Some(field) = field {
            if let Some(target) = field.target(id, self.seconds()) {
                return self.plot(field, target, arrival, requested_s);
            }
            let own = field.system().root(field.page());
            if own.map(crate::star::multiple::system_key) == Some(system) {
                return Err(format!("No body `{id}` in this ship's system"));
            }
        }
        if !self.warp.engaged {
            self.check_engage()?;
        }
        self.route = Some(Route {
            target: id.into(),
            system: system.into(),
            arrival,
            stage: Stage::Plotting,
            requested_s,
            trip_s: requested_s.unwrap_or(LONG_TRIP_S),
            start_tick: self.tick,
            standoff_m: 0.0,
            tau_s: 0.0,
            distance_m: 0.0,
            eta_s: 0.0,
            late_s: 0.0,
            capped: false,
            detour_m: 0.0,
            bends_s: 0.0,
            around: None,
            slowed_by: None,
            aligned: false,
            end: None,
            clearance_warning: None,
        });
        Ok(())
    }

    /// Plot a route to a target `field` knows and engage warp. All checks
    /// come first, so a refusal changes nothing.
    fn plot(
        &mut self,
        field: &impl Gravity,
        target: Target,
        arrival: ArrivalMode,
        requested_s: Option<f64>,
    ) -> Result<(), String> {
        let id = &target.id;
        if arrival == ArrivalMode::Orbit && target.reach_m.is_none() {
            return Err(format!("`{id}` cannot be orbited"));
        }
        let standoff = target.standoff()?;
        let seconds = self.seconds();
        let from = DVec3::from_array(self.state().position);
        let goal = DVec3::from_array(target.state.position);
        let distance = from.distance(goal);
        if distance <= standoff {
            return Err(format!(
                "Already within the standoff of `{id}`; use warp match to drop out here"
            ));
        }
        // Timed along the path round bodies in the way, with its bends.
        let obstacles = others(field, &target, seconds, from, standoff)?;
        let clearance_warning = warnings(field, seconds);
        let aim = wrap::aim(&obstacles, from, goal, standoff);
        let (path, bends) = wrap::path(&obstacles, from, goal, standoff);
        let turn_rate = self.turn_rate();
        let nose = DQuat::from_array(self.orientation) * -DVec3::Z;
        let align = align_seconds(nose.angle_between(aim.direction), turn_rate);
        let bends_s = bend_seconds(&bends, turn_rate);
        let trip = requested_s.unwrap_or_else(|| default_trip_s(distance));
        let tau = fit_tau(path, standoff, trip - align - bends_s);
        if !self.warp.engaged {
            self.engage_warp()?;
        }
        self.input.controls.rotation = [0.0; 3];
        self.autopilot = AttitudePilot::Point {
            target: PointTarget::Vector(aim.direction.to_array()),
            up: None,
            attitude: self.orientation,
        };
        let eta = align + bends_s + profile_seconds(path, standoff, tau);
        let capped = path / tau > MAX_SPEED;
        self.route = Some(Route {
            target: target.id,
            system: target.system,
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
            capped,
            detour_m: path - distance,
            bends_s,
            around: aim.around.map(|i| obstacles[i].id.clone()),
            slowed_by: (eta - trip > LATE_S)
                .then(|| slowed_by(align, &bends, turn_rate, capped))
                .flatten(),
            aligned: false,
            end: None,
            clearance_warning,
        });
        Ok(())
    }

    /// Plot a route waiting for its system's data, which `field` now has, or
    /// end it with the reason it cannot be flown. Turning is left alone,
    /// as nothing took it over.
    fn plot_waiting(&mut self, field: &impl Gravity) {
        let Some(route) = self.route.as_ref() else {
            return;
        };
        let (id, arrival, requested_s) = (route.target.clone(), route.arrival, route.requested_s);
        let result = match field.target(&id, self.seconds()) {
            Some(target) => self.plot(field, target, arrival, requested_s),
            None => Err(format!("No body `{id}` found in system `{}`", route.system)),
        };
        if let Err(reason) = result
            && let Some(route) = self.route.as_mut()
        {
            route.stage = Stage::Released;
            route.end = Some(reason);
        }
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
        if self
            .route
            .as_ref()
            .is_some_and(|route| route.stage == Stage::Plotting)
        {
            self.plot_waiting(field);
        }
        let Some(route) = self.route.as_ref().filter(|route| route.active()) else {
            return;
        };
        if !self.warp.engaged {
            self.end_route("Warp disengaged");
            return;
        }
        let seconds = self.seconds();
        let Some(target) = field.target(&route.target, seconds) else {
            let reason = if is_craft_id(&route.target) {
                format!("`{}` is gone", route.target)
            } else {
                format!("Lost track of `{}`", route.target)
            };
            self.end_route(&reason);
            return;
        };
        let state = self.state();
        let from = DVec3::from_array(state.position);
        let goal = DVec3::from_array(target.state.position);
        let offset = goal - from;
        let distance = offset.length();
        let standoff = match target.standoff().and_then(|s| {
            // A changing target's bound can grow, never retain a stale unsafe standoff.
            others(field, &target, seconds, from, s).map(|obstacles| (s, obstacles))
        }) {
            Ok(value) => value,
            Err(reason) => {
                self.end_route(&reason);
                self.disengage_warp(&reason);
                return;
            }
        };
        let refresh = self.tick.rem_euclid(REFERENCE_STEPS) == 0;
        let route = self.route.as_mut().expect("active route");
        route.standoff_m = standoff.0;
        // Once a second is enough to pick up the bodies of a system entered.
        if refresh {
            route.clearance_warning = warnings(field, seconds);
        }
        if distance <= standoff.0 {
            self.arrive(&target, distance);
            return;
        }
        let (standoff, obstacles) = standoff;
        let aim = wrap::aim(&obstacles, from, goal, standoff);
        let nose = DQuat::from_array(self.orientation) * -DVec3::Z;
        let turn_rate = self.turn_rate();
        let tick = self.tick;
        let bubble_velocity = nose * self.warp.actual_ms;
        let route = self.route.as_mut().expect("active route");
        let approach = distance < APPROACH_STANDOFFS * route.standoff_m;
        // Measure the wrapped path once lined up, then once a second, and
        // fit τ again until the approach: the turn, the bends and the speed
        // ramp rarely take exactly the time allowed for them.
        let first = !route.aligned && nose.angle_between(aim.direction) <= ALIGNED_DEG.to_radians();
        let due = route.aligned && (tick - route.start_tick) % REFERENCE_STEPS == 0;
        if first || due {
            let (path, bends) = wrap::path(&obstacles, from, goal, route.standoff_m);
            route.detour_m = path - distance;
            route.bends_s = bend_seconds(&bends, turn_rate);
            if first || !approach {
                let left = super::tick_seconds(route.start_tick) + route.trip_s - seconds;
                route.tau_s = fit_tau(path, route.standoff_m, left - route.bends_s);
            }
            let late =
                seconds + route.bends_s + profile_seconds(path, route.standoff_m, route.tau_s)
                    - super::tick_seconds(route.start_tick)
                    - route.trip_s;
            let capped = path / route.tau_s > MAX_SPEED;
            // Time lost to a bend already passed stays lost: keep its cause.
            route.slowed_by = if late > LATE_S {
                slowed_by(0.0, &bends, turn_rate, capped).or(route.slowed_by.take())
            } else {
                None
            };
        }
        route.aligned |= first;
        // Close on the target along the line to it, or the edge of the body
        // in the way, and cancel the drift of the ship's ordinary velocity
        // against the target's.
        let sight = offset / distance;
        let drift = DVec3::from_array(target.state.velocity)
            + DVec3::from_array(target.warp_velocity)
            - DVec3::from_array(state.velocity);
        let remaining = distance + route.detour_m;
        let mut speed = profile_speed(remaining, route.tau_s);
        // Flying off the line turns the line of sight, faster than pointing
        // follows once τ is short. Aiming ahead by as much as it turns over
        // pointing's response keeps the nose on it.
        let mut turn = sight.cross(drift - bubble_velocity) / distance;
        if let Some(i) = aim.around {
            // Rounding a body, the aim turns at speed over distance from its
            // centre, which must stay within a share of the turn rate. The
            // further the aim is pulled off the line, the more the ship turns.
            let obstacle = &obstacles[i];
            let deflection = sight.angle_between(aim.direction).min(FRAC_PI_2);
            let bend = BEND_SHARE * turn_rate * obstacle.centre.distance(from) / deflection.sin();
            speed = speed.min(bend);
            let velocity = DVec3::from_array(state.velocity) + bubble_velocity;
            turn = wrap::edge_turn(obstacle, from, velocity, aim.direction);
        }
        // Heading into a sphere, leave time to turn away and slow down.
        if let Some(gap) = wrap::impact(&obstacles, from, nose) {
            speed = speed.min(gap / IMPACT_SECONDS);
        }
        let bubble = aim.direction * speed + drift;
        let lead =
            (turn * attitude::WARP_POINT_SECONDS).clamp_length_max(MAX_LEAD_DEG.to_radians());
        let wanted = bubble.normalize_or(aim.direction);
        let direction = DQuat::from_scaled_axis(lead) * wanted;
        // Off by the lead alone, the nose is on its line.
        let angle = nose.angle_between(wanted);
        let share = ((ALIGNING_DEG.to_radians() - angle)
            / (ALIGNING_DEG - ALIGNED_DEG).to_radians())
        .clamp(0.0, 1.0);
        self.warp.requested_ms = (bubble.length() * share).clamp(MIN_SPEED, MAX_SPEED);
        let eta = if route.aligned {
            0.0
        } else {
            align_seconds(angle, turn_rate)
        } + route.bends_s
            + profile_seconds(remaining, route.standoff_m, route.tau_s);
        route.stage = match (route.aligned, approach) {
            (false, _) => Stage::Align,
            (true, false) => Stage::Cruise,
            (true, true) => Stage::Approach,
        };
        route.distance_m = distance;
        route.eta_s = eta;
        route.late_s = seconds + eta - super::tick_seconds(route.start_tick) - route.trip_s;
        route.capped = remaining / route.tau_s > MAX_SPEED;
        route.around = aim.around.map(|i| obstacles[i].id.clone());
        let standoff = route.standoff_m;
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
        // Drop out now if this step would carry the ship to the standoff, so
        // a ship still braking hard, as from between the stars, cannot pass
        // through it within one step.
        let travel = nose * self.warp_travel() - drift * STEP_SECONDS;
        if reaches(offset, travel, standoff) {
            self.arrive(&target, distance);
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
        route.around = None;
        route.distance_m = distance;
        route.eta_s = 0.0;
        route.late_s =
            super::tick_seconds(self.tick) - super::tick_seconds(route.start_tick) - route.trip_s;
        route.end = Some(end);
    }
}

#[cfg(test)]
mod tests;
