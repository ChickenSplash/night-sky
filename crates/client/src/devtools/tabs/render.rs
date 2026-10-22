//! The `render` tab: exposure readouts and rendering controls.
use crate::Navigation;
use crate::actions::{Display, GalaxyReference, UiCommand};
use crate::ui::terminal::{self};
use bevy::prelude::*;
use bevy_egui::egui;

pub fn render(ui: &mut egui::Ui, nav: &Navigation, out: &mut Vec<UiCommand>) {
    terminal::heading(ui, "display");
    if terminal::check(
        ui,
        nav.overlays,
        "Scene overlays",
        true,
        "Draw everything over the scene that is not in it: the options below, and selection and hover rings, star and barycentre markers and constellation lines. Clicks still select bodies when hidden.",
    ) {
        out.push(UiCommand::SetDisplay {
            option: Display::Overlays,
            on: !nav.overlays,
        });
    }
    // Each overlay's own switch, which applies only while overlays are on.
    ui.indent("scene-overlays", |ui| {
        for (option, on, label, enabled, detail) in [
            (
                Display::Orbits,
                nav.orbits,
                "Orbital paths",
                true,
                "Draw orbital paths for planets and companion stars. Orbits keep running when paths are hidden.",
            ),
            (
                Display::Labels,
                nav.labels,
                "Planet labels",
                true,
                "Show planet names over the scene, including task names where a planet has an attached task.",
            ),
            (
                Display::Markers,
                nav.markers,
                "Body markers",
                true,
                "Show small rings for planets too small to see at physical size. Selection and hover rings remain visible; hiding markers does not disable picking.",
            ),
            (
                Display::VelocityMarker,
                nav.velocity_marker,
                "Velocity marker",
                true,
                "Show the direction of measured camera travel. Turning in place does not create a velocity marker.",
            ),
            (
                Display::PredictedPaths,
                nav.predicted_paths,
                "Craft predicted paths",
                true,
                "Draw the selected craft's predicted coast path, with its apsides and reference changes.",
            ),
            (
                Display::Wireframe,
                nav.wireframe,
                "Wireframes over bodies",
                nav.realistic,
                "Overlay latitude and longitude wireframes on realistically lit bodies. Requires realistic lighting; without it, bodies are wireframes.",
            ),
        ] {
            if terminal::check(ui, on, label, nav.overlays && enabled, detail) {
                out.push(UiCommand::SetDisplay { option, on: !on });
            }
        }
    });
    if terminal::check(
        ui,
        nav.realistic,
        "Realistic lighting",
        true,
        "Draw stars by temperature and physical brightness, and light planets by their stars. When off, bodies use wireframes. Exposure and compression apply only with realistic lighting.",
    ) {
        out.push(UiCommand::SetDisplay {
            option: Display::Realistic,
            on: !nav.realistic,
        });
    }
    let mut degrees = nav.fov_deg;
    if ui
        .add(
            egui::Slider::new(&mut degrees, crate::FOV_DEG)
                .text("Field of view")
                .suffix("°"),
        )
        .changed()
    {
        out.push(UiCommand::SetFov { degrees });
    }
    terminal::heading(ui, "exposure");
    terminal::facts_with_details(ui, "debug-exposure", &crate::render::exposure::facts(nav));
    terminal::heading(ui, "realistic rendering");
    if terminal::check(
        ui,
        nav.galaxy_glow,
        "Smooth galaxy glow (prototype)",
        nav.realistic,
        "Draw unresolved galactic light as a smooth diffuse glow. This prototype has no dust or detailed spiral structure. Requires realistic lighting.",
    ) {
        out.push(UiCommand::SetDisplay {
            option: Display::GalaxyGlow,
            on: !nav.galaxy_glow,
        });
    }
    terminal::note(
        ui,
        if nav.realistic {
            "Exposure and Dynamic Range Compression are manual controls at the bottom right."
        } else {
            "Turn on realistic lighting above to use these."
        },
    );
    terminal::heading(ui, "galaxy reference views");
    ui.horizontal_wrapped(|ui| {
        for (view, label) in [
            (GalaxyReference::Inside, "inside"),
            (GalaxyReference::FaceOn, "face-on"),
            (GalaxyReference::EdgeOn, "edge-on"),
        ] {
            if terminal::button(ui, label) {
                out.push(UiCommand::GalaxyReference { view });
            }
        }
    });
    terminal::note(ui, "Smooth glow only: no dust or spiral detail yet.");
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The commands a click on the switch labelled `label` sends.
    fn click(nav: &Navigation, label: &str) -> Vec<UiCommand> {
        let ctx = egui::Context::default();
        let frame = |events| {
            let mut commands = Vec::new();
            let output = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(800.0, 1000.0),
                    )),
                    events,
                    ..default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| render(ui, nav, &mut commands));
                },
            );
            (output, commands)
        };
        let (output, _) = frame(vec![]);
        let pos = output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::epaint::Shape::Text(text) if text.galley.job.text == label => {
                    Some(text.pos + text.galley.size() * 0.5)
                }
                _ => None,
            })
            .unwrap_or_else(|| panic!("missing display switch: {label}"));
        let button = |pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        frame(vec![egui::Event::PointerMoved(pos), button(true)]);
        frame(vec![button(false)]).1
    }

    const OVERLAYS: [(&str, Display); 6] = [
        ("Orbital paths", Display::Orbits),
        ("Planet labels", Display::Labels),
        ("Body markers", Display::Markers),
        ("Velocity marker", Display::VelocityMarker),
        ("Craft predicted paths", Display::PredictedPaths),
        ("Wireframes over bodies", Display::Wireframe),
    ];

    #[test]
    fn display_switches_send_commands_from_render_tab() {
        let nav = Navigation::default();
        for (label, option) in OVERLAYS.into_iter().chain([
            ("Scene overlays", Display::Overlays),
            ("Realistic lighting", Display::Realistic),
        ]) {
            assert_eq!(
                click(&nav, label),
                vec![UiCommand::SetDisplay { option, on: false }],
                "{label}"
            );
        }
    }

    #[test]
    fn overlay_switches_wait_for_scene_overlays() {
        let nav = Navigation {
            overlays: false,
            ..default()
        };
        for (label, _) in OVERLAYS {
            assert!(click(&nav, label).is_empty(), "{label}");
        }
        assert!(!nav.overlay(nav.orbits));
        assert_eq!(
            click(&nav, "Scene overlays"),
            vec![UiCommand::SetDisplay {
                option: Display::Overlays,
                on: true
            }]
        );
    }
}
