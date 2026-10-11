//! The `state` tab: camera, navigation, anchor star and clock.
use crate::format::speed_label;
use crate::presentation::PresentationMode;
use crate::scene::{CameraPose, CameraRig, CameraVelocity};
use crate::ui::terminal;
use crate::{Clock, Navigation, Selection, Universe, Visit};
use bevy::prelude::*;
use bevy_egui::egui;
use space_simulation::{AU_METRES, LY_METRES};

pub fn state(
    ui: &mut egui::Ui,
    universe: &Universe,
    nav: &Navigation,
    clock: &Clock,
    rig: &CameraRig,
    pose: &CameraPose,
    velocity: &CameraVelocity,
) {
    let galaxy = &universe.0;
    let ly = |v: bevy::math::DVec3| format!("{:+.3}  {:+.3}  {:+.3}", v.x, v.y, v.z);
    let range = if pose.distance > LY_METRES * 0.01 {
        format!("{:.3} ly", pose.distance / LY_METRES)
    } else {
        format!("{:.4} AU", pose.distance / AU_METRES)
    };
    terminal::heading(ui, "camera");
    terminal::facts(
        ui,
        "debug-camera",
        &[
            (
                "mode",
                if nav.following {
                    "tracking"
                } else if nav.presentation_mode != PresentationMode::Simulation {
                    "auto-framed preset"
                } else {
                    "free navigation"
                }
                .into(),
            ),
            ("range", range),
            ("set speed", speed_label(nav.move_speed)),
            ("velocity", speed_label(velocity.metres_per_second.length())),
            ("galactic (ly)", ly(rig.galactic_ly(pose))),
            ("field of view", format!("{:.0}°", nav.fov_deg)),
            (
                "flight",
                rig.progress()
                    .map_or("none".into(), |p| format!("{:.0}%", p * 100.0)),
            ),
        ],
    );
    terminal::heading(ui, "navigation");
    let system = nav.system(galaxy);
    let star = |system: Option<&space_shared::StarSystem>| {
        system
            .and_then(|s| s.page_star(nav.page))
            .map(|star| crate::galaxy_nav::label(galaxy, &star.id))
    };
    terminal::facts(
        ui,
        "debug-navigation",
        &[
            (
                "trail",
                crate::ui::breadcrumb::text(&crate::ui::breadcrumb::trail(nav, galaxy)),
            ),
            (
                "location",
                star(nav.location(galaxy)).unwrap_or_else(|| "open space".into()),
            ),
            (
                "list",
                nav.list()
                    .and_then(|i| galaxy.systems.get(i))
                    .map_or("none".into(), |s| s.name.clone()),
            ),
            (
                "anchor",
                match &nav.visit {
                    None => "home".into(),
                    Some(Visit::List(_)) => star(system).unwrap_or_else(|| "?".into()),
                    Some(Visit::Generated(s)) => s.name.clone(),
                },
            ),
            ("page", nav.page.to_string()),
            ("presentation mode", format!("{:?}", nav.presentation_mode)),
            (
                "selected",
                match &nav.selection {
                    None => "none".into(),
                    Some(Selection::Star(star)) => format!("star {}", star.id),
                    Some(Selection::Planet(id)) => format!("planet {id}"),
                },
            ),
        ],
    );
    if let Some(system) = system {
        let [inclination, node, _] = crate::scene::page_tilt(system, nav.page);
        terminal::heading(ui, "anchor star");
        terminal::facts(
            ui,
            "debug-star",
            &[
                (
                    "position (ly)",
                    ly(crate::scene::page_origin_ly(system, nav.page)),
                ),
                (
                    "plane tilt",
                    format!(
                        "{:.0}°, node {:.0}°",
                        inclination.to_degrees(),
                        node.to_degrees()
                    ),
                ),
            ],
        );
    }
    terminal::heading(ui, "clock");
    terminal::facts(
        ui,
        "debug-clock",
        &[
            ("simulated", format!("{:.3} s", clock.0.seconds)),
            ("rate", format!("{} s/s", clock.0.rate)),
            ("paused", clock.0.paused.to_string()),
        ],
    );
}
