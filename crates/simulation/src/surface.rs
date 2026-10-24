//! A body's ground: heights above its spheroid (`shape`), measured along the
//! spheroid's normal as Earth's are from its reference ellipsoid. Height zero
//! is sea level. Drawing, collision and later water and air all ask here, so
//! the ground that is seen is the ground that is stood on.

use crate::shape::Spheroid;
use glam::DVec3;
use space_shared::Planet;

/// Height of the ground above the spheroid, in metres, in a unit `direction`
/// from the centre in the planet's axes (pole +Y). Deterministic. No terrain
/// yet: the ground is the spheroid itself.
pub fn height(_planet: &Planet, _direction: DVec3) -> f64 {
    0.0
}

/// The ground in a unit `direction`, from the centre, in the planet's axes.
pub fn ground(planet: &Planet, shape: &Spheroid, direction: DVec3) -> DVec3 {
    let sea = shape.along(direction);
    sea + shape.normal(sea) * height(planet, direction)
}
