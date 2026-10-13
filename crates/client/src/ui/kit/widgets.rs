//! Headings, notes, switches, buttons, text fields, chips and list rows.
//! Text-mode themes draw controls as text (`[ button ]`); framed themes draw
//! outlines. Every control shares `interact`'s hover and press light.
use super::Theme;
use super::interact::{Highlight, control_sense, held_click, highlight, hover_amount, row};
use bevy_egui::egui::{self, Align2, Color32, Rect, Sense, Ui, pos2, vec2};

/// A faint full-width line.
pub fn rule(ui: &mut Ui, theme: &Theme) {
    let line = ui.cursor().top();
    ui.painter()
        .hline(ui.max_rect().x_range(), line, (1.0, theme.line()));
    ui.add_space(1.0);
}

/// Small spaced capitals, above a title or a group.
pub fn eyebrow(ui: &mut Ui, theme: &Theme, text: &str) {
    ui.label(
        egui::RichText::new(text.to_uppercase())
            .font(egui::FontId::monospace(10.0))
            .extra_letter_spacing(1.5)
            .color(theme.accent),
    );
}

fn heading_text(theme: &Theme, text: &str, open: Option<bool>) -> String {
    match (theme.text_mode, open) {
        (true, None) => format!("# {text}"),
        (true, Some(open)) => format!("# {text} {}", if open { "-" } else { "+" }),
        (false, None) => text.to_uppercase(),
        (false, Some(open)) => format!("{} {}", text.to_uppercase(), if open { "▾" } else { "▸" }),
    }
}

/// A fixed heading, laid out like a `section` row.
pub fn heading(ui: &mut Ui, theme: &Theme, text: &str) {
    ui.add_space(6.0);
    let (rect, _, _) = row(ui, theme, theme.row, false);
    ui.painter().text(
        pos2(rect.left() + theme.inset, rect.center().y),
        Align2::LEFT_CENTER,
        heading_text(theme, text, None),
        theme.font.id(),
        theme.accent,
    );
}

/// A persistent section toggle. Call its body only while expanded.
pub fn section(ui: &mut Ui, theme: &Theme, text: &str) -> bool {
    let id = ui.make_persistent_id(("section", text));
    let mut open = ui.data_mut(|data| data.get_persisted::<bool>(id).unwrap_or(true));
    ui.add_space(6.0);
    let (rect, response, _) = row(ui, theme, theme.row, true);
    ui.painter().text(
        pos2(rect.left() + theme.inset, rect.center().y),
        Align2::LEFT_CENTER,
        heading_text(theme, text, Some(open)),
        theme.font.id(),
        theme.accent,
    );
    if response.clicked() {
        open = !open;
        ui.data_mut(|data| data.insert_persisted(id, open));
    }
    open
}

/// Small dim text, inset to line up with the rows' text.
pub fn note(ui: &mut Ui, theme: &Theme, text: &str) {
    egui::Frame::new()
        .inner_margin(egui::Margin {
            left: theme.inset as i8,
            right: theme.inset as i8,
            ..egui::Margin::ZERO
        })
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new(text)
                    .font(theme.small.id())
                    .color(theme.dim),
            );
        });
}

