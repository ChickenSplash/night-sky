//! One projection boundary for natural systems and optional task overlays.
use crate::star::multiple::Hosted;
use space_shared::{Planet, Task};

/// During migration the result candidate and legacy live projections are
/// deliberately separate. No field is partially switched to candidate geometry.
pub(crate) fn export_planet(
    hosted: Hosted,
    page: u32,
    designation: String,
    task: Option<Task>,
) -> Planet {
    super::validate_result(&hosted.body.initial_result)
        .expect("generated initial result must remain valid through atmosphere projection");
    Planet {
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
        inertia_factor: Some(hosted.body.structure.inertia_factor()),
        physical: Some(super::to_data(&hosted.body)),
        initial_result: Some(hosted.body.initial_result),
        resonance: hosted.body.resonance,
        spin: hosted.body.spin,
        task,
    }
}

#[cfg(test)]
mod tests;
