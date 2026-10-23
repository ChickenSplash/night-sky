//! Where the camera physically is: arriving at and leaving systems, and
//! the nearby body.
use super::{
    camera::{CameraPose, CameraRig},
    frames::{page_frame, page_origin_ly},
};
use crate::presentation::PresentationMode;
use crate::{Clock, Navigation, Universe};
use bevy::{
    math::{DQuat, DVec3},
    prelude::*,
};
use space_shared::StarSystem;
use space_simulation::{
    LY_METRES,
    galaxy::{GalaxyModel, PC_METRES},
};

/// Beyond this camera range stars draw at their true separation. Planet
/// visibility also depends on the actual size of the current page's system.
pub(super) const WIDE_METRES: f64 = 0.02 * LY_METRES;
/// Free flight this close to a loaded star makes it the visited system.
pub(super) const APPROACH_METRES: f64 = 0.02 * LY_METRES;
/// The camera leaves a system beyond this multiple of its `system_range`, so
/// the boundary cannot flicker.
pub(super) const LEAVE_FACTOR: f64 = 2.0;
/// A craft's neighbourhood: the camera moves with it within this range,
/// kept until `SWITCH_MARGIN` past it, as bodies are.
pub const CRAFT_NEIGHBOURHOOD_M: f64 = 1_000_000.0;
/// The camera is at a planet within at least this many of its radii, even
/// where its Hill sphere is smaller, so following (12 radii) is always inside.
pub(super) const PLANET_MIN_RADII: f64 = 30.0;

/// Within this range of its star the camera arrives at a system. Even unusually
/// wide systems include their initial fitted range and an outer planet being
/// followed. Small systems keep the original cutoff.
pub(super) fn system_range(system: &StarSystem, page: u32) -> f64 {
    let extent = space_simulation::system_extent_au(system, page);
    WIDE_METRES.max(4.0 * extent * space_simulation::AU_METRES)
}

/// While the camera is at the visited system, its planets and body are drawn
/// and this is how far from its star, in metres, they can be. Elsewhere the
/// star is a galactic point like any other, bound by the same magnitude limit,
/// and the planets are hidden.
pub fn body_range(nav: &Navigation, system: &StarSystem, pose: &CameraPose) -> Option<f64> {
    nav.arrived
        .then(|| LEAVE_FACTOR * system_range(system, pose.page))
}

fn rebase(
    nav: &mut Navigation,
    rig: &CameraRig,
    displayed_eye: DVec3,
    lists: &space_shared::Galaxy,
    next: Option<crate::Visit>,
    page: u32,
    galactic_ly: DVec3,
) {
    let old_tilt = nav
        .system(lists)
        .map_or(DQuat::IDENTITY, |s| page_frame(s, nav.frame_page(s)));
    let old_frame = rig.frame_ly;
    let new_system = match &next {
        Some(crate::Visit::List(index)) => lists.systems.get(*index),
        Some(crate::Visit::Generated(system)) => Some(system.as_ref()),
        None => None,
    };
    let new_tilt = new_system.map_or(DQuat::IDENTITY, |s| page_frame(s, page));
    let new_frame = new_system.map_or(galactic_ly, |s| page_origin_ly(s, page));
    if new_system.is_none() {
        nav.galactic_origin_ly = galactic_ly;
    }
    let old_eye = nav
        .eye
        .unwrap_or_else(|| old_tilt.inverse() * displayed_eye);
    let new_eye = new_tilt.inverse() * ((old_frame - new_frame) * LY_METRES + old_tilt * old_eye);
    nav.rotation = ((new_tilt.inverse() * old_tilt).as_quat() * nav.rotation).normalize();
    // Quat is f32: changing its basis can rotate the distance-sized camera
    // offset slightly. Reconstruct the target from the eye, not vice versa.
    nav.target = new_eye - (nav.rotation * Vec3::Z).as_dvec3() * nav.distance;
    nav.eye = Some(new_eye);
    nav.visit = next;
    nav.page = page;
    nav.location_body = None;
    nav.following = false;
}

/// Whether the camera is at a system `distance` metres from its star: within
/// `range` to arrive, and until `LEAVE_FACTOR` times it once there.
pub(super) fn within(arrived: bool, distance: f64, range: f64) -> bool {
    distance < if arrived { LEAVE_FACTOR * range } else { range }
}

