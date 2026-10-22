//! Which frame a craft flies in, and the handoffs between them, decided from
//! its own state and seeded or shared system data alone. Wherever it runs and
//! whatever a camera has loaded, the same inputs give the same path: when the
//! data a decision needs is missing, the step waits for it rather than
//! carrying on without it.
use super::{Craft, Field, Frame, Gravity, Order, Target};
use crate::galaxy::PC_METRES;
use crate::neighbourhood::{Candidate, Region};
use crate::{LY_METRES, State};
use glam::{DQuat, DVec3};
use std::collections::BTreeMap;

mod atlas;
pub use atlas::*;
pub mod loading;

/// Fastest warp at which a craft enters systems. Below it a star's pull on a
/// passing craft can matter; above it a craft crosses a system's range in a
/// few steps, so it stays in galactic space and needs no system data.
pub const ARRIVAL_MAX_SPEED: f64 = crate::LIGHT_SPEED_MS;
/// Galactic offsets beyond this move the frame's anchor, keeping local metres
/// precise. Only precision: it decides nothing else.
pub const ANCHOR_M: f64 = 0.125 * LY_METRES;
/// A system is left at this many times its entry range, so a craft on its
/// edge does not switch back and forth.
pub const LEAVE_RANGES: f64 = 2.0;
/// Maximum future orders held by one voyage. A server still needs per-player
/// rate limits and ownership checks at its network boundary.
pub const MAX_PENDING_ORDERS: usize = 1_024;
/// Live submission horizon: five simulated minutes. Long replays should feed
/// their log incrementally rather than occupy the live queue indefinitely.
pub const MAX_ORDER_AHEAD_TICKS: i64 = 300 * super::STEPS_PER_SECOND;

/// A delay or duration in seconds as whole steps, to the nearest step.
pub fn steps(seconds: f64) -> Result<i64, String> {
    let most = (MAX_ORDER_AHEAD_TICKS / super::STEPS_PER_SECOND) as f64;
    if !(0.0..=most).contains(&seconds) {
        return Err(format!("Times must be 0 to {most} seconds"));
    }
    Ok((seconds * super::STEPS_PER_SECOND as f64).round() as i64)
}
/// Approximate retained generation-cache budget between requests. A single
/// query's temporary working set is additional to this.
pub const SKY_CACHE_BYTES: usize = 32 * 1024 * 1024;

fn trim_sky(sky: &mut crate::cells::Sky) {
    if sky.cache_bytes() > SKY_CACHE_BYTES {
        // Keep the expensive coarse integration warm, but release fine cells
        // accumulated on long voyages. Rebuilding them changes no seeds.
        sky.retain(|cell| cell.level() <= 3);
    }
}

/// Rotation from a page's orbital-plane axes into galactic axes.
pub fn page_basis(system: &space_shared::StarSystem, page: u32) -> DQuat {
    let [inclination, node, twist] = system
        .page_star(page)
        .map_or([0.0; 3], |star| crate::plane_tilt(&star.id));
    DQuat::from_rotation_y(node)
        * DQuat::from_rotation_x(inclination)
        * DQuat::from_rotation_y(twist)
}

/// The ID a page's frame is named by: its assigned cell star, else the
/// list's legacy key.
pub fn page_star_id(system: &space_shared::StarSystem, page: u32) -> String {
    system
        .page_star(page)
        .map_or_else(|| crate::star_id(&system.id, page), |star| star.id.clone())
}

/// Where a page's frame is anchored, in parsecs from the galactic centre,
/// from seeds and the system alone. A cell star's page sits at the star's
/// seeded position, the same one arrival uses, so a craft placed in a system
/// and one flying into it agree to the metre. A legacy page with no cell
/// star sits where its list puts it.
pub fn frame_origin_pc(
    sky: &mut crate::cells::Sky,
    system: &space_shared::StarSystem,
    page: u32,
) -> [f64; 3] {
    let origin = system
        .page_star(page)
        .and_then(|star| sky.star(&star.id))
        .map_or_else(|| listed_origin_pc(system, page), |star| star.position_pc);
    trim_sky(sky);
    origin
}

