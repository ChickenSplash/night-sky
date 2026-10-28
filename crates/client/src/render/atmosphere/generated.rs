//! Adapter from validated simulation optics to the renderer. No preset fitting.
use super::*;
#[cfg(test)]
use space_shared::{AtmosphereReference, SurfaceKnowledge};
use space_shared::{Planet, PlanetResult};
use space_simulation::{
    system::atmosphere::{GasAtmosphere, optics::boundary::OpticalColumn},
    units::{EARTH_SOLAR, SUN_GM},
};

/// How the renderer may treat a planet's physical facts.
enum Source<'a> {
    /// Generated: the result is the authority for reference and gravity.
    Result(&'a PlanetResult),
    /// Hand-built record with no physical facts at all, such as a fixture.
    Bare,
    /// A physical record without its result cannot be checked.
    Invalid,
}

fn source(planet: &Planet) -> Source<'_> {
    if space_simulation::system::physical::validate(planet).is_err() {
        return Source::Invalid;
    }
    match (&planet.initial_result, &planet.physical) {
        (Some(result), _) => Source::Result(result),
        (None, None) => Source::Bare,
        (None, Some(_)) => Source::Invalid,
    }
}

/// Diagnostic classification, not the renderer's ground decision.
#[cfg(test)]
pub fn envelope(planet: &Planet) -> bool {
    match source(planet) {
        Source::Result(result) => result.surface_knowledge == SurfaceKnowledge::GasReference,
        Source::Bare => planet
            .environment
            .as_ref()
            .and_then(|e| e.atmosphere.as_ref())
            .is_some_and(|air| air.reference_level == AtmosphereReference::OneBar),
        Source::Invalid => false,
    }
}

/// There is room below the gas coordinate, independently of class or H/He
/// abundance. This is geometric camera access, never landing permission.
#[cfg(test)]
pub fn deep_envelope(planet: &Planet) -> bool {
    envelope(planet)
        && space_simulation::system::surface::geometry(planet).is_ok_and(|g| g.has_gas_depth())
}

/// Air and drawable condensed boundary are independent. If the bounded visual
/// continuation can reach the condensed top, draw that top through the air.
/// Otherwise retain a black opaque gas proxy, explicitly not missing ground.
/// No optical model means no invented descent/profile through the envelope.
pub fn lower_boundary(planet: &Planet, air: Option<Air>, descend: bool) -> (Option<Air>, bool) {
    let Ok(geometry) = space_simulation::system::surface::geometry(planet) else {
        return (air, false);
    };
    if !geometry.gas_reference {
        return (air, true);
    }
    let Some(air) = air.filter(|_| descend && geometry.has_gas_depth()) else {
        return (air, false);
    };
    let exact_ratio = geometry.condensed_radius_m / geometry.reference_radius_m;
    let mut ratio = exact_ratio as f32;
    // Never round the optical floor into the condensed material.
    if f64::from(ratio) < exact_ratio {
        ratio = ratio.next_up();
    }
    let (rebased, reached) = rebase(air, ratio);
    (Some(rebased), reached)
}

pub fn atmosphere(planet: &Planet) -> Option<Atmosphere> {
    let column = optical_column(planet)?;
    let optics = column.optics;
    // Use the declared profile and operational cutoff, never a structural edge.
    Some(Atmosphere {
        spherical: optics.column.profile.spherical,
        top: column.outer_cutoff.height_m,
        rayleigh: DVec3::from_array(optics.rayleigh_m_inv),
        rayleigh_height: optics.column.scale_height_m,
        gas_absorption: DVec3::from_array(optics.gas_absorption_m_inv),
        mie: DVec3::from_array(optics.aerosol_scattering_m_inv),
        mie_absorption: optics.aerosol_absorption_m_inv,
        mie_height: optics.aerosol_height_m,
        mie_asymmetry: optics.aerosol_asymmetry,
        absorption: DVec3::ZERO,
        layer_height: 0.0,
        layer_width: 1.0,
    })
}

/// Generated planets take reference radius and enclosed gravity from their
/// result, which must also agree with the gas. A disagreeing or unchecked
/// record draws no air. Only bare records fall back to total mass and radius.
pub fn optical_column(planet: &Planet) -> Option<OpticalColumn> {
    let data = planet.environment.as_ref()?.atmosphere.as_ref()?;
    let gas = GasAtmosphere::try_from(data).ok()?;
    match source(planet) {
        Source::Result(result) => OpticalColumn::from_result(result, &gas).ok(),
        Source::Bare => OpticalColumn::new(
            &gas,
            planet.mass_earth * EARTH_SOLAR * SUN_GM,
            planet.radius_km * 1e3,
        )
        .ok(),
        Source::Invalid => None,
    }
}

pub fn for_planet(
    planet: &Planet,
    selected: bool,
    override_air: Option<Atmosphere>,
    radius: f64,
) -> Option<Air> {
    let physical = if selected {
        override_air.or_else(|| atmosphere(planet))
    } else {
        atmosphere(planet)
    }?;
    let air = physical.scaled(radius);
    // Guard the f64 -> f32 boundary. Never send NaN/inf or collapsed shells.
    (air.top > 1.0
        && air.top.is_finite()
        && air.rayleigh.is_finite()
        && air.rayleigh.w > 0.0
        && air.mie.is_finite()
        && air.shape.is_finite()
        && air.shape.x > 0.0
        && air.absorption.is_finite()
        && air.gas_absorption.is_finite())
    .then_some(air)
}

/// Visual continuation below a one-bar reference, not an interior/EOS model.
/// Rebase the same gas profile six active scale heights down (at most
/// 20% of the radius). Nothing collides with this optical lower boundary.
/// The final part fades to black before we stop resolving the column.
#[cfg(test)]
pub fn descending(air: Air) -> Air {
    rebase(air, 0.0).0
}

fn rebase(air: Air, condensed_ratio: f32) -> (Air, bool) {
    let height = if air.mie.max_element() > 0.0 {
        air.rayleigh.w.min(air.shape.x)
    } else {
        air.rayleigh.w
    };
    let limit = (6.0 * height).min(0.2);
    let depth = limit.min((1.0 - condensed_ratio).max(0.0));
    let scale = 1.0 - depth;
    let reached = scale <= condensed_ratio;
    let gas = (depth / (air.rayleigh.w * if air.spherical { scale } else { 1.0 })).exp();
    let haze = if air.mie.max_element() > 0.0 {
        (depth / air.shape.x).exp()
    } else {
        1.0
    };
    let deep = Air {
        spherical: air.spherical,
        radius_scale: scale,
        opaque_proxy: !reached,
        top: air.top / scale,
        rayleigh: (air.rayleigh.truncate() * gas * scale)
            .extend(air.rayleigh.w * if air.spherical { scale } else { 1.0 / scale }),
        gas_absorption: air.gas_absorption * gas * scale,
        mie: air.mie * (haze * scale),
        shape: Vec4::new(
            air.shape.x / scale,
            air.shape.y,
            (air.shape.z + depth) / scale,
            air.shape.w / scale,
        ),
        absorption: air.absorption * scale,
        ground: if reached { air.ground } else { Vec3::ZERO },
    };
    // Recheck after rebasing: finite upper-column coefficients can overflow
    // when extrapolated. Retain the old visible reference sphere in that case.
    if deep.top.is_finite()
        && deep.rayleigh.is_finite()
        && deep.mie.is_finite()
        && deep.shape.is_finite()
        && deep.absorption.is_finite()
        && deep.gas_absorption.is_finite()
    {
        (deep, reached)
    } else {
        (air, false)
    }
}

#[cfg(test)]
mod tests;
