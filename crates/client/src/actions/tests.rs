use super::*;
use crate::galaxy_nav::page_pick;
use space_simulation::{
    cells::{Sky, parse_cell_star},
    stream::{StarCell, StarPoint},
};

#[test]
fn remote_planet_selection_and_inspection_never_move_the_camera() {
    use crate::{
        objects::ObjectRef,
        ui::breadcrumb::{Inspection, TrailSource},
    };
    let galaxy = space_simulation::demo_galaxy();
    let id = galaxy.systems[1].planets[0].id.clone();
    let mut app = run(galaxy, vec![]);
    let before = (
        nav(&app).target,
        nav(&app).eye,
        nav(&app).distance,
        nav(&app).page,
    );
    send(&mut app, vec![UiCommand::Inspect { planet: id.clone() }]);
    assert_eq!(nav(&app).selection, Some(Selection::Planet(id.clone())));
    assert!(nav(&app).visit.is_none() && !nav(&app).fly && !nav(&app).following);
    assert_eq!(
        nav(&app).selected_planet(&app.world().resource::<Universe>().0),
        None
    );
    assert_eq!(
        nav(&app).inspection.as_ref().unwrap().object,
        ObjectRef::Planet { id: id.clone() }
    );
    let star = crate::objects::parent_star(&id).unwrap().to_owned();
    for inspection in [
        Inspection {
            source: TrailSource::Location,
            object: ObjectRef::Galaxy,
        },
        Inspection {
            source: TrailSource::Selection,
            object: ObjectRef::Star { id: star },
        },
        Inspection {
            source: TrailSource::Selection,
            object: ObjectRef::Planet { id: id.clone() },
        },
    ] {
        send(
            &mut app,
            vec![UiCommand::InspectObject {
                inspection: inspection.clone(),
            }],
        );
        assert_eq!(nav(&app).inspection, Some(inspection));
        assert_eq!(nav(&app).selection, Some(Selection::Planet(id.clone())));
        assert_eq!(
            (
                nav(&app).target,
                nav(&app).eye,
                nav(&app).distance,
                nav(&app).page
            ),
            before
        );
        assert!(!nav(&app).fly);
    }
}

#[test]
fn an_unstreamed_generated_planet_can_be_selected_by_id() {
    use space_simulation::{cells::Cell, galaxy::GalaxyModel};
    let cell = Cell::containing(10, GalaxyModel::milky_way().home_pc).unwrap();
    let star = Sky::default().cell_star_at(cell, 0).unwrap().id;
    let system = crate::galaxy_nav::generated(&star, bevy::math::DVec3::ZERO).unwrap();
    let id = system.planets[0].id.clone();
    let app = run(
        space_simulation::demo_galaxy(),
        vec![UiCommand::Inspect { planet: id.clone() }],
    );
    assert_eq!(nav(&app).selection, Some(Selection::Planet(id.clone())));
    assert!(nav(&app).visit.is_none());
    let galaxy = &app.world().resource::<Universe>().0;
    let trail = crate::ui::breadcrumb::selection_trail(nav(&app), galaxy);
    assert_eq!(trail.last().unwrap().label, system.planets[0].designation);
    let info = crate::ui::inspector::describe(&crate::objects::ObjectRef::Planet { id }, galaxy);
    assert_eq!(info.title, system.planets[0].designation);
    assert!(info.rows.iter().any(|(label, _)| label == "Mass"));
}

#[test]
fn inspector_tabs_preserve_tracking_and_reject_absent_crumbs() {
    use crate::{
        objects::ObjectRef,
        ui::breadcrumb::{Inspection, TrailSource},
    };
    let galaxy = space_simulation::demo_galaxy();
    let id = galaxy.systems[0].planets[0].id.clone();
    let mut app = run(galaxy, vec![]);
    {
        let mut nav = app.world_mut().resource_mut::<Navigation>();
        nav.visit = Some(crate::Visit::List(0));
        nav.arrived = true;
        nav.select(Some(Selection::Planet(id)));
        nav.following = true;
    }
    let inspection = Inspection {
        source: TrailSource::Location,
        object: ObjectRef::Galaxy,
    };
    send(
        &mut app,
        vec![UiCommand::InspectObject {
            inspection: inspection.clone(),
        }],
    );
    assert!(nav(&app).following);
    send(
        &mut app,
        vec![UiCommand::InspectObject {
            inspection: Inspection {
                source: TrailSource::Location,
                object: ObjectRef::Star {
                    id: "absent".into(),
                },
            },
        }],
    );
    assert_eq!(nav(&app).inspection, Some(inspection));
    assert!(nav(&app).following);
}