/// A page's position as its list gives it, in parsecs from the galactic
/// centre: for legacy pages, which seeds cannot place.
pub fn listed_origin_pc(system: &space_shared::StarSystem, page: u32) -> [f64; 3] {
    let home = crate::galaxy::GalaxyModel::milky_way().home_pc;
    let ly = system
        .page_star(page)
        .map_or(system.position_ly, |star| star.position_ly);
    std::array::from_fn(|i| home[i] + ly[i] * LY_METRES / PC_METRES)
}

/// A system ready to be entered: its gravity, its origin and its exact range.
#[derive(Clone, Debug)]
pub struct Arrival {
    pub field: Field,
    pub origin_pc: [f64; 3],
    pub range_m: f64,
}

impl Arrival {
    pub fn new(field: Field, origin_pc: [f64; 3]) -> Self {
        Self {
            range_m: range_m(&field),
            field,
            origin_pc,
        }
    }
}

/// A system's entry range, in metres.
pub fn range_m(field: &Field) -> f64 {
    crate::neighbourhood::range_m(crate::system_extent_au(field.system(), field.page()))
}

/// Threads a native lookup may use; one in the browser.
pub fn threads() -> usize {
    if cfg!(target_arch = "wasm32") {
        1
    } else {
        std::thread::available_parallelism().map_or(1, |n| n.get())
    }
}

/// A candidate's position in light years from home, as systems carry it.
pub fn candidate_ly(candidate: &Candidate) -> [f64; 3] {
    let home = crate::galaxy::GalaxyModel::milky_way().home_pc;
    std::array::from_fn(|i| (candidate.position_pc[i] - home[i]) * PC_METRES / LY_METRES)
}

/// A seeded candidate's bodies, generated here.
pub fn generated_arrival(candidate: &Candidate) -> Arrival {
    seeded_arrival(
        candidate,
        crate::generated_system(&candidate.id, candidate_ly(candidate)),
    )
}

/// A seeded candidate's generated bodies, with its frame at its exact seeded
/// position. A star generation does not know becomes an empty system.
pub fn seeded_arrival(candidate: &Candidate, system: Option<space_shared::StarSystem>) -> Arrival {
    let system = system.unwrap_or_else(|| {
        let position_ly = candidate_ly(candidate);
        space_shared::StarSystem {
            id: candidate.id.clone(),
            name: candidate.id.clone(),
            position_ly,
            planets: Vec::new(),
            stars: Vec::new(),
            barycentres: Vec::new(),
            page_stars: vec![space_shared::PageStar {
                page: 0,
                id: candidate.id.clone(),
                position_ly,
            }],
        }
    });
    Arrival::new(Field::new(system, 0), candidate.position_pc)
}

/// System data to work out for a wait, as sent to a background worker.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Request {
    Region { region: Region },
    System { id: String, position_ly: [f64; 3] },
    Star { id: String },
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Reply {
    Region {
        candidates: Vec<Candidate>,
    },
    System {
        system: Option<space_shared::StarSystem>,
    },
    Star {
        candidate: Option<Candidate>,
    },
}

impl Request {
    pub fn of(wait: &Wait) -> Self {
        match wait {
            Wait::Region(region) => Self::Region { region: *region },
            Wait::System(candidate) => Self::System {
                id: candidate.id.clone(),
                position_ly: candidate_ly(candidate),
            },
            Wait::Star(id) => Self::Star { id: id.clone() },
        }
    }

    /// Work it out here and now, with a warm `sky` for speed.
    pub fn answer(&self, sky: &mut crate::cells::Sky, threads: usize) -> Reply {
        let reply = match self {
            Self::Region { region } => Reply::Region {
                candidates: crate::neighbourhood::candidates(sky, *region, threads),
            },
            Self::System { id, position_ly } => Reply::System {
                system: crate::generated_system(id, *position_ly),
            },
            Self::Star { id } => Reply::Star {
                candidate: crate::neighbourhood::candidate(sky, id),
            },
        };
        trim_sky(sky);
        reply
    }
}

