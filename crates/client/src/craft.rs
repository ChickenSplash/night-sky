//! Live craft: ships the player places, stepped with the simulation clock.
//! Their physics is `space_simulation::craft`; this keeps them, steps them in
//! fixed steps each frame, and blends between steps for drawing.
use crate::{Clock, Navigation, objects};
use bevy::prelude::*;
use space_shared::{Galaxy, StarSystem};
use space_simulation::search::{SearchResult, live::LiveObject};
use space_simulation::{
    EARTH_SOLAR, SUN_GM, SUN_RADIUS_METRES, State,
    craft::{Craft, CraftKind, Field, Frame, Reference, STEP_SECONDS, circular_speed, distance},
};

mod control;
pub use control::Order;
mod forecast;
pub use forecast::{Forecast, forecast};
mod shape;
pub use shape::{HULL_EDGES, HULL_VERTICES};

/// Safe requested-time ceiling during manoeuvres, without changing the
/// user's clock setting. Returning to quiet coasting restores that setting.
pub const MANOEUVRE_RATE: f64 = 10.0;

/// Steps one frame may run, about a millisecond natively. Past this the
/// clock waits for the craft, so time runs slower than its set rate rather
/// than the craft falling behind the bodies.
pub const MAX_STEPS_PER_FRAME: usize = 1_000;

/// Nose-to-tail length of the test hull, in metres.
pub const SHIP_SIZE_M: f64 = 30.0;
/// Conservative sphere around the test hull, for projection and picking.
pub const SHIP_RADIUS_M: f64 = 18.0;

/// The follow camera's distance when it starts following a craft, in metres.
pub const FOLLOW_M: f64 = 200.0;

/// Thrust the developer command accepts, in metres per second squared.
pub const MAX_THRUST: f64 = 100.0;

#[derive(Resource, Default)]
pub struct Fleet {
    pub craft: Vec<Live>,
    /// The last number given to a craft.
    numbered: u64,
    /// Whether the craft held the clock back this frame.
    pub held: bool,
}

/// The craft as they are at a simulation time, for readouts that must not
/// cache them.
#[derive(Clone, Copy)]
pub struct Now<'a> {
    pub fleet: &'a Fleet,
    pub seconds: f64,
}

#[cfg(test)]
impl Now<'static> {
    /// No craft at all.
    pub fn none() -> Self {
        static EMPTY: Fleet = Fleet {
            craft: Vec::new(),
            numbered: 0,
            held: false,
        };
        EMPTY.at(0.0)
    }
}

impl<'a> Now<'a> {
    pub fn get(&self, id: &str) -> Option<&'a Live> {
        self.fleet.get(id)
    }
}

pub struct Live {
    pub craft: Craft,
    pub field: Field,
    /// The state one step before `craft`'s, blended with it when drawn.
    previous: State,
    previous_orientation: [f64; 4],
    /// Its system's position, in parsecs from the galactic centre, where
    /// search measures its distance from.
    pub system_pc: [f64; 3],
    /// Where it would go if it coasted.
    pub forecast: Forecast,
}

impl Live {
    fn share_at(&self, seconds: f64) -> f64 {
        ((seconds - (self.craft.seconds - STEP_SECONDS)) / STEP_SECONDS).clamp(0.0, 1.0)
    }

    /// Shortest-arc attitude interpolation, on the same timeline as position.
    pub fn orientation_at(&self, seconds: f64) -> bevy::math::DQuat {
        bevy::math::DQuat::from_array(self.previous_orientation).slerp(
            bevy::math::DQuat::from_array(self.craft.orientation),
            self.share_at(seconds),
        )
    }

    /// Where the craft is at `seconds`, between its last two steps.
    pub fn state_at(&self, seconds: f64) -> State {
        let current = self.craft.state();
        let share = self.share_at(seconds);
        let blend =
            |a: [f64; 3], b: [f64; 3]| std::array::from_fn(|i| a[i] + (b[i] - a[i]) * share);
        State {
            position: blend(self.previous.position, current.position),
            velocity: blend(self.previous.velocity, current.velocity),
        }
    }

    /// The craft at `seconds`, and the reference its orbit is described
    /// around, kept from its last step within the switching margin.
    pub fn surroundings(&self, seconds: f64) -> (State, Option<Reference>) {
        let state = self.state_at(seconds);
        let reference =
            self.field
                .select_reference(self.craft.reference.as_deref(), state.position, seconds);
        (state, reference)
    }

