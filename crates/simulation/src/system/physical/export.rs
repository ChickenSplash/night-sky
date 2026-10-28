//! One projection boundary for natural systems and optional task overlays.
use crate::star::multiple::Hosted;
use space_shared::{Planet, Task};

/// Every live projection comes from the body's one result; `validate` checks
/// that the exported record still agrees with it.
pub(crate) fn export_planet(
    hosted: Hosted,
    page: u32,
    designation: String,
    task: Option<Task>,
) -> Planet {
    let planet = Planet {
        id: hosted.id(),
        designation,
        page,
        orbit_index: hosted.index as u32,
        host: hosted.host,
        orbit: hosted.body.orbit,
        radius_km: hosted.body.radius_km,
        mass_earth: hosted.body.mass_earth,
        class: hosted.body.class.name().into(),
        environment: hosted.environment,
        inertia_factor: Some(hosted.body.physical().inertia_factor),
        physical: Some(super::to_data(&hosted.body)),
        initial_result: Some(hosted.body.initial_result),
        resonance: hosted.body.resonance,
        spin: hosted.body.spin,
        task,
    };
    if let Err(error) = super::validate(&planet) {
        panic!("{}: generated live projection: {error}", planet.id);
    }
    planet
}

#[cfg(test)]
mod tests;
