//! Page star positions and orbital-plane frames.
use bevy::{
    math::{DQuat, DVec3},
    prelude::*,
};
use space_shared::{Spin, StarSystem};
use space_simulation::{LY_METRES, plane_tilt, spin_angle};

fn rotation([inclination, node, twist]: [f64; 3]) -> DQuat {
    DQuat::from_rotation_y(node)
        * DQuat::from_rotation_x(inclination)
        * DQuat::from_rotation_y(twist)
}

/// A page star's orbital-plane tilt: inclination, node and twist, in radians.
pub fn page_tilt(system: &StarSystem, page: u32) -> [f64; 3] {
    system
        .page_star(page)
        .map_or([0.0; 3], |star| plane_tilt(&star.id))
}

/// Rotation from a page's orbital-plane axes into galactic axes. Every page is its
/// own star system, so each has its own plane.
pub fn page_frame(system: &StarSystem, page: u32) -> DQuat {
    rotation(page_tilt(system, page))
}

/// A body's wireframe orientation in its system's plane: its axis tilted, then
/// turned to its current rotation angle.
pub(super) fn spin_basis(frame: DQuat, spin: &Spin, seconds: f64) -> DQuat {
    frame
        * DQuat::from_rotation_y(spin.azimuth_rad)
        * DQuat::from_rotation_x(spin.tilt_rad)
        // Orbit positions progress from +X towards +Z; positive Y rotation
        // turns +X towards -Z. Negate the spin to match orbital direction.
        * DQuat::from_rotation_y(-spin_angle(spin, seconds))
}

/// A page star's position: light years from home, in galactic axes.
pub fn page_origin_ly(system: &StarSystem, page: u32) -> DVec3 {
    DVec3::from_array(
        system
            .page_star(page)
            .map_or(system.position_ly, |star| star.position_ly),
    )
}

/// Every page star of a system relative to one of them, in galactic metres.
pub fn page_offsets(system: &StarSystem, from: u32) -> Vec<DVec3> {
    let here = page_origin_ly(system, from);
    system
        .pages()
        .iter()
        .map(|&page| (page_origin_ly(system, page) - here) * LY_METRES)
        .collect()
}
