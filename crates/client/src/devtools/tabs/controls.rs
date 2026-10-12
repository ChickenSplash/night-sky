//! The `controls` tab: camera, selection, time and console shortcuts.
use crate::ui::terminal;
use bevy_egui::egui;

pub fn controls(ui: &mut egui::Ui) {
    terminal::heading(ui, "controls");
    terminal::facts(
        ui,
        "debug-controls",
        &[
            ("Click", "Select a body, again to visit/follow".into()),
            ("Enter", "Visit the selected star".into()),
            ("Left drag", "Look around".into()),
            ("Right drag", "Orbit".into()),
            ("Shift + left drag", "Field of view".into()),
            ("Scroll", "Adjust movement speed".into()),
            ("Shift + scroll", "Zoom".into()),
            ("W A S D", "Move".into()),
            ("Shift", "Move faster".into()),
            ("Q / E", "Roll left / right".into()),
            ("R / F", "Up / down".into()),
            ("Space", "Pause / resume".into()),
            ("Esc", "Leave text input, reframe, clear or move up".into()),
            ("F3 / ` / cog", "Toggle debug console".into()),
            ("Drag set speed", "Adjust movement speed".into()),
            ("Click time rate", "Pause / resume".into()),
            ("Drag time rate", "Adjust simulation speed".into()),
            ("Drag Exposure", "Adjust manual exposure".into()),
            (
                "Drag Compression",
                "Adjust dynamic range compression".into(),
            ),
            (
                "Drag Magnitude limit",
                "Adjust visible stars on release".into(),
            ),
        ],
    );
}
