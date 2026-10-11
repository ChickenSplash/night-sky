//! Picking and visiting stars. List pages are ordinary galactic stars: the same
//! click picks and visits any of them, with tasks as an optional overlay.
use crate::{
    Navigation, Visit,
    scene::{CameraPose, CameraRig},
    stream::Starfield,
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
mod tests {
    use super::*;
    use crate::Selection;
    use space_simulation::population::VISIBLE_MAGNITUDE;
    use space_simulation::{
        cells::{Cell, Sky},
        galaxy::Component,
        stream::{Loader, StarCell, StarPoint},
    };

    fn lists(systems: Vec<StarSystem>) -> Galaxy {
        Galaxy {
            source: String::new(),
            systems,
        }
    }

    #[test]
    fn pointer_picks_visible_off_centre_star_by_stable_id() {
        let home = GalaxyModel::milky_way().home_pc;
        let cell = Cell::containing(10, home).unwrap();
        let mut field = Starfield::default();
        let star = StarPoint {
            position_pc: [home[0] + 0.01, home[1], home[2] - 1.0],
            luminosity_solar: 1.0,
            temperature_k: 5772.0,
            radius_solar: 1.0,
            component: Component::ThinDisc,
            index: 8,
        };
        field.cells.insert(cell, StarCell::new(cell, vec![star]));
        let rig = CameraRig::default();
        let pose = CameraPose::default();
        let tan_half = (50.0_f64.to_radians() / 2.0).tan();
        let x = 500.0 + 0.01 * 500.0 / tan_half;
        let picked = pick_pointer(
            &field,
            &rig,
            &pose,
            Quat::IDENTITY,
            [x, 500.0],
            [1000.0, 1000.0],
            (tan_half, VISIBLE_MAGNITUDE),
        )
        .unwrap();
        // A wider view puts the same star closer to the centre of the screen.
        let wide = (120.0_f64.to_radians() / 2.0).tan();
        let x_wide = 500.0 + 0.01 * 500.0 / wide;
        assert!(x_wide < x);
        assert_eq!(
            pick_pointer(
                &field,
                &rig,
                &pose,
                Quat::IDENTITY,
                [x_wide, 500.0],
                [1000.0, 1000.0],
                (wide, VISIBLE_MAGNITUDE),
            )
            .unwrap()
            .id,
            picked.id
        );
        assert_eq!(picked.id, star.id(cell));
        assert_eq!(
            picked_id(&field, &picked.id).unwrap().position_ly,
            relative_ly(star.position_pc)
        );
        assert!(
            pick_pointer(
                &field,
                &rig,
                &pose,
                Quat::IDENTITY,
                [x + 30.0, 500.0],
                [1000.0, 1000.0],
                (tan_half, VISIBLE_MAGNITUDE),
            )
            .is_none()
        );
        assert!(picked_id(&field, "g1.2.8").is_none());
    }

    #[test]
    fn grid_picking_matches_full_scan_and_caches_the_winner() {
        let home = DVec3::from_array(GalaxyModel::milky_way().home_pc);
        let cell = Cell::containing(10, home.to_array()).unwrap();
        let min = DVec3::from_array(cell.min_pc());
        let points: Vec<_> = (0..20_000u32)
            .map(|index| StarPoint {
                position_pc: (min
                    + DVec3::new(
                        f64::from(index % 97) / 97.0,
                        f64::from(index * 31 % 101) / 101.0,
                        f64::from(index * 47 % 103) / 103.0,
                    ) * cell.size_pc())
                .to_array(),
                luminosity_solar: if index % 3 == 0 { 0.001 } else { 10_000.0 },
                temperature_k: 5772.0,
                radius_solar: 1.0,
                component: Component::ThinDisc,
                index,
            })
            .collect();
        let mut field = Starfield::default();
        field.cells.insert(cell, StarCell::new(cell, points));
        let rig = CameraRig::default();
        let pose = CameraPose::default();
        let eye = rig.galactic_ly(&pose);
        let pc_per_ly = LY_METRES / PC_METRES;
        for axis in [
            DVec3::X,
            DVec3::Y,
            DVec3::Z,
            -DVec3::Z,
            DVec3::new(1.0, 0.2, -0.5).normalize(),
        ] {
            for magnitude in [6.5, 8.5] {
                let threshold = 1.0 / (1.0 + 0.01f64.powi(2)).sqrt();
                let reach = reach_sq_per_luminosity(magnitude) / pc_per_ly.powi(2);
                let mut expected: Option<(f64, String)> = None;
                for star in &field.cells[&cell].stars {
                    let delta = relative_ly(star.position_pc) - eye;
                    let distance = delta.length_squared();
                    if distance < 1e-16 || distance > f64::from(star.luminosity_solar) * reach {
                        continue;
                    }
                    let alignment = delta.dot(axis) / distance.sqrt();
                    if alignment < threshold {
                        continue;
                    }
                    let score = 1.0 - alignment;
                    let id = star.id(cell);
                    if expected.as_ref().is_none_or(|(old, previous)| {
                        score < *old || (score == *old && id < *previous)
                    }) {
                        expected = Some((score, id));
                    }
                }
                let actual = pick_ray(&field, &rig, &pose, axis, threshold, magnitude);
                assert_eq!(
                    actual.as_ref().map(|p| &p.id),
                    expected.as_ref().map(|(_, id)| id)
                );
                if let Some(picked) = actual {
                    assert_eq!(picked_id(&field, &picked.id), Some(picked.clone()));
                    assert_eq!(field.picked.lock().unwrap().as_ref().unwrap().0, picked.id);
                }
            }
        }
    }

    #[test]
    fn generated_system_is_stable_and_has_no_tasks() {
        let home = GalaxyModel::milky_way().home_pc;
        let mut loader = Loader::new(Sky::default(), 6.5);
        loader.aim(home);
        let events = loader.step(&mut || true);
        let (cell, star) = events
            .into_iter()
            .find_map(|event| match event {
                space_simulation::stream::Event::Loaded(c) => {
                    c.stars.first().copied().map(|s| (c.cell, s))
                }
                _ => None,
            })
            .expect("nearby star");
        let picked = PickedStar {
            id: star.id(cell),
            position_ly: relative_ly(star.position_pc),
        };
        let lists = lists(vec![]);
        let mut nav = Navigation {
            selection: Some(Selection::Star(picked.clone())),
            ..default()
        };
        assert!(visit_selected(&mut nav, &lists));
        let first = nav.system(&lists).cloned().unwrap();
        assert!(!first.planets.is_empty());
        assert!(first.planets.iter().all(|p| p.task.is_none()));
        assert_eq!(first.name, catalogue_name(&picked.id));
        assert_eq!(
            first.planets[0].designation,
            planet_designation(&picked.id, 0)
        );
        assert_eq!(nav.selection, None);
        nav.selection = Some(Selection::Star(picked));
        assert!(visit_selected(&mut nav, &lists));
        assert_eq!(nav.system(&lists), Some(&first));
    }

    #[test]
    fn visiting_an_assigned_star_shows_its_tasks_without_changing_orbits() {
        let home = GalaxyModel::milky_way().home_pc;
        let mut sky = Sky::default();
        let cell = Cell::containing(10, home).unwrap();
        let star = sky.cell_star_at(cell, 0).unwrap();
        let mut list = space_simulation::demo_galaxy().systems.remove(0);
        list.page_stars = vec![PageStar {
            page: 0,
            id: star.id.clone(),
            position_ly: relative_ly(star.position_pc).to_array(),
        }];
        list.position_ly = list.page_stars[0].position_ly;
        let task = list.tasks().next().unwrap().1.clone();
        let task_id = task.id.clone();
        space_simulation::place(&mut list, vec![task]);
        let lists = lists(vec![list.clone()]);
        let mut nav = Navigation {
            selection: page_pick(&list, 0).map(Selection::Star),
            ..default()
        };
        assert_eq!(
            describe(&lists, &star.id),
            (list.name.clone(), Some((0, 1)))
        );
        assert!(visit_selected(&mut nav, &lists));
        assert_eq!(nav.list(), Some(0));
        assert_eq!(nav.page, 0);
        let shown = nav.system(&lists).unwrap();
        assert!(shown.task_planet(&task_id).is_some());
        assert_eq!(
            shown.planets[0].orbit,
            cell_planetary_system(&star.id).unwrap()[0].orbit
        );
    }

    #[test]
    fn visiting_a_later_list_page_keeps_its_own_star_and_page() {
        let home = GalaxyModel::milky_way().home_pc;
        let mut sky = Sky::default();
        let cell = Cell::containing(10, home).unwrap();
        let first = sky.cell_star_at(cell, 0).unwrap();
        let second = sky.cell_star_at(cell, 1).unwrap();
        let mut list = space_simulation::demo_galaxy().systems.remove(0);
        list.page_stars = [first, second]
            .into_iter()
            .enumerate()
            .map(|(page, star)| PageStar {
                page: page as u32,
                id: star.id,
                position_ly: relative_ly(star.position_pc).to_array(),
            })
            .collect();
        list.position_ly = list.page_stars[0].position_ly;
        let mut task = list.tasks().next().unwrap().1.clone();
        task.slot = space_simulation::planet_count(&list.page_stars[0].id);
        space_simulation::place(&mut list, vec![task]);
        let lists = lists(vec![list.clone()]);
        let mut nav = Navigation {
            selection: page_pick(&list, 1).map(Selection::Star),
            ..default()
        };
        assert!(visit_selected(&mut nav, &lists));
        assert_eq!(nav.page, 1);
        assert_eq!(nav.system(&lists), Some(&list));
    }
}