/// What a step needs before it can be taken.
#[derive(Clone, Debug, PartialEq)]
pub enum Wait {
    Region(Region),
    System(Candidate),
    /// Where a system is, by its page star's ID: a route's destination.
    Star(String),
}

/// A change of frame coordinates: `rotation * x + translation` for positions,
/// `rotation * v + velocity` for velocities. Apply it to anything kept in the
/// old frame, such as interpolation states.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rebase {
    pub rotation: DQuat,
    pub translation: DVec3,
    pub velocity: DVec3,
    /// A different system or galactic space, not just a moved anchor.
    pub frame_changed: bool,
}

impl Rebase {
    pub fn state(&self, s: State) -> State {
        State {
            position: (self.rotation * DVec3::from_array(s.position) + self.translation).to_array(),
            velocity: (self.rotation * DVec3::from_array(s.velocity) + self.velocity).to_array(),
        }
    }

    pub fn orientation(&self, q: [f64; 4]) -> [f64; 4] {
        (self.rotation * DQuat::from_array(q)).to_array()
    }

    pub fn direction(&self, v: [f64; 3]) -> [f64; 3] {
        (self.rotation * DVec3::from_array(v)).to_array()
    }
}

/// A craft with the frame data it is stepped in.
#[derive(Clone, Debug)]
pub struct Voyage {
    pub craft: Craft,
    /// The current system's gravity. In galactic space, the last system's,
    /// kept for readouts but never stepped with.
    pub field: Field,
    /// The frame's origin, in parsecs from the galactic centre.
    pub origin_pc: [f64; 3],
    /// The current system's entry range, in metres.
    pub range_m: f64,
    /// Orders given for later world steps, by step number. Every key is
    /// after the craft's `tick`: an order is applied as soon as the craft
    /// reaches its step.
    orders: BTreeMap<i64, Vec<Queued>>,
    /// Every craft as it was at the start of this craft's step, given by its
    /// host before each step and order, so craft that read each other give
    /// the same paths whatever order they are stepped in. Not saved.
    pub traffic: Vec<Sighting>,
}

/// A craft as others see it: its state at a step, in its own frame, with
/// what converting it into another frame needs.
#[derive(Clone, Debug, PartialEq)]
pub struct Sighting {
    pub id: String,
    pub tick: i64,
    pub state: State,
    /// Its warp bubble's velocity, in its frame's axes.
    pub warp_velocity: [f64; 3],
    pub frame: Frame,
    pub origin_pc: [f64; 3],
    pub basis: DQuat,
}

/// A kept order, and for the end of a timed order the step it began at.
#[derive(Clone, Debug)]
struct Queued {
    order: Order,
    ends_from: Option<i64>,
}

/// One fixed step taken by `Voyage::step`.
#[derive(Clone, Debug, PartialEq)]
pub struct Step {
    /// The frame change made before moving, if any.
    pub rebase: Option<Rebase>,
    /// The craft's state and attitude as it set off, in its new frame.
    pub from: State,
    pub from_orientation: [f64; 4],
    /// Scheduled orders refused at the boundary reached by this step.
    pub refused: Vec<Refused>,
}

/// A scheduled order its step refused.
#[derive(Clone, Debug, PartialEq)]
pub struct Refused {
    pub tick: i64,
    pub order: Order,
    pub reason: String,
}

/// A step-keyed input, suitable for a replay log or saving pending orders
/// alongside the craft. Orders sharing a tick retain their submitted order.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ScheduledOrder {
    pub tick: i64,
    pub order: Order,
    /// For the end of a timed order, the step the order began at.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ends_from: Option<i64>,
}

