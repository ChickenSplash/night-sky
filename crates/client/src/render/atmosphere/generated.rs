//! Adapter from validated simulation optics to the renderer. No preset fitting.
use super::*;
use space_shared::{AtmosphereReference, Planet};
use space_simulation::{
    system::atmosphere::GasAtmosphere,
    units::{EARTH_SOLAR, SUN_GM},
};

pub fn envelope(planet: &Planet) -> bool {
    planet
        .environment
        .as_ref()
        .and_then(|e| e.atmosphere.as_ref())
        .is_some_and(|air| air.reference_level == AtmosphereReference::OneBar)
}

pub fn atmosphere(planet: &Planet) -> Option<Atmosphere> {
    let data = planet.environment.as_ref()?.atmosphere.as_ref()?;
    let gas = GasAtmosphere::try_from(data).ok()?;
    let optics = gas
        .optics(
            planet.mass_earth * EARTH_SOLAR * SUN_GM,
            planet.radius_km * 1e3,
        )
        .ok()?;
    // Extended columns deliberately retain the constant-gravity estimate and
    // inspector warning. No artificial radius cap or silently changed pressure.
    Some(Atmosphere {
        top: optics.column.extent_m,
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

#[cfg(test)]
mod tests;
