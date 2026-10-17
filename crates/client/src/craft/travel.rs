//! Unsafe manual travel and demand-driven frame handoffs. Reuse loaded star
//! positions; generate bodies off-thread only when approaching a candidate.
use super::*;
use bevy::math::{DQuat, DVec3};
use space_simulation::{
    AU_METRES, LY_METRES,
    craft::Gravity,
    galaxy::{GalaxyModel, PC_METRES},
};

// Local precision threshold only. No search is restarted at this distance.
const REACH_M: f64 = 0.25 * LY_METRES;

#[derive(Default)]
pub struct NearbySystems {
    pending: Option<ArrivalLoad>,
    attempted: Option<String>,
    fields: Vec<Known>,
    pub ready: bool,
    pub error: Option<String>,
}

struct ArrivalLoad {
    id: String,
    origin_pc: [f64; 3],
    job: super::loading::Pending,
}

struct Known {
    field: Field,
    origin_pc: [f64; 3],
}

impl Live {
    pub fn origin_pc(&self) -> [f64; 3] {
        match self.craft.frame {
            Frame::Galaxy { origin_pc } => origin_pc,
            Frame::System { .. } => self.system_pc,
        }
    }

    pub fn basis(&self) -> DQuat {
        match self.craft.frame {
            Frame::Galaxy { .. } => DQuat::IDENTITY,
            Frame::System { .. } => {
                crate::scene::page_frame(self.field.system(), self.field.page())
            }
        }
    }

    pub fn position_pc(&self, seconds: f64) -> [f64; 3] {
        (DVec3::from_array(self.origin_pc())
            + self.basis() * DVec3::from_array(self.state_at(seconds).position) / PC_METRES)
            .to_array()
    }

    pub fn origin_ly(&self) -> DVec3 {
        if matches!(self.craft.frame, Frame::System { .. }) {
            return crate::scene::page_origin_ly(self.field.system(), self.field.page());
        }
        (DVec3::from_array(self.origin_pc()) - DVec3::from_array(GalaxyModel::milky_way().home_pc))
            * (PC_METRES / LY_METRES)
    }

    pub fn nearby_status(&self) -> &str {
        if let Some(error) = &self.nearby.error {
            error
        } else if self.nearby.ready {
            "loaded-system proximity checks only"
        } else {
            "loading approached system in background (warp unrestricted)"
        }
    }

    pub fn prepare_nearby(&mut self, galaxy: &Galaxy) {
        // Keep galactic offsets local too. Re-anchor by subtracting a nearby
        // representable origin, retaining the residual in metres rather than
        // replacing the craft position with rounded galactocentric parsecs.
        if let Frame::Galaxy { origin_pc } = self.craft.frame
            && DVec3::from_array(self.craft.state().position).length() > REACH_M / 2.0
        {
            let next = self.position_pc(self.craft.seconds);
            let shift = (DVec3::from_array(next) - DVec3::from_array(origin_pc)) * PC_METRES;
            let mut state = self.craft.state();
            state.position = (DVec3::from_array(state.position) - shift).to_array();
            self.previous.position = (DVec3::from_array(self.previous.position) - shift).to_array();
            if let Some(displayed) = &mut self.displayed {
                displayed.position = (DVec3::from_array(displayed.position) - shift).to_array();
            }
            self.craft.place(state, self.craft.seconds);
            self.craft.frame = Frame::Galaxy { origin_pc: next };
            self.system_pc = next;
        }
        if let Some(result) = self.nearby.pending.as_ref().and_then(|p| p.job.poll()) {
            let pending = self.nearby.pending.take().unwrap();
            if let Some(system) = result {
                let known = Known {
                    field: Field::new(system, 0),
                    origin_pc: pending.origin_pc,
                };
                // Results cannot change the frame after the craft has flown on.
                if self.inside(&known) {
                    self.remember(known);
                }
            } else {
                self.nearby.error = Some(format!(
                    "Body loading failed for {} (warp unrestricted)",
                    pending.id
                ));
            }
        }
        self.nearby.ready = self.nearby.pending.is_none();
        if !self.needs_arrival() {
            return;
        }
        // Lists are already loaded. No procedural census or body generation.
        let here = self.origin_ly()
            + self.basis() * DVec3::from_array(self.craft.state().position) / LY_METRES;
        for system in &galaxy.systems {
            for page in system.pages() {
                let origin = crate::scene::page_origin_ly(system, page);
                if (origin - here).length() * LY_METRES < system_range(system, page) {
                    let known = Known {
                        field: Field::new(system.clone(), page),
                        origin_pc: (origin * (LY_METRES / PC_METRES)
                            + DVec3::from_array(GalaxyModel::milky_way().home_pc))
                        .to_array(),
                    };
                    self.remember(known);
                    return;
                }
            }
        }
    }

    fn inside(&self, known: &Known) -> bool {
        let delta = (DVec3::from_array(self.origin_pc()) - DVec3::from_array(known.origin_pc))
            * PC_METRES
            + self.basis() * DVec3::from_array(self.craft.state().position);
        delta.length() < range(&known.field)
    }

    fn needs_arrival(&self) -> bool {
        (self.star().is_empty()
            || DVec3::from_array(self.craft.state().position).length() >= 2.0 * range(&self.field))
            && !self.nearby.fields.iter().any(|known| self.inside(known))
    }