impl Voyage {
    pub fn new(craft: Craft, field: Field, origin_pc: [f64; 3]) -> Self {
        Self {
            range_m: range_m(&field),
            craft,
            field,
            origin_pc,
            orders: BTreeMap::new(),
            traffic: Vec::new(),
        }
    }

    /// This craft as others see it now.
    pub fn sighting(&self) -> Sighting {
        let warp = &self.craft.warp;
        let nose = DQuat::from_array(self.craft.orientation) * -DVec3::Z;
        Sighting {
            id: self.craft.id.clone(),
            tick: self.craft.tick,
            state: self.craft.state(),
            warp_velocity: if warp.engaged {
                (nose * warp.actual_ms).to_array()
            } else {
                [0.0; 3]
            },
            frame: self.craft.frame.clone(),
            origin_pc: self.origin_pc,
            basis: self.basis(),
        }
    }

    /// The other craft in `traffic` as route targets in this frame, carried
    /// on at their speed to this craft's step should a sighting be from
    /// another.
    pub fn ships(&self) -> Vec<Target> {
        self.traffic
            .iter()
            .filter(|seen| seen.id != self.craft.id)
            .map(|seen| {
                let rebase = self.rebase_from(&seen.frame, seen.origin_pc, seen.basis);
                let mut state = rebase.state(seen.state);
                let warp = rebase.direction(seen.warp_velocity);
                let dt = self.craft.seconds() - super::tick_seconds(seen.tick);
                if dt != 0.0 {
                    state.position = std::array::from_fn(|i| {
                        state.position[i] + (state.velocity[i] + warp[i]) * dt
                    });
                }
                Target::craft(&seen.id, state, warp)
            })
            .collect()
    }

    /// The craft's system, for checking orders, when it is in one.
    fn system_field(&self) -> Option<&Field> {
        (!self.in_galaxy()).then_some(&self.field)
    }

    /// Give an order for world step `tick`, so a server can key every input
    /// by step number. An order for the craft's current step applies at
    /// once, returning any refusal; it acts from that step. A later one is
    /// kept until the craft reaches its step, and a refusal then goes to
    /// the step's result. An order for a step already taken is refused.
    pub fn order(&mut self, tick: i64, order: Order) -> Result<(), String> {
        self.schedule(ScheduledOrder {
            tick,
            order,
            ends_from: None,
        })
    }

    /// Give `order` from step `tick` and its `Order::off` at step `end`,
    /// both or neither. The end is dropped if the order is refused at its
    /// step, or once a later order on the same controls replaces it, so an
    /// earlier burn's end never cuts a newer one.
    pub fn timed_order(&mut self, tick: i64, end: i64, order: Order) -> Result<(), String> {
        let off = self.check_timed_order(tick, end, &order)?;
        self.order(tick, order)?;
        self.orders.entry(end).or_default().push(Queued {
            order: off,
            ends_from: Some(tick),
        });
        Ok(())
    }

    /// Check a timed order without changing the voyage, returning its end.
    pub fn check_timed_order(&self, tick: i64, end: i64, order: &Order) -> Result<Order, String> {
        let off = order
            .off()
            .ok_or("This order has no off state, so it cannot be timed")?;
        if end <= tick {
            return Err("A timed order must last at least one step (0.05 s)".into());
        }
        self.check_order(tick, order)?;
        if end.saturating_sub(self.craft.tick) > MAX_ORDER_AHEAD_TICKS {
            return Err("Ship orders may be scheduled at most 300 simulated seconds ahead".into());
        }
        if self.pending() + 1 + usize::from(tick != self.craft.tick) > MAX_PENDING_ORDERS {
            return Err("Too many pending ship orders".into());
        }
        Ok(off)
    }

