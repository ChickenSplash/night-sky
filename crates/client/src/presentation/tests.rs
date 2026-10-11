use super::*;
use space_simulation::{planet_count, system_page_of_slot};

#[test]
fn presentation_modes_use_the_public_names() {
    for (mode, name) in [
        (PresentationMode::Simulation, "simulation"),
        (PresentationMode::Table, "table"),
        (PresentationMode::Constellation, "constellation"),
    ] {
        let json = serde_json::to_string(&mode).unwrap();
        assert_eq!(json, format!("\"{name}\""));
        assert_eq!(
            serde_json::from_str::<PresentationMode>(&json).unwrap(),
            mode
        );
    }
    assert_eq!(PresentationMode::default(), PresentationMode::Simulation);
}

#[test]
fn constellation_pages_share_one_frame_without_reframing() {
    let galaxy = space_simulation::demo_galaxy();
    let system = &galaxy.systems[0];
    let pages = system.pages();
    assert!(pages.len() > 2);
    let mut nav = Navigation::default();
    nav.select_list(0, &galaxy);
    nav.fit_preset(&galaxy);
    let (target, distance, frame) = (nav.target, nav.distance, nav.frame_page(system));
    nav.rotation = Quat::from_rotation_y(0.7) * Quat::from_rotation_x(-0.4);
    let rotated = nav.rotation;
    nav.fly = false;
    for &page in pages.iter().skip(1).take(2) {
        nav.show_page(page, &galaxy);
        nav.fit_preset(&galaxy);
        assert_eq!(nav.presentation_mode, PresentationMode::Constellation);
        assert_eq!(
            nav.selected_star(),
            crate::galaxy_nav::page_pick(system, page).as_ref()
        );
        assert_eq!(nav.frame_page(system), frame);
        assert_eq!((nav.target, nav.distance), (target, distance));
        assert_eq!(nav.rotation, rotated);
        assert!(!nav.fly);
    }
}

fn large_galaxy() -> Galaxy {
    let mut galaxy = space_simulation::demo_galaxy();
    let template = galaxy.systems[0].planets[0].task.clone().unwrap();
    let tasks = (0..50)
        .map(|slot| space_shared::Task {
            id: format!("task-{slot}"),
            slot,
            ..template.clone()
        })
        .collect();
    space_simulation::place(&mut galaxy.systems[0], tasks);
    galaxy
}

#[test]
fn fifty_tasks_have_stable_pages_after_edits_and_removals() {
    let mut galaxy = large_galaxy();
    let system = &mut galaxy.systems[0];
    let last = system_page_of_slot(system, 49).0;
    assert!(system.pages().starts_with(&(0..=last).collect::<Vec<_>>()));
    assert_eq!(system.pages().len(), system.page_stars.len());
    for page in 0..last {
        assert_eq!(
            system.on_page(page).count() as u32,
            planet_count(&system.page_star(page).unwrap().id)
        );
    }
    let before = system.planets.clone();
    system.planets.reverse();
    system
        .planets
        .retain(|p| p.task.as_ref().is_none_or(|t| t.slot != 12));
    for task in system.planets.iter_mut().filter_map(|p| p.task.as_mut()) {
        task.status = "complete".into();
        task.due_date_ms = Some(123);
    }
    space_simulation::shape(system);
    // Every planet keeps its body; the removed task leaves a bare planet.
    assert_eq!(system.planets.len(), before.len());
    for planet in &system.planets {
        let original = before.iter().find(|p| p.id == planet.id).unwrap();
        assert_eq!(
            (planet.orbit, planet.page, planet.orbit_index),
            (original.orbit, original.page, original.orbit_index)
        );
        let Some(task) = &planet.task else {
            assert!(original.task.as_ref().is_none_or(|t| t.slot == 12));
            continue;
        };
        assert_eq!(task.id, original.task.as_ref().unwrap().id);
        let slot: u32 = task.id.strip_prefix("task-").unwrap().parse().unwrap();
        assert_eq!(planet.page(), system_page_of_slot(system, slot).0);
    }
}

