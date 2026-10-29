//! Headings, notes, switches, buttons, text fields and list rows.
//! Text-mode themes draw controls as text (`[ button ]`); framed themes draw
//! outlines. Every control shares `interact`'s hover and press light.
use super::interact::{
    Highlight, control_sense, focus_input_padding, held_click, highlight, hover_amount,
    input_edges, row,
};
use super::{SPACING, Theme};
use bevy_egui::egui::{self, Align2, Color32, Rect, Sense, Ui, pos2, vec2};

/// A faint full-width line.
pub fn rule(ui: &mut Ui, theme: &Theme) {
    let line = ui.cursor().top();
    ui.painter()
        .hline(ui.max_rect().x_range(), line, (1.0, theme.line()));
}

/// Shared section and table-heading typography.
pub fn eyebrow_text(theme: &Theme, text: &str) -> egui::RichText {
    egui::RichText::new(text.to_uppercase())
        .font(egui::FontId::monospace(10.0))
        .extra_letter_spacing(1.5)
        .color(theme.accent)
}

/// Small spaced capitals, above a title or a group.
pub fn eyebrow(ui: &mut Ui, theme: &Theme, text: &str) {
    ui.label(eyebrow_text(theme, text));
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
    section_with_default(ui, theme, text, true)
}

/// Like `section`, with an initial state before the user's first toggle.
pub fn section_with_default(ui: &mut Ui, theme: &Theme, text: &str, default_open: bool) -> bool {
    let id = ui.make_persistent_id(("section", text));
    let mut open = ui.data_mut(|data| data.get_persisted::<bool>(id).unwrap_or(default_open));
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

/// `[ label ]` in text mode, else a neutral square action.
pub fn button(ui: &mut Ui, theme: &Theme, label: &str) -> egui::Response {
    if theme.text_mode {
        chip_response(ui, theme, format!("[ {label} ]"), theme.text)
    } else {
        input_button(ui, theme, label)
    }
}

/// A neutral action with destructive-colour interaction feedback.
pub fn destructive_button(ui: &mut Ui, theme: &Theme, label: &str) -> egui::Response {
    button(
        ui,
        &Theme {
            accent: Theme::DESTRUCTIVE,
            ..*theme
        },
        label,
    )
}

/// The main action of a group: accent text in a lit frame. Disabled ones
/// are dim and ignore clicks.
pub fn primary_button(ui: &mut Ui, theme: &Theme, label: &str, enabled: bool) -> egui::Response {
    if theme.text_mode {
        framed(ui, theme, label, true, enabled)
    } else {
        flat_button(ui, theme, label, true, enabled, false)
    }
}

/// A larger close glyph with red hover feedback and no highlight frame.
pub fn close_button(ui: &mut Ui, theme: &Theme) -> egui::Response {
    let galley = ui.painter().layout_no_wrap(
        "×".into(),
        egui::FontId::proportional(22.0),
        Color32::PLACEHOLDER,
    );
    let (rect, response) = ui.allocate_exact_size(vec2(28.0, 28.0), control_sense(true));
    let response = held_click(response);
    let hover = hover_amount(ui, &response, true);
    let colour = theme.text.lerp_to_gamma(Theme::DESTRUCTIVE, hover);
    ui.painter().galley(
        rect.center() - galley.mesh_bounds.center().to_vec2(),
        galley,
        colour,
    );
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Align the visible close glyph with the title, not the whole header block.
pub fn aligned_close_button(ui: &mut Ui, theme: &Theme, title: Rect) -> egui::Response {
    let rect = Rect::from_center_size(
        pos2(ui.max_rect().right() - 14.0, title.center().y),
        vec2(28.0, 28.0),
    );
    ui.place(rect, |ui: &mut Ui| close_button(ui, theme))
}

/// A square, borderless button matching labelled number inputs.
pub fn input_button(ui: &mut Ui, theme: &Theme, label: &str) -> egui::Response {
    flat_button(ui, theme, label, false, true, false)
}

fn flat_button(
    ui: &mut Ui,
    theme: &Theme,
    label: &str,
    primary: bool,
    enabled: bool,
    selected: bool,
) -> egui::Response {
    let colour = if !enabled {
        theme.dim.gamma_multiply(0.6)
    } else if primary {
        theme.accent
    } else {
        theme.text
    };
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_owned(), theme.font.id(), colour);
    let (rect, response) = ui.allocate_exact_size(
        flat_button_size(ui, theme, galley.size()),
        control_sense(enabled),
    );
    let response = held_click(response);
    let fill = if primary {
        theme.accent.gamma_multiply(0.16)
    } else {
        theme.input_fill()
    };
    ui.painter().rect_filled(rect, 0, fill);
    input_edges(
        ui,
        theme,
        rect,
        response.id,
        enabled
            && (selected
                || response.hovered()
                || response.is_pointer_button_down_on()
                || response.has_focus()),
    );
    ui.painter()
        .galley(rect.center() - galley.size() / 2.0, galley, colour);
    if enabled {
        response.on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        response
    }
}

/// Intrinsic sizing of `button`, for layouts reserving its space in either theme.
pub fn button_size(ui: &Ui, theme: &Theme, label: &str) -> egui::Vec2 {
    if theme.text_mode {
        return ui
            .painter()
            .layout_no_wrap(format!("[ {label} ]"), theme.font.id(), theme.text)
            .size()
            + vec2(8.0, 6.0);
    }
    let text = ui
        .painter()
        .layout_no_wrap(label.into(), theme.font.id(), theme.text)
        .size();
    flat_button_size(ui, theme, text)
}

fn flat_button_size(ui: &Ui, theme: &Theme, text: egui::Vec2) -> egui::Vec2 {
    vec2(text.x, text.y.max(ui.spacing().interact_size.y)) + theme.input_padding().sum()
}

/// Text that acts like a button without a frame, such as `Reset`.
pub fn link(ui: &mut Ui, theme: &Theme, label: &str) -> egui::Response {
    if theme.text_mode {
        chip_response(ui, theme, label.to_owned(), theme.dim)
    } else {
        input_button(ui, theme, label)
    }
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
    flat_button(ui, theme, label, chosen, true, chosen).clicked()
}

/// `grep text`: a one-line text field after a prompt, with animated end edges.
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
    field(ui, theme, prompt, "", text, width)
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
    let height = theme.field_height();
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
    ui.painter().set(
        frame,
        egui::epaint::RectShape::new(
            rect,
            0,
            theme.input_fill(),
            egui::Stroke::NONE,
            egui::StrokeKind::Inside,
        ),
    );
    focus_input_padding(ui, &response, rect, text);
    input_edges(
        ui,
        theme,
        rect,
        response.id,
        response.has_focus()
            || ui.rect_contains_pointer(rect)
            || response.is_pointer_button_down_on(),
    );
    response
}