    /// Give a step-keyed input, such as one `pending_orders` saved.
    pub fn schedule(&mut self, input: ScheduledOrder) -> Result<(), String> {
        let ScheduledOrder {
            tick,
            order,
            ends_from,
        } = input;
        if tick == self.craft.tick {
            let ships = self.ships();
            let in_system = !self.in_galaxy();
            order.apply_among(&mut self.craft, in_system.then_some(&self.field), &ships)?;
            if ends_from.is_none() {
                self.replace_ends(tick, &order);
            }
            Ok(())
        } else {
            self.check_order(tick, &order)?;
            self.orders
                .entry(tick)
                .or_default()
                .push(Queued { order, ends_from });
            Ok(())
        }
    }

    fn pending(&self) -> usize {
        self.orders.values().map(Vec::len).sum()
    }

    /// Drop the pending ends of timed orders begun before step `tick` that
    /// `order`, applied at it, replaces.
    fn replace_ends(&mut self, tick: i64, order: &Order) {
        self.drop_queued(|q| {
            q.ends_from.is_some_and(|from| from < tick) && order.overlaps(&q.order)
        });
    }

    /// Drop the end of a timed order refused at its step `tick`.
    fn drop_end(&mut self, tick: i64, order: &Order) {
        if let Some(off) = order.off() {
            self.drop_queued(|q| q.ends_from == Some(tick) && q.order == off);
        }
    }

    fn drop_queued(&mut self, drop: impl Fn(&Queued) -> bool) {
        for queued in self.orders.values_mut() {
            queued.retain(|q| !drop(q));
        }
        self.orders.retain(|_, queued| !queued.is_empty());
    }

    /// Whether an order would be accepted now, without applying it.
    pub fn check(&self, order: &Order) -> Result<(), String> {
        order.apply_among(&mut self.craft.clone(), self.system_field(), &self.ships())
    }

    /// Check submission without changing the voyage. Future orders are
    /// checked against drive state only when their step arrives.
    pub fn check_order(&self, tick: i64, order: &Order) -> Result<(), String> {
        if tick < self.craft.tick {
            return Err(format!(
                "An order for step {tick} came after step {}",
                self.craft.tick
            ));
        }
        if tick == self.craft.tick {
            return self.check(order);
        }
        if tick.saturating_sub(self.craft.tick) > MAX_ORDER_AHEAD_TICKS {
            return Err("Ship orders may be scheduled at most 300 simulated seconds ahead".into());
        }
        order.validate()?;
        if self.pending() >= MAX_PENDING_ORDERS {
            return Err("Too many pending ship orders".into());
        }
        Ok(())
    }

    /// Orders kept for later steps, soonest first.
    pub fn scheduled(&self) -> impl Iterator<Item = (i64, &Order)> {
        self.orders
            .iter()
            .flat_map(|(tick, orders)| orders.iter().map(move |q| (*tick, &q.order)))
    }

    /// Save pending inputs alongside a craft snapshot. On load, submit them
    /// through `schedule` after reconstructing the voyage from the saved craft.
    pub fn pending_orders(&self) -> Vec<ScheduledOrder> {
        self.orders
            .iter()
            .flat_map(|(tick, orders)| {
                orders.iter().map(|q| ScheduledOrder {
                    tick: *tick,
                    order: q.order.clone(),
                    ends_from: q.ends_from,
                })
            })
            .collect()
    }

    /// Apply the orders kept for the step the craft has reached.
    fn apply_due(&mut self, ships: &[Target]) -> Vec<Refused> {
        let tick = self.craft.tick;
        let Some(mut orders) = self.orders.remove(&tick) else {
            return Vec::new();
        };
        // Timed orders' ends yield to orders given for the same step.
        orders.sort_by_key(|q| q.ends_from.is_none());
        let mut refused = Vec::new();
        let in_system = !self.in_galaxy();
        for Queued { order, ends_from } in orders {
            match order.apply_among(&mut self.craft, in_system.then_some(&self.field), ships) {
                Ok(()) if ends_from.is_none() => self.replace_ends(tick, &order),
                Ok(()) => {}
                Err(reason) => {
                    if ends_from.is_none() {
                        self.drop_end(tick, &order);
                    }
                    refused.push(Refused {
                        tick,
                        order,
                        reason,
                    });
                }
            }
        }
        refused
    }

