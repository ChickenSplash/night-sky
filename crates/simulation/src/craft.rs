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
use serde::{Deserialize, Serialize};
use space_shared::{Planet, StarSystem};

use crate::{EARTH_SOLAR, SUN_GM, State};

/// Fixed simulation time per physics step, shared by every client and server.
pub const STEP_SECONDS: f64 = 1.0 / 20.0;
/// Simulation time between choices of a craft's orbit reference.
pub const REFERENCE_SECONDS: f64 = 1.0;

mod attitude;
pub use attitude::{AttitudePilot, PointTarget};
mod reference;
pub use reference::{Reference, ReferenceKind, SWITCH_MARGIN};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
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

/// What a craft's position is measured from.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "frame", rename_all = "snake_case")]
pub enum Frame {
    /// A star system, by its page star's ID: metres from the page's outermost
    /// barycentre, in the system's plane axes.
    System { star: String },
}

impl Frame {
    pub fn star(&self) -> &str {
        match self {
            Self::System { star } => star,
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
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Input {
    /// Thrust acceleration in metres per second squared, in frame axes.
    pub thrust: [f64; 3],
    #[serde(default)]
    pub controls: Controls,
}

/// Body axes: +X right, +Y up, -Z forward. Rotation is right-handed about
/// X (pitch), Y (yaw), Z (roll). Commands persist until replaced.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Controls {
    pub throttle: f64,
    /// Right, up, back, each in [-1, 1]. Independent fine thrusters.
    pub translation: [f64; 3],
    /// Pitch, yaw, roll, each in [-1, 1].
    pub rotation: [f64; 3],
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Thrusters {
    /// Main and reverse acceleration in m/s²; zero disables a drive.
    pub forward: f64,
    pub reverse: f64,
    pub fine: f64,
    /// Angular acceleration in rad/s² about each body axis. This is not g:
    /// a future inertia model can derive it from torque and hull geometry.
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
    /// What its orbit is described around, by ID, chosen again each
    /// `REFERENCE_SECONDS` of steps.
    /// Kept between steps so it switches only past `SWITCH_MARGIN`.
    #[serde(default)]
    pub reference: Option<String>,
    /// Simulation time `motion` describes, in seconds.
    pub seconds: f64,
    pub input: Input,
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
}

impl Craft {
    pub fn new(id: String, name: String, kind: CraftKind, frame: Frame, seconds: f64) -> Self {
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
            seconds,
            input: Input::default(),
            gravity: None,
        }
    }

    pub fn state(&self) -> State {
        match self.motion {
            Motion::Free(state) => state,
        }
    }

    /// Accept a complete manual input atomically, rejecting unsupported or
    /// non-finite commands instead of silently clipping them.
    pub fn control(&mut self, controls: Controls) -> Result<(), &'static str> {
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
    pub fn place(&mut self, state: State, seconds: f64) {
        self.motion = Motion::Free(state);
        self.seconds = seconds;
        self.gravity = None;
    }

    /// One fixed step: kick, drift, kick (velocity Verlet), which keeps
    /// orbital energy from drifting over many laps.
    pub fn step(&mut self, gravity: &impl Gravity) {
        let thrust = self.thrust();
        let rotation = self.guidance(gravity);
        self.turn(rotation);
        let end_thrust = self.thrust();
        let Motion::Free(state) = &mut self.motion;
        let half = STEP_SECONDS / 2.0;
        let before = self
            .gravity
            .unwrap_or_else(|| gravity.acceleration(state.position, self.seconds));
        for i in 0..3 {
            state.velocity[i] += (before[i] + thrust[i]) * half;
            state.position[i] += state.velocity[i] * STEP_SECONDS;
        }
        self.seconds += STEP_SECONDS;
        let after = gravity.acceleration(state.position, self.seconds);
        for i in 0..3 {
            state.velocity[i] += (after[i] + end_thrust[i]) * half;
        }
        self.gravity = Some(after);
        // Once a simulated second: choosing costs more than the step itself,
        // and no craft crosses a region in a second.
        let position = state.position;
        let second = |t: f64| (t / REFERENCE_SECONDS).floor();
        if self.reference.is_none() || second(self.seconds) != second(self.seconds - STEP_SECONDS) {
            self.reference = gravity
                .orbit_reference(self.reference.as_deref(), position, self.seconds)
                .map(|reference| reference.id);
        }
    }

    /// Step until the craft is at or past `until`, at most `max_steps` times.
    /// Returns the steps taken.
    pub fn advance(&mut self, gravity: &impl Gravity, until: f64, max_steps: usize) -> usize {
        let mut steps = 0;
        while self.seconds < until && steps < max_steps {
            self.step(gravity);
            steps += 1;
        }
        steps
    }
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
        let at = |id: &str| crate::find(&members, id).unwrap_or_default();
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

    fn acceleration(&self, position: [f64; 3], seconds: f64) -> [f64; 3] {
        let mut sum = [0.0; 3];
        let members = crate::member_positions(&self.system, self.page, seconds);
        let at = |id: &str| crate::find(&members, id).unwrap_or([0.0; 3]);
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
