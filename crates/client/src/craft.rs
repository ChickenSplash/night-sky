//! Live craft: ships the player places, stepped with the simulation clock.
//! Their physics is `space_simulation::craft`; this keeps them, steps them in
//! fixed steps each frame, and blends between steps for drawing.
use crate::{Clock, Navigation, objects};
use bevy::prelude::*;
use space_shared::{Galaxy, StarSystem};
use space_simulation::search::{SearchResult, live::LiveObject};
use space_simulation::{
    EARTH_SOLAR, SUN_GM, SUN_RADIUS_METRES, State,
    craft::{
        Craft, CraftKind, Field, Frame, Reference, STEP_SECONDS, circular_speed, distance,
        predict::Prediction,
        tick_at, tick_seconds,
        voyage::{Voyage, Wait},
    },
};

pub use space_simulation::craft::Order;
mod forecast;
pub use forecast::{Forecast, forecast};
mod loading;
mod readout;
pub use readout::{facts, readout};
mod shape;
mod travel;
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

#[derive(Resource, Default)]
pub struct Fleet {
    pub craft: Vec<Live>,
    /// System data the craft have waited for, shared between them.
    pub charts: travel::Charts,
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
        static EMPTY: std::sync::OnceLock<Fleet> = std::sync::OnceLock::new();
        EMPTY.get_or_init(Fleet::default).at(0.0)
    }
}

impl<'a> Now<'a> {
    pub fn get(&self, id: &str) -> Option<&'a Live> {
        self.fleet.get(id)
    }
}

/// A live craft: its voyage, which steps it, and what drawing needs. Reads
/// through to the voyage's craft, field and frame origin.
pub struct Live {
    pub voyage: Voyage,
    /// The state one step before `craft`'s, blended with it when drawn.
    previous: State,
    previous_orientation: [f64; 4],
    /// Last displayed state, transformed alongside physics during any rebase.
    displayed: Option<State>,
    /// This update's displayed displacement in galactic axes, excluding camera orbit.
    pub display_delta_m: bevy::math::DVec3,
    /// Where it would go if it coasted.
    pub forecast: Forecast,
    /// What its next step waits for: the clock is held until it is known.
    pub waiting: Option<Wait>,
}

impl std::ops::Deref for Live {
    type Target = Voyage;
    fn deref(&self) -> &Voyage {
        &self.voyage
    }
}

impl std::ops::DerefMut for Live {
    fn deref_mut(&mut self) -> &mut Voyage {
        &mut self.voyage
    }
}

impl Live {
    fn share_at(&self, seconds: f64) -> f64 {
        let start = tick_seconds(self.craft.tick - 1);
        // Use the represented interval. At large clock epochs, dividing by literal 0.05 gives a slightly different
        // share, which becomes kilometres at interstellar warp speeds.
        ((seconds - start) / (self.craft.seconds() - start)).clamp(0.0, 1.0)
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
        if matches!(self.craft.frame, Frame::Galaxy { .. }) {
            return (state, None);
        }
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
            // In warp, turning steers the bubble rather than the thrusters.
            let turning = !craft.warp.engaged
                && (input.controls.rotation != [0.0; 3]
                    || craft
                        .spin
                        .iter()
                        .any(|v| v.abs() > space_simulation::craft::warp::SPIN_TOLERANCE)
                    || !matches!(craft.autopilot, AttitudePilot::Manual));
            input.thrust != [0.0; 3]
                || input.controls.throttle != 0.0
                || input.controls.translation != [0.0; 3]
                || turning
                || live.scheduled().any(|(_, order)| match order {
                    Order::Throttle { value } => *value != 0.0,
                    Order::Translate { right, up, forward } => [*right, *up, *forward] != [0.0; 3],
                    Order::Rotate { pitch, yaw, roll } => [*pitch, *yaw, *roll] != [0.0; 3],
                    Order::Thrust { thrust } => *thrust != [0.0; 3],
                    Order::Point { .. } | Order::Stop | Order::Cut => true,
                    _ => false,
                })
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
            .map(|live| live.craft.seconds())
            .fold(f64::INFINITY, f64::min);
        self.at(if seconds.is_finite() { seconds } else { 0.0 })
    }

    pub fn get(&self, id: &str) -> Option<&Live> {
        self.craft.iter().find(|live| live.craft.id == id)
    }

    pub fn get_mut(&mut self, id: &str) -> Option<&mut Live> {
        self.craft.iter_mut().find(|live| live.craft.id == id)
    }

