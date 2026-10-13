//! The `log` tab: the client log with a level filter and grep.
use crate::devtools::{
    DevTools,
    capture::{LEVELS, Line},
};
use crate::ui::terminal::{self, DIM, TEXT};
use bevy::prelude::*;
use bevy_egui::egui;

/// Errors and warnings logged out of sight, such as `2 err 5 warn`.
pub fn unseen_badges(ui: &mut egui::Ui, unseen: [usize; 2]) {
    for (level, (count, label)) in unseen.into_iter().zip(["err", "warn"]).enumerate() {
        if count > 0 {
            let count = if count > 99 {
                "99+".to_string()
            } else {
                count.to_string()
            };
            ui.label(
                egui::RichText::new(format!("{count} {label}"))
                    .font(egui::FontId::monospace(11.0))
                    .color(level_colour(level)),
            );
        }
    }
}

pub fn log(ui: &mut egui::Ui, tools: &mut DevTools) {
    ui.horizontal(|ui| {
        for (i, level) in LEVELS.iter().enumerate() {
            if terminal::toggle(ui, tools.levels[i], level) {
                tools.levels[i] = !tools.levels[i];
            }
        }
        terminal::input(ui, "grep", &mut tools.grep, 160.0);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if terminal::button(ui, "clear") {
                tools.lines.clear();
            }
        });
    });
    let grep = tools.grep.trim().to_lowercase();
    let rows: Vec<&Line> = tools
        .lines
        .iter()
        .filter(|l| tools.levels[l.level] && (grep.is_empty() || l.search.contains(&grep)))
        .collect();
    terminal::note(
        ui,
        &format!("{} of {} lines", rows.len(), tools.lines.len()),
    );
    let font = egui::FontId::monospace(12.0);
    let height = ui.fonts_mut(|f| f.row_height(&font));
    egui::ScrollArea::both()
        .scroll_source(terminal::SCROLL_SOURCE)
        .id_salt("debug-log")
        .auto_shrink(false)
        .stick_to_bottom(true)
        .show_rows(ui, height, rows.len(), |ui, range| {
            ui.style_mut().spacing.item_spacing.y = 0.0;
            for line in &rows[range] {
                let mut job = egui::text::LayoutJob::default();
                let mut add = |text: &str, colour| {
                    job.append(text, 0.0, egui::TextFormat::simple(font.clone(), colour))
                };
                add(&format!("{:>9.3} ", line.seconds), DIM);
                add(
                    &format!("{:<5} ", LEVELS[line.level].to_uppercase()),
                    level_colour(line.level),
                );
                add(&format!("{}: ", line.target), DIM);
                add(&line.text, TEXT);
                ui.add(egui::Label::new(job).wrap_mode(egui::TextWrapMode::Extend));
            }
        });
}

pub fn level_colour(level: usize) -> egui::Color32 {
    match level {
        0 => egui::Color32::from_rgb(255, 110, 110),
        1 => egui::Color32::from_rgb(255, 196, 100),
        2 => TEXT,
        _ => DIM,
    }
}