    /// The page star the craft's frame is measured from.
    pub fn star(&self) -> &str {
        self.craft.frame.star()
    }
}

/// How a spawned ship starts relative to its body.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Launch {
    /// A circular orbit at its height.
    #[default]
    Circular,
    /// Prograde at this speed, in metres per second.
    Speed(f64),
    /// This velocity in the frame's axes, in metres per second.
    Velocity([f64; 3]),
}

impl Fleet {
    pub fn safe_rate(&self, requested: f64) -> f64 {
        use space_simulation::craft::AttitudePilot;
        let active = self.craft.iter().any(|live| {
            let craft = &live.craft;
            let input = craft.input;
            input.thrust != [0.0; 3]
                || input.controls.throttle != 0.0
                || input.controls.translation != [0.0; 3]
                || input.controls.rotation != [0.0; 3]
                || craft.spin.iter().any(|v| v.abs() > 1e-6)
                || !matches!(craft.autopilot, AttitudePilot::Manual)
        });
        if active {
            requested.min(MANOEUVRE_RATE)
        } else {
            requested
        }
    }
    /// The craft at a simulation time.
    pub fn at(&self, seconds: f64) -> Now<'_> {
        Now {
            fleet: self,
            seconds,
        }
    }

    /// The craft where they last stepped to, for checks without a clock.
    pub fn latest(&self) -> Now<'_> {
        let seconds = self
            .craft
            .iter()
            .map(|live| live.craft.seconds)
            .fold(f64::INFINITY, f64::min);
        self.at(if seconds.is_finite() { seconds } else { 0.0 })
    }

    pub fn get(&self, id: &str) -> Option<&Live> {
        self.craft.iter().find(|live| live.craft.id == id)
    }

    pub fn get_mut(&mut self, id: &str) -> Option<&mut Live> {
        self.craft.iter_mut().find(|live| live.craft.id == id)
    }

    /// A new ship `altitude` metres above a body's surface, at `seconds`.
    /// Returns its ID.
    pub fn spawn(
        &mut self,
        system: StarSystem,
        page: u32,
        body: &str,
        altitude: f64,
        launch: Launch,
        seconds: f64,
    ) -> Result<String, String> {
        let star = objects::star_id(&system, page);
        let mass = body_mass(&system, page, body).ok_or_else(|| format!("No body `{body}`"))?;
        let field = Field::new(system, page);
        let system = field.system();
        let (here, parent, radius) = match system.planets.iter().find(|p| p.id == body) {
            Some(planet) => (
                space_simulation::planet_state(system, planet, seconds),
                Some(space_simulation::member_state(
                    system,
                    page,
                    &planet.host,
                    seconds,
                )),
                planet.radius_km * 1_000.0,
            ),
            None => (
                space_simulation::member_state(system, page, body, seconds),
                system
                    .pair_of(page, body)
                    .map(|pair| space_simulation::member_state(system, page, &pair.id, seconds)),
                system
                    .stars_on(page)
                    .find(|star| star.id == body)
                    .map_or(0.0, |star| star.radius_solar * SUN_RADIUS_METRES),
            ),
        };
        let (out, forward) = orbit_axes(here, parent);
        let r = radius + altitude;
        let velocity = match launch {
            Launch::Circular => forward.map(|v| v * circular_speed(mass * SUN_GM, r)),
            Launch::Speed(speed) => forward.map(|v| v * speed),
            Launch::Velocity(velocity) => velocity,
        };
        let state = State {
            position: std::array::from_fn(|i| here.position[i] + out[i] * r),
            velocity: std::array::from_fn(|i| here.velocity[i] + velocity[i]),
        };
        self.numbered += 1;
        let id = format!("craft-{}", self.numbered);
        let name = format!("Ship {}", self.numbered);
        let mut craft = Craft::new(
            id.clone(),
            name,
            CraftKind::Ship,
            Frame::System { star },
            seconds,
        );
        craft.place(state, seconds);
        info!(target: "craft", "Spawned {id} {:.0} km above {body}", altitude / 1_000.0);
        self.craft.push(Live {
            previous_orientation: craft.orientation,
            craft,
            field,
            previous: state,
            system_pc: [0.0; 3],
            forecast: Forecast::default(),
        });
        Ok(id)
    }

    /// Remove one craft, or every craft without an ID. Returns how many went.
    pub fn despawn(&mut self, id: Option<&str>) -> usize {
        let before = self.craft.len();
        self.craft
            .retain(|live| id.is_some_and(|id| live.craft.id != id));
        before - self.craft.len()
    }
}

