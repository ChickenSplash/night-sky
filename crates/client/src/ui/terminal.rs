//! The terminal look of the debug window, after the desktop's Hyprland
//! windows: a floating, translucent black window with a light grey border and
//! text-mode controls.
use bevy_egui::egui::{self, Align2, Color32, FontId, Rect, Sense, Ui, epaint, pos2, vec2};

/// Space between the window and the screen's edges when it first opens.
const GAP: f32 = 12.0;
const RADIUS: u8 = 10;
const BORDER: f32 = 2.0;
const ROW: f32 = 24.0;
pub const CYAN: Color32 = Color32::from_rgb(0x33, 0xcc, 0xff);
const GREEN: Color32 = Color32::from_rgb(0x00, 0xff, 0x99);
const GREY: Color32 = Color32::from_rgb(0xc8, 0xcc, 0xd0);
pub const TEXT: Color32 = Color32::from_rgb(214, 228, 244);
pub const DIM: Color32 = Color32::from_rgb(110, 125, 140);
const FILL: Color32 = Color32::from_black_alpha(205);

pub fn font() -> FontId {
    FontId::monospace(13.0)
}

/// Clears the HUD icons at the top right, so the window never covers them.
fn below_icons(ctx: &egui::Context, top: f32) -> f32 {
    ctx.data(|data| data.get_temp::<(Rect, bool)>(egui::Id::new("icons-pointer-capture")))
        .map_or(top, |(icons, _)| top.max(icons.bottom() + GAP))
}

/// A floating window, movable and resizable, that opens at the top right,
/// below the HUD icons.
/// Its `[ × ]` button clears `open`.
pub fn window(ctx: &egui::Context, title: &str, open: &mut bool, add: impl FnOnce(&mut Ui)) {
    if !*open {
        return;
    }
    let screen = ctx.content_rect();
    let size = vec2(
        (screen.width() * 0.4).clamp(420.0, 760.0),
        (screen.height() * 0.6).clamp(320.0, 680.0),
    );
    let mut close = false;
    egui::Window::new(title)
        .id(egui::Id::new(("terminal", title)))
        .title_bar(false)
        .frame(
            egui::Frame::new()
                .fill(FILL)
                .stroke((BORDER, GREY.gamma_multiply(0.93)))
                .corner_radius(RADIUS)
                .inner_margin(16)
                .shadow(epaint::Shadow {
                    offset: [0, 6],
                    blur: 24,
                    spread: 0,
                    color: Color32::from_black_alpha(140),
                }),
        )
        .default_rect(Rect::from_min_size(
            pos2(
                screen.right() - GAP - size.x,
                below_icons(ctx, screen.top() + GAP),
            ),
            size,
        ))
        .min_size(vec2(320.0, 200.0))
        .resizable(true)
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            ui.style_mut().spacing.item_spacing = vec2(8.0, 4.0);
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("night-sky:~$ ").font(font()).color(DIM));
                ui.label(egui::RichText::new(title).font(font()).color(CYAN));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    close = button(ui, "×");
                });
            });
            ui.add_space(4.0);
            rule(ui);
            ui.add_space(4.0);
            add(ui);
        });
    if close {
        *open = false;
    }
}

/// A faint full-width line.
pub fn rule(ui: &mut Ui) {
    let line = ui.cursor().top();
    ui.painter().hline(
        ui.max_rect().x_range(),
        line,
        (1.0, DIM.gamma_multiply(0.4)),
    );
    ui.add_space(1.0);
}

/// The hover light shared by every row: a faint bar with a bright left edge.
fn highlight(ui: &Ui, rect: Rect, hover: f32) {
    if hover > 0.0 {
        let painter = ui.painter();
        painter.rect_filled(rect, 4, CYAN.gamma_multiply(0.12 * hover));
        painter.rect_filled(
            Rect::from_min_size(rect.min, vec2(2.0, rect.height())),
            0,
            CYAN.gamma_multiply(hover),
        );
    }
}

fn row(ui: &mut Ui, sense: Sense) -> (Rect, egui::Response, f32) {
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), ROW), sense);
    let hover = ui.ctx().animate_bool_with_time(
        response.id,
        response.hovered() && sense != Sense::hover(),
        0.1,
    );
    highlight(ui, rect, hover);
    let response = if sense == Sense::hover() {
        response
    } else {
        response.on_hover_cursor(egui::CursorIcon::PointingHand)
    };
    (rect, response, hover)
}

pub fn heading(ui: &mut Ui, text: &str) {
    ui.add_space(6.0);
    ui.label(
        egui::RichText::new(format!("# {text}"))
            .font(font())
            .color(CYAN),
    );
}

/// A persistent section toggle. Call its body only while expanded.
pub fn section(ui: &mut Ui, text: &str) -> bool {
    let id = ui.make_persistent_id(("section", text));
    let mut open = ui.data_mut(|data| data.get_persisted::<bool>(id).unwrap_or(true));
    ui.add_space(6.0);
    let (rect, response, _) = row(ui, Sense::click());
    ui.painter().text(
        pos2(rect.left() + 8.0, rect.center().y),
        Align2::LEFT_CENTER,
        format!("# {text} {}", if open { "-" } else { "+" }),
        font(),
        CYAN,
    );
    if response.clicked() {
        open = !open;
        ui.data_mut(|data| data.insert_persisted(id, open));
    }
    open
}

pub fn note(ui: &mut Ui, text: &str) {
    ui.label(
        egui::RichText::new(text)
            .font(FontId::monospace(11.0))
            .color(DIM),
    );
}

