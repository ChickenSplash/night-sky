use super::*;
use crate::Selection;
use space_shared::PageStar;
use space_simulation::galaxy::cells::planet_designation;
use space_simulation::galaxy::population::VISIBLE_MAGNITUDE;
use space_simulation::{
    galaxy::Component,
    galaxy::cells::{Cell, Sky},
    galaxy::stream::{Loader, StarCell, StarPoint},
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
        separation_au: 0.0,
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
            separation_au: 0.0,
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
        for magnitude in [6.5, 8.5, 10.5] {
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
                if expected
                    .as_ref()
                    .is_none_or(|(old, previous)| score < *old || (score == *old && id < *previous))
                {
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
            space_simulation::galaxy::stream::Event::Loaded(c) => {
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
        planet_designation(&first.planets[0].host, 0)
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
        space_simulation::galaxy::cells::cell_planetary_system(&star.id).unwrap()[0].orbit
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
