//! The `render` tab: exposure readouts and rendering controls.
use crate::Navigation;
use crate::actions::{Display, GalaxyReference, UiCommand};
use crate::ui::terminal::{self};
use bevy::prelude::*;
use bevy_egui::egui;

pub fn render(ui: &mut egui::Ui, nav: &Navigation, out: &mut Vec<UiCommand>) {
    terminal::heading(ui, "display");
    for (option, on, label, detail) in [
        (
            Display::Orbits,
            nav.orbits,
            "Orbital paths",
            "Draw orbital paths for planets and companion stars. Orbits keep running when paths are hidden.",
        ),
        (
            Display::Labels,
            nav.labels,
            "Planet labels",
            "Show planet names over the scene, including task names where a planet has an attached task.",
        ),
        (
            Display::Markers,
            nav.markers,
            "Body markers",
            "Show small rings for planets too small to see at physical size. Selection and hover rings remain visible; hiding markers does not disable picking.",
        ),
        (
            Display::VelocityMarker,
            nav.velocity_marker,
            "Velocity marker",
            "Show the direction of measured camera travel. Turning in place does not create a velocity marker.",
        ),
        (
            Display::Realistic,
            nav.realistic,
            "Realistic lighting",
            "Draw stars by temperature and physical brightness, and light planets by their stars. When off, bodies use wireframes. Exposure and compression apply only with realistic lighting.",
        ),
    ] {
        if terminal::check(ui, on, label, true, detail) {
            out.push(UiCommand::SetDisplay { option, on: !on });
        }
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
    for (option, on, label, detail) in [
        (
            Display::GalaxyGlow,
            nav.galaxy_glow,
            "Smooth galaxy glow (prototype)",
            "Draw unresolved galactic light as a smooth diffuse glow. This prototype has no dust or detailed spiral structure. Requires realistic lighting.",
        ),
        (
            Display::Wireframe,
            nav.wireframe,
            "Wireframes over bodies",
            "Overlay latitude and longitude wireframes on realistically lit bodies. Requires realistic lighting.",
        ),
    ] {
        if terminal::check(ui, on, label, nav.realistic, detail) {
            out.push(UiCommand::SetDisplay { option, on: !on });
        }
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

    #[test]
    fn display_switches_send_commands_from_render_tab() {
        let ctx = egui::Context::default();
        let nav = Navigation::default();
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
                    egui::CentralPanel::default().show(ctx, |ui| render(ui, &nav, &mut commands));
                },
            );
            (output, commands)
        };
        let (output, _) = frame(vec![]);
        for (label, option, on) in [
            ("Orbital paths", Display::Orbits, nav.orbits),
            ("Planet labels", Display::Labels, nav.labels),
            ("Body markers", Display::Markers, nav.markers),
            (
                "Velocity marker",
                Display::VelocityMarker,
                nav.velocity_marker,
            ),
            ("Realistic lighting", Display::Realistic, nav.realistic),
        ] {
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
            let (_, commands) = frame(vec![button(false)]);
            assert_eq!(commands, vec![UiCommand::SetDisplay { option, on: !on }]);
        }
    }
}