/// Outward and prograde directions for an orbit around a body: in the plane
/// of its own orbit around its parent, turning the same way, on its far side.
/// A body with no parent uses its system's plane, orbits turning from +X
/// towards +Z as planets do.
fn orbit_axes(body: State, parent: Option<State>) -> ([f64; 3], [f64; 3]) {
    let unit = |v: [f64; 3]| {
        let length = distance(v, [0.0; 3]);
        (length > 0.0).then(|| v.map(|x| x / length))
    };
    let cross = |a: [f64; 3], b: [f64; 3]| {
        [
            a[1] * b[2] - a[2] * b[1],
            a[2] * b[0] - a[0] * b[2],
            a[0] * b[1] - a[1] * b[0],
        ]
    };
    let axes = parent.and_then(|parent| {
        let offset = std::array::from_fn(|i| body.position[i] - parent.position[i]);
        let motion = std::array::from_fn(|i| body.velocity[i] - parent.velocity[i]);
        let out = unit(offset)?;
        let pole = unit(cross(offset, motion))?;
        Some((out, cross(pole, out)))
    });
    axes.unwrap_or(([1.0, 0.0, 0.0], [0.0, 0.0, 1.0]))
}

/// A star's, barycentre's or planet's mass in solar masses.
fn body_mass(system: &StarSystem, page: u32, id: &str) -> Option<f64> {
    if let Some(planet) = system.planets.iter().find(|p| p.id == id) {
        return Some(planet.mass_earth * EARTH_SOLAR);
    }
    let member = system.stars_on(page).any(|s| s.id == id) || system.barycentre(id).is_some();
    member.then(|| system.member_mass(page, id))
}

/// The system holding a body, and the body itself: a planet, a star or
/// barycentre (`key#B`, `key#AB`), or a system by its key, meaning its star
/// or outermost barycentre. Its star must be loaded, visited or charted.
pub fn locate_body(
    galaxy: &Galaxy,
    nav: &Navigation,
    field: &crate::stars::stream::Starfield,
    id: &str,
) -> Result<(StarSystem, u32, String), String> {
    let key = space_simulation::multiple::system_key(id);
    if !crate::galaxy_nav::reachable(nav, galaxy, field, key) {
        return Err(format!(
            "`{id}` is not in a loaded, visited or charted system"
        ));
    }
    let (system, page) = match &nav.visit {
        Some(crate::Visit::Generated(system)) if system.id == key => ((**system).clone(), 0),
        _ => objects::system(galaxy, key)
            .map(|(system, page)| (system.into_owned(), page))
            .ok_or_else(|| format!("No system for `{id}`"))?,
    };
    let body = if id == key {
        system.root(page).unwrap_or(key).to_owned()
    } else {
        id.to_owned()
    };
    if body_mass(&system, page, &body).is_none() {
        return Err(format!("`{id}` is not a star, barycentre or planet"));
    }
    Ok((system, page, body))
}

/// Steps every craft up to the clock, at most `MAX_STEPS_PER_FRAME` steps
/// each. A craft that cannot keep up holds the clock back to it.
pub fn step(mut clock: ResMut<Clock>, mut fleet: ResMut<Fleet>) {
    let until = clock.0.seconds;
    let mut reached = f64::INFINITY;
    for live in &mut fleet.craft {
        let mut steps = 0;
        while live.craft.seconds < until && steps < MAX_STEPS_PER_FRAME {
            live.previous = live.craft.state();
            live.previous_orientation = live.craft.orientation;
            live.craft.step(&live.field);
            steps += 1;
        }
        reached = reached.min(live.craft.seconds);
    }
    fleet.held = reached < until;
    if fleet.held {
        clock.0.seconds = reached;
    }
}

