//! The debug console's look: the kit's widgets in `Theme::TERMINAL`, a
//! floating, translucent black window after the desktop's Hyprland windows,
//! with a light grey border and text-mode controls.
use super::kit::{self, Theme};
use bevy_egui::egui::{self, Color32, FontId, Ui};

pub use kit::SCROLL_SOURCE;
pub use kit::ScrollArea;

pub const THEME: Theme = Theme::TERMINAL;
pub const CYAN: Color32 = THEME.accent;
pub const TEXT: Color32 = THEME.text;
pub const DIM: Color32 = THEME.dim;

pub fn font() -> FontId {
    THEME.font.id()
}

/// A faint full-width line.
pub fn rule(ui: &mut Ui) {
    kit::rule(ui, &THEME);
}

/// A floating window, movable and resizable, that opens at the top right,
/// below the HUD icons. Its title-aligned `×` button clears `open`.
pub fn window(ctx: &egui::Context, title: &str, open: &mut bool, add: impl FnOnce(&mut Ui)) {
    kit::Window::new(title).show(ctx, &THEME, open, |ui| {
        // Console rows are contiguous; horizontal controls retain kit spacing.
        ui.spacing_mut().item_spacing.y = 0.0;
        add(ui);
    });
}

pub fn heading(ui: &mut Ui, text: &str) {
    kit::heading(ui, &THEME, text);
}

/// A persistent section toggle. Call its body only while expanded.
pub fn section(ui: &mut Ui, text: &str) -> bool {
    kit::section(ui, &THEME, text)
}

pub fn note(ui: &mut Ui, text: &str) {
    kit::note(ui, &THEME, text);
}

/// Split row: toggle on the left, square description button on the right.
pub fn check(ui: &mut Ui, on: bool, label: &str, enabled: bool, detail: &str) -> bool {
    kit::check(ui, &THEME, on, label, enabled, detail)
}

/// `[ label ]`, sized to its text.
pub fn button(ui: &mut Ui, label: &str) -> bool {
    kit::button(ui, &THEME, label).clicked()
}

pub fn toggle(ui: &mut Ui, on: bool, label: &str) -> bool {
    kit::toggle(ui, &THEME, on, label)
}

pub fn tab(ui: &mut Ui, label: &str, open: bool) -> bool {
    kit::tab(ui, &THEME, label, open)
}

/// `grep text`: a one-line text field with animated end edges.
pub fn input(ui: &mut Ui, prompt: &str, text: &mut String, width: f32) -> egui::Response {
    kit::input(ui, &THEME, prompt, text, width)
}

pub fn facts(ui: &mut Ui, id: &str, rows: &[(&str, String)]) {
    kit::facts(ui, &THEME, id, rows);
}

/// Facts whose rows with a detail show it below when clicked.
pub fn facts_with_details(ui: &mut Ui, id: &str, rows: &[(&str, String, Option<&str>)]) {
    kit::facts_with_details(ui, &THEME, id, rows);
}