/// `[x] label`, clicked to toggle. Disabled rows are dimmed and ignore clicks.
pub fn check(ui: &mut Ui, on: bool, label: &str, enabled: bool) -> bool {
    let (rect, response, hover) = row(
        ui,
        if enabled {
            Sense::click()
        } else {
            Sense::hover()
        },
    );
    let painter = ui.painter();
    let text = if enabled {
        TEXT.lerp_to_gamma(CYAN, hover)
    } else {
        DIM.gamma_multiply(0.6)
    };
    let mark = match (enabled, on) {
        (false, _) => text,
        (true, true) => GREEN,
        (true, false) => text,
    };
    let left = rect.left() + 8.0;
    let galley = painter.layout_no_wrap(if on { "[x]" } else { "[ ]" }.into(), font(), mark);
    let width = galley.size().x;
    painter.galley(
        pos2(left, rect.center().y - galley.size().y / 2.0),
        galley,
        mark,
    );
    painter.text(
        pos2(left + width + 8.0, rect.center().y),
        Align2::LEFT_CENTER,
        label,
        font(),
        text,
    );
    enabled && response.clicked()
}

/// Text sized to itself that lights up on hover. Returns whether it was clicked.
fn chip(ui: &mut Ui, text: String, colour: Color32) -> bool {
    let size = ui
        .painter()
        .layout_no_wrap(text.clone(), font(), colour)
        .size()
        + vec2(8.0, 6.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let hover = ui
        .ctx()
        .animate_bool_with_time(response.id, response.hovered(), 0.1);
    let painter = ui.painter();
    painter.rect_filled(rect, 4, CYAN.gamma_multiply(0.15 * hover));
    painter.text(
        rect.center(),
        Align2::CENTER_CENTER,
        text,
        font(),
        colour.lerp_to_gamma(CYAN, hover),
    );
    response
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .clicked()
}

/// `[ label ]`, sized to its text.
pub fn button(ui: &mut Ui, label: &str) -> bool {
    chip(ui, format!("[ {label} ]"), TEXT)
}

/// `[x] label` on one line with other controls.
pub fn toggle(ui: &mut Ui, on: bool, label: &str) -> bool {
    chip(
        ui,
        format!("[{}] {label}", if on { "x" } else { " " }),
        if on { TEXT } else { DIM },
    )
}

/// A tab name, bracketed while it is the open tab.
pub fn tab(ui: &mut Ui, label: &str, open: bool) -> bool {
    if open {
        chip(ui, format!("[{label}]"), CYAN)
    } else {
        chip(ui, format!(" {label} "), DIM)
    }
}

/// `grep text`: a one-line text field, underlined.
pub fn input(ui: &mut Ui, prompt: &str, text: &mut String, width: f32) -> egui::Response {
    ui.label(egui::RichText::new(prompt).font(font()).color(DIM));
    let response = ui.add(
        egui::TextEdit::singleline(text)
            .font(font())
            .text_color(TEXT)
            .frame(false)
            .desired_width(width),
    );
    let colour = if response.has_focus() {
        CYAN
    } else {
        DIM.gamma_multiply(0.6)
    };
    ui.painter().hline(
        response.rect.x_range(),
        response.rect.bottom(),
        (1.0, colour),
    );
    response
}

/// Labels and values in two aligned columns.
pub fn facts(ui: &mut Ui, id: &str, rows: &[(&str, String)]) {
    let rows: Vec<_> = rows
        .iter()
        .map(|(label, value)| (*label, value.clone(), None))
        .collect();
    facts_with_details(ui, id, &rows);
}

/// As `facts`, but a row with a detail has a clickable label, `label +`,
/// that shows the detail below it until clicked again.
pub fn facts_with_details(ui: &mut Ui, id: &str, rows: &[(&str, String, Option<&str>)]) {
    let mut rows = rows.iter().peekable();
    let mut part = 0;
    while rows.peek().is_some() {
        let mut opened = None;
        // Each open detail ends a grid; the columns line up across them
        // because every label is narrower than the minimum column width.
        egui::Grid::new((id, part))
            .num_columns(2)
            .min_col_width(130.0)
            .spacing(vec2(16.0, 2.0))
            .show(ui, |ui| {
                for (label, value, detail) in rows.by_ref() {
                    let open = match detail {
                        Some(detail) => {
                            expander(ui, egui::Id::new((id, *label)), label).then_some(*detail)
                        }
                        None => {
                            ui.label(egui::RichText::new(*label).font(font()).color(DIM));
                            None
                        }
                    };
                    ui.label(egui::RichText::new(value).font(font()).color(TEXT));
                    ui.end_row();
                    if open.is_some() {
                        opened = open;
                        break;
                    }
                }
            });
        if let Some(detail) = opened {
            note(ui, detail);
        }
        part += 1;
    }
}

/// `label +` that lights up on hover and toggles on click, `label -` while
/// open. Returns whether it is open.
fn expander(ui: &mut Ui, id: egui::Id, label: &str) -> bool {
    let mut open = ui.data(|data| data.get_temp::<bool>(id).unwrap_or(false));
    let text = format!("{label} {}", if open { "-" } else { "+" });
    let galley = ui.painter().layout_no_wrap(text, font(), DIM);
    let (rect, response) = ui.allocate_exact_size(galley.size(), Sense::click());
    let hover = ui
        .ctx()
        .animate_bool_with_time(response.id, response.hovered(), 0.1);
    ui.painter().rect_filled(
        rect.expand2(vec2(4.0, 1.0)),
        4,
        CYAN.gamma_multiply(0.15 * hover),
    );
    ui.painter()
        .galley_with_override_text_color(rect.min, galley, DIM.lerp_to_gamma(CYAN, hover));
    if response
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .clicked()
    {
        open = !open;
        ui.data_mut(|data| data.insert_temp(id, open));
    }
    open
}
