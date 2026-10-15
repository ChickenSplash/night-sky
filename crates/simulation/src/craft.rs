//! Man-made objects: ships now, later stations, rovers and probes. Unlike
//! natural objects they are live state, not generated from seeds, so they
//! change every step and carry IDs of their own.
//!
//! - Free motion is integrated numerically in fixed steps of `STEP_SECONDS`
//!   of simulation time, under the summed gravity of every star and planet of
//!   its system at their analytic positions. A craft pulls on nothing.
//! - A craft changes only through its `Input`, applied at every step, so a
//!   server can run the same craft from the same inputs.
use serde::{Deserialize, Serialize};
use space_shared::{Planet, StarSystem};

use crate::{EARTH_SOLAR, SUN_GM, State};

/// Fixed simulation time per physics step, shared by every client and server.
pub const STEP_SECONDS: f64 = 1.0 / 20.0;

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

    /// Place the craft, as when it is built or loaded.
    pub fn place(&mut self, state: State, seconds: f64) {
        self.motion = Motion::Free(state);
        self.seconds = seconds;
        self.gravity = None;
    }

    /// One fixed step: kick, drift, kick (velocity Verlet), which keeps
    /// orbital energy from drifting over many laps.
    pub fn step(&mut self, gravity: &impl Gravity) {
        let Motion::Free(state) = &mut self.motion;
        let half = STEP_SECONDS / 2.0;
        let thrust = self.input.thrust;
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
            state.velocity[i] += (after[i] + thrust[i]) * half;
        }
        self.gravity = Some(after);
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
        Self {
            system,
            page,
            stars,
            planets,
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

    /// The body a craft here mostly answers to: the nearest planet whose
    /// Hill sphere holds it, else the star pulling hardest. `bodies` are this
    /// field's, in `bodies` order.
    pub fn dominant<'a>(&self, bodies: &'a [Body], position: [f64; 3]) -> Option<&'a Body> {
        let planet = bodies
            .iter()
            .skip(self.stars.len())
            .zip(&self.planets)
            .filter(|(body, (planet, gm, _, host))| {
                let periapsis = planet.orbit.semi_major_au
                    * (1.0 - planet.orbit.eccentricity)
                    * crate::AU_METRES;
                let hill = periapsis * (gm / (3.0 * host * SUN_GM)).cbrt();
                distance(body.state.position, position) < hill
            })
            .map(|(body, _)| body)
            .min_by(|a, b| {
                distance(a.state.position, position)
                    .total_cmp(&distance(b.state.position, position))
            });
        planet.or_else(|| {
            bodies
                .iter()
                .filter(|body| body.kind == BodyKind::Star)
                .max_by(|a, b| {
                    let pull =
                        |body: &Body| body.gm / distance(body.state.position, position).powi(2);
                    pull(a).total_cmp(&pull(b))
                })
        })
    }
}

impl Gravity for Field {
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
