//! Which frame a craft flies in, and the handoffs between them, decided from
//! its own state and seeded or shared system data alone. Wherever it runs and
//! whatever a camera has loaded, the same inputs give the same path: when the
//! data a decision needs is missing, the step waits for it rather than
//! carrying on without it.
use super::{Craft, Field, Frame, Gravity, warp::Cutoff};
use crate::galaxy::PC_METRES;
use crate::neighbourhood::{Candidate, Region};
use crate::{LY_METRES, State};
use glam::{DQuat, DVec3};

/// Fastest warp at which a craft enters systems. Below it a star's pull on a
/// passing craft can matter; above it a craft crosses a system's range in a
/// few steps, so it stays in galactic space and needs no system data.
pub const ARRIVAL_MAX_SPEED: f64 = 299_792_458.0;
/// Galactic offsets beyond this move the frame's anchor, keeping local metres
/// precise. Only precision: it decides nothing else.
pub const ANCHOR_M: f64 = 0.125 * LY_METRES;
/// A system is left at this many times its entry range, so a craft on its
/// edge does not switch back and forth.
pub const LEAVE_RANGES: f64 = 2.0;

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

/// Where a voyage's systems come from. A server answers from seeds and its
/// lists at once; a client from caches it fills in the background.
pub trait Atlas {
    /// Systems that may reach into a region, or `None` until known.
    fn region(&self, region: Region) -> Option<&[Candidate]>;
    /// A candidate system's bodies, or `None` until loaded.
    fn arrival(&self, star: &str) -> Option<&Arrival>;
}

/// An atlas that knows nothing yet, for steps that need no system data.
pub struct Blank;

impl Atlas for Blank {
    fn region(&self, _: Region) -> Option<&[Candidate]> {
        None
    }
    fn arrival(&self, _: &str) -> Option<&Arrival> {
        None
    }
}

/// An atlas that works out what a step waits for there and then, as a
/// server or test would. Charted (list) systems are added with `chart`.
pub struct Library {
    sky: crate::cells::Sky,
    regions: std::collections::HashMap<Region, Vec<Candidate>>,
    arrivals: std::collections::HashMap<String, Arrival>,
    threads: usize,
}

impl Default for Library {
    fn default() -> Self {
        Self {
            sky: crate::cells::Sky::default(),
            regions: Default::default(),
            arrivals: Default::default(),
            threads: threads(),
        }
    }
}

impl Library {
    /// Work out what a step waited for.
    pub fn supply(&mut self, wait: &Wait) {
        let reply = Request::of(wait).answer(&mut self.sky, self.threads);
        self.accept(wait, reply);
    }

    /// Store a worked-out reply to a wait.
    pub fn accept(&mut self, wait: &Wait, reply: Reply) {
        match (wait, reply) {
            (Wait::Region(region), Reply::Region { candidates }) => {
                self.regions.insert(*region, candidates);
            }
            (Wait::System(candidate), Reply::System { system }) => {
                let arrival = seeded_arrival(candidate, system);
                self.arrivals.insert(candidate.id.clone(), arrival);
            }
            _ => {}
        }
    }

    /// A list system's page, which takes the place of a generated system of
    /// the same star, or adds a legacy page with no cell star.
    pub fn chart(&mut self, arrival: Arrival) {
        let id = page_star_id(arrival.field.system(), arrival.field.page());
        self.arrivals.insert(id, arrival);
    }

    /// Step, supplying whatever the step waits for first.
    pub fn step(&mut self, voyage: &mut Voyage) -> Option<Rebase> {
        loop {
            match voyage.step(self) {
                Ok(rebase) => return rebase,
                Err(wait) => self.supply(&wait),
            }
        }
    }
}

impl Atlas for Library {
    fn region(&self, region: Region) -> Option<&[Candidate]> {
        self.regions.get(&region).map(Vec::as_slice)
    }
    fn arrival(&self, star: &str) -> Option<&Arrival> {
        self.arrivals.get(star)
    }
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
}

impl Request {
    pub fn of(wait: &Wait) -> Self {
        match wait {
            Wait::Region(region) => Self::Region { region: *region },
            Wait::System(candidate) => Self::System {
                id: candidate.id.clone(),
                position_ly: candidate_ly(candidate),
            },
        }
    }

    /// Work it out here and now, with a warm `sky` for speed.
    pub fn answer(&self, sky: &mut crate::cells::Sky, threads: usize) -> Reply {
        match self {
            Self::Region { region } => Reply::Region {
                candidates: crate::neighbourhood::candidates(sky, *region, threads),
            },
            Self::System { id, position_ly } => Reply::System {
                system: crate::generated_system(id, *position_ly),
            },
        }
    }
}

/// What a step needs before it can be taken.
#[derive(Clone, Debug, PartialEq)]
pub enum Wait {
    Region(Region),
    System(Candidate),
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
}

impl Voyage {
    pub fn new(craft: Craft, field: Field, origin_pc: [f64; 3]) -> Self {
        Self {
            range_m: range_m(&field),
            craft,
            field,
            origin_pc,
        }
    }

    /// Replace the system field, as when a craft is placed in it.
    pub fn set_field(&mut self, field: Field) {
        self.range_m = range_m(&field);
        self.field = field;
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
    pub fn step(&mut self, atlas: &impl Atlas) -> Result<Option<Rebase>, Wait> {
        let rebase = self.settle(atlas)?;
        self.advance();
        Ok(rebase)
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

    /// Moves the craft by one fixed step in its current frame.
    pub fn advance(&mut self) {
        if self.in_galaxy() {
            self.craft.step(&Empty);
        } else {
            self.craft.step(&self.field);
        }
    }

    /// Manual dropout inside the step that brackets the clock. See
    /// `Craft::truncate_warp_segment`.
    pub fn truncate_warp(&mut self, from: State, from_seconds: f64, at: f64) -> Option<Cutoff> {
        if self.in_galaxy() {
            self.craft
                .truncate_warp_segment(from, from_seconds, at, &Empty)
        } else {
            self.craft
                .truncate_warp_segment(from, from_seconds, at, &self.field)
        }
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
        self.craft.warp.bubble_delta_m = rebase.direction(self.craft.warp.bubble_delta_m);
        self.craft.place(state, self.craft.seconds);
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

/// No gravity: galactic space has no local system.
pub struct Empty;

impl Gravity for Empty {
    fn acceleration(&self, _: [f64; 3], _: f64) -> [f64; 3] {
        [0.0; 3]
    }
}

#[cfg(test)]
mod tests;