/// Track where the camera physically is (`Navigation::arrived`). Arriving by
/// free flight uses the same star body and planetary system as clicking Visit;
/// only cells already streamed in are eligible, and it never starts a flight or
/// re-frames the camera. Leaving a system in simulation mode, however it was
/// reached, returns to open space without moving the eye. Following a craft in
/// galactic space arrives and leaves the same way, so a system comes into view
/// as the camera nears it, not when the craft slows enough to enter it; a
/// craft in a system brings the camera there itself (`place_camera`).
#[allow(clippy::too_many_arguments)]
pub fn locate(
    field: Res<crate::stars::stream::Starfield>,
    mut metrics: ResMut<crate::stars::stream::StreamMetrics>,
    lists: Res<Universe>,
    rig: Res<CameraRig>,
    pose: Res<CameraPose>,
    fleet: Res<crate::craft::Fleet>,
    mut nav: ResMut<Navigation>,
) {
    let eye_ly = rig.galactic_ly(&pose);
    let arrived = nav.arrived;
    // The constellation frames a whole list from afar, so it is never at a star.
    nav.arrived = rig.placed()
        && nav.presentation_mode != PresentationMode::Constellation
        && nav.system(&lists.0).is_some_and(|s| {
            let distance = (page_origin_ly(s, nav.page) - eye_ly).length() * LY_METRES;
            within(arrived, distance, system_range(s, nav.page))
        });
    let craft = nav
        .following
        .then(|| nav.selected_craft().and_then(|id| fleet.get(id)))
        .flatten();
    // Flights and presets are deliberate: wait until they land.
    if nav.presentation_mode != PresentationMode::Simulation
        || craft.is_some_and(|live| !live.star().is_empty())
        || rig.flying()
        || nav.fly
        || !rig.placed()
        || nav.arrived
    {
        return;
    }
    let following = craft.is_some();
    if nav.system(&lists.0).is_some() {
        // Stay in the system's frame for now: the eye is near it.
        let galactic_ly = galactic_frame_ly(&nav, &fleet).unwrap_or(rig.frame_ly);
        rebase(&mut nav, &rig, pose.eye, &lists.0, None, 0, galactic_ly);
        nav.following = following;
        // Let place_camera switch the displayed frame before another handoff.
        return;
    }
    let eye_pc =
        DVec3::from_array(GalaxyModel::milky_way().home_pc) + eye_ly * (LY_METRES / PC_METRES);
    let reach_pc = APPROACH_METRES / PC_METRES;
    let Some((_, cell, star)) = field.nearest(eye_pc, reach_pc, &mut metrics) else {
        return;
    };
    let (id, position_pc) = (star.id(cell), star.position_pc);
    let position_ly = (DVec3::from_array(position_pc)
        - DVec3::from_array(GalaxyModel::milky_way().home_pc))
        * (PC_METRES / LY_METRES);
    let (visit, page) = if let Some((list, page)) = crate::galaxy_nav::mapped(&lists.0, &id) {
        (crate::Visit::List(list), page)
    } else if let Some(system) = crate::galaxy_nav::generated(&id, position_ly) {
        (crate::Visit::Generated(Box::new(system)), 0)
    } else {
        return;
    };
    rebase(
        &mut nav,
        &rig,
        pose.eye,
        &lists.0,
        Some(visit),
        page,
        DVec3::ZERO,
    );
    nav.following = following;
    nav.arrived = true;
}

/// Measure bodies after camera placement, so a followed planet and the eye
/// use the same simulation instant even at very high time rates.
pub fn locate_body(
    lists: Res<Universe>,
    rig: Res<CameraRig>,
    pose: Res<CameraPose>,
    clock: Res<Clock>,
    mut field: Local<Option<space_simulation::craft::Field>>,
    mut nav: ResMut<Navigation>,
) {
    let body = nav.location(&lists.0).and_then(|system| {
        // Never cancel huge galactic coordinates for metre-scale measurements.
        if rig.frame_ly != page_origin_ly(system, nav.page) {
            return None;
        }
        let stale = field
            .as_ref()
            .is_none_or(|f| f.system().id != system.id || f.page() != nav.page);
        if stale || lists.is_changed() {
            *field = Some(space_simulation::craft::Field::new(
                system.clone(),
                nav.page,
            ));
        }
        let eye = page_frame(system, nav.page).inverse() * pose.eye;
        nearby_body(
            field.as_ref()?,
            eye,
            clock.0.seconds,
            nav.location_body.as_deref(),
            nav.presentation_mode,
        )
    });
    nav.location_body = body;
}

