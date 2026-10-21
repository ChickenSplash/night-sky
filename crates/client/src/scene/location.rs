//! Where the camera physically is: arriving at and leaving systems, and
//! the nearby planet.
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
    planet_position,
};

/// Beyond this camera range stars draw at their true separation. Planet
/// visibility also depends on the actual size of the current page's system.
pub(super) const WIDE_METRES: f64 = 0.02 * LY_METRES;
/// Free flight this close to a loaded star makes it the visited system.
pub(super) const APPROACH_METRES: f64 = 0.02 * LY_METRES;
/// The camera leaves a system beyond this multiple of its `system_range`, so
/// the boundary cannot flicker.
pub(super) const LEAVE_FACTOR: f64 = 2.0;
/// Open-space free flight moves its origin to the camera beyond this eye
/// distance, where an f64 position still resolves about 0.1 mm.
pub(super) const REANCHOR_METRES: f64 = 1e12;

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
    let new_frame = new_system.map_or(nav.open_origin_ly, |s| page_origin_ly(s, page));
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
    nav.location_planet = None;
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
/// reached, returns to open space without moving the eye.
pub fn locate(
    #[cfg(target_arch = "wasm32")] benchmark: Option<
        Res<crate::devtools::benchmark::BenchmarkFlight>,
    >,
    field: Res<crate::stars::stream::Starfield>,
    mut metrics: ResMut<crate::stars::stream::StreamMetrics>,
    lists: Res<Universe>,
    rig: Res<CameraRig>,
    pose: Res<CameraPose>,
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
    #[cfg(target_arch = "wasm32")]
    if benchmark.is_some() {
        return;
    }
    // Flights and presets are deliberate: wait until they land.
    if nav.presentation_mode != PresentationMode::Simulation
        || (nav.following && nav.selected_craft().is_some())
        || rig.flying()
        || nav.fly
        || !rig.placed()
        || nav.arrived
    {
        return;
    }
    if nav.system(&lists.0).is_some() {
        // Open space stays anchored at the star just left.
        nav.open_origin_ly = rig.frame_ly;
        rebase(&mut nav, &rig, pose.eye, &lists.0, None, 0);
        // Let place_camera switch the displayed frame before another handoff.
        return;
    }
    let eye_pc =
        DVec3::from_array(GalaxyModel::milky_way().home_pc) + eye_ly * (LY_METRES / PC_METRES);
    if let Some((visit, page)) = approached(&field, &mut metrics, &lists.0, eye_pc) {
        rebase(&mut nav, &rig, pose.eye, &lists.0, Some(visit), page);
        nav.arrived = true;
    } else if nav.open_origin_ly == rig.frame_ly
        && let Some(eye) = nav.eye.filter(|eye| eye.length() > REANCHOR_METRES)
    {
        nav.open_origin_ly = rig.frame_ly + eye / LY_METRES;
        rebase(&mut nav, &rig, pose.eye, &lists.0, None, 0);
    }
}

/// The streamed system free flight at `eye_pc` has come close enough to enter.
fn approached(
    field: &crate::stars::stream::Starfield,
    metrics: &mut crate::stars::stream::StreamMetrics,
    lists: &space_shared::Galaxy,
    eye_pc: DVec3,
) -> Option<(crate::Visit, u32)> {
    let reach_pc = APPROACH_METRES / PC_METRES;
    let (_, cell, star) = field.nearest(eye_pc, reach_pc, metrics)?;
    let (id, position_pc) = (star.id(cell), star.position_pc);
    let position_ly = (DVec3::from_array(position_pc)
        - DVec3::from_array(GalaxyModel::milky_way().home_pc))
        * (PC_METRES / LY_METRES);
    if let Some((list, page)) = crate::galaxy_nav::mapped(lists, &id) {
        Some((crate::Visit::List(list), page))
    } else {
        crate::galaxy_nav::generated(&id, position_ly)
            .map(|system| (crate::Visit::Generated(Box::new(system)), 0))
    }
}

/// Measure planets after camera placement, so a followed planet and the eye
/// use the same simulation instant even at very high time rates.
pub fn locate_planet(
    lists: Res<Universe>,
    rig: Res<CameraRig>,
    pose: Res<CameraPose>,
    clock: Res<Clock>,
    mut nav: ResMut<Navigation>,
) {
    nav.location_planet = nav.location(&lists.0).and_then(|system| {
        // Never cancel huge galactic coordinates for metre-scale measurements.
        if rig.frame_ly != page_origin_ly(system, nav.page) {
            return None;
        }
        let eye = page_frame(system, nav.page).inverse() * pose.eye;
        nearby_planet(
            system,
            nav.page,
            eye,
            clock.0.seconds,
            nav.location_planet.as_deref(),
            nav.presentation_mode,
        )
    });
}

/// Enter within 20 physical radii; remain until 30. No visual exaggeration,
/// selection or following is involved. Table/constellation are not physical.
pub(super) fn nearby_planet(
    system: &space_shared::StarSystem,
    page: u32,
    eye: DVec3,
    seconds: f64,
    previous: Option<&str>,
    mode: PresentationMode,
) -> Option<String> {
    if mode != PresentationMode::Simulation {
        return None;
    }
    let distance = |planet: &space_shared::Planet| {
        (eye - DVec3::from_array(planet_position(system, planet, seconds))).length()
    };
    if let Some(planet) = system
        .on_page(page)
        .find(|planet| Some(planet.id.as_str()) == previous)
        && distance(planet) < planet.radius_km * 1000.0 * 30.0
    {
        return Some(planet.id.clone());
    }
    system
        .on_page(page)
        .filter_map(|planet| {
            let d = distance(planet);
            (d < planet.radius_km * 1000.0 * 20.0).then_some((d, &planet.id))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0).then_with(|| a.1.cmp(b.1)))
        .map(|(_, id)| id.clone())
}
