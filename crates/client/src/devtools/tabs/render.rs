//! The `render` tab: exposure readouts and rendering controls.
use crate::Navigation;
use crate::actions::{Display, GalaxyReference, UiCommand};
use crate::ui::terminal::{self};
use bevy::prelude::*;
use bevy_egui::egui;

pub fn render(ui: &mut egui::Ui, nav: &Navigation, out: &mut Vec<UiCommand>) {
    terminal::heading(ui, "exposure");
    terminal::facts_with_details(ui, "debug-exposure", &crate::render::exposure::facts(nav));
    terminal::heading(ui, "realistic rendering");
    for (option, on, label) in [
        (
            Display::GalaxyGlow,
            nav.galaxy_glow,
            "Smooth galaxy glow (prototype)",
        ),
        (Display::Wireframe, nav.wireframe, "Wireframes over bodies"),
    ] {
        if terminal::check(ui, on, label, nav.realistic) {
            out.push(UiCommand::SetDisplay { option, on: !on });
        }
    }
    terminal::note(
        ui,
        if nav.realistic {
            "Exposure and Dynamic Range Compression are manual controls at the bottom right."
        } else {
            "Turn on realistic lighting in Settings to use these."
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