/// The craft's developer readout, as `ns ship state` shows it.
pub fn readout(fleet: &Fleet, seconds: f64) -> serde_json::Value {
    use serde_json::json;
    let craft: Vec<_> = fleet
        .craft
        .iter()
        .map(|live| {
            let (state, reference) = live.surroundings(seconds);
            let target_direction = match &live.craft.autopilot {
                space_simulation::craft::AttitudePilot::Point { target, .. } => target
                    .direction(live.craft.state(), live.craft.seconds, &live.field, live.craft.reference.as_deref())
                    .map(|v| v.to_array()),
                _ => None,
            };
            let forward = bevy::math::DQuat::from_array(live.craft.orientation) * -bevy::math::DVec3::Z;
            let pointing_error_deg = target_direction.map(|v| forward.angle_between(bevy::math::DVec3::from_array(v)).to_degrees());
            let around = reference.map(|body| {
                let relative = relative(state, body.state);
                let orbit = space_simulation::craft::Elements::of(relative, body.gm);
                json!({
                    "id": body.id, "kind": body.kind,
                    "altitude_km": (distance(relative.position, [0.0; 3]) - body.radius_m) / 1_000.0,
                    "speed_kms": distance(relative.velocity, [0.0; 3]) / 1_000.0,
                    "orbit": {"eccentricity": orbit.eccentricity,
                        "apsides_altitude_km": orbit.apsides().map(|r| r.map(|v| (v - body.radius_m) / 1000.0)),
                        "period_seconds": orbit.period()},
                })
            });
            json!({
                "id": live.craft.id, "name": live.craft.name, "kind": live.craft.kind,
                "frame": live.craft.frame, "seconds": live.craft.seconds,
                "position_m": state.position, "velocity_ms": state.velocity,
                "speed_kms": distance(state.velocity, [0.0; 3]) / 1_000.0,
                "input": live.craft.input, "around": around,
                "orientation_xyzw": live.craft.orientation,
                "forward": forward.to_array(),
                "target_direction": target_direction, "pointing_error_deg": pointing_error_deg,
                "up": (bevy::math::DQuat::from_array(live.craft.orientation) * bevy::math::DVec3::Y).to_array(),
                "spin_rads": live.craft.spin,
                "thrusters": live.craft.thrusters,
                "autopilot": live.craft.autopilot,
                "prediction": prediction_readout(live),
            })
        })
        .collect();
    json!({"craft": craft, "held_clock": fleet.held, "step_seconds": STEP_SECONDS})
}

/// The shown coast prediction, for `ns ship state`.
fn prediction_readout(live: &Live) -> serde_json::Value {
    use space_simulation::craft::predict::End;
    let forecast = &live.forecast;
    let Some(p) = &forecast.shown else {
        return serde_json::json!({"updating": forecast.updating()});
    };
    let end = match &p.end {
        Some(End::Horizon) => "horizon".to_owned(),
        Some(End::Truncated) => "truncated".to_owned(),
        Some(End::Impact { body }) => format!("impact:{body}"),
        None => "running".to_owned(),
    };
    serde_json::json!({
        "reference": p.reference,
        "age_seconds": live.craft.seconds - p.start_seconds,
        "current": forecast.current(&live.craft),
        "updating": forecast.updating(),
        "bound": p.bound,
        "horizon_seconds": p.horizon_seconds,
        "achieved_seconds": p.achieved_seconds(),
        "samples": p.samples.len(),
        "end": end,
    })
}

/// The inspector's title, facts and note for a craft, live at `seconds`.
pub fn facts(live: &Live, galaxy: &Galaxy, seconds: f64) -> (String, Vec<(String, String)>) {
    use crate::format::{km_text, period_text, speed_label};
    let (state, reference) = live.surroundings(seconds);
    let mut rows = vec![("Type".to_owned(), live.craft.kind.label().to_owned())];
    let key = space_simulation::multiple::system_key(live.star());
    rows.push(("System".into(), crate::galaxy_nav::label(galaxy, key)));
    if let Some(body) = reference {
        let relative = relative(state, body.state);
        let r = distance(relative.position, [0.0; 3]);
        let v = distance(relative.velocity, [0.0; 3]);
        rows.push(("Around".into(), body_label(live, galaxy, &body)));
        rows.push(("Altitude".into(), km_text((r - body.radius_m) / 1_000.0)));
        rows.push(("Speed".into(), speed_label(v)));
        let orbit = space_simulation::craft::Elements::of(relative, body.gm);
        match orbit.apsides() {
            Some([low, high]) => {
                rows.push(("Periapsis".into(), km_text((low - body.radius_m) / 1_000.0)));
                rows.push(("Apoapsis".into(), km_text((high - body.radius_m) / 1_000.0)));
                let period = orbit.period().unwrap_or(f64::INFINITY);
                let text = if period < 2.0 * 86_400.0 {
                    format!("{:.2} hours", period / 3_600.0)
                } else {
                    period_text(period / 86_400.0)
                };
                rows.push(("Orbital period".into(), text));
            }
            None => rows.push(("Orbit".into(), "Escaping".into())),
        }
    }
    let thrust = distance(live.craft.input.thrust, [0.0; 3]);
    rows.push((
        "Thrust".into(),
        if thrust > 0.0 {
            format!("{thrust:.2} m/s²")
        } else {
            "Off".into()
        },
    ));
    rows.push((
        "Throttle".into(),
        format!("{:+.1}%", live.craft.input.controls.throttle * 100.0),
    ));
    rows.push((
        "Spin (pitch/yaw/roll)".into(),
        live.craft
            .spin
            .map(|v| format!("{:+.2}", v.to_degrees()))
            .join(" / ")
            + " °/s",
    ));
    rows.push((
        "Attitude pilot".into(),
        match &live.craft.autopilot {
            space_simulation::craft::AttitudePilot::Manual => "Manual".into(),
            space_simulation::craft::AttitudePilot::Stop => "Stop rotation".into(),
            space_simulation::craft::AttitudePilot::Point { target, .. } => format!("{target:?}"),
        },
    ));
    (live.craft.name.clone(), rows)
}

