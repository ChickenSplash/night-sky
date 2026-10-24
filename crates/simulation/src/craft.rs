//! Man-made objects: ships now, later stations, rovers and probes. Unlike
//! natural objects they are live state, not generated from seeds, so they
//! change every step and carry IDs of their own.
//!
//! - Free motion is integrated numerically in fixed steps of `STEP_SECONDS`
//!   of simulation time, under the summed gravity of every star and planet of
//!   its system at their analytic positions. A craft pulls on nothing.
//! - A craft changes only through its `Input`, applied at every step, so a
//!   server can run the same craft from the same inputs.
use glam::{DQuat, DVec3};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use space_shared::{Planet, StarSystem};

use crate::{EARTH_SOLAR, SUN_GM, State};

/// Fixed physics steps per simulated second, shared by every client and server.
pub const STEPS_PER_SECOND: i64 = 20;
/// Fixed simulation time per physics step.
pub const STEP_SECONDS: f64 = 1.0 / STEPS_PER_SECOND as f64;
/// Steps between choices of a craft's orbit reference: one simulated second.
pub const REFERENCE_STEPS: i64 = STEPS_PER_SECOND;

/// The simulation time of a step boundary. Every craft steps on this one
/// world-wide grid, counted from time zero, so a step number names the same
/// instant on any client or server and inputs can be keyed by it.
pub fn tick_seconds(tick: i64) -> f64 {
    tick as f64 / STEPS_PER_SECOND as f64
}

/// The first step boundary at or after `seconds`.
pub fn tick_at(seconds: f64) -> i64 {
    let steps = seconds * STEPS_PER_SECOND as f64;
    let nearest = steps.round();
    // A boundary's own time, or a sum of steps, rounded maps back to that
    // boundary: allow a few units in the last place.
    if (steps - nearest).abs() <= 16.0 * f64::EPSILON * nearest.abs().max(1.0) {
        nearest as i64
    } else {
        steps.ceil() as i64
    }
}

mod attitude;
pub use attitude::{AttitudePilot, PointTarget};
mod order;
pub use order::{MAX_BODY_ID, MAX_THRUST, Order, WarpOrder};
mod reference;
pub use reference::{Reference, ReferenceKind, SWITCH_MARGIN, planet_reach_m};
pub mod route;
pub use route::{ArrivalMode, Route, Target};
pub mod predict;
pub mod voyage;
pub mod warp;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CraftKind {
    Ship,
    Station,
    Rover,
    Probe,
}

impl CraftKind {
    pub const ALL: [Self; 4] = [Self::Ship, Self::Station, Self::Rover, Self::Probe];

    /// The registry's kind name, such as "ship".
    pub fn name(self) -> &'static str {
        match self {
            Self::Ship => "ship",
            Self::Station => "station",
            Self::Rover => "rover",
            Self::Probe => "probe",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Ship => "Ship",
            Self::Station => "Station",
            Self::Rover => "Rover",
            Self::Probe => "Probe",
        }
    }
}

/// Nose-to-tail length of the test hull, in metres.
pub const SHIP_LENGTH_M: f64 = 30.0;

/// A craft's ID from its number, as hosts give them.
pub fn craft_id(number: u64) -> String {
    format!("craft-{number}")
}

/// Whether an ID has the form of a craft's, whether or not it still exists.
pub fn is_craft_id(id: &str) -> bool {
    id.strip_prefix("craft-")
        .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
}

/// What a craft's position is measured from.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "frame", rename_all = "snake_case")]
pub enum Frame {
    /// A star system, by its page star's ID: metres from the page's outermost
    /// barycentre, in the system's plane axes.
    System { star: String },
    /// Local metres in galactic axes, anchored at a fixed galactocentric point.
    Galaxy {
        #[schemars(extend("x-unit" = "pc"))]
        origin_pc: [f64; 3],
    },
}

impl Frame {
    pub fn star(&self) -> &str {
        match self {
            Self::System { star } => star,
            Self::Galaxy { .. } => "",
        }
    }
}

/// How a craft moves. Only free flight exists yet; on-rails orbits and
/// surface positions come with stations and rovers.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum Motion {
    /// Integrated under gravity and thrust.
    Free(State),
}

/// What the craft is told to do, held until changed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Input {
    /// Legacy thrust acceleration, in frame axes.
    #[schemars(extend("x-unit" = "m/s²"))]
    pub thrust: [f64; 3],
    #[serde(default)]
    pub controls: Controls,
}