fn run(galaxy: space_shared::Galaxy, commands: Vec<UiCommand>) -> App {
    let mut app = App::new();
    app.add_message::<UiCommand>()
        .insert_resource(Universe(galaxy))
        .init_resource::<Navigation>()
        .init_resource::<Clock>()
        .init_resource::<crate::stars::stream::Starfield>()
        .init_resource::<crate::render::debug::RenderDebug>()
        .add_systems(Update, apply);
    send(&mut app, commands);
    app
}

fn send(app: &mut App, commands: Vec<UiCommand>) {
    for command in commands {
        app.world_mut().write_message(command);
    }
    app.update();
}

/// Streams a cell star into the loaded field, as the loader would.
fn stream(app: &mut App, id: &str) {
    let (cell, component, index) = parse_cell_star(id).unwrap();
    let star = Sky::default().star(id).unwrap();
    let mut field = app
        .world_mut()
        .resource_mut::<crate::stars::stream::Starfield>();
    let mut stars = field
        .cells
        .remove(&cell)
        .map_or_else(Vec::new, |loaded| loaded.stars);
    stars.push(StarPoint {
        position_pc: star.position_pc,
        luminosity_solar: star.star.luminosity_solar as f32,
        temperature_k: star.star.temperature_k as f32,
        radius_solar: star.star.radius_solar as f32,
        component,
        index: index as u32,
    });
    field.cells.insert(cell, StarCell::new(cell, stars));
}

fn nav(app: &App) -> &Navigation {
    app.world().resource::<Navigation>()
}

#[test]
fn velocity_marker_can_be_disabled_without_disabling_body_markers() {
    let mut app = run(
        space_simulation::demo_galaxy(),
        vec![UiCommand::SetDisplay {
            option: Display::VelocityMarker,
            on: false,
        }],
    );
    assert!(!nav(&app).velocity_marker);
    assert!(nav(&app).markers);
    send(
        &mut app,
        vec![UiCommand::SetDisplay {
            option: Display::VelocityMarker,
            on: true,
        }],
    );
    assert!(nav(&app).velocity_marker);
}

#[test]
fn field_of_view_is_clamped_to_its_range() {
    let set = |degrees| UiCommand::SetFov { degrees };
    let mut app = run(space_simulation::demo_galaxy(), vec![set(75.0)]);
    assert_eq!(nav(&app).fov_deg, 75.0);
    send(&mut app, vec![set(10.0)]);
    assert_eq!(nav(&app).fov_deg, 30.0);
    send(&mut app, vec![set(500.0), set(f32::NAN)]);
    assert_eq!(nav(&app).fov_deg, 130.0);
}

#[test]
fn magnitude_is_clamped_to_its_range_in_tenths() {
    let set = |magnitude| UiCommand::SetMagnitude { magnitude };
    let mut app = run(space_simulation::demo_galaxy(), vec![set(5.54)]);
    assert_eq!(nav(&app).magnitude, 5.5);
    send(&mut app, vec![set(1.0)]);
    assert_eq!(nav(&app).magnitude, 3.0);
    send(&mut app, vec![set(9.0), set(f64::NAN)]);
    assert_eq!(nav(&app).magnitude, 8.5);
}

#[test]
fn movement_speed_is_clamped_and_rejects_non_finite_values() {
    let set = |speed| UiCommand::SetMoveSpeed { speed };
    let mut app = run(space_simulation::demo_galaxy(), vec![set(1234.5)]);
    assert_eq!(nav(&app).move_speed, 1234.5);
    send(&mut app, vec![set(-10.0)]);
    assert_eq!(nav(&app).move_speed, crate::scene::MIN_SPEED);
    send(
        &mut app,
        vec![set(f64::MAX), set(f64::NAN), set(f64::INFINITY)],
    );
    assert_eq!(nav(&app).move_speed, crate::scene::MAX_SPEED);
}

