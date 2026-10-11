//! The Settings window: display options, realistic lighting and controls help.
use super::{ACCENT, heading, muted};
use crate::Navigation;
use crate::actions::{Display, UiCommand};
use bevy::prelude::*;
use bevy_egui::egui;

pub(super) fn settings(ui: &mut egui::Ui, nav: &Navigation, out: &mut Vec<UiCommand>) {
    heading(ui, "DISPLAY");
    for (option, mut on, label) in [
        (Display::Exaggerate, nav.exaggerate, "Readable planet sizes"),
        (Display::Orbits, nav.orbits, "Orbital paths"),
        (Display::Labels, nav.labels, "Planet labels"),
        (Display::Markers, nav.markers, "Body markers"),
        (
            Display::VelocityMarker,
            nav.velocity_marker,
            "Velocity marker",
        ),
    ] {
        if ui.checkbox(&mut on, label).changed() {
            out.push(UiCommand::SetDisplay { option, on });
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
    muted(
        ui,
        "Simulation mode uses physical distances. The task table keeps true body sizes with schematic spacing. Orbits keep running in every presentation mode.",
    );
    ui.separator();
    heading(ui, "RENDERING");
    let mut realistic = nav.realistic;
    if ui.checkbox(&mut realistic, "Realistic lighting").changed() {
        out.push(UiCommand::SetDisplay {
            option: Display::Realistic,
            on: realistic,
        });
    }
    muted(
        ui,
        "Realistic lighting draws stars by temperature and true brightness, and lights planets by their star.",
    );
    ui.separator();
    heading(ui, "CONTROLS");
    egui::Grid::new("controls").num_columns(2).show(ui, |ui| {
        for (key, action) in HELP {
            ui.label(egui::RichText::new(key).monospace().color(ACCENT));
            ui.label(action);
            ui.end_row();
        }
    });
}

const HELP: [(&str, &str); 12] = [
    ("Click", "Select a star, again to visit"),
    ("Enter", "Visit the selected star"),
    ("Left drag", "Look around"),
    ("Right drag", "Orbit"),
    ("Shift + left drag", "Field of view"),
    ("Scroll", "Adjust movement speed"),
    ("Shift + scroll", "Zoom"),
    ("W A S D", "Move"),
    ("Q / E", "Roll left / right"),
    ("R / F", "Up / down"),
    ("Space", "Pause"),
    ("Esc", "Leave a text field"),
];