    fn remember(&mut self, known: Known) {
        // A bounded two-entry arrival cache, not an accumulating neighbourhood.
        if self.nearby.fields.len() >= 2 {
            self.nearby.fields.remove(0);
        }
        self.nearby.fields.push(known);
    }

    pub fn prepare_streamed(
        &mut self,
        stars: &crate::stars::stream::Starfield,
        metrics: &mut crate::stars::stream::StreamMetrics,
    ) {
        if !self.needs_arrival() || self.nearby.pending.is_some() {
            return;
        }
        let here = DVec3::from_array(self.position_pc(self.craft.seconds));
        let Some((_, cell, star)) = stars.nearest(here, 0.02 * LY_METRES / PC_METRES, metrics)
        else {
            self.nearby.attempted = None;
            return;
        };
        self.request_arrival(star.id(cell), star.position_pc);
    }

    fn request_arrival(&mut self, id: String, origin_pc: [f64; 3]) {
        if self.nearby.pending.is_some() || self.nearby.attempted.as_ref() == Some(&id) {
            return;
        }
        self.nearby.attempted = Some(id.clone());
        self.nearby.error = None;
        let ly = ((DVec3::from_array(origin_pc)
            - DVec3::from_array(GalaxyModel::milky_way().home_pc))
            * (PC_METRES / LY_METRES))
            .to_array();
        if let Some(job) = super::loading::Pending::start(id.clone(), ly) {
            self.nearby.pending = Some(ArrivalLoad { id, origin_pc, job });
            self.nearby.ready = false;
        } else {
            self.nearby.error = Some("Cannot start body loader (warp unrestricted)".into());
        }
    }

    pub fn travel_step(&mut self) {
        if !self.craft.warp.engaged {
            if matches!(self.craft.frame, Frame::Galaxy { .. }) {
                self.craft.step(&Empty);
            } else {
                self.craft.step(&self.field);
            }
            self.handoff();
            return;
        }
        let (candidate, speed, bubble) = if self.star().is_empty() {
            self.craft.warp_candidate(&Empty)
        } else {
            self.craft.warp_candidate(&self.field)
        };
        // Manual warp intentionally permits body crossings and unknown space.
        self.craft.commit_warp(candidate, speed, bubble);
        self.forecast = Forecast::default();
        self.handoff();
    }

    fn handoff(&mut self) {
        let origin = DVec3::from_array(self.origin_pc());
        let old_basis = self.basis();
        let state = self.craft.state();
        let here = old_basis * DVec3::from_array(state.position);
        let current = self.star().to_owned();
        let inside_current = !current.is_empty() && here.length() < 2.0 * range(&self.field);
        if inside_current {
            return;
        }
        let target = self.nearby.fields.iter().find(|known| {
            let delta = (origin - DVec3::from_array(known.origin_pc)) * PC_METRES + here;
            delta.length() < range(&known.field)
        });
        let (frame, new_origin, new_basis, new_field) = match target {
            Some(known) => (
                Frame::System {
                    star: objects::star_id(known.field.system(), known.field.page()),
                },
                DVec3::from_array(known.origin_pc),
                crate::scene::page_frame(known.field.system(), known.field.page()),
                Some(known.field.clone()),
            ),
            None if current.is_empty() => return,
            None => (
                Frame::Galaxy {
                    origin_pc: origin.to_array(),
                },
                origin,
                DQuat::IDENTITY,
                None,
            ),
        };
        let old_velocity = frame_velocity(&self.craft.frame, self.system_pc);
        let new_velocity = frame_velocity(&frame, new_origin.to_array());
        let rotation = new_basis.inverse() * old_basis;
        let transform = |s: State| State {
            position: (new_basis.inverse()
                * ((origin - new_origin) * PC_METRES + old_basis * DVec3::from_array(s.position)))
            .to_array(),
            velocity: (new_basis.inverse()
                * (old_basis * DVec3::from_array(s.velocity) + old_velocity - new_velocity))
                .to_array(),
        };
        self.previous = transform(self.previous);
        self.displayed = self.displayed.map(transform);
        self.previous_orientation =
            (rotation * DQuat::from_array(self.previous_orientation)).to_array();
        self.craft.orientation = (rotation * DQuat::from_array(self.craft.orientation)).to_array();
        self.craft.warp.bubble_delta_m =
            (rotation * DVec3::from_array(self.craft.warp.bubble_delta_m)).to_array();
        self.craft.place(transform(state), self.craft.seconds);
        self.craft.reference = None;
        self.craft.frame = frame;
        self.system_pc = new_origin.to_array();
        if let Some(field) = new_field {
            self.field = field;
        }
        self.forecast = Forecast::default();
    }
}

fn range(field: &Field) -> f64 {
    system_range(field.system(), field.page())
}

fn system_range(system: &StarSystem, page: u32) -> f64 {
    (0.02 * LY_METRES).max(4.0 * space_simulation::system_extent_au(system, page) * AU_METRES)
}

fn frame_velocity(frame: &Frame, pc: [f64; 3]) -> DVec3 {
    match frame {
        Frame::Galaxy { .. } => DVec3::ZERO,
        Frame::System { star } => {
            DVec3::from_array(space_simulation::kinematics::system_velocity_kms(star, pc)) * 1_000.0
        }
    }
}

struct Empty;
impl Gravity for Empty {
    fn acceleration(&self, _: [f64; 3], _: f64) -> [f64; 3] {
        [0.0; 3]
    }
}

#[cfg(test)]
mod tests;
