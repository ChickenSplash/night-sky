use super::*;
use crate::{Selection, Visit, presentation::PresentationMode};

#[test]
fn selection_trail_is_remote_and_inspector_falls_back_when_a_location_disappears() {
    let galaxy = space_simulation::demo_galaxy();
    let mut nav = at_list(&galaxy, 0);
    let planet = &galaxy.systems[1].planets[0];
    nav.select(Some(Selection::Planet(planet.id.clone())));
    let location = trail(&nav, &galaxy);
    let selected = selection_trail(&nav, &galaxy);
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
    assert_eq!(trail[2].label, "System 2");
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
        selection_trail(&nav, &galaxy).last().unwrap().level,
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
