//! Label and value rows: labels on the left, values flush right, joined by
//! a dotted leader.
use super::Theme;
use super::interact::{Highlight, control_sense, held_click, highlight, hover_amount};
use bevy_egui::egui::{self, Ui, pos2, vec2};

/// Space kept between a label, its leader and its value.
const LEADER_GAP: f32 = 8.0;

pub fn facts(ui: &mut Ui, theme: &Theme, id: &str, rows: &[(&str, String)]) {
    for (label, value) in rows {
        fact(ui, theme, egui::Id::new((id, *label)), label, value, false);
    }
}

/// As `facts`, but a row with a detail has a clickable label, `label +`,
/// that shows the detail below it until clicked again.
pub fn facts_with_details(
    ui: &mut Ui,
    theme: &Theme,
    id: &str,
    rows: &[(&str, String, Option<&str>)],
) {
    for (label, value, detail) in rows {
        let open = fact(
            ui,
            theme,
            egui::Id::new((id, *label)),
            label,
            value,
            detail.is_some(),
        );
        if let (true, Some(detail)) = (open, detail) {
            super::note(ui, theme, detail);
        }
    }
}

/// One full-width fact row, lit on hover like every other row; a row with a
/// detail toggles it when clicked anywhere. A value too wide to share the
/// line with its label wraps, still flush right. Returns whether its detail
/// is open.
fn fact(ui: &mut Ui, theme: &Theme, id: egui::Id, label: &str, value: &str, expands: bool) -> bool {
    let mut open = expands && ui.data(|data| data.get_temp::<bool>(id).unwrap_or(false));
    let label = if expands {
        format!("{label} {}", if open { "-" } else { "+" })
    } else {
        label.to_owned()
    };
    let (font, inset) = (theme.font.id(), theme.inset);
    let width = ui.available_width();
    let label = ui.painter().layout_no_wrap(label, font.clone(), theme.dim);
    let wrap = (width - 2.0 * inset - label.size().x - 2.0 * LEADER_GAP).max(120.0);
    let value = ui
        .painter()
        .layout(value.to_owned(), font, theme.text, wrap);
    // Wrapped values keep a single row's padding above and below.
    let padding = (theme.row - label.size().y) / 2.0;
    let height = theme.row.max(value.size().y + 2.0 * padding);
    let (rect, response) = ui.allocate_exact_size(vec2(width, height), control_sense(expands));
    let response = held_click(response);
    let hover = hover_amount(ui, &response, true);
    highlight(ui, theme, rect, hover, Highlight::Whole);
    if expands
        && response
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .clicked()
    {
        open = !open;
        ui.data_mut(|data| data.insert_temp(id, open));
    }

    let painter = ui.painter();
    let top = rect.top() + padding;
    let label_right = rect.left() + inset + label.size().x;
    let value_left = rect.right() - inset - value.size().x;
    let dots = egui::Rangef::new(label_right + LEADER_GAP, value_left - LEADER_GAP);
    let y = top + label.size().y * 0.62;
    let mut x = (dots.min / 6.0).ceil() * 6.0;
    while x <= dots.max {
        painter.circle_filled(
            pos2(x, y),
            0.75,
            theme.dim.gamma_multiply(0.45 + 0.4 * hover),
        );
        x += 6.0;
    }
    painter.galley_with_override_text_color(
        pos2(rect.left() + inset, top),
        label,
        theme.dim.lerp_to_gamma(theme.accent, hover),
    );
    painter.galley(pos2(value_left, top), value, theme.text);
    open
}
