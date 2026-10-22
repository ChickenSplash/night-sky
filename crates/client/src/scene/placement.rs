//! Places the 3D camera each frame from `Navigation` and the rig's flight.
use super::{
    camera::{CameraPose, CameraRig, SpaceCamera},
    frames::{page_frame, page_origin_ly},
};
use crate::flight::{Flight, Pose, STAR_JUMP_SECONDS};
use crate::presentation::PresentationMode;
use crate::{Clock, Navigation, Universe};
use bevy::{
    math::{DQuat, DVec3},
    prelude::*,
};
use big_space::prelude::*;
use space_simulation::{
    LY_METRES,
    galaxy::{GalaxyModel, PC_METRES},
    planet_position, planet_state,
};

/// Keep nearby streamed points well clear of the camera's near clip plane even
/// when the selected camera range is interstellar.
const PRECISE_SCALE_PC: f64 = 20.0 * LY_METRES / PC_METRES;

#[allow(clippy::too_many_arguments)]
pub fn place_camera(
    time: Res<Time>,
    universe: Res<Universe>,
    field: Res<crate::stars::stream::Starfield>,
    mut metrics: ResMut<crate::stars::stream::StreamMetrics>,
    clock: Res<Clock>,
    mut nav: ResMut<Navigation>,
    mut rig: ResMut<CameraRig>,
    mut pose: ResMut<CameraPose>,
    fleet: Res<crate::craft::Fleet>,
    grids: Query<&Grid>,
    mut camera: Query<
        (
            &mut Transform,
            &mut CellCoord,
            &ChildOf,
            &mut Projection,
            &mut Camera,
        ),
        With<SpaceCamera>,
    >,
) {
    // Following changes the camera's coordinate frame, never the craft's.
    // Re-express orientation without starting a navigation flight at a handoff.
    // A craft in a system brings the camera there; one in galactic space
    // leaves arriving and leaving to the camera's own proximity (`locate`).
    if nav.following
        && let Some(id) = nav.selected_craft()
        && let Some(live) = fleet.get(id)
        && !live.star().is_empty()
    {
        let old_basis = nav
            .system(&universe.0)
            .map_or(DQuat::IDENTITY, |s| page_frame(s, nav.page));
        let same = nav
            .system(&universe.0)
            .is_some_and(|s| crate::objects::star_id(s, nav.page) == live.star());
        if !same {
            nav.rotation =
                ((live.basis().inverse() * old_basis).as_quat() * nav.rotation).normalize();
            nav.visit = Some(crate::Visit::Generated(Box::new(
                live.field.system().clone(),
            )));
            nav.page = live.field.page();
            nav.arrived = true;
        }
    }
    nav.fit_preset(&universe.0);
    if nav.fly || nav.following || nav.presentation_mode != PresentationMode::Simulation {
        nav.eye = None;
    }
    let fly = std::mem::take(&mut nav.fly);
    let system = nav.system(&universe.0);
    // The constellation uses one page's frame for the whole list; page selection
    // does not move the camera. Other views use the selected page's plane. In
    // open space the camera is relative to home, in galactic axes.
    let frame_page = system.map_or(0, |system| nav.frame_page(system));
    let frame_tilt = system.map_or(DQuat::IDENTITY, |system| page_frame(system, frame_page));
    let frame = system.map_or_else(
        || {
            super::location::galactic_frame_ly(&nav, &fleet)
                .unwrap_or_else(|| super::location::open_space_frame_ly(&nav, &rig))
        },
        |system| page_origin_ly(system, frame_page),
    );
    // Move with the anchor: by its motion since the last frame. Its frame
    // may move with it in galactic space; its offset is kept either way.
    let anchored = nav
        .anchor
        .clone()
        .filter(|_| nav.presentation_mode == PresentationMode::Simulation)
        .and_then(|anchor| {
            // Velocity in galactic axes, per simulated second.
            let (at, velocity) = match &anchor {
                crate::Anchor::Body(id) => {
                    let system = system?;
                    let state = match system.planets.iter().find(|p| &p.id == id) {
                        Some(planet) => space_simulation::State {
                            position: planet_position(system, planet, clock.0.seconds),
                            ..planet_state(system, planet, clock.0.seconds)
                        },
                        None => {
                            space_simulation::member_state(system, frame_page, id, clock.0.seconds)
                        }
                    };
                    (
                        DVec3::from_array(state.position),
                        frame_tilt * DVec3::from_array(state.velocity),
                    )
                }
                crate::Anchor::Craft(id) => {
                    let live = fleet.get(id)?;
                    let state = live.state_at(clock.0.seconds);
                    (
                        in_camera_frame(live, state, frame, frame_tilt),
                        live.basis() * live.display_velocity(),
                    )
                }
            };
            Some((anchor, frame, at, velocity))
        });
    let galactic = system.is_none() && rig.galactic && rig.placed();
    let carried = anchored
        .as_ref()
        .zip(rig.carried.as_ref())
        .filter(|((anchor, frame, ..), (was, was_frame, _))| {
            anchor == was
                && (frame == was_frame || galactic && matches!(anchor, crate::Anchor::Craft(_)))
        })
        .map(|((.., at, velocity), (.., was_at))| (*at - *was_at, *velocity));
    let mut carried_velocity = None;
    let carry = match carried {
        Some((moved, velocity)) if !nav.following && !fly && rig.flight.is_none() => {
            carried_velocity = Some(velocity);
            moved
        }
        // Otherwise a galactic frame change keeps the eye where it was,
        // from the frame `Navigation` is in. Leaving or entering a system
        // re-expresses it already (`locate`).
        _ if galactic => (nav.galactic_origin_ly - frame) * LY_METRES,
        _ => DVec3::ZERO,
    };
    let open_space = system.is_none();
    rig.carried = anchored.map(|(anchor, frame, at, _)| (anchor, frame, at));
    let craft = crate::craft::followed(&nav, &universe.0, &fleet, clock.0.seconds);
    let followed = nav
        .following
        .then(|| {
            craft
                .zip(nav.selected_craft().and_then(|id| fleet.get(id)))
                .map(|(state, live)| in_camera_frame(live, state, frame, frame_tilt))
                .or_else(|| {
                    system.and_then(|system| {
                        let planet = system.planets.get(nav.selected_planet(&universe.0)?)?;
                        let state = planet_state(system, planet, clock.0.seconds);
                        carried_velocity = Some(frame_tilt * DVec3::from_array(state.velocity));
                        Some(DVec3::from_array(planet_position(
                            system,
                            planet,
                            clock.0.seconds,
                        )))
                    })
                })
        })
        .flatten();
    let desired = Pose {
        target: frame_tilt * followed.unwrap_or(nav.target + carry),
        distance: nav.distance,
        rotation: frame_tilt.as_quat() * nav.rotation,
        layout: f64::from(u8::from(nav.presentation_mode == PresentationMode::Table)),
    };
    let mut shown = rig.shown.unwrap_or(desired);
    let frame_shift_ly = (frame - rig.frame_ly).length();
    if rig.shown.is_some() && frame != rig.frame_ly {
        // Re-express the camera relative to the new star, so jumps start in place.
        let offset = (rig.frame_ly - frame) * LY_METRES;
        shown.target += offset;
        if let Some(flight) = &mut rig.flight {
            flight.shift(offset);
        }
    }
    let retarget = nav.following && nav.selected_planet(&universe.0) != rig.selected;
    let star_jump = rig.shown.is_some() && frame != rig.frame_ly;
    rig.frame_ly = frame;
    rig.galactic = system.is_none();
    rig.selected = nav.selected_planet(&universe.0);
    // Page jumps, list jumps, task focus and preset changes all use this flight.
    if fly || retarget {
        rig.flight = Flight::new(shown, desired);
        // Every jump to another star, in this list or another, keeps one rhythm
        // whatever the distance travelled.
        if star_jump && let Some(flight) = &mut rig.flight {
            flight.set_duration(STAR_JUMP_SECONDS);
        }
        rig.warned_stall = false;
        if let Some(flight) = &rig.flight {
            info!(
                "Flight started: list={} page={} presentation_mode={:?} star travel={:.3} ly duration={:.3} s",
                system.map_or("galaxy", |s| s.id.as_str()),
                nav.page + 1,
                nav.presentation_mode,
                frame_shift_ly,
                flight.duration_seconds(),
            );
        }
    }
    if rig.flight.is_some() && time.delta_secs_f64() > 0.12 && !rig.warned_stall {
        warn!(
            "Flight frame delayed by {:.0} ms (list={}, page={})",
            time.delta_secs_f64() * 1000.0,
            system.map_or("galaxy", |s| s.id.as_str()),
            nav.page + 1,
        );
        rig.warned_stall = true;
    }
    shown = match &mut rig.flight {
        Some(flight) => {
            flight.advance(time.delta_secs_f64());
            if flight.finished() {
                desired
            } else {
                flight.sample(desired)
            }
        }
        None => desired,
    };
    if rig.flight.as_ref().is_some_and(Flight::finished) {
        info!(
            "Flight landed: list={} page={} presentation_mode={:?}",
            system.map_or("galaxy", |s| s.id.as_str()),
            nav.page + 1,
            nav.presentation_mode
        );
        rig.flight = None;
    }
    if let Some(followed) = followed {
        nav.target = followed;
    } else {
        nav.target += carry;
        if let Some(eye) = &mut nav.eye {
            *eye += carry;
        }
    }
    if open_space {
        nav.galactic_origin_ly = frame;
    }
    rig.shown = Some(shown);
    pose.page = frame_page;
    pose.carried_velocity = carried_velocity.filter(|_| rig.flight.is_none());

    let rotation = shown.rotation;
    let eye_offset = (rotation * Vec3::Z).as_dvec3() * shown.distance;
    pose.followed = if rig.flight.is_none() && nav.following {
        nav.selected_craft().map(|id| super::camera::FollowPose {
            id: id.to_owned(),
            eye_offset,
        })
    } else {
        None
    };
    pose.eye = if rig.flight.is_none() {
        nav.eye
            .map_or_else(|| shown.target + eye_offset, |eye| frame_tilt * eye)
    } else {
        shown.target + eye_offset
    };
    pose.distance = shown.distance;
    // The camera range is only a framing control. A streamed point can need
    // camera-relative precision well before its planets/body are activated.
    let eye_pc = DVec3::from_array(GalaxyModel::milky_way().home_pc)
        + frame * (LY_METRES / PC_METRES)
        + pose.eye / PC_METRES;
    pose.scale_distance = proximity_scale_distance(&field, eye_pc, shown.distance, &mut metrics);
    if nav.arrived {
        pose.scale_distance = pose
            .scale_distance
            .min(pose.eye.length().max(super::scale_floor(shown.distance)));
    }
    pose.layout = shown.layout;
    let Ok((mut transform, mut cell, parent, mut projection, mut camera)) = camera.single_mut()
    else {
        return;
    };
    camera.viewport = nav.viewport.clone();
    let Ok(grid) = grids.get(parent.parent()) else {
        return;
    };
    let absolute = frame * LY_METRES + pose.eye;
    let (new_cell, offset) = grid.translation_to_grid(absolute);
    *cell = new_cell;
    transform.translation = offset;
    transform.rotation = rotation;
    if let Projection::Perspective(p) = &mut *projection {
        // Render units, see `CameraPose::render_scale`.
        p.near = 0.01;
        p.fov = nav.fov_deg.to_radians();
    }
}

pub(super) fn proximity_scale_distance(
    field: &crate::stars::stream::Starfield,
    eye_pc: DVec3,
    selected_range: f64,
    metrics: &mut crate::stars::stream::StreamMetrics,
) -> f64 {
    field
        .nearest(eye_pc, PRECISE_SCALE_PC, metrics)
        .map_or(selected_range, |(distance, ..)| {
            selected_range.min(distance * PC_METRES)
        })
        .max(super::scale_floor(selected_range))
}

/// A followed craft's position in the camera's frame: from `frame_ly`, in the
/// axes `tilt` turns into galactic ones. Its own frame needs no conversion.
pub(super) fn in_camera_frame(
    live: &crate::craft::Live,
    state: space_simulation::State,
    frame_ly: DVec3,
    tilt: DQuat,
) -> DVec3 {
    let position = DVec3::from_array(state.position);
    let (origin, basis) = (live.origin_ly(), live.basis());
    if origin == frame_ly && basis == tilt {
        return position;
    }
    tilt.inverse() * ((origin - frame_ly) * LY_METRES + basis * position)
}
