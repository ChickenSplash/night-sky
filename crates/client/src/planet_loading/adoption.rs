//! Conservative publication barrier: never replace an in-use craft/route field.
use super::*;
use crate::{
    craft::Fleet,
    navigation::presentation::PresentationMode,
    scene::{CameraPose, CameraRig},
};
use space_shared::{Galaxy, Planet, StarSystem};
use space_simulation::system::clearance::Policy;

pub(super) fn refresh_status(
    loading: &PlanetLoading,
    nav: &mut Navigation,
    galaxy: &mut Galaxy,
) -> bool {
    use space_shared::{RefinementStatus, ResultProvenance};
    let mut changed = false;
    let mut update = |planet: &mut Planet| {
        let Some(active) = &mut planet.initial_result else {
            return;
        };
        if active.provenance != ResultProvenance::Fallback
            || active.record.fallback_reason.is_some()
            || active.revision.sequence != 0
        {
            return;
        }
        let status = if !loading.enabled {
            RefinementStatus::Unavailable {
                reason: "Background refinement paused".into(),
            }
        } else {
            match loading.status(&planet.id) {
                Layered::State(PlanetState::Running) => RefinementStatus::Running,
                Layered::State(PlanetState::Failed(reason)) => RefinementStatus::Failed {
                    reason: reason.clone(),
                },
                _ => match loading.adoption(&planet.id) {
                    Some(reason) if reason.starts_with("Not applied:") => {
                        RefinementStatus::Unavailable {
                            reason: reason.into(),
                        }
                    }
                    _ => RefinementStatus::Pending,
                },
            }
        };
        if active.refinement != status {
            active.refinement = status;
            changed = true;
        }
    };
    for planet in galaxy.systems.iter_mut().flat_map(|s| &mut s.planets) {
        update(planet);
    }
    if let Some(crate::Visit::Generated(system)) = &mut nav.visit {
        for planet in &mut system.planets {
            update(planet);
        }
    }
    changed
}

fn protected(fleet: &Fleet, id: &str) -> Option<&'static str> {
    if fleet
        .craft
        .iter()
        .any(|live| live.field.system().planets.iter().any(|p| p.id == id))
    {
        return Some("Deferred: a craft retains this system's physical field");
    }
    // Routes may retain destination/obstacle geometry outside the craft's current
    // field. Until route rebasing exists, conservatively pin all such revisions.
    if fleet.craft.iter().any(|live| {
        live.craft
            .route
            .as_ref()
            .is_some_and(|route| route.active())
    }) {
        return Some("Deferred: a craft retains route geometry");
    }
    None
}

fn camera_guard(
    system: &StarSystem,
    old: &Planet,
    next: &Planet,
    nav: &Navigation,
    rig: &CameraRig,
    pose: &CameraPose,
    seconds: f64,
) -> Result<(), &'static str> {
    if old.page() != nav.page {
        return Ok(());
    }
    if !rig.placed() || rig.flying() || nav.fly {
        return Err("Deferred: camera placement or flight in progress");
    }
    if nav.presentation_mode != PresentationMode::Simulation
        || pose.layout != 0.0
        || pose.page != nav.page
        || rig.frame_origin_ly() != crate::scene::page_origin_ly(system, nav.page)
    {
        return Err("Deferred: camera is not in the physical system frame");
    }
    let bound = |p: &Planet| -> Result<f64, &'static str> {
        let clearance = Policy::default()
            .planet(p)
            .map_err(|_| "Deferred: navigation clearance unsupported")?;
        let shape = space_simulation::planet_shape(p);
        let top = p
            .environment
            .as_ref()
            .and_then(|e| e.atmosphere.as_ref())
            .map_or(0.0, |a| a.extent_m);
        Ok(clearance.preferred_m.max(shape.equatorial + top))
    };
    let radius = 2.0 * bound(old)?.max(bound(next)?);
    let eye = crate::scene::page_frame(system, nav.page).inverse() * pose.eye;
    let centre =
        bevy::math::DVec3::from_array(space_simulation::planet_position(system, old, seconds));
    let distance = (eye - centre).length();
    if !distance.is_finite() || distance <= radius {
        return Err("Deferred: camera is near the old or refined body/atmosphere/clearance");
    }
    Ok(())
}