/// Body axes: +X right, +Y up, -Z forward. Rotation is right-handed about
/// X (pitch), Y (yaw), Z (roll). Commands persist until replaced.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Controls {
    /// Main drive, or reverse when negative.
    #[schemars(extend("x-unit" = "fraction"))]
    pub throttle: f64,
    /// Right, up, back, each in [-1, 1]. Independent fine thrusters.
    #[schemars(extend("x-unit" = "fraction"))]
    pub translation: [f64; 3],
    /// Pitch, yaw, roll, each in [-1, 1].
    #[schemars(extend("x-unit" = "fraction"))]
    pub rotation: [f64; 3],
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Thrusters {
    /// Main drive acceleration; zero disables it.
    #[schemars(range(min = 0.0), extend("x-unit" = "m/s²"))]
    pub forward: f64,
    /// Reverse drive acceleration; zero disables it.
    #[schemars(range(min = 0.0), extend("x-unit" = "m/s²"))]
    pub reverse: f64,
    /// Fine translation acceleration on each axis.
    #[schemars(range(min = 0.0), extend("x-unit" = "m/s²"))]
    pub fine: f64,
    /// Angular acceleration about each body axis (pitch, yaw, roll).
    ///
    /// This is not g: a future inertia model can derive it from torque and
    /// hull geometry.
    #[schemars(extend("x-unit" = "rad/s²"))]
    pub angular: [f64; 3],
}

