use super::*;
use crate::{Selection, Visit, presentation::PresentationMode};

#[test]
fn selection_trail_is_remote_and_inspector_falls_back_when_a_location_disappears() {
    let galaxy = space_simulation::demo_galaxy();
    let mut nav = at_list(&galaxy, 0);
    let planet = &galaxy.systems[1].planets[0];
    nav.select(Some(Selection::Planet(planet.id.clone())));
    let location = trail(&nav, &galaxy);
    let selected = selection_trail(&nav, &galaxy, crate::craft::Now::none());
    assert_eq!(location.last().unwrap().level, Level::Star);
    assert_eq!(selected.last().unwrap().object, planet_ref(planet));
    assert_eq!(
        active_inspection(&nav, &galaxy, &location, &selected).object,
        planet_ref(planet)
    );
    nav.inspection = Some(Inspection {
        source: TrailSource::Location,
        object: location.last().unwrap().object.clone(),
    });
    nav.arrived = false;
    assert_eq!(
        active_inspection(&nav, &galaxy, &trail(&nav, &galaxy), &selected).object,
        planet_ref(planet)
    );
}

fn planet_ref(planet: &space_shared::Planet) -> ObjectRef {
    ObjectRef::Planet {
        id: planet.id.clone(),
    }
}

#[test]
fn a_local_planet_selection_survives_reordering_by_identity() {
    let mut galaxy = space_simulation::demo_galaxy();
    let mut nav = at_list(&galaxy, 0);
    let id = galaxy.systems[0].planets[0].id.clone();
    nav.select(Some(Selection::Planet(id.clone())));
    galaxy.systems[0].planets.reverse();
    let index = nav.selected_planet(&galaxy).unwrap();
    assert_eq!(galaxy.systems[0].planets[index].id, id);
}

fn labels(nav: &Navigation, galaxy: &Galaxy) -> Vec<String> {
    trail(nav, galaxy).into_iter().map(|c| c.label).collect()
}

fn at_list(galaxy: &Galaxy, page_index: usize) -> Navigation {
    let page = galaxy.systems[0].pages()[page_index];
    Navigation {
        visit: Some(Visit::List(0)),
        page,
        arrived: true,
        ..Navigation::default()
    }
}

#[test]
fn open_space_is_only_the_galaxy() {
    let galaxy = space_simulation::demo_galaxy();
    let nav = Navigation::default();
    assert_eq!(labels(&nav, &galaxy), ["Milky Way"]);
    assert_eq!(up(&nav, &galaxy), None);
}

#[test]
fn a_constellation_is_not_a_physical_location() {
    let galaxy = space_simulation::demo_galaxy();
    let nav = Navigation {
        presentation_mode: PresentationMode::Constellation,
        arrived: false,
        ..at_list(&galaxy, 0)
    };
    assert_eq!(labels(&nav, &galaxy), ["Milky Way"]);
    assert_eq!(
        up(&nav, &galaxy),
        Some(UiCommand::GalaxyReference {
            view: GalaxyReference::FaceOn
        })
    );
}

#[test]
fn a_list_star_slots_its_constellation_in_and_numbers_its_system() {
    let galaxy = space_simulation::demo_galaxy();
    let nav = at_list(&galaxy, 1);
    let trail = trail(&nav, &galaxy);
    assert_eq!(
        trail.iter().map(|c| c.level).collect::<Vec<_>>(),
        [Level::Galaxy, Level::List, Level::Star]
    );
    // A multiple system is named for all its stars, as its barycentre.
    let system = &galaxy.systems[0];
    let letters = system
        .barycentres_on(nav.page)
        .last()
        .map_or(String::new(), |b| format!(" {}", b.component));
    assert_eq!(trail[2].label, format!("System 2{letters}"));
    assert_eq!(
        up(&nav, &galaxy),
        Some(UiCommand::Jump {
            list: galaxy.systems[0].id.clone()
        })
    );
}