#[test]
fn clicking_a_streamed_star_selects_then_visits_its_generated_system() {
    use space_simulation::cells::Cell;
    use space_simulation::galaxy::GalaxyModel;
    let home = GalaxyModel::milky_way().home_pc;
    let cell = Cell::containing(10, home).unwrap();
    let galaxy = space_simulation::demo_galaxy();
    let mut sky = Sky::default();
    let id = (0..)
        .map(|ordinal| sky.cell_star_at(cell, ordinal).unwrap().id)
        .find(|id| crate::galaxy_nav::mapped(&galaxy, id).is_none())
        .unwrap();
    let mut app = run(
        galaxy,
        vec![UiCommand::SetPresentationMode {
            presentation_mode: PresentationMode::Table,
        }],
    );
    // Presets need a system to frame.
    assert_eq!(nav(&app).presentation_mode, PresentationMode::Simulation);
    stream(&mut app, &id);
    send(&mut app, vec![UiCommand::PickStar { id: id.clone() }]);
    assert_eq!(nav(&app).selected_star().unwrap().id, id);
    assert!(nav(&app).visit.is_none());
    send(&mut app, vec![UiCommand::PickStar { id: id.clone() }]);
    let universe = &app.world().resource::<Universe>().0;
    let system = nav(&app).system(universe).unwrap();
    assert_eq!(system.id, id);
    assert!(system.planets.iter().all(|p| p.task.is_none()));
    assert_eq!(nav(&app).list(), None);
    // The table and page controls work on a generated star as on a list page.
    send(
        &mut app,
        vec![
            UiCommand::SetPresentationMode {
                presentation_mode: PresentationMode::Table,
            },
            UiCommand::ShowPage { page: 1 },
        ],
    );
    assert_eq!(nav(&app).presentation_mode, PresentationMode::Table);
    assert_eq!(nav(&app).page, 0);
}

#[test]
fn clicking_a_list_page_star_visits_the_list_at_that_page() {
    let galaxy = space_simulation::demo_galaxy();
    let star = galaxy.systems[1].page_star(1).unwrap().id.clone();
    let mut app = run(galaxy, vec![]);
    stream(&mut app, &star);
    send(
        &mut app,
        vec![
            UiCommand::PickStar { id: star.clone() },
            UiCommand::PickStar { id: star },
        ],
    );
    assert_eq!(nav(&app).list(), Some(1));
    assert_eq!(nav(&app).page, 1);
    assert_eq!(nav(&app).presentation_mode, PresentationMode::Simulation);
    assert_eq!(nav(&app).selection, None);
}

#[test]
fn inspector_actions_reach_remote_stars_and_planets_like_a_second_click() {
    let galaxy = space_simulation::demo_galaxy();
    let star = galaxy.systems[1].page_star(1).unwrap().id.clone();
    let planet = galaxy.systems[1]
        .planets
        .iter()
        .find(|planet| planet.page() == 1)
        .unwrap()
        .id
        .clone();
    let mut app = run(galaxy, vec![]);
    for (presentation_mode, selected) in [
        (PresentationMode::Table, false),
        (PresentationMode::Constellation, true),
        (PresentationMode::Simulation, false),
    ] {
        send(
            &mut app,
            vec![UiCommand::VisitStar {
                id: star.clone(),
                presentation_mode,
            }],
        );
        assert_eq!((nav(&app).list(), nav(&app).page), (Some(1), 1));
        assert_eq!(nav(&app).presentation_mode, presentation_mode);
        assert_eq!(
            nav(&app).selected_star().map(|picked| picked.id.as_str()),
            selected.then_some(star.as_str())
        );
    }
    send(
        &mut app,
        vec![UiCommand::GalaxyReference {
            view: GalaxyReference::FaceOn,
        }],
    );
    send(
        &mut app,
        vec![UiCommand::FlyToPlanet {
            planet: planet.clone(),
        }],
    );
    assert_eq!((nav(&app).list(), nav(&app).page), (Some(1), 1));
    assert_eq!(nav(&app).selection, Some(Selection::Planet(planet)));
    assert!(nav(&app).following);
}