/// Open space re-anchors the camera's frame on the eye beyond this, so
/// positions in it stay precise to well under a millimetre.
pub(super) const REANCHOR_METRES: f64 = 1e9;

/// A craft's frame for the camera in galactic space, in light years from
/// home: the followed craft's, else the anchoring craft's, for close-up
/// precision.
pub(super) fn galactic_frame_ly(nav: &Navigation, fleet: &crate::craft::Fleet) -> Option<DVec3> {
    let id = if nav.following {
        nav.selected_craft()
    } else {
        match &nav.anchor {
            Some(crate::Anchor::Craft(id)) => Some(id.as_str()),
            _ => None,
        }
    };
    id.and_then(|id| fleet.get(id)).map(|live| live.origin_ly())
}

/// The camera's own frame in open space: `Navigation::galactic_origin_ly`,
/// moved onto the eye once it is `REANCHOR_METRES` away. Light-year
/// coordinates from home are only precise to tens of kilometres out here.
pub(super) fn open_space_frame_ly(nav: &Navigation, rig: &CameraRig) -> DVec3 {
    let origin = nav.galactic_origin_ly;
    if !(rig.galactic && rig.placed()) || rig.flying() || nav.fly || nav.following {
        return origin;
    }
    let eye = nav
        .eye
        .unwrap_or_else(|| nav.target + (nav.rotation * Vec3::Z).as_dvec3() * nav.distance);
    if eye.length() > REANCHOR_METRES {
        origin + eye / LY_METRES
    } else {
        origin
    }
}

/// What the camera moves with (`Navigation::anchor`): a craft within
/// `CRAFT_NEIGHBOURHOOD_M`, else the body it is at (`location_body`),
/// else nothing, staying in its system's or galactic frame. Only the
/// camera's position decides, not selection.
pub fn locate_anchor(
    lists: Res<Universe>,
    rig: Res<CameraRig>,
    pose: Res<CameraPose>,
    clock: Res<Clock>,
    fleet: Res<crate::craft::Fleet>,
    mut nav: ResMut<Navigation>,
) {
    let anchor = if nav.presentation_mode == PresentationMode::Simulation && rig.placed() {
        nearest_anchor(&nav, &lists.0, &rig, &pose, &fleet, clock.0.seconds)
    } else {
        None
    };
    nav.anchor = anchor;
}

fn nearest_anchor(
    nav: &Navigation,
    lists: &space_shared::Galaxy,
    rig: &CameraRig,
    pose: &CameraPose,
    fleet: &crate::craft::Fleet,
    seconds: f64,
) -> Option<crate::Anchor> {
    let system = nav.system(lists);
    let tilt = system.map_or(DQuat::IDENTITY, |s| page_frame(s, pose.page));
    let eye = tilt.inverse() * pose.eye;
    let range = |anchor: crate::Anchor, neighbourhood: f64| {
        if nav.anchor.as_ref() == Some(&anchor) {
            neighbourhood * (1.0 + space_simulation::craft::SWITCH_MARGIN)
        } else {
            neighbourhood
        }
    };
    let craft = fleet
        .craft
        .iter()
        .filter_map(|live| {
            let at =
                super::placement::in_camera_frame(live, live.state_at(seconds), rig.frame_ly, tilt);
            let distance = (at - eye).length();
            let anchor = crate::Anchor::Craft(live.craft.id.clone());
            (distance < range(anchor.clone(), CRAFT_NEIGHBOURHOOD_M)).then_some((distance, anchor))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0));
    craft
        .map(|(_, anchor)| anchor)
        .or_else(|| nav.location_body.clone().map(crate::Anchor::Body))
}

/// The body the camera at `eye` is at, exactly as a craft there would pick
/// its orbit reference (`Field::select_reference`): a planet within its
/// Hill sphere, or `PLANET_MIN_RADII` if larger, else the innermost star or
/// pair whose stable zone holds it, each kept until `SWITCH_MARGIN` past it.
/// The whole system is `None`. No visual exaggeration, selection or
/// following is involved. Table/constellation are not physical.
pub(super) fn nearby_body(
    field: &space_simulation::craft::Field,
    eye: DVec3,
    seconds: f64,
    previous: Option<&str>,
    mode: PresentationMode,
) -> Option<String> {
    if mode != PresentationMode::Simulation {
        return None;
    }
    let body =
        field.select_reference_floored(previous, eye.to_array(), seconds, PLANET_MIN_RADII)?;
    (field.system().root(field.page()) != Some(body.id.as_str())).then_some(body.id)
}
