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

/// Select the closest star to the middle of the camera view. Only loaded
/// points are eligible, and selection never depends on their upload order.
pub fn pick_centre(
    field: &Starfield,
    rig: &CameraRig,
    pose: &CameraPose,
    rotation: Quat,
) -> Option<PickedStar> {
    pick_ray(field, rig, pose, (rotation * -Vec3::Z).as_dvec3(), 0.99985)
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
    tan_half: f64,
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
    )
}

fn pick_ray(
    field: &Starfield,
    rig: &CameraRig,
    pose: &CameraPose,
    forward: DVec3,
    min_alignment: f64,
) -> Option<PickedStar> {
    let eye = rig.galactic_ly(pose);
    let pc_per_ly = LY_METRES / PC_METRES;
    let reach = reach_sq_per_luminosity(crate::stream::MAGNITUDE) / (pc_per_ly * pc_per_ly);
    let mut best: Option<(f64, PickedStar)> = None;
    for (&cell, stars) in &field.cells {
        for star in &stars.stars {
            let position_ly = relative_ly(star.position_pc);
            let delta = position_ly - eye;
            let distance_sq = delta.length_squared();
            if distance_sq < 1e-16 || distance_sq > f64::from(star.luminosity_solar) * reach {
                continue;
            }
            let alignment = delta.dot(forward) / distance_sq.sqrt();
            // Prefer angular proximity within the caller's hit cone; a stable
            // ID resolves equal scores regardless of HashMap ordering.
            if alignment < min_alignment {
                continue;
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
            }
        }
    }
    best.map(|(_, star)| star)
}

/// Find a picked point by stable ID, never by its batch or vertex number.
pub fn picked_id(field: &Starfield, id: &str) -> Option<PickedStar> {
    let (cell, component, index) = space_simulation::cells::parse_cell_star(id)?;
    let star = field
        .cells
        .get(&cell)?
        .stars
        .iter()
        .find(|s| s.component == component && u64::from(s.index) == index)?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Selection;
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
    fn centre_pick_ignores_stars_below_the_visible_limit() {
        let home = GalaxyModel::milky_way().home_pc;
        let point = |distance: f64, index| StarPoint {
            position_pc: [home[0], home[1], home[2] - distance],
            luminosity_solar: 1.0,
            temperature_k: 5772.0,
            radius_solar: 1.0,
            component: Component::ThinDisc,
            index,
        };
        let cell = Cell::from_coords(0, [0, 0, 0]).unwrap();
        let mut field = Starfield::default();
        field.cells.insert(
            cell,
            StarCell {
                cell,
                stars: vec![point(100.0, 0), point(0.01, 1)],
            },
        );
        let rig = CameraRig::default();
        let pose = CameraPose::default();
        let picked = pick_centre(&field, &rig, &pose, Quat::IDENTITY).unwrap();
        assert_eq!(picked.id, point(0.01, 1).id(cell));
        field.cells.get_mut(&cell).unwrap().stars.pop();
        assert!(pick_centre(&field, &rig, &pose, Quat::IDENTITY).is_none());
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
        field.cells.insert(
            cell,
            StarCell {
                cell,
                stars: vec![star],
            },
        );
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
            tan_half,
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
                wide,
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
                tan_half,
            )
            .is_none()
        );
        assert!(picked_id(&field, "g1.2.8").is_none());
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