#[test]
fn list_systems_select_like_a_star_click_and_keep_the_list_tab() {
    use crate::{
        objects::ObjectRef,
        ui::breadcrumb::{Inspection, TrailSource},
    };
    let galaxy = space_simulation::demo_galaxy();
    let list = galaxy.systems[1].id.clone();
    let second = galaxy.systems[1].page_star(1).unwrap().id.clone();
    let tab = Inspection {
        source: TrailSource::Selection,
        object: ObjectRef::List { id: list.clone() },
    };
    let mut app = run(galaxy, vec![UiCommand::Jump { list: list.clone() }]);
    send(
        &mut app,
        vec![UiCommand::InspectObject {
            inspection: tab.clone(),
        }],
    );
    // Unstreamed, as the stars of a distant list usually are.
    let pick = || {
        vec![
            UiCommand::PickStar { id: second.clone() },
            UiCommand::InspectObject {
                inspection: tab.clone(),
            },
        ]
    };
    send(&mut app, pick());
    assert_eq!(nav(&app).selected_star().unwrap().id, second);
    assert_eq!(nav(&app).presentation_mode, PresentationMode::Constellation);
    assert_eq!(nav(&app).inspection, Some(tab.clone()));
    send(&mut app, pick());
    assert_eq!((nav(&app).list(), nav(&app).page), (Some(1), 1));
    assert_eq!(nav(&app).presentation_mode, PresentationMode::Simulation);
    assert_eq!(nav(&app).inspection, Some(tab));
}

#[test]
fn unstreamed_generated_stars_are_not_visited_by_id() {
    let galaxy = space_simulation::demo_galaxy();
    use space_simulation::cells::Cell;
    let home = space_simulation::galaxy::GalaxyModel::milky_way().home_pc;
    let cell = Cell::containing(10, home).unwrap();
    let id = (0..)
        .map(|ordinal| Sky::default().cell_star_at(cell, ordinal).unwrap().id)
        .find(|id| crate::galaxy_nav::mapped(&galaxy, id).is_none())
        .unwrap();
    let mut app = run(
        galaxy,
        vec![UiCommand::FlyToPlanet {
            planet: format!("{id}/0"),
        }],
    );
    assert!(nav(&app).visit.is_none());
    stream(&mut app, &id);
    send(
        &mut app,
        vec![UiCommand::VisitStar {
            id: id.clone(),
            presentation_mode: PresentationMode::Table,
        }],
    );
    let universe = &app.world().resource::<Universe>().0;
    assert_eq!(nav(&app).system(universe).unwrap().id, id);
    assert_eq!(nav(&app).presentation_mode, PresentationMode::Table);
}

#[test]
fn commands_find_lists_and_tasks_by_id_not_position() {
    let galaxy = space_simulation::demo_galaxy();
    assert!(galaxy.systems.len() > 1);
    let list = galaxy.systems[1].id.clone();
    let task = galaxy.systems[1].tasks().last().unwrap().1.id.clone();
    let mut reordered = galaxy.clone();
    reordered.systems.reverse();
    let expected = reordered.systems.iter().position(|s| s.id == list).unwrap();
    let app = run(
        reordered,
        vec![
            UiCommand::FindTask {
                list: list.clone(),
                task: task.clone(),
            },
            UiCommand::SetPaused { paused: true },
            UiCommand::SetDisplay {
                option: Display::Labels,
                on: false,
            },
        ],
    );
    let nav = nav(&app);
    let universe = app.world().resource::<Universe>();
    assert_eq!(nav.list(), Some(expected));
    let selected = nav
        .selected_planet(&universe.0)
        .map(|i| &universe.0.systems[expected].planets[i]);
    assert_eq!(
        selected.and_then(|p| Some(&p.task.as_ref()?.id)),
        Some(&task)
    );
    assert!(!nav.labels);
    assert!(app.world().resource::<Clock>().0.paused);
}

#[test]
fn unknown_ids_are_ignored() {
    let app = run(
        space_simulation::demo_galaxy(),
        vec![
            UiCommand::Jump {
                list: "missing".into(),
            },
            UiCommand::Inspect {
                planet: "missing".into(),
            },
            UiCommand::PickStar {
                id: "g1.2.8".into(),
            },
        ],
    );
    let nav = nav(&app);
    assert!(nav.visit.is_none());
    assert_eq!(nav.selection, None);
}

