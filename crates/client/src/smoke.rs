//! An opt-in graphical integration smoke test, also useful for visual regression review.
use crate::{Clock, Navigation, Universe, Visit, presentation::PresentationMode, scene::CameraRig};
use bevy::{
    app::AppExit,
    prelude::*,
    render::view::screenshot::{Screenshot, save_to_disk},
};

/// Flies to a list's first system.
fn jump(nav: &mut Navigation, lists: &space_shared::Galaxy, index: usize) {
    let page = lists.systems.get(index).map_or(0, |s| s.first_page());
    nav.arrive(Visit::List(index), page, lists);
}

/// Captures a system-to-system jump halfway through its camera flight.
const MID_FLIGHT: usize = 8;
/// Captures a flight between two page stars at its interstellar midpoint.
const MID_PAGE: usize = 18;

#[allow(clippy::too_many_arguments)]
pub fn exercise(
    mut commands: Commands,
    mut step: Local<usize>,
    mut settled: Local<u32>,
    mut enabled: Local<Option<bool>>,
    mut nav: ResMut<Navigation>,
    mut universe: ResMut<Universe>,
    mut clock: ResMut<Clock>,
    rig: Res<CameraRig>,
    mut exit: MessageWriter<AppExit>,
) {
    let first = enabled.is_none();
    if !*enabled.get_or_insert_with(|| std::env::args().any(|a| a == "--smoke-test")) {
        return;
    }
    if first {
        // Wireframe captures first; the realistic ones opt in from step 20.
        nav.realistic = false;
    }
    let capture = |commands: &mut Commands, name: &str| {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../data")
            .join(format!("smoke-{name}.png"));
        commands
            .spawn(Screenshot::primary_window())
            .observe(save_to_disk(path));
    };
    if *step == MID_FLIGHT || *step == MID_PAGE {
        let (name, midway) = if *step == MID_FLIGHT {
            ("jump-flight", rig.progress().is_some_and(|p| p >= 0.45))
        } else {
            ("page-flight", rig.progress().is_some_and(|p| p >= 0.45))
        };
        if midway {
            capture(&mut commands, name);
            *step += 1;
        } else if !rig.flying() {
            *settled += 1;
            if *settled > 30 {
                warn!("Smoke test: no animation started for {name}");
                *step += 1;
            }
        }
        return;
    }
    // Screenshots wait for each camera flight to land, whatever the frame rate.
    if rig.flying() {
        *settled = 0;
        return;
    }
    *settled += 1;
    if *settled < if *step == 0 { 80 } else { 20 } {
        return;
    }
    *settled = 0;
    match *step {
        0 => {
            clock.0.paused = true;
            capture(&mut commands, "system");
        }
        1 => {
            if let Some(selection) = nav
                .system(&universe.0)
                .map(|system| crate::Selection::planet(system, 0))
            {
                nav.select(Some(selection));
            }
            nav.focus_selected(&universe.0);
        }
        2 => capture(&mut commands, "planet"),
        3 => {
            // Zoomed out from the visited star into the surrounding galaxy.
            nav.frame(&universe.0);
            nav.distance = 120.0 * space_simulation::LY_METRES;
        }
        4 => capture(&mut commands, "galaxy"),
        5 => jump(
            &mut nav,
            &universe.0,
            usize::from(universe.0.systems.len() > 1),
        ),
        6 => capture(&mut commands, "jump"),
        7 => jump(&mut nav, &universe.0, 0),
        9 => {
            // A deterministic large list exercises pagination without live credentials.
            let count = std::env::var("SPACE_SMOKE_TASKS")
                .ok()
                .and_then(|s| s.parse::<u32>().ok())
                .unwrap_or(50)
                .clamp(50, 1500);
            universe.0 = space_simulation::demo_galaxy();
            let system = &mut universe.0.systems[0];
            let template = system.planets[0].task.clone().unwrap();
            let tasks = (0..count)
                .map(|slot| space_shared::Task {
                    id: format!("smoke-{slot}"),
                    name: format!("Task {}: review the mission navigation checklist", slot + 1),
                    slot,
                    ..template.clone()
                })
                .collect();
            space_simulation::place(system, tasks);
            jump(&mut nav, &universe.0, 0);
            nav.show_page(0, &universe.0);
            nav.preset(PresentationMode::Table, &universe.0);
        }
        10 => capture(&mut commands, "table"),
        11 => nav.show_page(
            universe.0.systems[0].pages().last().copied().unwrap_or(4),
            &universe.0,
        ),
        12 => capture(&mut commands, "table-last-page"),
        13 => nav.preset(PresentationMode::Constellation, &universe.0),
        14 => capture(&mut commands, "constellation"),
        15 => nav.preset(PresentationMode::Simulation, &universe.0),
        16 => capture(&mut commands, "restored-orbits"),
        17 => nav.show_page(1, &universe.0),
        19 => capture(&mut commands, "orbit-page-2"),
        20 => {
            // Bodies alone, so the points are not hidden under their outlines.
            nav.realistic = true;
            // Galaxy exposure: compression must expose the system for itself.
            nav.set_exposure(0.0);
            nav.set_compression(1.0);
            nav.wireframe = false;
            nav.markers = false;
            jump(&mut nav, &universe.0, 0);
        }
        21 => capture(&mut commands, "realistic"),
        22 => nav.set_compression(0.0),
        23 => capture(&mut commands, "realistic-linear"),
        24 => {
            nav.set_compression(1.0);
            nav.wireframe = true;
            nav.exaggerate = true;
            if let Some(selection) = nav
                .system(&universe.0)
                .map(|system| crate::Selection::planet(system, 0))
            {
                nav.select(Some(selection));
            }
            nav.focus_selected(&universe.0);
        }
        25 => capture(&mut commands, "realistic-planet"),
        26 => {
            nav.exaggerate = false;
            nav.wireframe = false;
            nav.preset(PresentationMode::Constellation, &universe.0);
        }
        27 => capture(&mut commands, "realistic-constellation"),
        28 => {
            // Galaxy-friendly exposure: the corona must fade out smoothly.
            nav.set_exposure(0.0);
            jump(&mut nav, &universe.0, 0);
            let radius = universe.0.systems[0]
                .star(nav.page)
                .map_or(1.0, |s| s.radius_solar);
            nav.distance = 5.0 * radius * space_simulation::SUN_RADIUS_METRES;
            nav.rotation = Quat::from_rotation_y(0.3) * Quat::from_rotation_x(-0.2);
        }
        29 => capture(&mut commands, "realistic-star"),
        30 => nav.set_compression(0.0),
        31 => capture(&mut commands, "realistic-star-linear"),
        _ => {
            exit.write(AppExit::Success);
        }
    }
    *step += 1;
}