    /// A new ship `altitude` metres above a body's surface, placed at the
    /// first world step at or after `seconds`. Returns its ID.
    pub fn spawn(
        &mut self,
        system: StarSystem,
        page: u32,
        body: &str,
        altitude: f64,
        launch: Launch,
        seconds: f64,
    ) -> Result<String, String> {
        let tick = tick_at(seconds);
        let seconds = tick_seconds(tick);
        let star = objects::star_id(&system, page);
        let mass = body_mass(&system, page, body).ok_or_else(|| format!("No body `{body}`"))?;
        let origin_pc =
            space_simulation::craft::voyage::frame_origin_pc(self.charts.sky(), &system, page);
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
            tick,
        );
        craft.place(state, tick);
        craft.reference = field
            .select_reference(None, state.position, seconds)
            .map(|reference| reference.id);
        info!(target: "craft", "Spawned {id} {:.0} km above {body}", altitude / 1_000.0);
        self.craft.push(Live {
            previous_orientation: craft.orientation,
            voyage: Voyage::new(craft, field, origin_pc),
            previous: state,
            displayed: None,
            display_delta_m: bevy::math::DVec3::ZERO,
            forecast: Forecast::default(),
            waiting: None,
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
        _ => objects::known_system(galaxy, key)
            .map(|(system, page)| (system.clone(), page))
            .or_else(|| {
                let picked = crate::galaxy_nav::selectable_star(nav, galaxy, field, key)?;
                crate::galaxy_nav::generated(key, picked.position_ly).map(|system| (system, 0))
            })
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
/// each. A craft that cannot keep up, or waits for system data, holds the
/// clock back to it.
pub fn step(
    mut clock: ResMut<Clock>,
    mut fleet: ResMut<Fleet>,
    galaxy: Res<crate::Universe>,
    metrics: Option<ResMut<crate::stars::stream::StreamMetrics>>,
) {
    use bevy::platform::time::Instant;
    let start = Instant::now();
    let mut timing = crate::stars::stream::CraftFrame::default();
    let until = clock.0.seconds;
    let mut reached = f64::INFINITY;
    let Fleet { craft, charts, .. } = &mut *fleet;
    let discovery_start = Instant::now();
    charts.poll(&galaxy.0);
    timing.discovery_ms += discovery_start.elapsed().as_secs_f64() * 1000.0;
    for live in craft.iter_mut() {
        let stepping_start = Instant::now();
        let steps = live.catch_up(charts, &galaxy.0, until, &mut timing.discovery_ms);
        reached = reached.min(live.craft.seconds());
        timing.stepping_ms += stepping_start.elapsed().as_secs_f64() * 1000.0;
        timing.steps += steps;
        charts.prefetch(&live.voyage, &galaxy.0);
    }
    charts.dispatch();
    fleet.held = reached < until;
    if fleet.held {
        clock.0.seconds = reached;
    }
    // Use the final shared clock, including catch-up holds, just as drawing does.
    for live in &mut fleet.craft {
        let state = live.state_at(clock.0.seconds);
        live.display_delta_m = live.displayed.map_or(bevy::math::DVec3::ZERO, |previous| {
            live.basis()
                * (bevy::math::DVec3::from_array(state.position)
                    - bevy::math::DVec3::from_array(previous.position))
        });
        live.displayed = Some(state);
    }
    timing.total_ms = start.elapsed().as_secs_f64() * 1000.0;
    if let Some(mut metrics) = metrics {
        metrics.craft_latest = timing;
    }
}

impl Live {
    /// Step towards `until`, at most `MAX_STEPS_PER_FRAME` steps, stopping
    /// early while system data the next step needs is worked out. Returns
    /// the steps taken; `discovery_ms` gains the time spent asking for data.
    pub fn catch_up(
        &mut self,
        charts: &mut travel::Charts,
        galaxy: &Galaxy,
        until: f64,
        discovery_ms: &mut f64,
    ) -> usize {
        use bevy::platform::time::Instant;
        let mut steps = 0;
        self.waiting = None;
        while self.craft.seconds() < until && steps < MAX_STEPS_PER_FRAME {
            match self.voyage.step(&*charts) {
                Ok(step) => {
                    if let Some(rebase) = &step.rebase {
                        self.rebase(rebase);
                    }
                    self.previous = step.from;
                    self.previous_orientation = step.from_orientation;
                    for refused in step.refused {
                        warn!(target: "craft", "Ship order for step {} refused: {}", refused.tick, refused.reason);
                    }
                }
                Err(wait) => {
                    let start = Instant::now();
                    let known = charts.offer(&wait, galaxy);
                    *discovery_ms += start.elapsed().as_secs_f64() * 1000.0;
                    if known {
                        continue;
                    }
                    self.waiting = Some(wait);
                    break;
                }
            }
            if self.craft.warp.engaged {
                self.forecast = Forecast::default();
            }
            steps += 1;
        }
        steps
    }
}

/// The shown prediction still ahead, in the craft's frame, for drawing.
pub struct Path {
    /// From the craft where it is drawn now, then each sample still ahead,
    /// and back to the craft when it closes a revolution.
    pub points: Vec<[f64; 3]>,
    pub marks: Vec<([f64; 3], PathMark)>,
    /// Made under the craft's current inputs and reference.
    pub current: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PathMark {
    Apsis,
    Reference,
    Impact,
}

/// The shown prediction from `seconds` on. Its samples are relative to its
/// reference, so they are anchored at where that reference is now: an
/// orbit around a moving planet is drawn closed around the planet.
///
/// The real craft and an older prediction drift apart by metres, and a
/// revolution ends a little off where it began, so drawn as it is the path
/// would kink at the craft when seen close up. Both differences are spread
/// smoothly along the path instead: it starts at the craft and, when it
/// closes, ends there too.
pub fn predicted_path(live: &Live, seconds: f64) -> Option<Path> {
    if live.craft.warp.engaged || live.star().is_empty() {
        return None;
    }
    use space_simulation::craft::predict::{End, Event};
    let p = live.forecast.shown.as_ref()?;
    let anchor = live
        .field
        .reference_by_id(&p.reference, seconds)?
        .state
        .position;
    let ahead = p.ahead(seconds);
    let last = ahead.last()?;
    let here = live.state_at(seconds).position;
    let relative: [f64; 3] = std::array::from_fn(|i| here[i] - anchor[i]);
    let current = live.forecast.current(&live.craft);
    let closes = p.closing_lap(seconds).filter(|_| current);
    let gap = |t: f64| -> [f64; 3] {
        let at = p.position_at(t).unwrap_or(relative);
        std::array::from_fn(|i| relative[i] - at[i])
    };
    let (start, end) = (gap(seconds), closes.map_or([0.0; 3], gap));
    let span = (p.shown_end(seconds) - seconds).max(f64::MIN_POSITIVE);
    let place = |t: f64, position: [f64; 3]| -> [f64; 3] {
        let share = ((t - seconds) / span).clamp(0.0, 1.0);
        std::array::from_fn(|i| anchor[i] + position[i] + start[i] * (1.0 - share) + end[i] * share)
    };
    let mut points: Vec<_> = std::iter::once(here)
        .chain(ahead.iter().map(|s| place(s.seconds, s.position)))
        .collect();
    if closes.is_some() {
        points.push(here);
    }
    let mut marks: Vec<_> = p
        .upcoming(seconds)
        .filter_map(|event| {
            let mark = match event {
                Event::Reference { .. } => PathMark::Reference,
                _ => PathMark::Apsis,
            };
            let t = event.seconds();
            Some((place(t, p.position_at(t)?), mark))
        })
        .collect();
    if let Some(End::Impact { .. }) = p.end
        && p.end_seconds() <= p.shown_end(seconds)
    {
        marks.push((place(last.seconds, last.position), PathMark::Impact));
    }
    Some(Path {
        points,
        marks,
        current,
    })
}

/// A predicted event still ahead, as readouts show it.
pub enum Upcoming {
    /// Altitude above the prediction reference's surface.
    Apsis {
        periapsis: bool,
        seconds: f64,
        altitude_m: f64,
    },
    /// An exit when the new reference holds the old one, else an encounter.
    Reference {
        exit: bool,
        seconds: f64,
        id: String,
    },
}

/// The prediction's events in its shown part from `seconds`: every reference
/// change, and the next periapsis and apoapsis, soonest first.
pub fn upcoming(live: &Live, p: &Prediction, seconds: f64) -> Vec<Upcoming> {
    use space_simulation::craft::predict::Event;
    let radius = live
        .field
        .reference_by_id(&p.reference, seconds)
        .map_or(0.0, |r| r.radius_m);
    let end = p.shown_end(seconds);
    let mut previous = p.reference.clone();
    let mut out = Vec::new();
    let (mut periapsis, mut apoapsis) = (false, false);
    for event in &p.events {
        match event {
            Event::Reference { seconds: at, id } => {
                let exit = live.field.lineage(&previous).contains(id);
                previous = id.clone();
                if (seconds..=end).contains(at) {
                    out.push(Upcoming::Reference {
                        exit,
                        seconds: *at,
                        id: id.clone(),
                    });
                }
            }
            Event::Periapsis {
                seconds: at,
                distance_m,
            } if (seconds..=end).contains(at) && !periapsis => {
                periapsis = true;
                out.push(Upcoming::Apsis {
                    periapsis: true,
                    seconds: *at,
                    altitude_m: distance_m - radius,
                });
            }
            Event::Apoapsis {
                seconds: at,
                distance_m,
            } if (seconds..=end).contains(at) && !apoapsis => {
                apoapsis = true;
                out.push(Upcoming::Apsis {
                    periapsis: false,
                    seconds: *at,
                    altitude_m: distance_m - radius,
                });
            }
            _ => {}
        }
    }
    out
}

/// A body's name: a planet's designation, or a star's or pair's with its
/// letters.
pub fn body_label(live: &Live, galaxy: &Galaxy, id: &str) -> String {
    let system = live.field.system();
    if let Some(planet) = system.planets.iter().find(|p| p.id == id) {
        return planet.designation.clone();
    }
    let key = space_simulation::multiple::system_key(id);
    objects::member_label(
        &crate::galaxy_nav::label(galaxy, key),
        space_simulation::multiple::member_letters(id),
    )
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
                    position_pc: live.position_pc(seconds),
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

/// Whether the camera follows a craft this frame, and where it is.
pub fn followed(nav: &Navigation, _galaxy: &Galaxy, fleet: &Fleet, seconds: f64) -> Option<State> {
    let Some(crate::Selection::Craft(id)) = &nav.selection else {
        return None;
    };
    fleet.get(id).map(|live| live.state_at(seconds))
}

#[cfg(test)]
mod tests;
