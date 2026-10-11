//! The `terminal` tab: typed commands and their output.
use super::log::level_colour;
use crate::devtools::DevTools;
use crate::ui::terminal::{self, TEXT};
use bevy::prelude::*;
use bevy_egui::egui;

/// Commands and their output, with the command line below.
pub fn terminal_tab(ui: &mut egui::Ui, tools: &mut DevTools, lines: &mut Vec<String>) {
    let font = egui::FontId::monospace(12.0);
    let height = ui.fonts_mut(|f| f.row_height(&font));
    let prompt = ui.fonts_mut(|f| f.row_height(&terminal::font())) + 12.0;
    if tools.terminal.is_empty() {
        terminal::note(
            ui,
            "Type help for the commands. On the desktop, the terminal that ran ./scripts/run.sh takes them too.",
        );
    }
    egui::ScrollArea::both()
        .id_salt("debug-terminal")
        .auto_shrink(false)
        .max_height((ui.available_height() - prompt).max(height))
        .stick_to_bottom(true)
        .show_rows(ui, height, tools.terminal.len(), |ui, range| {
            ui.style_mut().spacing.item_spacing.y = 0.0;
            for line in tools.terminal.range(range) {
                let colour = if line.text.starts_with("> ") {
                    terminal::CYAN
                } else if line.level < 2 {
                    level_colour(line.level)
                } else {
                    TEXT
                };
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(&line.text)
                            .font(font.clone())
                            .color(colour),
                    )
                    .wrap_mode(egui::TextWrapMode::Extend),
                );
            }
        });
    ui.add_space(4.0);
    if let Some(line) = command_line(ui, tools) {
        // Clearing is the window's own business; everything else runs as typed.
        if line.trim() == "clear" {
            tools.terminal.clear();
        } else {
            lines.push(line);
        }
    }
}

/// `> command`: Enter returns it, the arrow keys recall earlier ones.
fn command_line(ui: &mut egui::Ui, tools: &mut DevTools) -> Option<String> {
    let response = ui
        .horizontal(|ui| {
            let width = ui.available_width() - 24.0;
            terminal::input(ui, ">", &mut tools.command, width)
        })
        .inner;
    if std::mem::take(&mut tools.focus) {
        response.request_focus();
    }
    if response.has_focus() {
        // Up steps back through the history; down past the newest clears the line.
        let steps = ui.input(|i| {
            i.num_presses(egui::Key::ArrowDown) as isize
                - i.num_presses(egui::Key::ArrowUp) as isize
        });
        let last = tools.history.len() as isize;
        if steps != 0 && last > 0 {
            let from = tools.recalled.map_or(last, |i| i as isize);
            let to = (from + steps).clamp(0, last);
            let recalled = (to < last).then_some(to as usize);
            tools.recalled = recalled;
            tools.command = recalled.map_or(String::new(), |i| tools.history[i].clone());
            // Put the cursor after the recalled text, not where the arrow left it.
            if let Some(mut state) = egui::TextEdit::load_state(ui.ctx(), response.id) {
                let end = egui::text::CCursor::new(tools.command.chars().count());
                state
                    .cursor
                    .set_char_range(Some(egui::text::CCursorRange::one(end)));
                state.store(ui.ctx(), response.id);
            }
        }
    }
    if !(response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))) {
        return None;
    }
    response.request_focus();
    let line = std::mem::take(&mut tools.command);
    tools.recalled = None;
    if line.trim().is_empty() {
        return None;
    }
    if tools.history.last() != Some(&line) {
        tools.history.push(line.clone());
    }
    Some(line)
}