/// Split row: toggle on the left, square description button on the right.
/// Disabled switches can still explain themselves, but never toggle.
pub fn check(
    ui: &mut Ui,
    theme: &Theme,
    on: bool,
    label: &str,
    enabled: bool,
    detail: &str,
) -> bool {
    let id = ui.make_persistent_id(("check-detail", label));
    let mut open = ui.data(|data| data.get_temp::<bool>(id).unwrap_or(false));
    let (inset, font) = (theme.inset, theme.font.id());
    let mut height = theme.row;
    let label = loop {
        let galley = ui.painter().layout(
            label.to_owned(),
            font.clone(),
            theme.text,
            (ui.available_width() - 2.0 * inset - 40.0 - height).max(40.0),
        );
        let needed = theme.row.max(galley.size().y + 8.0);
        if needed <= height {
            break galley;
        }
        height = needed;
    };
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::hover());
    let divider = rect.right() - height;
    let toggle_rect = Rect::from_min_max(rect.min, pos2(divider, rect.bottom()));
    let detail_rect = Rect::from_min_max(pos2(divider, rect.top()), rect.max);
    let response = held_click(ui.interact(toggle_rect, id.with("toggle"), control_sense(enabled)));
    let expand = held_click(ui.interact(detail_rect, id.with("expand"), control_sense(true)))
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    let hover = hover_amount(ui, &response, enabled);
    let detail_hover = hover_amount(ui, &expand, true);
    highlight(ui, theme, toggle_rect, hover, Highlight::Left);
    highlight(ui, theme, detail_rect, detail_hover, Highlight::Right);
    ui.painter()
        .vline(divider, rect.y_range(), (1.0, theme.line()));
    if expand.clicked() {
        open = !open;
    }
    let painter = ui.painter();
    let text = if enabled {
        theme.text.lerp_to_gamma(theme.accent, hover)
    } else {
        theme.dim.gamma_multiply(0.6)
    };
    let mark = if enabled && on { theme.on } else { text };
    let left = rect.left() + inset;
    let galley = painter.layout_no_wrap(if on { "[x]" } else { "[ ]" }.into(), font.clone(), mark);
    let width = galley.size().x;
    painter.galley(
        pos2(left, rect.center().y - galley.size().y / 2.0),
        galley,
        mark,
    );
    painter.galley_with_override_text_color(
        pos2(left + width + 8.0, rect.center().y - label.size().y / 2.0),
        label,
        text,
    );
    painter.text(
        detail_rect.center(),
        Align2::CENTER_CENTER,
        if open { "-" } else { "+" },
        font,
        theme.text.lerp_to_gamma(theme.accent, detail_hover),
    );
    let toggled = enabled && response.clicked();
    if enabled {
        response.on_hover_cursor(egui::CursorIcon::PointingHand);
    }
    ui.data_mut(|data| data.insert_temp(id, open));
    if open {
        note(ui, theme, detail);
    }
    toggled
}

/// Text sized to itself that lights up on hover.
pub(super) fn chip_response(
    ui: &mut Ui,
    theme: &Theme,
    text: String,
    colour: Color32,
) -> egui::Response {
    let size = ui
        .painter()
        .layout_no_wrap(text.clone(), theme.font.id(), colour)
        .size()
        + vec2(8.0, 6.0);
    let (rect, response) = ui.allocate_exact_size(size, control_sense(true));
    let response = held_click(response);
    let hover = hover_amount(ui, &response, true);
    highlight(ui, theme, rect, hover, Highlight::Whole);
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        text,
        theme.font.id(),
        colour.lerp_to_gamma(theme.accent, hover),
    );
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// An outlined control with `text`; `strong` fills it with the accent.
fn framed(ui: &mut Ui, theme: &Theme, text: &str, strong: bool, enabled: bool) -> egui::Response {
    let colour = if !enabled {
        theme.dim.gamma_multiply(0.6)
    } else if strong {
        theme.accent
    } else {
        theme.text
    };
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_owned(), theme.font.id(), colour);
    let size = galley.size() + vec2(24.0, 14.0);
    let (rect, response) = ui.allocate_exact_size(size, control_sense(enabled));
    let response = held_click(response);
    let hover = hover_amount(ui, &response, enabled);
    let base = if strong { 0.1 } else { 0.03 };
    ui.painter().rect(
        rect,
        theme.control_radius,
        theme.accent.gamma_multiply(base + 0.1 * hover),
        (
            1.0,
            theme.line().lerp_to_gamma(
                theme.accent.gamma_multiply(0.7),
                hover.max(f32::from(strong) * 0.5),
            ),
        ),
        egui::StrokeKind::Inside,
    );
    ui.painter()
        .galley(rect.center() - galley.size() / 2.0, galley, colour);
    if enabled {
        response.on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        response
    }
}

/// `[ label ]` in text mode, else an outlined button.
pub fn button(ui: &mut Ui, theme: &Theme, label: &str) -> egui::Response {
    if theme.text_mode {
        chip_response(ui, theme, format!("[ {label} ]"), theme.text)
    } else {
        framed(ui, theme, label, false, true)
    }
}

/// The main action of a group: accent text in a lit frame. Disabled ones
/// are dim and ignore clicks.
pub fn primary_button(ui: &mut Ui, theme: &Theme, label: &str, enabled: bool) -> egui::Response {
    framed(ui, theme, label, true, enabled)
}

/// Text that acts like a button without a frame, such as `Reset`.
pub fn link(ui: &mut Ui, theme: &Theme, label: &str) -> egui::Response {
    chip_response(ui, theme, label.to_owned(), theme.dim)
}

