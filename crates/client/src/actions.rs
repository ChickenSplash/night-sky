//! Every panel action goes through `UiCommand`, whether it comes from egui or a
//! browser test script (`automation.rs`). Lists and tasks are named by ID, so a
//! data refresh that reorders them cannot redirect a click.
use crate::{Clock, Navigation, Selection, Universe, views::View};
use bevy::prelude::*;
use serde::Deserialize;

#[derive(Message, Clone, Debug, PartialEq, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum UiCommand {
    /// Select the visible star nearest the centre of the view.
    PickCentre,
    /// Select a streamed star by stable ID, or visit it on a second click.
    PickStar {
        id: String,
    },
    /// Open a list's constellation from any view.
    Jump {
        list: String,
    },
    /// Fly to a task's page in its list and select it.
    FindTask {
        list: String,
        task: String,
    },
    /// Select a planet in the visited system without moving the camera.
    Inspect {
        planet: String,
    },
    ShowPage {
        page: u32,
    },
    SetView {
        view: View,
    },
    /// Visits the picked star, or re-frames the visited system.
    FrameSystem,
    Follow,
    SetPaused {
        paused: bool,
    },
    SetRate {
        rate: f64,
    },
    SetDisplay {
        option: Display,
        on: bool,
    },
    /// Vertical field of view, clamped to `FOV_DEG`.
    SetFov {
        degrees: f32,
    },
    /// Repeatable views for evaluating the smooth galaxy prototype.
    GalaxyReference {
        view: GalaxyReference,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Display {
    Exaggerate,
    Orbits,
    Labels,
    Markers,
    VelocityMarker,
    Realistic,
    GalaxyGlow,
    Physical,
    Wireframe,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GalaxyReference {
    Inside,
    FaceOn,
    EdgeOn,
}

fn galaxy_reference(nav: &mut Navigation, view: GalaxyReference) {
    use bevy::math::DVec3;
    use space_simulation::galaxy::{GalaxyModel, PC_METRES};
    let home = DVec3::from_array(GalaxyModel::milky_way().home_pc);
    let eye = match view {
        GalaxyReference::Inside => home,
        GalaxyReference::FaceOn => DVec3::new(0.0, 38000.0, 0.0),
        GalaxyReference::EdgeOn => DVec3::new(0.0, 0.0, 38000.0),
    };
    let up = if view == GalaxyReference::FaceOn {
        Vec3::Z
    } else {
        Vec3::Y
    };
    nav.visit = None;
    nav.page = 0;
    nav.selection = None;
    nav.following = false;
    nav.view = View::Orbit;
    // Keep the normal zoom range even for an external reference eye. Otherwise
    // the first wheel event clamps 124,000 ly to 40,000 ly and jumps inward.
    nav.distance = (eye.length() * PC_METRES).min(40_000.0 * space_simulation::LY_METRES);
    nav.target = (eye - home) * PC_METRES - eye.normalize() * nav.distance;
    nav.rotation = Transform::default()
        .looking_to((-eye).as_vec3(), up)
        .rotation;
    nav.eye = Some((eye - home) * PC_METRES);
    nav.set_fov(50.0);
    nav.realistic = true;
    nav.fly = true;
    nav.notice = "Smooth galaxy prototype: no dust or spiral detail yet".into();
}

pub fn apply(
    mut commands: MessageReader<UiCommand>,
    universe: Res<Universe>,
    mut nav: ResMut<Navigation>,
    mut clock: ResMut<Clock>,
    field: Res<crate::stream::Starfield>,
    rig: Res<crate::scene::CameraRig>,
    pose: Res<crate::scene::CameraPose>,
) {
    let galaxy = &universe.0;
    let list = |id: &str| galaxy.systems.iter().position(|s| s.id == id);
    for command in commands.read() {
        match command {
            UiCommand::PickCentre => {
                let star = crate::galaxy_nav::pick_centre(&field, &rig, &pose, rig.rotation());
                nav.select(star.map(Selection::Star));
            }
            UiCommand::PickStar { id } => {
                if nav.selected_star().is_some_and(|star| star.id == *id) {
                    crate::galaxy_nav::visit_selected(&mut nav, galaxy);
                } else if let Some(star) = crate::galaxy_nav::picked_id(&field, id) {
                    nav.select(Some(Selection::Star(star)));
                }
            }
            UiCommand::Jump { list: id } => {
                if let Some(index) = list(id) {
                    nav.select_list(index, galaxy);
                }
            }
            UiCommand::FindTask { list: id, task } => {
                if let Some(index) = list(id)
                    && let Some(planet) = galaxy.systems[index].task_planet(task)
                {
                    nav.select_task(index, planet, galaxy);
                }
            }
            UiCommand::Inspect { planet } => {
                if let Some(index) = nav
                    .system(galaxy)
                    .and_then(|s| s.planets.iter().position(|p| &p.id == planet))
                {
                    nav.select(Some(Selection::Planet(index)));
                }
            }
            UiCommand::ShowPage { page } => nav.show_page(*page, galaxy),
            UiCommand::SetView { view } => nav.preset(*view, galaxy),
            UiCommand::FrameSystem => {
                if !crate::galaxy_nav::visit_selected(&mut nav, galaxy) {
                    nav.frame(galaxy);
                }
            }
            UiCommand::Follow => nav.focus_selected(galaxy),
            UiCommand::SetPaused { paused } => clock.0.paused = *paused,
            UiCommand::SetRate { rate } => clock.0.rate = *rate,
            UiCommand::SetDisplay { option, on } => match option {
                Display::Exaggerate => nav.exaggerate = *on,
                Display::Orbits => nav.orbits = *on,
                Display::Labels => nav.labels = *on,
                Display::Markers => nav.markers = *on,
                Display::VelocityMarker => nav.velocity_marker = *on,
                Display::Realistic => nav.realistic = *on,
                Display::GalaxyGlow => nav.galaxy_glow = *on,
                Display::Physical => nav.physical = *on,
                Display::Wireframe => nav.wireframe = *on,
            },
            UiCommand::SetFov { degrees } => nav.set_fov(*degrees),
            UiCommand::GalaxyReference { view } => galaxy_reference(&mut nav, *view),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::galaxy_nav::page_pick;
    use space_simulation::{
        cells::{Sky, parse_cell_star},
        stream::{StarCell, StarPoint},
    };

    fn run(galaxy: space_shared::Galaxy, commands: Vec<UiCommand>) -> App {
        let mut app = App::new();
        app.add_message::<UiCommand>()
            .insert_resource(Universe(galaxy))
            .init_resource::<Navigation>()
            .init_resource::<Clock>()
            .init_resource::<crate::stream::Starfield>()
            .init_resource::<crate::scene::CameraRig>()
            .init_resource::<crate::scene::CameraPose>()
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
        let mut field = app.world_mut().resource_mut::<crate::stream::Starfield>();
        field.cells.entry(cell).or_insert(StarCell {
            cell,
            stars: vec![],
        });
        field.cells.get_mut(&cell).unwrap().stars.push(StarPoint {
            position_pc: star.position_pc,
            luminosity_solar: star.star.luminosity_solar as f32,
            temperature_k: star.star.temperature_k as f32,
            radius_solar: star.star.radius_solar as f32,
            component,
            index: index as u32,
        });
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
        let mut app = run(galaxy, vec![UiCommand::SetView { view: View::Table }]);
        // Presets need a system to frame.
        assert_eq!(nav(&app).view, View::Orbit);
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
                UiCommand::SetView { view: View::Table },
                UiCommand::ShowPage { page: 1 },
            ],
        );
        assert_eq!(nav(&app).view, View::Table);
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
        assert_eq!(nav(&app).view, View::Orbit);
        assert_eq!(nav(&app).selection, None);
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
            .selected_planet()
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
        for start in [View::Constellation, View::Table, View::Orbit] {
            let app = run(
                galaxy.clone(),
                vec![
                    UiCommand::Jump {
                        list: galaxy.systems[0].id.clone(),
                    },
                    UiCommand::SetView { view: start },
                    UiCommand::Jump { list: list.clone() },
                ],
            );
            let nav = nav(&app);
            assert_eq!(nav.list(), Some(1));
            assert_eq!(nav.page, first);
            assert_eq!(nav.view, View::Constellation);
            assert_eq!(
                nav.selected_star(),
                page_pick(&galaxy.systems[1], first).as_ref()
            );
            assert!(nav.fly);
        }
    }

    #[test]
    fn page_navigation_selects_constellation_stars_and_keeps_other_views() {
        let galaxy = space_simulation::demo_galaxy();
        for start in [View::Constellation, View::Table, View::Orbit] {
            let app = run(
                galaxy.clone(),
                vec![
                    UiCommand::Jump {
                        list: galaxy.systems[0].id.clone(),
                    },
                    UiCommand::SetView { view: start },
                    UiCommand::ShowPage { page: 1 },
                ],
            );
            let nav = nav(&app);
            assert_eq!(nav.page, 1);
            assert_eq!(nav.view, start);
            assert_eq!(
                nav.selection,
                (start == View::Constellation)
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
            parse(r#"{"type":"set_view","view":"constellation"}"#),
            UiCommand::SetView {
                view: View::Constellation
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
                selection: Some(Selection::Planet(0)),
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
}