#[test]
fn list_selection_always_opens_constellation() {
    let galaxy = space_simulation::demo_galaxy();
    let list = galaxy.systems[1].id.clone();
    let first = galaxy.systems[1].first_page();
    for start in [
        PresentationMode::Constellation,
        PresentationMode::Table,
        PresentationMode::Simulation,
    ] {
        let app = run(
            galaxy.clone(),
            vec![
                UiCommand::Jump {
                    list: galaxy.systems[0].id.clone(),
                },
                UiCommand::SetPresentationMode {
                    presentation_mode: start,
                },
                UiCommand::Jump { list: list.clone() },
            ],
        );
        let nav = nav(&app);
        assert_eq!(nav.list(), Some(1));
        assert_eq!(nav.page, first);
        assert_eq!(nav.presentation_mode, PresentationMode::Constellation);
        assert_eq!(
            nav.selected_star(),
            page_pick(&galaxy.systems[1], first).as_ref()
        );
        assert!(nav.fly);
    }
}

#[test]
fn page_navigation_selects_constellation_stars_and_keeps_other_presentation_modes() {
    let galaxy = space_simulation::demo_galaxy();
    for start in [
        PresentationMode::Constellation,
        PresentationMode::Table,
        PresentationMode::Simulation,
    ] {
        let app = run(
            galaxy.clone(),
            vec![
                UiCommand::Jump {
                    list: galaxy.systems[0].id.clone(),
                },
                UiCommand::SetPresentationMode {
                    presentation_mode: start,
                },
                UiCommand::ShowPage { page: 1 },
            ],
        );
        let nav = nav(&app);
        assert_eq!(nav.page, 1);
        assert_eq!(nav.presentation_mode, start);
        assert_eq!(
            nav.selection,
            (start == PresentationMode::Constellation)
                .then(|| page_pick(&galaxy.systems[0], 1))
                .flatten()
                .map(Selection::Star)
        );
    }
}

