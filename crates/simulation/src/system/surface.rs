//! Reference and condensed-top geometry, separate from optical proxies and
//! landing permission. Heights are measured above the caller's condensed
//! spheroid along its normal. No terrain, collisions or safe landing is implied.

use crate::system::shape::Spheroid;
use glam::DVec3;
use space_shared::{Planet, PlanetResult, RadiusReference, SurfaceKnowledge};

/// Consumer geometry, independent of visibility and landing permission. The
/// condensed boundary includes any surface frost, not just the water/rock host.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize)]
pub struct Geometry {
    pub reference_radius_m: f64,
    pub condensed_radius_m: f64,
    pub gas_reference: bool,
    /// Phase knowledge at the condensed top, not at a gas coordinate above it.
    pub condensed_phase: SurfaceKnowledge,
}

impl Geometry {
    pub fn from_result(result: &PlanetResult) -> Result<Self, &'static str> {
        super::physical::validate_result(result)?;
        let record = &result.record;
        let gas_reference = record.physical.radius_reference == RadiusReference::OneBar;
        // Thin frost tops can be outside physical.condensed_radius_m, which
        // describes their loaded host. Never turn an atmosphere edge into ground.
        let condensed_radius_m = record
            .layers
            .iter()
            .filter(|layer| !matches!(layer.kind.as_str(), "atmosphere" | "envelope"))
            .map(|layer| layer.outer_radius_m)
            .fold(0.0, f64::max);
        if condensed_radius_m <= 0.0 {
            return Err("missing condensed boundary");
        }
        Ok(Self {
            reference_radius_m: record.physical.reference_radius_m,
            condensed_radius_m,
            gas_reference,
            condensed_phase: if gas_reference {
                SurfaceKnowledge::Unresolved
            } else {
                result.surface_knowledge
            },
        })
    }

    /// Same bounded flattening at both levels, not a new interior figure solve.
    pub fn condensed_shape(self, reference: Spheroid) -> Spheroid {
        let scale = self.condensed_radius_m / self.reference_radius_m;
        Spheroid {
            equatorial: reference.equatorial * scale,
            polar: reference.polar * scale,
        }
    }

    pub fn has_gas_depth(self) -> bool {
        self.gas_reference && self.condensed_radius_m < self.reference_radius_m
    }
}

/// Bare hand-built fixtures retain their geometric spheroid. An unchecked
/// physical record never acquires surface semantics from its class or pressure.
pub fn geometry(planet: &Planet) -> Result<Geometry, &'static str> {
    if let Some(result) = &planet.initial_result {
        super::physical::validate(planet)?;
        return Geometry::from_result(result);
    }
    if planet.physical.is_some() {
        return Err("physical record without its result");
    }
    let radius = planet.radius_km * 1000.0;
    if !radius.is_finite() || radius <= 0.0 {
        return Err("invalid reference radius");
    }
    Ok(Geometry {
        reference_radius_m: radius,
        condensed_radius_m: radius,
        gas_reference: planet
            .environment
            .as_ref()
            .and_then(|e| e.atmosphere.as_ref())
            .is_some_and(|air| air.reference_level == space_shared::AtmosphereReference::OneBar),
        condensed_phase: SurfaceKnowledge::Unresolved,
    })
}

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

#[cfg(test)]
mod tests;
