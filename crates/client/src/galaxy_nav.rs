//! Picking and visiting stars. List pages are ordinary galactic stars: the same
//! click picks and visits any of them, with tasks as an optional overlay.
use crate::{
    Navigation, Visit,
    scene::{CameraPose, CameraRig},
    stars::stream::Starfield,
};
use bevy::{math::DVec3, prelude::*};
use space_shared::{Galaxy, PageStar, Planet, StarSystem};
use space_simulation::{
    LY_METRES,
    cells::{catalogue_name, cell_planetary_system, cell_star, planet_designation},
    galaxy::{GalaxyModel, PC_METRES},
    population::reach_sq_per_luminosity,
};

/// A picked star, identified by its stable cell ID, never a mesh index.
#[derive(Clone, Debug, PartialEq)]
pub struct PickedStar {
    pub id: String,
    /// Light years from home, in galactic axes.
    pub position_ly: DVec3,
}

fn relative_ly(position: [f64; 3]) -> DVec3 {
    (DVec3::from_array(position) - DVec3::from_array(GalaxyModel::milky_way().home_pc))
        * (PC_METRES / LY_METRES)
}

/// Pick a visible streamed star along the pointer ray. The threshold is the
/// angular size of a ten-pixel hit target at the current viewport height.
/// `tan_half` is the tangent of half the vertical field of view.
pub fn pick_pointer(
    field: &Starfield,
    rig: &CameraRig,
    pose: &CameraPose,
    rotation: Quat,
    pointer: [f64; 2],
    viewport: [f64; 2],
    (tan_half, magnitude): (f64, f64),
) -> Option<PickedStar> {
    let [x, y] = pointer;
    let [width, height] = viewport;
    let right = (2.0 * x - width) / height * tan_half;
    let up = (1.0 - 2.0 * y / height) * tan_half;
    let direction = (rotation * Vec3::new(right as f32, up as f32, -1.0).normalize()).as_dvec3();
    let radius = 10.0 * 2.0 * tan_half / height;
    pick_ray(
        field,
        rig,
        pose,
        direction,
        1.0 / (1.0 + radius * radius).sqrt(),
        magnitude,
    )
}

fn pick_ray(
    field: &Starfield,
    rig: &CameraRig,
    pose: &CameraPose,
    forward: DVec3,
    min_alignment: f64,
    magnitude: f64,
) -> Option<PickedStar> {
    let eye = rig.galactic_ly(pose);
    let pc_per_ly = LY_METRES / PC_METRES;
    let reach = reach_sq_per_luminosity(magnitude) / (pc_per_ly * pc_per_ly);
    let eye_pc = eye * pc_per_ly + DVec3::from_array(GalaxyModel::milky_way().home_pc);
    // Widen traversal slightly for the f32 quaternion/ray rounding; the
    // exact original angular test below still decides the winner.
    let tan = (1.0 / (min_alignment * min_alignment) - 1.0).sqrt() + 1e-5;
    let plane_scale = (1.0 + tan * tan).sqrt();
    let cone_axis = forward.normalize();
    let start = bevy::platform::time::Instant::now();
    let mut checked = 0usize;
    let mut winning_point = None;
    let mut best: Option<(f64, PickedStar)> = None;
    for (&cell, stars) in &field.cells {
        stars.query_blocks(
            |centre, radius| {
                let delta = DVec3::from_array(centre) - eye_pc;
                let depth = delta.dot(cone_axis);
                let lateral = (delta - cone_axis * depth).length();
                depth >= -radius && lateral - depth * tan <= radius * plane_scale
            },
            |star| {
                checked += 1;
                let position_ly = relative_ly(star.position_pc);
                let delta = position_ly - eye;
                let distance_sq = delta.length_squared();
                if distance_sq < 1e-16 || distance_sq > f64::from(star.luminosity_solar) * reach {
                    return;
                }
                let alignment = delta.dot(forward) / distance_sq.sqrt();
                // Prefer angular proximity within the caller's hit cone; a stable
                // ID resolves equal scores regardless of HashMap ordering.
                if alignment < min_alignment {
                    return;
                }
                let score = 1.0 - alignment;
                let picked = PickedStar {
                    id: star.id(cell),
                    position_ly,
                };
                if best.as_ref().is_none_or(|(old, previous)| {
                    score < *old || (score == *old && picked.id < previous.id)
                }) {
                    best = Some((score, picked));
                    winning_point = Some(*star);
                }
            },
        );
    }
    if let (Some((_, picked)), Some(point)) = (&best, winning_point) {
        *field
            .picked
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some((picked.id.clone(), point));
    }
    bevy::log::info!(
        "Star picking: {:.2} ms, {} of {} stars examined",
        start.elapsed().as_secs_f64() * 1000.0,
        checked,
        field.stars
    );
    best.map(|(_, star)| star)
}