/// A body's name: a planet's designation, or a star's with its letters.
pub fn body_label(live: &Live, galaxy: &Galaxy, body: &Reference) -> String {
    let system = live.field.system();
    if let Some(planet) = system.planets.iter().find(|p| p.id == body.id) {
        return planet.designation.clone();
    }
    let key = space_simulation::multiple::system_key(&body.id);
    objects::member_label(
        &crate::galaxy_nav::label(galaxy, key),
        space_simulation::multiple::member_letters(&body.id),
    )
}

/// Where a system's star is, in parsecs from the galactic centre, or the
/// centre itself if it cannot be found.
pub fn system_pc(
    nav: &Navigation,
    galaxy: &Galaxy,
    field: &crate::stars::stream::Starfield,
    star: &str,
) -> [f64; 3] {
    let Some(picked) = crate::galaxy_nav::selectable_star(nav, galaxy, field, star) else {
        return [0.0; 3];
    };
    let home = space_simulation::galaxy::GalaxyModel::milky_way().home_pc;
    let ly = picked.position_ly.to_array();
    std::array::from_fn(|i| home[i] + ly[i] * space_simulation::search::PC_PER_LY)
}

/// Every craft as search sees it, at `seconds`.
pub fn live_objects(fleet: &Fleet, galaxy: &Galaxy, seconds: f64) -> Vec<LiveObject> {
    fleet
        .craft
        .iter()
        .map(|live| {
            let (state, reference) = live.surroundings(seconds);
            let key = space_simulation::multiple::system_key(live.star()).to_owned();
            let (altitude, speed) = reference.as_ref().map_or((f64::NAN, f64::NAN), |body| {
                let relative = relative(state, body.state);
                let r = distance(relative.position, [0.0; 3]);
                (
                    (r - body.radius_m) / 1_000.0,
                    distance(relative.velocity, [0.0; 3]) / 1_000.0,
                )
            });
            LiveObject {
                result: SearchResult {
                    object: objects::ObjectRef::Craft {
                        id: live.craft.id.clone(),
                    },
                    kind: live.craft.kind.name().into(),
                    name: live.craft.name.clone(),
                    class: live.craft.kind.label().into(),
                    system: crate::galaxy_nav::label(galaxy, &key),
                    distance_ly: 0.0,
                    position_pc: live.system_pc,
                },
                system_key: key,
                altitude_km: altitude,
                speed_kms: speed,
                orbits: reference.map(|body| body.id),
            }
        })
        .collect()
}

/// A state relative to another.
pub fn relative(state: State, to: State) -> State {
    State {
        position: std::array::from_fn(|i| state.position[i] - to.position[i]),
        velocity: std::array::from_fn(|i| state.velocity[i] - to.velocity[i]),
    }
}

/// Keeps craft out of reach from being drawn, picked or followed elsewhere:
/// a craft is shown only in its own system, on the page its frame names.
pub fn shown_here<'a>(
    fleet: &'a Fleet,
    system: &'a StarSystem,
    page: u32,
) -> impl Iterator<Item = &'a Live> {
    let star = objects::star_id(system, page);
    fleet.craft.iter().filter(move |live| live.star() == star)
}

/// Whether the camera follows a craft this frame, and where it is.
pub fn followed(nav: &Navigation, galaxy: &Galaxy, fleet: &Fleet, seconds: f64) -> Option<State> {
    let Some(crate::Selection::Craft(id)) = &nav.selection else {
        return None;
    };
    let system = nav.system(galaxy)?;
    shown_here(fleet, system, nav.page)
        .find(|live| live.craft.id == *id)
        .map(|live| live.state_at(seconds))
}

#[cfg(test)]
mod tests;