/// A full-width list row `height` tall, for its caller to paint into; lit
/// on hover or selection, easing both transitions through the same state.
pub fn list_row(
    ui: &mut Ui,
    theme: &Theme,
    height: f32,
    selected: bool,
) -> (Rect, egui::Response, f32) {
    let (rect, response) =
        ui.allocate_exact_size(vec2(ui.available_width(), height), control_sense(true));
    let response = held_click(response);
    let hover = super::interact::selection_amount(ui, &response, true, selected);
    highlight(ui, theme, rect, hover, Highlight::Whole);
    (
        rect,
        response.on_hover_cursor(egui::CursorIcon::PointingHand),
        hover,
    )
}

/// A row in a menu: a label, and dim text such as a unit at the right.
pub fn item(
    ui: &mut Ui,
    theme: &Theme,
    label: &str,
    detail: &str,
    selected: bool,
) -> egui::Response {
    let (rect, response, hover) = list_row(ui, theme, theme.row + 6.0, selected);
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

/// A square accent bar with inset status text: `Some` share done, or `None`
/// for a moving band while the amount is unknown.
pub fn progress(ui: &mut Ui, theme: &Theme, done: Option<f32>, text: &str) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), theme.row), Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(
        rect,
        0,
        theme.accent.gamma_multiply(Theme::SUBTLE_ACCENT_OPACITY),
    );
    let lit = match done {
        Some(share) => Rect::from_min_size(
            rect.min,
            vec2(rect.width() * share.clamp(0.0, 1.0), rect.height()),
        ),
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
    painter.rect_filled(lit, 0, theme.accent);
    let mut job =
        egui::text::LayoutJob::simple_singleline(text.to_owned(), theme.small.id(), theme.text);
    job.wrap = egui::text::TextWrapping::truncate_at_width((rect.width() - 2.0 * SPACING).max(0.0));
    let galley = painter.layout_job(job);
    let position = rect.center() - galley.size() * 0.5;
    let clip = ui.clip_rect().intersect(rect);
    painter
        .with_clip_rect(clip)
        .galley(position, galley.clone(), theme.text);
    // Dark ink over the full-strength accent keeps the text readable as the
    // fill advances through it. The unfilled part retains normal theme text.
    painter
        .with_clip_rect(clip.intersect(lit))
        .galley_with_override_text_color(position, galley, theme.fill.to_opaque());
}