impl Default for Thrusters {
    fn default() -> Self {
        Self {
            forward: 9.80665,
            reverse: 9.80665,
            fine: 0.980665,
            angular: [0.1; 3],
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Craft {
    pub id: String,
    pub name: String,
    pub kind: CraftKind,
    pub frame: Frame,
    pub motion: Motion,
    /// Attitude as a unit quaternion (x, y, z, w) in frame axes.
    pub orientation: [f64; 4],
    /// Body-local angular velocity, rad/s. No passive damping.
    #[serde(default)]
    pub spin: [f64; 3],
    #[serde(default)]
    pub thrusters: Thrusters,
    #[serde(default)]
    pub autopilot: AttitudePilot,
    /// What its orbit is described around, by ID. Chosen again each
    /// `REFERENCE_STEPS` and kept between, so it switches only past
    /// `SWITCH_MARGIN`.
    #[serde(default)]
    pub reference: Option<String>,
    /// The world step `motion` describes (see `tick_seconds`). Orders given
    /// now take effect from this step.
    pub tick: i64,
    pub input: Input,
    #[serde(default)]
    pub warp: warp::Warp,
    /// The plotted warp route, kept after it ends for inspection.
    #[serde(default)]
    pub route: Option<Route>,
    /// Gravity at the current state, kept from the last step's end. Any
    /// client or server recomputes the same value when it is missing.
    #[serde(skip)]
    gravity: Option<[f64; 3]>,
}

/// A source of gravitational acceleration at a place and time.
pub trait Gravity {
    /// Acceleration in metres per second squared at `position`, in metres.
    fn acceleration(&self, position: [f64; 3], seconds: f64) -> [f64; 3];
    /// A named star, barycentre or planet, or the orbit reference here when
    /// no ID is supplied. Guidance needs velocity as well as position;
    /// gravity-only fields may omit it.
    fn reference(&self, _id: Option<&str>, _position: [f64; 3], _seconds: f64) -> Option<State> {
        None
    }
    /// The orbit reference for a craft here, keeping `current` within the
    /// switching margin. Fields without a hierarchy may omit it.
    fn orbit_reference(
        &self,
        _current: Option<&str>,
        _position: [f64; 3],
        _seconds: f64,
    ) -> Option<Reference> {
        None
    }
    /// The body that sets the pace of motion here: the one with the shortest
    /// free-fall time. Fields without bodies may omit it.
    fn closest(&self, _position: [f64; 3], _seconds: f64) -> Option<Closest> {
        None
    }
    /// What the route pilot needs to know about a target. Fields without
    /// targets may omit it.
    fn target(&self, _id: &str, _seconds: f64) -> Option<Target> {
        None
    }
    /// The keep-clear spheres a route wraps round. Fields without bodies may
    /// omit them.
    fn obstacles(&self, _seconds: f64) -> Vec<route::Obstacle> {
        Vec::new()
    }
}

/// The body with the shortest free-fall time from a point.
#[derive(Clone, Debug, PartialEq)]
pub struct Closest {
    pub id: String,
    /// sqrt(r³/GM) in seconds: a circular orbit's period over 2π.
    pub timescale: f64,
    /// Whether the point is inside it.
    pub inside: bool,
}

impl Craft {
    pub fn new(id: String, name: String, kind: CraftKind, frame: Frame, tick: i64) -> Self {
        Self {
            id,
            name,
            kind,
            frame,
            motion: Motion::Free(State::default()),
            orientation: [0.0, 0.0, 0.0, 1.0],
            spin: [0.0; 3],
            thrusters: Thrusters::default(),
            autopilot: AttitudePilot::default(),
            reference: None,
            tick,
            input: Input::default(),
            warp: warp::Warp::default(),
            route: None,
            gravity: None,
        }
    }

    /// Simulation time `motion` describes, in seconds.
    pub fn seconds(&self) -> f64 {
        tick_seconds(self.tick)
    }

    pub fn state(&self) -> State {
        match self.motion {
            Motion::Free(state) => state,
        }
    }

    /// Accept a complete manual input atomically, rejecting unsupported or
    /// non-finite commands instead of silently clipping them. During warp,
    /// rotation steers the bubble rather than firing turning thrusters.
    pub fn control(&mut self, controls: Controls) -> Result<(), &'static str> {
        if self.warp.engaged && (controls.throttle != 0.0 || controls.translation != [0.0; 3]) {
            return Err("Disengage warp before using thrusters");
        }
        if !std::iter::once(&controls.throttle)
            .chain(controls.translation.iter())
            .chain(controls.rotation.iter())
            .all(|x| x.is_finite() && (-1.0..=1.0).contains(x))
        {
            return Err("controls must be finite fractions in [-1, 1]");
        }
        if controls.throttle < 0.0 && self.thrusters.reverse <= 0.0 {
            return Err("ship has no reverse drive");
        }
        self.input.controls = controls;
        if controls.rotation != [0.0; 3] {
            self.release();
        }
        Ok(())
    }

    fn thrust(&self) -> [f64; 3] {
        let c = self.input.controls;
        if c.throttle == 0.0 && c.translation == [0.0; 3] {
            return self.input.thrust;
        }
        let drive = if c.throttle >= 0.0 {
            self.thrusters.forward
        } else {
            self.thrusters.reverse
        };
        let local =
            DVec3::from_array(c.translation) * self.thrusters.fine - DVec3::Z * c.throttle * drive;
        (DQuat::from_array(self.orientation) * local + DVec3::from_array(self.input.thrust))
            .to_array()
    }

    fn turn(&mut self, rotation: [f64; 3]) {
        let acceleration = DVec3::from_array(rotation) * DVec3::from_array(self.thrusters.angular);
        let midpoint = DVec3::from_array(self.spin) + acceleration * (STEP_SECONDS / 2.0);
        if midpoint != DVec3::ZERO {
            self.orientation = (DQuat::from_array(self.orientation)
                * DQuat::from_scaled_axis(midpoint * STEP_SECONDS))
            .normalize()
            .to_array();
        }
        self.spin = (DVec3::from_array(self.spin) + acceleration * STEP_SECONDS).to_array();
    }

    /// Place the craft, as when it is built or loaded.
    pub fn place(&mut self, state: State, tick: i64) {
        self.motion = Motion::Free(state);
        self.tick = tick;
        self.gravity = None;
    }

    /// One fixed step: kick, drift, kick (velocity Verlet), which keeps
    /// orbital energy from drifting over many laps; or bubble transport
    /// while warp is engaged, after the route pilot's orders.
    pub fn step(&mut self, gravity: &impl Gravity) {
        self.fly_route(gravity);
        if self.warp.engaged {
            self.warp_step(gravity);
            return;
        }
        let thrust = self.thrust();
        let rotation = self.guidance(gravity);
        self.turn(rotation);
        let end_thrust = self.thrust();
        let seconds = self.seconds();
        let Motion::Free(state) = &mut self.motion;
        let before = self
            .gravity
            .unwrap_or_else(|| gravity.acceleration(state.position, seconds));
        let after = verlet(
            state,
            gravity,
            seconds,
            STEP_SECONDS,
            before,
            [thrust, end_thrust],
        );
        self.tick += 1;
        self.gravity = Some(after);
        // Once a simulated second: choosing costs more than the step itself,
        // and no craft crosses a region in a second.
        let position = state.position;
        if self.reference.is_none() || self.tick.rem_euclid(REFERENCE_STEPS) == 0 {
            self.reference = gravity
                .orbit_reference(self.reference.as_deref(), position, self.seconds())
                .map(|reference| reference.id);
        }
    }

    /// Step until the craft is at or past `until`, at most `max_steps` times.
    /// Returns the steps taken.
    pub fn advance(&mut self, gravity: &impl Gravity, until: f64, max_steps: usize) -> usize {
        let mut steps = 0;
        while self.seconds() < until && steps < max_steps {
            self.step(gravity);
            steps += 1;
        }
        steps
    }
}

/// One kick, drift, kick step of `dt` from `seconds`, with gravity `before`
/// at the start and thrust at the start and end. Returns gravity at the end.
fn verlet(
    state: &mut State,
    gravity: &impl Gravity,
    seconds: f64,
    dt: f64,
    before: [f64; 3],
    thrust: [[f64; 3]; 2],
) -> [f64; 3] {
    let half = dt / 2.0;
    for i in 0..3 {
        state.velocity[i] += (before[i] + thrust[0][i]) * half;
        state.position[i] += state.velocity[i] * dt;
    }
    let after = gravity.acceleration(state.position, seconds + dt);
    for i in 0..3 {
        state.velocity[i] += (after[i] + thrust[1][i]) * half;
    }
    after
}

/// A gravitating star or planet at one instant.
#[derive(Clone, Debug, PartialEq)]
pub struct Body {
    pub id: String,
    /// Gravitational parameter GM in m³/s².
    pub gm: f64,
    pub radius_m: f64,
    pub state: State,
    pub kind: BodyKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BodyKind {
    Star,
    Planet,
}

/// The gravity of one system's stars and planets, at their analytic orbits.
#[derive(Clone, Debug)]
pub struct Field {
    system: StarSystem,
    page: u32,
    stars: Vec<(String, f64, f64)>,
    planets: Vec<(Planet, f64, f64, f64)>,
    /// Each pair member's stable zone radius, in metres.
    zones: Vec<(String, f64)>,
}

impl Field {
    pub fn new(system: StarSystem, page: u32) -> Self {
        let stars = system
            .stars_on(page)
            .map(|s| {
                let radius = s.radius_solar * crate::SUN_RADIUS_METRES;
                (s.id.clone(), s.mass_solar * SUN_GM, radius)
            })
            .collect();
        let planets = system
            .on_page(page)
            .map(|p| {
                let host = system.host_mass(p);
                (
                    p.clone(),
                    p.mass_earth * EARTH_SOLAR * SUN_GM,
                    p.radius_km * 1_000.0,
                    host,
                )
            })
            .collect();
        let zones = Self::zones(&system, page);
        Self {
            system,
            page,
            stars,
            planets,
            zones,
        }
    }

    pub fn system(&self) -> &StarSystem {
        &self.system
    }

    pub fn page(&self) -> u32 {
        self.page
    }

    /// Every star and planet with its state at this instant.
    pub fn bodies(&self, seconds: f64) -> Vec<Body> {
        let members = crate::member_states(&self.system, self.page, seconds);
        let at = |id: &str| crate::orbit::find(&members, id).unwrap_or_default();
        let stars = self.stars.iter().map(|(id, gm, radius)| Body {
            id: id.clone(),
            gm: *gm,
            radius_m: *radius,
            state: at(id),
            kind: BodyKind::Star,
        });
        let planets = self.planets.iter().map(|(planet, gm, radius, host)| Body {
            id: planet.id.clone(),
            gm: *gm,
            radius_m: *radius,
            state: at(&planet.host).offset(crate::orbit_state(&planet.orbit, *host, seconds), 1.0),
            kind: BodyKind::Planet,
        });
        stars.chain(planets).collect()
    }
}

impl Gravity for Field {
    fn reference(&self, id: Option<&str>, position: [f64; 3], seconds: f64) -> Option<State> {
        let reference = match id {
            Some(id) => self.reference_by_id(id, seconds),
            None => self.select_reference(None, position, seconds),
        }?;
        Some(reference.state)
    }

    fn orbit_reference(
        &self,
        current: Option<&str>,
        position: [f64; 3],
        seconds: f64,
    ) -> Option<Reference> {
        self.select_reference(current, position, seconds)
    }

    fn target(&self, id: &str, seconds: f64) -> Option<Target> {
        Field::target(self, id, seconds)
    }

    fn obstacles(&self, seconds: f64) -> Vec<route::Obstacle> {
        self.bodies(seconds)
            .into_iter()
            .map(|body| route::Obstacle {
                id: body.id,
                centre: DVec3::from_array(body.state.position),
                velocity: DVec3::from_array(body.state.velocity),
                clear_m: route::KEEP_CLEAR * body.radius_m,
            })
            .collect()
    }

    fn closest(&self, position: [f64; 3], seconds: f64) -> Option<Closest> {
        self.bodies(seconds)
            .into_iter()
            .map(|body| {
                let r = distance(body.state.position, position);
                let timescale = (r.powi(3) / body.gm).sqrt();
                (body, r, timescale)
            })
            .min_by(|a, b| a.2.total_cmp(&b.2))
            .map(|(body, r, timescale)| Closest {
                inside: r < body.radius_m,
                id: body.id,
                timescale,
            })
    }

    fn acceleration(&self, position: [f64; 3], seconds: f64) -> [f64; 3] {
        let mut sum = [0.0; 3];
        let members = crate::orbit::member_positions(&self.system, self.page, seconds);
        let at = |id: &str| crate::orbit::find(&members, id).unwrap_or([0.0; 3]);
        for (id, gm, radius) in &self.stars {
            pull(&mut sum, position, at(id), *gm, *radius);
        }
        for (planet, gm, radius, host) in &self.planets {
            let base = at(&planet.host);
            let local = crate::position(&planet.orbit, *host, seconds);
            let centre = std::array::from_fn(|i| base[i] + local[i]);
            pull(&mut sum, position, centre, *gm, *radius);
        }
        sum
    }
}

/// Add one body's pull. Inside it, the field of a uniform sphere, falling to
/// zero at the centre rather than growing without limit.
fn pull(sum: &mut [f64; 3], position: [f64; 3], centre: [f64; 3], gm: f64, radius: f64) {
    let offset: [f64; 3] = std::array::from_fn(|i| centre[i] - position[i]);
    let r2 = offset.iter().map(|v| v * v).sum::<f64>();
    let r = r2.sqrt();
    let scale = if r >= radius {
        gm / (r2 * r)
    } else {
        gm / radius.powi(3)
    };
    for i in 0..3 {
        sum[i] += offset[i] * scale;
    }
}

pub fn distance(a: [f64; 3], b: [f64; 3]) -> f64 {
    (0..3).map(|i| (a[i] - b[i]).powi(2)).sum::<f64>().sqrt()
}

/// The two-body orbit a state relative to one body would follow alone,
/// ignoring every other body: for readouts, not for motion.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Elements {
    /// Specific orbital energy, J/kg; negative when bound.
    pub energy: f64,
    pub eccentricity: f64,
    gm: f64,
}

impl Elements {
    pub fn of(relative: State, gm: f64) -> Self {
        let [r, v] = [relative.position, relative.velocity];
        let radius = distance(r, [0.0; 3]);
        let speed2 = v.iter().map(|x| x * x).sum::<f64>();
        let energy = speed2 / 2.0 - gm / radius;
        // The eccentricity vector, which stays exact for circular orbits.
        let along = (0..3).map(|i| r[i] * v[i]).sum::<f64>();
        let e: [f64; 3] =
            std::array::from_fn(|i| ((speed2 - gm / radius) * r[i] - along * v[i]) / gm);
        let eccentricity = distance(e, [0.0; 3]);
        Self {
            energy,
            eccentricity,
            gm,
        }
    }

    /// Semi-major axis in metres, when bound.
    pub fn semi_major(&self) -> Option<f64> {
        (self.energy < 0.0 && self.eccentricity < 1.0).then(|| -self.gm / (2.0 * self.energy))
    }

    /// Nearest and farthest distances from the body's centre, when bound.
    pub fn apsides(&self) -> Option<[f64; 2]> {
        let a = self.semi_major()?;
        Some([a * (1.0 - self.eccentricity), a * (1.0 + self.eccentricity)])
    }

    pub fn period(&self) -> Option<f64> {
        let a = self.semi_major()?;
        Some(std::f64::consts::TAU * (a.powi(3) / self.gm).sqrt())
    }
}

/// Speed of a circular orbit `radius_m` from a body's centre.
pub fn circular_speed(gm: f64, radius_m: f64) -> f64 {
    (gm / radius_m).sqrt()
}

#[cfg(test)]
mod tests;