/// Stage every loaded copy first, then publish without yielding. Existing craft
/// fields are never rebuilt independently; their presence defers publication.
#[allow(clippy::too_many_arguments)]
pub(super) fn publish(
    loading: &mut PlanetLoading,
    nav: &mut Navigation,
    galaxy: &mut Galaxy,
    fleet: &mut Fleet,
    rig: &CameraRig,
    pose: &CameraPose,
    seconds: f64,
) -> bool {
    if !loading.enabled {
        return false;
    }
    let Some(key) = loading.active.clone() else {
        return false;
    };
    let Some(cache) = loading.cache.get_mut(&key) else {
        return false;
    };
    let mut ids: Vec<_> = cache.ready.keys().cloned().collect();
    if let Some(cursor) = &cache.check_cursor {
        let start = ids.iter().position(|id| id > cursor).unwrap_or(0);
        ids.rotate_left(start);
    }
    let mut applied = 0;
    // Bound checks as well as publications. Rotate so a nearby/unsupported
    // planet cannot starve every later candidate in the ordered map.
    for id in ids.into_iter().take(RESULTS_PER_FRAME) {
        cache.check_cursor = Some(id.clone());
        let refinement = &cache.ready[&id];
        if let Some(reason) = protected(fleet, &id) {
            cache.adoption.insert(id, reason.into());
            continue;
        }
        let visited = nav
            .system(galaxy)
            .and_then(|s| s.planets.iter().find(|p| p.id == id).map(|p| (s, p)));
        let source = visited.or_else(|| {
            galaxy
                .systems
                .iter()
                .find_map(|s| s.planets.iter().find(|p| p.id == id).map(|p| (s, p)))
        });
        let Some((system, old)) = source else {
            cache
                .adoption
                .insert(id, "Deferred: system is not loaded".into());
            continue;
        };
        let next = match refinement.project(old) {
            Ok(next) => next,
            Err(reason) => {
                cache
                    .adoption
                    .insert(id.clone(), format!("Discarded: {reason}"));
                cache.ready.remove(&id);
                continue;
            }
        };
        if visited.is_some()
            && let Err(reason) = camera_guard(system, old, &next, nav, rig, pose, seconds)
        {
            cache.adoption.insert(id, reason.into());
            continue;
        }
        // A duplicate loaded copy must have the same lineage. Never update only
        // half of the world when a backend snapshot has changed its inputs.
        let valid = galaxy
            .systems
            .iter()
            .flat_map(|s| &s.planets)
            .filter(|p| p.id == id)
            .all(|p| refinement.project(p).is_ok());
        if !valid {
            cache.adoption.insert(
                id.clone(),
                "Discarded: loaded copies have different physical lineages".into(),
            );
            cache.ready.remove(&id);
            continue;
        }
        for system in &mut galaxy.systems {
            for planet in system.planets.iter_mut().filter(|p| p.id == id) {
                *planet = refinement
                    .project(planet)
                    .expect("staged loaded projection");
            }
        }
        if let Some(crate::Visit::Generated(system)) = &mut nav.visit {
            for planet in system.planets.iter_mut().filter(|p| p.id == id) {
                *planet = refinement
                    .project(planet)
                    .expect("staged visited projection");
            }
        }
        fleet
            .charts
            .retain_refinement(id.clone(), refinement.clone());
        cache
            .adoption
            .insert(id.clone(), "Applied: active physical revision".into());
        cache.ready.remove(&id);
        applied += 1;
    }
    applied > 0
}

#[cfg(test)]
mod tests;
