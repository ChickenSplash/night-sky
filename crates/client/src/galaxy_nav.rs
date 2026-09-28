//! Galaxy travel and on-demand systems, independent of ClickUp list pages.
use crate::{
    Navigation,
    scene::{CameraPose, CameraRig},
    stream::Starfield,
};
use bevy::{math::DVec3, prelude::*};
use space_shared::{Galaxy, PageStar, Planet, StarSystem};
use space_simulation::{
    AU_METRES, LY_METRES,
    cells::cell_planetary_system,
    cells::cell_star,
    galaxy::{GalaxyModel, PC_METRES},
};

/// A picked streamed star, identified by its stable cell ID, never a mesh index.
#[derive(Clone, Debug)]
pub struct PickedStar {
    pub id: String,
    pub position_pc: [f64; 3],
}

#[derive(Resource, Default)]
pub struct Galactic {
    pub picked: Option<PickedStar>,
    pub system: Option<StarSystem>,
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
    let eye = rig.galactic_ly(pose);
    let forward = (rotation * -Vec3::Z).as_dvec3();
    let mut best: Option<(f64, PickedStar)> = None;
    for (&cell, stars) in &field.cells {
        for star in &stars.stars {
            let delta = relative_ly(star.position_pc) - eye;
            let distance = delta.length();
            if distance < 1e-8 {
                continue;
            }
            let pc = distance * LY_METRES / PC_METRES;
            let magnitude = 4.83 - 2.5 * f64::from(star.luminosity_solar).max(1e-30).log10()
                + 5.0 * pc.log10()
                - 5.0;
            if magnitude > crate::stream::MAGNITUDE {
                continue;
            }
            let alignment = delta.dot(forward) / distance;
            // Approximately a one-degree crosshair. Prefer angular proximity;
            // a stable ID resolves equal scores regardless of HashMap ordering.
            if alignment < 0.99985 {
                continue;
            }
            let score = 1.0 - alignment;
            let picked = PickedStar {
                id: star.id(cell),
                position_pc: star.position_pc,
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

pub fn overview(nav: &mut Navigation, galactic: &mut Galactic, centre: DVec3) {
    nav.galactic = true;
    galactic.system = None;
    nav.atlas = false;
    nav.following = false;
    nav.selected = None;
    nav.view = crate::views::View::Orbit;
    nav.target = centre * LY_METRES;
    nav.distance = 120.0 * LY_METRES;
    nav.fly = true;
}

pub fn enter(nav: &mut Navigation, galactic: &mut Galactic, lists: &Galaxy) -> bool {
    let Some(picked) = &galactic.picked else {
        return false;
    };
    if let Some((system, page)) = lists.systems.iter().find_map(|system| {
        system
            .page_stars
            .iter()
            .find(|star| star.id == picked.id)
            .map(|star| (system, star.page))
    }) {
        galactic.system = Some(system.clone());
        nav.galactic = true;
        nav.page = page;
        nav.view = crate::views::View::Orbit;
        nav.target = DVec3::ZERO;
        nav.distance = fit_distance(system, page);
        nav.following = false;
        nav.selected = None;
        nav.fly = true;
        return true;
    }
    let Some((_, mut star)) = cell_star(&picked.id) else {
        return false;
    };
    let Some(bodies) = cell_planetary_system(&picked.id) else {
        return false;
    };
    star.page = 0;
    let planets = bodies
        .into_iter()
        .enumerate()
        .map(|(index, body)| Planet {
            id: format!("{}:{index}", picked.id),
            designation: format!("{} {}", picked.id, index + 1),
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
        .collect::<Vec<_>>();
    galactic.system = Some(StarSystem {
        id: picked.id.clone(),
        name: picked.id.clone(),
        position_ly: relative_ly(picked.position_pc).to_array(),
        planets,
        stars: vec![star],
        page_stars: vec![PageStar {
            page: 0,
            id: picked.id.clone(),
            position_ly: relative_ly(picked.position_pc).to_array(),
        }],
    });
    nav.galactic = true;
    nav.page = 0;
    nav.view = crate::views::View::Orbit;
    nav.target = DVec3::ZERO;
    nav.distance = fit_distance(galactic.system.as_ref().expect("created system"), 0);
    nav.following = false;
    nav.selected = None;
    nav.fly = true;
    true
}

pub fn fit_distance(system: &StarSystem, page: u32) -> f64 {
    system
        .on_page(page)
        .map(|p| p.orbit.semi_major_au * (1.0 + p.orbit.eccentricity))
        .fold(3.0, f64::max)
        * 3.1
        * AU_METRES
}

pub fn exit(nav: &mut Navigation, galactic: &mut Galactic) {
    let centre = galactic
        .picked
        .as_ref()
        .map_or(DVec3::ZERO, |p| relative_ly(p.position_pc));
    overview(nav, galactic, centre);
}

#[cfg(test)]
mod tests {
    use super::*;
    use space_simulation::{
        cells::{Cell, Sky},
        galaxy::Component,
        stream::{Loader, StarCell, StarPoint},
    };

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
            position_pc: star.position_pc,
        };
        let mut galactic = Galactic {
            picked: Some(picked.clone()),
            system: None,
        };
        let mut nav = Navigation::default();
        assert!(enter(
            &mut nav,
            &mut galactic,
            &Galaxy {
                source: String::new(),
                systems: vec![]
            }
        ));
        let first = galactic.system.clone().unwrap();
        assert!(!first.planets.is_empty());
        assert!(first.planets.iter().all(|p| p.task.is_none()));
        exit(&mut nav, &mut galactic);
        assert!(enter(
            &mut nav,
            &mut galactic,
            &Galaxy {
                source: String::new(),
                systems: vec![]
            }
        ));
        assert_eq!(galactic.system.as_ref(), Some(&first));
        assert_eq!(galactic.picked.as_ref().unwrap().id, picked.id);
    }

    #[test]
    fn entering_an_assigned_star_shows_its_tasks_without_changing_orbits() {
        let home = GalaxyModel::milky_way().home_pc;
        let mut sky = Sky::default();
        let cell = space_simulation::cells::Cell::containing(10, home).unwrap();
        let star = sky.cell_star_at(cell, 0).unwrap();
        let picked = PickedStar {
            id: star.id.clone(),
            position_pc: star.position_pc,
        };
        let mut list = space_simulation::demo_galaxy().systems.remove(0);
        list.page_stars = vec![PageStar {
            page: 0,
            id: star.id,
            position_ly: relative_ly(star.position_pc).to_array(),
        }];
        list.position_ly = list.page_stars[0].position_ly;
        let task = list.tasks().next().unwrap().1.clone();
        let task_id = task.id.clone();
        space_simulation::place(&mut list, vec![task]);
        let lists = Galaxy {
            source: String::new(),
            systems: vec![list.clone()],
        };
        let mut galactic = Galactic {
            picked: Some(picked),
            system: None,
        };
        let mut nav = Navigation::default();
        assert!(enter(&mut nav, &mut galactic, &lists));
        assert_eq!(galactic.system.as_ref(), Some(&list));
        assert!(
            galactic
                .system
                .as_ref()
                .unwrap()
                .task_planet(&task_id)
                .is_some()
        );
        assert_eq!(nav.page, 0);
        assert_eq!(
            list.planets[0].orbit,
            space_simulation::cells::cell_planetary_system(&list.page_stars[0].id).unwrap()[0]
                .orbit
        );
    }

    #[test]
    fn visiting_a_later_list_page_keeps_its_own_star_and_page() {
        let home = GalaxyModel::milky_way().home_pc;
        let mut sky = Sky::default();
        let cell = space_simulation::cells::Cell::containing(10, home).unwrap();
        let first = sky.cell_star_at(cell, 0).unwrap();
        let second = sky.cell_star_at(cell, 1).unwrap();
        let mut list = space_simulation::demo_galaxy().systems.remove(0);
        list.page_stars = [first, second.clone()]
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
        let lists = Galaxy {
            source: String::new(),
            systems: vec![list.clone()],
        };
        let mut galactic = Galactic {
            picked: Some(PickedStar {
                id: second.id,
                position_pc: second.position_pc,
            }),
            system: None,
        };
        let mut nav = Navigation::default();
        assert!(enter(&mut nav, &mut galactic, &lists));
        assert_eq!(nav.page, 1);
        assert_eq!(galactic.system.unwrap(), list);
    }
}
