//! Conservative f64 frustum tests for spatial blocks and multiple systems.
use bevy::{math::DVec3, prelude::*};

/// The view's four side planes in f64, for culling blocks: a sphere outside
/// any of them cannot reach the screen. Planes widen by a margin in pixels, so
/// a star at a block's edge keeps its whole footprint.
pub(super) struct Sight {
    pub(super) right: DVec3,
    pub(super) up: DVec3,
    pub(super) forward: DVec3,
    pub(super) tan_x: f64,
    pub(super) tan_y: f64,
}

impl Sight {
    pub(super) fn new(
        camera: &GlobalTransform,
        tan_half_fov: f64,
        size: UVec2,
        margin_px: f64,
    ) -> Self {
        let pad = margin_px * 2.0 * tan_half_fov / f64::from(size.y);
        Self {
            right: camera.right().as_dvec3(),
            up: camera.up().as_dvec3(),
            forward: camera.forward().as_dvec3(),
            tan_x: tan_half_fov * f64::from(size.x) / f64::from(size.y) + pad,
            tan_y: tan_half_fov + pad,
        }
    }

    /// Whether a sphere `offset` from the eye can show. Conservative: a few
    /// spheres just off screen, or just behind the camera, pass.
    pub(super) fn sees(&self, offset: DVec3, radius: f64) -> bool {
        let depth = offset.dot(self.forward);
        let inside = |lateral: f64, tan: f64| {
            (lateral.abs() - depth * tan) / (1.0 + tan * tan).sqrt() <= radius
        };
        inside(offset.dot(self.right), self.tan_x) && inside(offset.dot(self.up), self.tan_y)
    }
}

#[cfg(test)]
mod tests;