#[test]
fn table_uses_true_sizes_and_fits_the_star() {
    let galaxy = large_galaxy();
    let system = &galaxy.systems[0];
    let layout = TableLayout::new(system, 1);
    let columns: Vec<_> = system.on_page(1).map(|p| layout.position(p).x).collect();
    assert_eq!(
        columns.len() as u32,
        planet_count(&system.page_star(1).unwrap().id)
    );
    assert!(columns.windows(2).all(|r| r[1] - r[0] > 2.0 * layout.unit));
    assert!(system.on_page(1).all(|p| layout.position(p).z == 0.0));
    // A large star shows only its right tip; a small one fits whole.
    let star = system.star(1).unwrap().radius_solar * SUN_RADIUS_METRES;
    if star >= 5.0 * layout.unit {
        assert!(((star - layout.left) / layout.unit - 1.0).abs() < 1e-10);
    } else {
        assert!(layout.left < -star);
    }
    assert!(columns[0] - layout.unit > star);
    assert!(columns[columns.len() - 1] + layout.unit < layout.right);
    let mut nav = Navigation {
        page: 1,
        visit: Some(crate::Visit::List(0)),
        ..Navigation::default()
    };
    nav.preset(PresentationMode::Table, &galaxy);
    nav.fit_preset(&galaxy);
    assert!(nav.distance < 3.0 * (layout.right - layout.left) && nav.distance > 3.0 * layout.unit);
}

#[test]
fn table_frames_every_planet_for_dwarfs_and_giants() {
    let mut galaxy = large_galaxy();
    let page = 1;
    for radius_solar in [0.01, 1.0, 1000.0] {
        galaxy.systems[0]
            .stars
            .iter_mut()
            .find(|star| star.page == page)
            .unwrap()
            .radius_solar = radius_solar;
        let system = &galaxy.systems[0];
        let layout = TableLayout::new(system, page);
        let nav = Navigation {
            page,
            visit: Some(crate::Visit::List(0)),
            presentation_mode: PresentationMode::Table,
            ..Navigation::default()
        };
        let (target, distance, top_down) = nav.framing(system).unwrap();
        assert!(top_down);
        assert!(distance.is_finite());
        // Default framing uses a square viewport and a 50° vertical FOV.
        let half_width = distance * 25.0_f64.to_radians().tan();
        let tolerance = layout.unit * 1e-8;
        assert!(layout.left + tolerance >= target.x - half_width);
        assert!(layout.right - tolerance <= target.x + half_width);
        for planet in system.on_page(page) {
            let x = layout.position(planet).x;
            let radius = planet.radius_km * 1000.0;
            assert!(x - radius > layout.left);
            assert!(x + radius < layout.right);
        }
    }
}

#[test]
fn presets_and_search_find_off_page_tasks() {
    let galaxy = large_galaxy();
    let mut nav = Navigation::default();
    nav.select_list(0, &galaxy);
    nav.select_task(0, 42, &galaxy);
    let page = galaxy.systems[0].planets[42].page;
    assert!(page > 0);
    assert_eq!(nav.page, page);
    assert_eq!(nav.selected_planet(&galaxy), Some(42));
    assert!(nav.presentation_mode == PresentationMode::Table);
    nav.fit_preset(&galaxy);
    assert!(nav.distance.is_finite());
    nav.preset(PresentationMode::Simulation, &galaxy);
    assert_eq!(nav.page, page);
    assert!(!nav.following);
}

#[test]
fn one_selection_replaces_the_other_and_following_stays_with_planets() {
    let galaxy = space_simulation::demo_galaxy();
    let mut nav = Navigation::default();
    nav.select_list(0, &galaxy);
    assert!(nav.selected_star().is_some());
    nav.select(Some(Selection::planet(&galaxy.systems[0], 0)));
    assert_eq!(nav.selected_star(), None);
    nav.following = true;
    nav.select(Some(Selection::planet(&galaxy.systems[0], 1)));
    assert!(nav.following);
    nav.select(crate::galaxy_nav::page_pick(&galaxy.systems[0], 0).map(Selection::Star));
    assert_eq!(nav.selected_planet(&galaxy), None);
    assert!(!nav.following);
}