    /// Replace the system field, as when a craft is placed in it.
    pub fn set_field(&mut self, field: Field) {
        self.range_m = range_m(&field);
        self.field = field;
        self.craft.gravity = None;
    }

    pub fn in_galaxy(&self) -> bool {
        matches!(self.craft.frame, Frame::Galaxy { .. })
    }

    /// Rotation from the frame's axes into galactic axes.
    pub fn basis(&self) -> DQuat {
        if self.in_galaxy() {
            DQuat::IDENTITY
        } else {
            page_basis(self.field.system(), self.field.page())
        }
    }

    /// A position in this frame, in parsecs from the galactic centre.
    pub fn galactic_pc(&self, position: [f64; 3]) -> [f64; 3] {
        (DVec3::from_array(self.origin_pc) + self.basis() * DVec3::from_array(position) / PC_METRES)
            .to_array()
    }

    /// The frame's own velocity in galactic axes, in m/s.
    pub fn frame_velocity(&self) -> DVec3 {
        frame_velocity(&self.craft.frame, self.origin_pc)
    }

    /// Whether the craft may enter systems now.
    pub fn arriving(&self) -> bool {
        self.in_galaxy()
            && !(self.craft.warp.engaged && self.craft.warp.actual_ms >= ARRIVAL_MAX_SPEED)
    }

    /// One fixed step: any frame change due at the current state, then
    /// motion. On `Err` nothing has changed; supply the data and step again.
    pub fn step(&mut self, atlas: &impl Atlas) -> Result<Step, Wait> {
        let destination = self.destination(atlas)?;
        let rebase = self.settle(atlas)?;
        let mut step = Step {
            rebase,
            from: self.craft.state(),
            from_orientation: self.craft.orientation,
            refused: Vec::new(),
        };
        step.refused = self.advance(destination);
        Ok(step)
    }

