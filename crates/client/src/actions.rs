//! Every panel action goes through `UiCommand`, whether it comes from egui or the
//! browser's HTML panels. Lists and tasks are named by ID, so a data refresh that
//! reorders them cannot redirect a click.
use crate::{Clock, Navigation, Universe, views::View};
use bevy::prelude::*;
use serde::Deserialize;

#[derive(Message, Clone, Debug, PartialEq, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum UiCommand {
    Atlas,
    GalaxyView,
    PickGalactic,
    EnterGalactic,
    LeaveGalactic,
    ReturnToLists,
    /// Open a list's constellation from any view.
    Jump {
        list: String,
    },
    /// Fly to a task's page in its list and select it.
    FindTask {
        list: String,
        task: String,
    },
    /// Select a planet in the current list without moving the camera.
    Inspect {
        planet: String,
    },
    ShowPage {
        page: u32,
    },
    SetView {
        view: View,
    },
    /// Picks a star without moving the camera.
    Pick {
        list: String,
        page: u32,
    },
    /// Flies to the picked star, or re-frames the current system.
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
    /// Space the HTML panels cover at each edge of the canvas, in CSS pixels. The 3D
    /// view fits the rest, and the egui panels are not drawn.
    Panels {
        top: f32,
        right: f32,
        bottom: f32,
        left: f32,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Display {
    Exaggerate,
    Orbits,
    Labels,
    Markers,
    Realistic,
    Physical,
    Wireframe,
}

/// Set once HTML panels have taken over from egui: top, right, bottom, left.
#[derive(Resource, Default)]
pub struct PanelInsets(pub Option<[f32; 4]>);

#[allow(clippy::too_many_arguments)]
pub fn apply(
    mut commands: MessageReader<UiCommand>,
    universe: Res<Universe>,
    mut nav: ResMut<Navigation>,
    mut clock: ResMut<Clock>,
    mut insets: ResMut<PanelInsets>,
    mut galactic: ResMut<crate::galaxy_nav::Galactic>,
    field: Res<crate::stream::Starfield>,
    rig: Res<crate::scene::CameraRig>,
    pose: Res<crate::scene::CameraPose>,
) {
    let galaxy = &universe.0;
    let list = |id: &str| galaxy.systems.iter().position(|s| s.id == id);
    for command in commands.read() {
        match command {
            UiCommand::Atlas => nav.show_atlas(galaxy),
            UiCommand::GalaxyView => {
                crate::galaxy_nav::overview(&mut nav, &mut galactic, rig.galactic_ly(&pose))
            }
            UiCommand::PickGalactic => {
                if nav.galactic && galactic.system.is_none() {
                    galactic.picked =
                        crate::galaxy_nav::pick_centre(&field, &rig, &pose, rig.rotation());
                }
            }
            UiCommand::EnterGalactic => {
                if nav.galactic {
                    crate::galaxy_nav::enter(&mut nav, &mut galactic, galaxy);
                }
            }
            UiCommand::LeaveGalactic => crate::galaxy_nav::exit(&mut nav, &mut galactic),
            UiCommand::ReturnToLists => {
                let system = nav.system;
                nav.jump(system, galaxy);
                galactic.system = None;
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
                let active = if nav.galactic {
                    galactic.system.as_ref()
                } else {
                    galaxy.systems.get(nav.system)
                };
                if let Some(index) =
                    active.and_then(|s| s.planets.iter().position(|p| &p.id == planet))
                {
                    nav.selected = Some(index);
                }
            }
            UiCommand::ShowPage { page } => {
                if !nav.galactic {
                    nav.show_page(*page, galaxy);
                }
            }
            UiCommand::SetView { view } => {
                if nav.galactic {
                    if galactic.system.is_some() {
                        nav.view = *view;
                        nav.fly = true;
                    }
                } else {
                    nav.preset(*view, galaxy);
                }
            }
            UiCommand::Pick { list: id, page } => {
                if let Some(index) = list(id) {
                    nav.picked = Some((index, *page));
                }
            }
            UiCommand::FrameSystem => {
                if nav.galactic {
                    if let Some(system) = &galactic.system {
                        nav.view = View::Orbit;
                        nav.target = bevy::math::DVec3::ZERO;
                        nav.distance = crate::galaxy_nav::fit_distance(system, nav.page);
                        nav.fly = true;
                    }
                    continue;
                }
                let (system, page) = nav.picked.unwrap_or((nav.system, nav.page));
                if system != nav.system {
                    nav.jump(system, galaxy);
                }
                nav.page = page;
                nav.jump(system, galaxy);
            }
            UiCommand::Follow => {
                if nav.galactic {
                    if let Some(planet) = galactic
                        .system
                        .as_ref()
                        .and_then(|s| nav.selected.and_then(|i| s.planets.get(i)))
                    {
                        nav.view = View::Orbit;
                        nav.following = true;
                        nav.fly = true;
                        nav.distance = planet.radius_km * 12_000.0;
                    }
                } else {
                    nav.focus_selected(galaxy);
                }
            }
            UiCommand::SetPaused { paused } => clock.0.paused = *paused,
            UiCommand::SetRate { rate } => clock.0.rate = *rate,
            UiCommand::SetDisplay { option, on } => match option {
                Display::Exaggerate => nav.exaggerate = *on,
                Display::Orbits => nav.orbits = *on,
                Display::Labels => nav.labels = *on,
                Display::Markers => nav.markers = *on,
                Display::Realistic => nav.realistic = *on,
                Display::Physical => nav.physical = *on,
                Display::Wireframe => nav.wireframe = *on,
            },
            UiCommand::Panels {
                top,
                right,
                bottom,
                left,
            } => insets.0 = Some([*top, *right, *bottom, *left].map(|v| v.max(0.0))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(galaxy: space_shared::Galaxy, commands: Vec<UiCommand>) -> App {
        let mut app = App::new();
        app.add_message::<UiCommand>()
            .insert_resource(Universe(galaxy))
            .init_resource::<Navigation>()
            .init_resource::<Clock>()
            .init_resource::<PanelInsets>()
            .init_resource::<crate::galaxy_nav::Galactic>()
            .init_resource::<crate::stream::Starfield>()
            .init_resource::<crate::scene::CameraRig>()
            .init_resource::<crate::scene::CameraPose>()
            .add_systems(Update, apply);
        for command in commands {
            app.world_mut().write_message(command);
        }
        app.update();
        app
    }

    #[test]
    fn galaxy_mode_remains_separate_from_list_view_controls() {
        let mut app = run(space_simulation::demo_galaxy(), vec![UiCommand::GalaxyView]);
        assert!(app.world().resource::<Navigation>().galactic);
        app.world_mut()
            .write_message(UiCommand::SetView { view: View::Orbit });
        app.world_mut()
            .write_message(UiCommand::ShowPage { page: 1 });
        app.update();
        let nav = app.world().resource::<Navigation>();
        assert!(nav.galactic);
        assert_eq!(nav.page, 0);
        app.world_mut().write_message(UiCommand::ReturnToLists);
        app.update();
        assert!(!app.world().resource::<Navigation>().galactic);
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
        let nav = app.world().resource::<Navigation>();
        let universe = app.world().resource::<Universe>();
        assert_eq!(nav.system, expected);
        let selected = nav
            .selected
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
            ],
        );
        let nav = app.world().resource::<Navigation>();
        assert_eq!(nav.system, 0);
        assert_eq!(nav.selected, None);
    }

    #[test]
    fn list_selection_always_opens_constellation() {
        let galaxy = space_simulation::demo_galaxy();
        let list = galaxy.systems[1].id.clone();
        for start in [
            UiCommand::Atlas,
            UiCommand::SetView {
                view: View::Constellation,
            },
            UiCommand::SetView { view: View::Table },
            UiCommand::SetView { view: View::Orbit },
        ] {
            let app = run(
                galaxy.clone(),
                vec![start, UiCommand::Jump { list: list.clone() }],
            );
            let nav = app.world().resource::<Navigation>();
            assert_eq!(nav.system, 1);
            assert_eq!(nav.page, galaxy.systems[1].first_page());
            assert_eq!(nav.view, View::Constellation);
            assert_eq!(nav.picked, Some((1, galaxy.systems[1].first_page())));
            assert!(!nav.atlas);
            assert!(nav.fly);
        }
        let app = run(
            galaxy.clone(),
            vec![
                UiCommand::ShowPage { page: 2 },
                UiCommand::Jump {
                    list: galaxy.systems[0].id.clone(),
                },
            ],
        );
        let nav = app.world().resource::<Navigation>();
        assert_eq!(nav.page, galaxy.systems[0].first_page());
        assert_eq!(nav.picked, Some((0, nav.page)));
    }

    #[test]
    fn page_navigation_selects_constellation_stars_and_keeps_other_views() {
        let galaxy = space_simulation::demo_galaxy();
        for (start, expected) in [
            (View::Constellation, View::Constellation),
            (View::Table, View::Table),
            (View::Orbit, View::Orbit),
        ] {
            let app = run(
                galaxy.clone(),
                vec![
                    UiCommand::SetView { view: start },
                    UiCommand::ShowPage { page: 1 },
                ],
            );
            let nav = app.world().resource::<Navigation>();
            assert_eq!(nav.page, 1);
            assert_eq!(nav.view, expected);
            assert_eq!(nav.picked, (start == View::Constellation).then_some((0, 1)));
        }
    }

    #[test]
    fn browser_json_parses_into_commands() {
        let parse = |json| serde_json::from_str::<UiCommand>(json).unwrap();
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
        assert_eq!(parse(r#"{"type":"atlas"}"#), UiCommand::Atlas);
    }
}