#[test]
fn script_json_parses_into_commands() {
    let parse = |json| serde_json::from_str::<UiCommand>(json).unwrap();
    assert_eq!(
        parse(r#"{"type":"galaxy_reference","view":"face_on"}"#),
        UiCommand::GalaxyReference {
            view: GalaxyReference::FaceOn
        }
    );
    assert_eq!(
        parse(r#"{"type":"set_presentation_mode","presentation_mode":"constellation"}"#),
        UiCommand::SetPresentationMode {
            presentation_mode: PresentationMode::Constellation
        }
    );
    assert_eq!(
        parse(r#"{"type":"set_display","option":"orbits","on":true}"#),
        UiCommand::SetDisplay {
            option: Display::Orbits,
            on: true
        }
    );
    assert_eq!(
        parse(r#"{"type":"set_fov","degrees":90}"#),
        UiCommand::SetFov { degrees: 90.0 }
    );
    assert_eq!(
        parse(r#"{"type":"pick_star","id":"g1.2.3"}"#),
        UiCommand::PickStar {
            id: "g1.2.3".into()
        }
    );
}

#[test]
fn reference_views_face_the_centre_and_clear_system_selection() {
    use space_simulation::galaxy::{GalaxyModel, PC_METRES};
    let home = bevy::math::DVec3::from_array(GalaxyModel::milky_way().home_pc);
    for view in [
        GalaxyReference::Inside,
        GalaxyReference::FaceOn,
        GalaxyReference::EdgeOn,
    ] {
        let mut nav = Navigation {
            visit: Some(crate::Visit::List(0)),
            selection: Some(Selection::Planet("fixture/0".into())),
            ..default()
        };
        galaxy_reference(&mut nav, view);
        let eye = nav.eye.unwrap() / PC_METRES + home;
        let direction = (nav.rotation * Vec3::NEG_Z).as_dvec3();
        assert!(direction.dot(-eye.normalize()) > 0.999999);
        assert!(nav.visit.is_none() && nav.selection.is_none());
        assert!(nav.realistic && nav.fly);
    }
}

#[test]
fn up_clears_the_selection_first_then_climbs_the_trail() {
    let galaxy = space_simulation::demo_galaxy();
    let id = galaxy.systems[0].id.clone();
    let mut app = run(galaxy, vec![UiCommand::Jump { list: id }]);
    assert!(nav(&app).selection.is_some());
    send(&mut app, vec![UiCommand::Up]);
    assert!(nav(&app).selection.is_none());
    assert_eq!(nav(&app).list(), Some(0));
    // From the constellation, up is the galaxy seen from above.
    send(&mut app, vec![UiCommand::Up]);
    assert!(nav(&app).visit.is_none());
    assert!(nav(&app).distance > 10_000.0 * space_simulation::LY_METRES);
    // The galaxy is the top.
    send(&mut app, vec![UiCommand::Up]);
    assert!(nav(&app).visit.is_none());
}

#[test]
fn up_from_a_tracked_planet_frames_its_system_then_climbs_to_its_parent() {
    use space_simulation::{cells::Cell, galaxy::GalaxyModel};
    let galaxy = space_simulation::demo_galaxy();
    let cell = Cell::containing(10, GalaxyModel::milky_way().home_pc).unwrap();
    let star = Sky::default().cell_star_at(cell, 0).unwrap();
    let generated = crate::galaxy_nav::generated(&star.id, bevy::math::DVec3::ZERO).unwrap();
    for visit in [
        crate::Visit::List(0),
        crate::Visit::Generated(Box::new(generated)),
    ] {
        let mut app = run(galaxy.clone(), vec![]);
        let (page, planet_id, distance) = {
            let nav = &mut app.world_mut().resource_mut::<Navigation>();
            let page = match &visit {
                crate::Visit::List(index) => galaxy.systems[*index].pages()[1],
                crate::Visit::Generated(system) => system.first_page(),
            };
            nav.arrive(visit, page, &galaxy);
            nav.arrived = true;
            let system = nav.system(&galaxy).unwrap();
            (
                page,
                system.on_page(page).next().unwrap().id.clone(),
                crate::presentation::fit_distance(system, page),
            )
        };
        send(
            &mut app,
            vec![UiCommand::Inspect { planet: planet_id }, UiCommand::Follow],
        );
        assert!(nav(&app).following);
        assert!(nav(&app).distance < distance);
        app.world_mut().resource_mut::<Navigation>().fly = false;
        send(&mut app, vec![UiCommand::Up]);
        let current = nav(&app);
        assert!(!current.following && current.selection.is_none());
        assert!(current.fly);
        assert_eq!(current.page, page);
        assert_eq!(current.presentation_mode, PresentationMode::Simulation);
        assert_eq!(current.target, bevy::math::DVec3::ZERO);
        assert_eq!(current.distance, distance);
        assert!(current.eye.is_none());
        assert_eq!(
            crate::ui::breadcrumb::trail(current, &galaxy)
                .last()
                .unwrap()
                .level,
            crate::ui::breadcrumb::Level::Star
        );
        let has_list = current.list().is_some();
        send(&mut app, vec![UiCommand::Up]);
        if has_list {
            assert_eq!(nav(&app).presentation_mode, PresentationMode::Constellation);
            assert_eq!(nav(&app).list(), Some(0));
        } else {
            assert!(nav(&app).visit.is_none());
        }
    }
}

#[test]
fn up_from_a_selected_but_untracked_planet_only_deselects() {
    let galaxy = space_simulation::demo_galaxy();
    let system = &galaxy.systems[0];
    let planet = system
        .on_page(system.first_page())
        .next()
        .unwrap()
        .id
        .clone();
    let mut app = run(
        galaxy.clone(),
        vec![
            UiCommand::Jump {
                list: system.id.clone(),
            },
            UiCommand::SetPresentationMode {
                presentation_mode: PresentationMode::Simulation,
            },
            UiCommand::Inspect { planet },
        ],
    );
    assert!(
        nav(&app)
            .selected_planet(&app.world().resource::<Universe>().0)
            .is_some()
            && !nav(&app).following
    );
    app.world_mut().resource_mut::<Navigation>().fly = false;
    let distance = nav(&app).distance;
    send(&mut app, vec![UiCommand::Up]);
    assert!(nav(&app).selection.is_none() && !nav(&app).fly);
    assert_eq!(nav(&app).distance, distance);
    assert_eq!(nav(&app).list(), Some(0));
}