    /// The system an active route flies to, when it is not the craft's own:
    /// where it is, then its bodies, waited for like any other data. An ID
    /// that names no system gives none, and the route ends unplotted.
    fn destination<'a>(&self, atlas: &'a impl Atlas) -> Result<Option<&'a Arrival>, Wait> {
        let Some(route) = self.craft.route.as_ref().filter(|route| route.active()) else {
            return Ok(None);
        };
        if route.system.is_empty() || !self.in_galaxy() && route.system == self.craft.frame.star() {
            return Ok(None);
        }
        if let Some(arrival) = atlas.arrival(&route.system) {
            return Ok(Some(arrival));
        }
        match atlas.locate(&route.system) {
            None => Err(Wait::Star(route.system.clone())),
            Some(None) => Ok(None),
            Some(Some(candidate)) => Err(Wait::System(candidate.clone())),
        }
    }

    /// From another system's frame into this one.
    fn rebase_from_system(&self, arrival: &Arrival) -> Rebase {
        let (system, page) = (arrival.field.system(), arrival.field.page());
        let frame = Frame::System {
            star: page_star_id(system, page),
        };
        self.rebase_from(&frame, arrival.origin_pc, page_basis(system, page))
    }

    /// From another frame into this one. The same frame gives no change.
    fn rebase_from(&self, frame: &Frame, origin_pc: [f64; 3], basis: DQuat) -> Rebase {
        let inverse = self.basis().inverse();
        Rebase {
            rotation: inverse * basis,
            translation: inverse
                * ((DVec3::from_array(origin_pc) - DVec3::from_array(self.origin_pc)) * PC_METRES),
            velocity: inverse * (frame_velocity(frame, origin_pc) - self.frame_velocity()),
            frame_changed: *frame != self.craft.frame,
        }
    }

    /// The frame change due at the current state, if any, before moving.
    /// Leaving or re-anchoring never waits; entering may wait for data.
    pub fn settle(&mut self, atlas: &impl Atlas) -> Result<Option<Rebase>, Wait> {
        let position = DVec3::from_array(self.craft.state().position);
        if !self.in_galaxy() {
            if position.length() >= LEAVE_RANGES * self.range_m {
                // Anchored where the craft is, so its local offset starts small.
                let origin = self.galactic_pc(position.to_array());
                return Ok(Some(self.rebase(
                    Frame::Galaxy { origin_pc: origin },
                    origin,
                    DQuat::IDENTITY,
                    None,
                )));
            }
            return Ok(None);
        }
        if self.arriving()
            && let Some(arrival) = self.arrival(atlas, position)?
        {
            let frame = Frame::System {
                star: page_star_id(arrival.field.system(), arrival.field.page()),
            };
            let basis = page_basis(arrival.field.system(), arrival.field.page());
            return Ok(Some(self.rebase(
                frame,
                arrival.origin_pc,
                basis,
                Some(arrival.clone()),
            )));
        }
        if position.length() > ANCHOR_M {
            let next = self.galactic_pc(position.to_array());
            return Ok(Some(self.rebase(
                Frame::Galaxy { origin_pc: next },
                next,
                DQuat::IDENTITY,
                None,
            )));
        }
        Ok(None)
    }

    /// The first candidate system whose exact range holds the craft.
    fn arrival<'a>(
        &self,
        atlas: &'a impl Atlas,
        position: DVec3,
    ) -> Result<Option<&'a Arrival>, Wait> {
        let region = Region::of(self.galactic_pc(position.to_array()));
        let candidates = atlas.region(region).ok_or(Wait::Region(region))?;
        let offset = |origin_pc: [f64; 3]| {
            ((DVec3::from_array(self.origin_pc) - DVec3::from_array(origin_pc)) * PC_METRES
                + position)
                .length()
        };
        for candidate in candidates {
            if offset(candidate.position_pc) >= candidate.reach_pc * PC_METRES {
                continue;
            }
            let arrival = atlas
                .arrival(&candidate.id)
                .ok_or_else(|| Wait::System(candidate.clone()))?;
            if offset(arrival.origin_pc) < arrival.range_m {
                return Ok(Some(arrival));
            }
        }
        Ok(None)
    }

    /// Moves the craft by one fixed step in its current frame, then applies
    /// the orders kept for the step it reaches. A route to another system
    /// sees `destination`'s target and bodies from this frame, and one to a
    /// craft sees it in `traffic`.
    pub fn advance(&mut self, destination: Option<&Arrival>) -> Vec<Refused> {
        let ships = self.ships();
        let remote = destination
            .filter(|arrival| {
                self.in_galaxy()
                    || page_star_id(arrival.field.system(), arrival.field.page())
                        != self.craft.frame.star()
            })
            .map(|arrival| Remote {
                field: &arrival.field,
                rebase: self.rebase_from_system(arrival),
            });
        if self.in_galaxy() {
            self.craft.step(&Bound {
                local: &Empty,
                remote,
                ships: &ships,
            });
        } else {
            self.craft.step(&Bound {
                local: &self.field,
                remote,
                ships: &ships,
            });
        }
        self.apply_due(&ships)
    }

    /// Re-express the craft in another frame. Position, attitude and
    /// galactic momentum are unchanged: frame velocities are converted, never
    /// matched.
    fn rebase(
        &mut self,
        frame: Frame,
        origin_pc: [f64; 3],
        basis: DQuat,
        arrival: Option<Arrival>,
    ) -> Rebase {
        let inverse = basis.inverse();
        let frame_changed = frame != self.craft.frame
            && !matches!(
                (&frame, &self.craft.frame),
                (Frame::Galaxy { .. }, Frame::Galaxy { .. })
            );
        let rebase = Rebase {
            rotation: inverse * self.basis(),
            translation: inverse
                * ((DVec3::from_array(self.origin_pc) - DVec3::from_array(origin_pc)) * PC_METRES),
            velocity: inverse * (self.frame_velocity() - frame_velocity(&frame, origin_pc)),
            frame_changed,
        };
        let state = rebase.state(self.craft.state());
        self.craft.orientation = rebase.orientation(self.craft.orientation);
        self.craft.input.thrust = rebase.direction(self.craft.input.thrust);
        if let super::AttitudePilot::Point {
            target,
            up,
            attitude,
        } = &mut self.craft.autopilot
        {
            if let super::PointTarget::Vector(vector) = target {
                *vector = rebase.direction(*vector);
            }
            *up = up.map(|vector| rebase.direction(vector));
            *attitude = rebase.orientation(*attitude);
        }
        self.craft.warp.bubble_delta_m = rebase.direction(self.craft.warp.bubble_delta_m);
        self.craft.place(state, self.craft.tick);
        self.craft.reference = None;
        self.craft.frame = frame;
        self.origin_pc = origin_pc;
        if let Some(arrival) = arrival {
            self.field = arrival.field;
            self.range_m = arrival.range_m;
        }
        rebase
    }
}