/// `[x] label` on one line with other controls.
pub fn toggle(ui: &mut Ui, theme: &Theme, on: bool, label: &str) -> bool {
    chip_response(
        ui,
        theme,
        format!("[{}] {label}", if on { "x" } else { " " }),
        if on { theme.text } else { theme.dim },
    )
    .clicked()
}

/// A tab name, bracketed while it is the open tab.
pub fn tab(ui: &mut Ui, theme: &Theme, label: &str, open: bool) -> bool {
    if open {
        chip_response(ui, theme, format!("[{label}]"), theme.accent)
    } else {
        chip_response(ui, theme, format!(" {label} "), theme.dim)
    }
    .clicked()
}

/// One of a row of choices, such as a scope; lit while chosen.
pub fn choice(ui: &mut Ui, theme: &Theme, label: &str, chosen: bool) -> bool {
    if theme.text_mode {
        return tab(ui, theme, label, chosen);
    }
    let colour = if chosen { theme.accent } else { theme.dim };
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_owned(), theme.small.id(), colour);
    let size = galley.size() + vec2(18.0, 10.0);
    let (rect, response) = ui.allocate_exact_size(size, control_sense(true));
    let response = held_click(response);
    let hover = hover_amount(ui, &response, true);
    let lit = if chosen { 1.0 } else { hover };
    ui.painter().rect(
        rect,
        theme.control_radius,
        theme.accent.gamma_multiply(0.1 * lit),
        (
            1.0,
            theme
                .line()
                .lerp_to_gamma(theme.accent.gamma_multiply(0.6), lit),
        ),
        egui::StrokeKind::Inside,
    );
    ui.painter().galley(
        rect.center() - galley.size() / 2.0,
        galley,
        colour.lerp_to_gamma(theme.accent, hover),
    );
    response
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .clicked()
}

/// `grep text`: a one-line text field after a prompt, underlined.
pub fn input(
    ui: &mut Ui,
    theme: &Theme,
    prompt: &str,
    text: &mut String,
    width: f32,
) -> egui::Response {
    ui.label(
        egui::RichText::new(prompt)
            .font(theme.font.id())
            .color(theme.dim),
    );
    let response = ui.add(
        egui::TextEdit::singleline(text)
            .font(theme.font.id())
            .text_color(theme.text)
            .frame(false)
            .desired_width(width),
    );
    let colour = if response.has_focus() {
        theme.accent
    } else {
        theme.dim.gamma_multiply(0.6)
    };
    ui.painter().hline(
        response.rect.x_range(),
        response.rect.bottom(),
        (1.0, colour),
    );
    response
}

/// A framed one-line field, `width` wide (the rest of the line if
/// infinite), with dim `hint` text while empty.
pub fn field(
    ui: &mut Ui,
    theme: &Theme,
    id: &str,
    hint: &str,
    text: &mut String,
    width: f32,
) -> egui::Response {
    let width = width.min(ui.available_width());
    let height = theme.row + 12.0;
    let (rect, _) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
    // The frame goes behind the text, which is laid out first.
    let frame = ui.painter().add(egui::Shape::Noop);
    let inner = rect.shrink2(vec2(12.0, 0.0));
    let response = ui
        .scope_builder(
            egui::UiBuilder::new()
                .max_rect(inner)
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
            |ui| {
                ui.add(
                    egui::TextEdit::singleline(text)
                        .id(egui::Id::new(("field", id)))
                        .font(theme.font.id())
                        .text_color(theme.text)
                        .hint_text(
                            egui::RichText::new(hint)
                                .font(theme.font.id())
                                .color(theme.dim.gamma_multiply(0.8)),
                        )
                        .frame(false)
                        .desired_width(inner.width()),
                )
            },
        )
        .inner;
    let focus =
        ui.ctx()
            .animate_bool_with_time(response.id.with("focus"), response.has_focus(), 0.1);
    ui.painter().set(
        frame,
        egui::epaint::RectShape::new(
            rect,
            theme.control_radius,
            Color32::from_black_alpha(70),
            (
                1.0,
                theme
                    .line()
                    .lerp_to_gamma(theme.accent.gamma_multiply(0.8), focus),
            ),
            egui::StrokeKind::Inside,
        ),
    );
    response
}

/// What happened to a removable chip.
#[derive(Default)]
pub struct Pill {
    /// Its text was clicked.
    pub open: bool,
    /// Its `×` was clicked.
    pub remove: bool,
}

