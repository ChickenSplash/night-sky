//! The `render` tab: exposure readouts, rendering and performance switches.
use crate::Navigation;
use crate::actions::{Display, GalaxyReference, UiCommand};
use crate::render::debug::RenderDebug;
use crate::ui::terminal::{self};
use bevy::prelude::*;
use bevy_egui::egui;

pub fn render(ui: &mut egui::Ui, nav: &Navigation, debug: &RenderDebug, out: &mut Vec<UiCommand>) {
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
    terminal::heading(ui, "performance switches");
    for (switch, label) in RenderDebug::SWITCHES {
        let on = debug.get(switch);
        if terminal::check(ui, on, label, true) {
            out.push(UiCommand::SetDebug { switch, on: !on });
        }
    }
    terminal::note(
        ui,
        "For A/B tests against the perf tab; they reset on restart. Freeze first so the star count holds still. Without pixel shading, points still run the vertex shader, which separates vertex cost from fill. Compact points clip a little of each star's light.",
    );
}
