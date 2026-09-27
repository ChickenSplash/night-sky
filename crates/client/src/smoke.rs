//! An opt-in graphical integration smoke test, also useful for visual regression review.
use crate::{Clock, Navigation, Universe, scene::CameraRig, views::View};
use bevy::{
    app::AppExit,
    prelude::*,
    render::view::screenshot::{Screenshot, save_to_disk},
};

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
    if !*enabled.get_or_insert_with(|| std::env::args().any(|a| a == "--smoke-test")) {
        return;
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
            nav.selected = Some(0);
            nav.focus_selected(&universe.0);
        }
        2 => capture(&mut commands, "planet"),
        3 => nav.show_atlas(&universe.0),
        4 => capture(&mut commands, "atlas"),
        5 => nav.jump(usize::from(universe.0.systems.len() > 1), &universe.0),
        6 => capture(&mut commands, "jump"),
        7 => nav.jump(0, &universe.0),
        9 => {
            // A deterministic large list exercises pagination without live credentials.
            let count = std::env::var("SPACE_SMOKE_TASKS")
                .ok()
                .and_then(|s| s.parse::<u32>().ok())
                .unwrap_or(50)
                .clamp(50, 1500);
            universe.0 = space_simulation::demo_galaxy();
            let system = &mut universe.0.systems[0];
            let template = system.tasks[0].clone();
            system.tasks = (0..count)
                .map(|slot| {
                    let mut task = template.clone();
                    task.id = format!("smoke-{slot}");
                    task.name =
                        format!("Task {}: review the mission navigation checklist", slot + 1);
                    task.slot = Some(slot);
                    task
                })
                .collect();
            space_simulation::shape(system);
            nav.jump(0, &universe.0);
            nav.show_page(0, &universe.0);
            nav.preset(View::Table, &universe.0);
        }
        10 => capture(&mut commands, "table"),
        11 => nav.show_page(
            universe.0.systems[0].pages().last().copied().unwrap_or(4),
            &universe.0,
        ),
        12 => capture(&mut commands, "table-last-page"),
        13 => nav.preset(View::Constellation, &universe.0),
        14 => capture(&mut commands, "constellation"),
        15 => nav.preset(View::Orbit, &universe.0),
        16 => capture(&mut commands, "restored-orbits"),
        17 => nav.show_page(1, &universe.0),
        19 => capture(&mut commands, "orbit-page-2"),
        20 => {
            nav.realistic = true;
            nav.jump(0, &universe.0);
        }
        21 => capture(&mut commands, "realistic"),
        22 => nav.physical = true,
        23 => capture(&mut commands, "realistic-physical"),
        24 => {
            nav.physical = false;
            nav.exaggerate = true;
            nav.selected = Some(0);
            nav.focus_selected(&universe.0);
        }
        25 => capture(&mut commands, "realistic-planet"),
        26 => {
            nav.exaggerate = false;
            nav.preset(View::Constellation, &universe.0);
        }
        27 => capture(&mut commands, "realistic-constellation"),
        _ => {
            exit.write(AppExit::Success);
        }
    }
    *step += 1;
}