/// A rounded chip with a `×`, such as a search filter.
pub fn pill(ui: &mut Ui, theme: &Theme, label: &str) -> Pill {
    let font = theme.small.id();
    let text = ui
        .painter()
        .layout_no_wrap(label.to_owned(), font.clone(), theme.text);
    let cross = ui.painter().layout_no_wrap("×".into(), font, theme.text);
    let height = text.size().y + 12.0;
    let width = 12.0 + text.size().x + 10.0 + cross.size().x + 10.0;
    let (rect, _) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
    let split = rect.right() - cross.size().x - 18.0;
    let body = Rect::from_min_max(rect.min, pos2(split, rect.bottom()));
    let close = Rect::from_min_max(pos2(split, rect.top()), rect.max);
    let id = ui.id().with(("pill", label));
    let open = held_click(ui.interact(body, id.with("open"), control_sense(true)))
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    let remove = held_click(ui.interact(close, id.with("remove"), control_sense(true)))
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text("Remove");
    let (hover, cross_hover) = (
        hover_amount(ui, &open, true),
        hover_amount(ui, &remove, true),
    );
    let painter = ui.painter();
    painter.rect(
        rect,
        egui::CornerRadius::same((height / 2.0) as u8),
        theme
            .accent
            .gamma_multiply(0.07 + 0.08 * hover.max(cross_hover)),
        (
            1.0,
            theme
                .accent
                .gamma_multiply(0.25 + 0.3 * hover.max(cross_hover)),
        ),
        egui::StrokeKind::Inside,
    );
    let tint = theme.accent.lerp_to_gamma(theme.text, 0.55);
    painter.galley(
        pos2(rect.left() + 12.0, rect.center().y - text.size().y / 2.0),
        text,
        tint.lerp_to_gamma(Color32::WHITE, hover * 0.6),
    );
    painter.galley(
        close.center() - cross.size() / 2.0,
        cross,
        tint.lerp_to_gamma(Color32::WHITE, cross_hover),
    );
    Pill {
        open: open.clicked(),
        remove: remove.clicked(),
    }
}

/// A full-width list row `height` tall, for its caller to paint into; lit
/// on hover, and with a steady accent edge while `selected`.
pub fn list_row(
    ui: &mut Ui,
    theme: &Theme,
    height: f32,
    selected: bool,
) -> (Rect, egui::Response, f32) {
    let (rect, response) =
        ui.allocate_exact_size(vec2(ui.available_width(), height), control_sense(true));
    let response = held_click(response);
    let hover = hover_amount(ui, &response, true);
    if selected {
        ui.painter()
            .rect_filled(rect, theme.control_radius, theme.accent.gamma_multiply(0.1));
        ui.painter().rect_filled(
            Rect::from_min_size(rect.min, vec2(2.0, rect.height())),
            0,
            theme.accent,
        );
    } else {
        highlight(ui, theme, rect, hover, Highlight::Whole);
    }
    (
        rect,
        response.on_hover_cursor(egui::CursorIcon::PointingHand),
        hover,
    )
}

/// A row in a menu: a label, and dim text such as a unit at the right.
pub fn item(ui: &mut Ui, theme: &Theme, label: &str, detail: &str) -> egui::Response {
    let (rect, response, hover) = row(ui, theme, theme.row + 6.0, true);
    let painter = ui.painter();
    painter.text(
        pos2(rect.left() + theme.inset, rect.center().y),
        Align2::LEFT_CENTER,
        label,
        theme.font.id(),
        theme.text.lerp_to_gamma(theme.accent, hover),
    );
    painter.text(
        pos2(rect.right() - theme.inset, rect.center().y),
        Align2::RIGHT_CENTER,
        detail,
        theme.small.id(),
        theme.dim,
    );
    response
}

/// A thin bar: `Some` share done, or `None` for a moving band while the
/// amount is unknown.
pub fn progress(ui: &mut Ui, theme: &Theme, done: Option<f32>) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 2.0), Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, 1, theme.line());
    let lit = match done {
        Some(share) => {
            Rect::from_min_size(rect.min, vec2(rect.width() * share.clamp(0.0, 1.0), 2.0))
        }
        None => {
            let time = ui.input(|input| input.time) as f32;
            let width = rect.width() * 0.25;
            let x = rect.left() + (time * 0.6).fract() * (rect.width() + width) - width;
            ui.ctx().request_repaint();
            Rect::from_min_max(
                pos2(x.max(rect.left()), rect.top()),
                pos2((x + width).min(rect.right()), rect.bottom()),
            )
        }
    };
    painter.rect_filled(lit, 1, theme.accent);
}