/// A frame's velocity in galactic axes, in m/s: a system's galactic velocity,
/// or zero for a galactic anchor.
pub fn frame_velocity(frame: &Frame, origin_pc: [f64; 3]) -> DVec3 {
    match frame {
        Frame::Galaxy { .. } => DVec3::ZERO,
        Frame::System { star } => {
            DVec3::from_array(crate::kinematics::system_velocity_kms(star, origin_pc)) * 1_000.0
        }
    }
}

/// The gravity a craft steps in, with the target and bodies of another
/// system its route flies to and the other craft, seen from the craft's
/// frame. They steer the route only: gravity is the local system's alone.
struct Bound<'a, G> {
    local: &'a G,
    remote: Option<Remote<'a>>,
    ships: &'a [Target],
}

struct Remote<'a> {
    field: &'a Field,
    /// From its frame into the craft's.
    rebase: Rebase,
}

impl<G: Gravity> Gravity for Bound<'_, G> {
    fn acceleration(&self, position: [f64; 3], seconds: f64) -> [f64; 3] {
        self.local.acceleration(position, seconds)
    }

    fn reference(&self, id: Option<&str>, position: [f64; 3], seconds: f64) -> Option<State> {
        self.local.reference(id, position, seconds)
    }

    fn orbit_reference(
        &self,
        current: Option<&str>,
        position: [f64; 3],
        seconds: f64,
    ) -> Option<super::Reference> {
        self.local.orbit_reference(current, position, seconds)
    }

    fn closest(&self, position: [f64; 3], seconds: f64) -> Option<super::Closest> {
        self.local.closest(position, seconds)
    }

    fn target(&self, id: &str, seconds: f64) -> Option<Target> {
        if let Some(ship) = self.ships.iter().find(|ship| ship.id == id) {
            return Some(ship.clone());
        }
        self.local.target(id, seconds).or_else(|| {
            let remote = self.remote.as_ref()?;
            let mut target = remote.field.target(id, seconds)?;
            target.state = remote.rebase.state(target.state);
            Some(target)
        })
    }

    fn obstacles(&self, seconds: f64) -> Vec<super::route::Obstacle> {
        let mut obstacles = self.local.obstacles(seconds);
        if let Some(remote) = &self.remote {
            obstacles.extend(
                remote
                    .field
                    .obstacles(seconds)
                    .into_iter()
                    .map(|mut obstacle| {
                        let state = remote.rebase.state(State {
                            position: obstacle.centre.to_array(),
                            velocity: obstacle.velocity.to_array(),
                        });
                        obstacle.centre = DVec3::from_array(state.position);
                        obstacle.velocity = DVec3::from_array(state.velocity);
                        obstacle
                    }),
            );
        }
        obstacles
    }
}

/// No gravity: galactic space has no local system.
pub struct Empty;

impl Gravity for Empty {
    fn acceleration(&self, _: [f64; 3], _: f64) -> [f64; 3] {
        [0.0; 3]
    }
}

#[cfg(test)]
mod tests;