#[test]
fn physical_planet_location_is_independent_of_selection() {
    let galaxy = space_simulation::demo_galaxy();
    let mut nav = at_list(&galaxy, 0);
    let (_, planet) = galaxy.systems[0]
        .planets
        .iter()
        .enumerate()
        .find(|(_, p)| p.page() == nav.page)
        .unwrap();
    nav.select(Some(Selection::Planet(planet.id.clone())));
    assert_eq!(trail(&nav, &galaxy).last().unwrap().level, Level::Star);
    assert_eq!(
        selection_trail(&nav, &galaxy, crate::craft::Now::none())
            .last()
            .unwrap()
            .level,
        Level::Planet
    );
    nav.location_planet = Some(planet.id.clone());
    nav.select(None);
    let trail = trail(&nav, &galaxy);
    assert_eq!(trail.last().unwrap().label, planet.designation);
    assert_eq!(
        trail.last().unwrap().object,
        ObjectRef::Planet {
            id: planet.id.clone()
        }
    );
    assert_eq!(
        up(&nav, &galaxy),
        Some(UiCommand::ShowPage { page: nav.page })
    );
}

#[test]
fn a_generated_star_follows_the_galaxy_and_a_star_elsewhere_is_left_out() {
    use space_simulation::{
        cells::{Cell, Sky},
        galaxy::GalaxyModel,
    };
    let galaxy = space_simulation::demo_galaxy();
    let home = GalaxyModel::milky_way().home_pc;
    let cell = Cell::containing(10, home).unwrap();
    let id = Sky::default().cell_star_at(cell, 0).unwrap().id;
    let system = crate::galaxy_nav::generated(&id, bevy::math::DVec3::ZERO).unwrap();
    let name = system.name.clone();
    let nav = Navigation {
        visit: Some(Visit::Generated(Box::new(system))),
        arrived: true,
        selection: crate::galaxy_nav::page_pick(&galaxy.systems[1], galaxy.systems[1].first_page())
            .map(Selection::Star),
        ..Navigation::default()
    };
    assert_eq!(labels(&nav, &galaxy), ["Milky Way".to_owned(), name]);
    assert_eq!(text(&trail(&nav, &galaxy)).matches(" › ").count(), 1);
}

#[test]
fn not_yet_arrived_shows_no_star() {
    let galaxy = space_simulation::demo_galaxy();
    let nav = Navigation {
        arrived: false,
        ..at_list(&galaxy, 0)
    };
    assert_eq!(trail(&nav, &galaxy).len(), 1);
}

#[test]
fn a_companions_planet_is_reached_through_the_barycentre_and_its_star() {
    let galaxy = space_simulation::demo_galaxy();
    let system = &galaxy.systems[0];
    let mut nav = at_list(&galaxy, 1);
    let root = system
        .barycentres_on(nav.page)
        .last()
        .expect("the demo's second system is multiple");
    let primary = &system.star(nav.page).unwrap().id;
    let planet = system
        .on_page(nav.page)
        .find(|p| p.host != *primary && system.barycentre(&p.host).is_none())
        .expect("a planet of a companion");
    let letter = space_simulation::multiple::member_letters(&planet.host);
    let expected = [
        "Milky Way".to_owned(),
        system.name.clone(),
        format!("System 2 {}", root.component),
        format!("System 2 {letter}"),
        planet.designation.clone(),
    ];
    nav.location_planet = Some(planet.id.clone());
    let location = trail(&nav, &galaxy);
    assert_eq!(
        location.iter().map(|c| &c.label).collect::<Vec<_>>(),
        expected.iter().collect::<Vec<_>>()
    );
    assert_eq!(
        location[2].object,
        ObjectRef::Star {
            id: objects::star_id(system, nav.page)
        }
    );
    assert_eq!(
        location[3].object,
        ObjectRef::Star {
            id: planet.host.clone()
        }
    );
    nav.location_planet = None;
    nav.select(Some(Selection::Planet(planet.id.clone())));
    let selected = selection_trail(&nav, &galaxy, crate::craft::Now::none());
    assert_eq!(
        selected.iter().map(|c| &c.label).collect::<Vec<_>>(),
        expected.iter().collect::<Vec<_>>()
    );
    // Each crumb describes its own object.
    let fact = |object: &ObjectRef, label: &str| {
        crate::ui::inspector::describe(object, &galaxy, None, crate::craft::Now::none())
            .rows
            .into_iter()
            .find(|(l, _)| l == label)
            .map(|(_, value)| value)
    };
    assert_eq!(
        fact(&location[2].object, "Type").unwrap(),
        "Binary star system"
    );
    assert!(fact(&location[3].object, "Companion").is_some());
    assert!(
        fact(&location[4].object, "Orbits")
            .unwrap()
            .ends_with(letter)
    );
}
