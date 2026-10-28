//! Adapter from validated simulation optics to the renderer. No preset fitting.
use super::*;
use space_shared::{AtmosphereReference, Planet};
use space_simulation::{
    system::atmosphere::{GasAtmosphere, optics::boundary::OpticalColumn},
    units::{EARTH_SOLAR, SUN_GM},
};

pub fn envelope(planet: &Planet) -> bool {
    planet
        .environment
        .as_ref()
        .and_then(|e| e.atmosphere.as_ref())
        .is_some_and(|air| air.reference_level == AtmosphereReference::OneBar)
}

/// A pressure reference alone is not evidence for a groundless giant. Restrict
/// the visual continuation to bulk H/He beyond the condensed-boundary handoff.
/// Do not use the descriptive class, which can change independently of physics.
pub fn deep_envelope(planet: &Planet) -> bool {
    use space_shared::RadiusReference;
    use space_simulation::system::structure::parameters::MAX_BOUNDARY_GAS_FRACTION;

    envelope(planet)
        && planet.physical.as_ref().is_some_and(|data| {
            data.radius_reference == RadiusReference::OneBar
                && data.h_he_mass_fraction.is_finite()
                && (2.0 * MAX_BOUNDARY_GAS_FRACTION..=1.0).contains(&data.h_he_mass_fraction)
        })
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

/// Live projections, which generation installs from the planet's result.
/// Moving this adapter onto the checked `OpticalColumn::from_result` entry
/// point is a separate consumer migration.
pub fn optical_column(planet: &Planet) -> Option<OpticalColumn> {
    let data = planet.environment.as_ref()?.atmosphere.as_ref()?;
    let gas = GasAtmosphere::try_from(data).ok()?;
    OpticalColumn::new(
        &gas,
        planet.mass_earth * EARTH_SOLAR * SUN_GM,
        planet.radius_km * 1e3,
    )
    .ok()
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
pub fn descending(air: Air) -> Air {
    let height = if air.mie.max_element() > 0.0 {
        air.rayleigh.w.min(air.shape.x)
    } else {
        air.rayleigh.w
    };
    let depth = (6.0 * height).min(0.2);
    let scale = 1.0 - depth;
    let gas = (depth / (air.rayleigh.w * if air.spherical { scale } else { 1.0 })).exp();
    let haze = if air.mie.max_element() > 0.0 {
        (depth / air.shape.x).exp()
    } else {
        1.0
    };
    let deep = Air {
        spherical: air.spherical,
        radius_scale: scale,
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
        ground: Vec3::ZERO,
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
        deep
    } else {
        air
    }
}

#[cfg(test)]
mod tests;