/// Find a picked point by stable ID, never by its batch or vertex number.
pub fn picked_id(field: &Starfield, id: &str) -> Option<PickedStar> {
    let (cell, component, index) = space_simulation::cells::parse_cell_star(id)?;
    let mut cached = field
        .picked
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some((previous, star)) = &*cached
        && previous == id
        && field.cells.contains_key(&cell)
    {
        return Some(PickedStar {
            id: id.to_owned(),
            position_ly: relative_ly(star.position_pc),
        });
    }
    let star = field
        .cells
        .get(&cell)?
        .stars
        .iter()
        .find(|s| s.component == component && u64::from(s.index) == index)?;
    *cached = Some((id.to_owned(), *star));
    Some(PickedStar {
        id: id.to_owned(),
        position_ly: relative_ly(star.position_pc),
    })
}

/// A list page's star, as if picked.
pub fn page_pick(system: &StarSystem, page: u32) -> Option<PickedStar> {
    system.page_star(page).map(|star| PickedStar {
        id: star.id.clone(),
        position_ly: DVec3::from_array(star.position_ly),
    })
}

/// The list and page a star carries, if any.
pub fn mapped(lists: &Galaxy, id: &str) -> Option<(usize, u32)> {
    lists
        .systems
        .iter()
        .enumerate()
        .find_map(|(index, system)| {
            system
                .page_stars
                .iter()
                .find(|star| star.id == id)
                .map(|star| (index, star.page))
        })
}

/// A star's name and, for a list page, its page and the list's page count.
pub fn describe(lists: &Galaxy, id: &str) -> (String, Option<(u32, usize)>) {
    match mapped(lists, id) {
        Some((list, page)) => {
            let system = &lists.systems[list];
            (system.name.clone(), Some((page, system.pages().len())))
        }
        None => (catalogue_name(id), None),
    }
}

/// A star's display name, with its system number for a list page.
pub fn label(lists: &Galaxy, id: &str) -> String {
    match describe(lists, id) {
        (name, Some((page, _))) => format!("{name} · system {}", page + 1),
        (name, None) => name,
    }
}

/// A task-free system built from a cell star's ID alone.
pub fn generated(id: &str, position_ly: DVec3) -> Option<StarSystem> {
    let (_, mut star) = cell_star(id)?;
    star.page = 0;
    let planets = cell_planetary_system(id)?
        .into_iter()
        .enumerate()
        .map(|(index, body)| Planet {
            id: format!("{id}/{index}"),
            designation: planet_designation(id, index),
            page: 0,
            orbit_index: index as u32,
            orbit: body.orbit,
            radius_km: body.radius_km,
            mass_earth: body.mass_earth,
            class: body.class.name().into(),
            resonance: body.resonance,
            spin: body.spin,
            task: None,
        })
        .collect();
    Some(StarSystem {
        id: id.to_owned(),
        name: catalogue_name(id),
        position_ly: position_ly.to_array(),
        planets,
        stars: vec![star],
        page_stars: vec![PageStar {
            page: 0,
            id: id.to_owned(),
            position_ly: position_ly.to_array(),
        }],
    })
}

/// Fly to the selected star: its list page when it has one, or its generated
/// system.
pub fn visit_selected(nav: &mut Navigation, lists: &Galaxy) -> bool {
    let Some(picked) = nav.selected_star().cloned() else {
        return false;
    };
    if let Some((list, page)) = mapped(lists, &picked.id) {
        nav.arrive(Visit::List(list), page, lists);
    } else if let Some(system) = generated(&picked.id, picked.position_ly) {
        nav.arrive(Visit::Generated(Box::new(system)), 0, lists);
    } else {
        return false;
    }
    true
}

/// How a star by ID can be visited from here.
enum Route {
    List(usize, u32),
    Visited,
    Streamed(PickedStar),
}

/// Resolves a route without generating a system, so the UI can ask every frame.
fn route(nav: &Navigation, lists: &Galaxy, field: &Starfield, id: &str) -> Option<Route> {
    if let Some((system, page)) = crate::objects::known_system(lists, id) {
        let list = lists.systems.iter().position(|list| list.id == system.id)?;
        return Some(Route::List(list, page));
    }
    if matches!(&nav.visit, Some(Visit::Generated(system)) if system.id == id) {
        return Some(Route::Visited);
    }
    picked_id(field, id).map(Route::Streamed)
}

pub fn reachable(nav: &Navigation, lists: &Galaxy, field: &Starfield, id: &str) -> bool {
    route(nav, lists, field, id).is_some()
}

/// Fly to any star's system and frame it, as a second click on the star does.
pub fn visit_star(nav: &mut Navigation, lists: &Galaxy, field: &Starfield, id: &str) -> bool {
    match route(nav, lists, field, id) {
        Some(Route::List(list, page)) => nav.arrive(Visit::List(list), page, lists),
        Some(Route::Visited) => nav.frame(lists),
        Some(Route::Streamed(star)) => match generated(id, star.position_ly) {
            Some(system) => nav.arrive(Visit::Generated(Box::new(system)), 0, lists),
            None => return false,
        },
        None => return false,
    }
    true
}

#[cfg(test)]
mod tests;
