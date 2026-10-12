//! The terminal look of the debug window, after the desktop's Hyprland
//! windows: a floating, translucent black window with a light grey border and
//! text-mode controls.
use bevy_egui::egui::{self, Align2, Color32, FontId, Rect, Sense, Ui, epaint, pos2, vec2};

/// Space between the window and the screen's edges when it first opens.
const GAP: f32 = 12.0;
const RADIUS: u8 = 10;
const BORDER: f32 = 2.0;
const ROW: f32 = 24.0;
/// Where text starts inside a row, clear of the hover light's edge.
const INSET: f32 = 8.0;
pub const CYAN: Color32 = Color32::from_rgb(0x33, 0xcc, 0xff);
const GREEN: Color32 = Color32::from_rgb(0x00, 0xff, 0x99);
const GREY: Color32 = Color32::from_rgb(0xc8, 0xcc, 0xd0);
pub const TEXT: Color32 = Color32::from_rgb(214, 228, 244);
pub const DIM: Color32 = Color32::from_rgb(110, 125, 140);
const FILL: Color32 = Color32::from_black_alpha(205);
/// Content never owns drags: only scrollbars and the wheel scroll the console.
pub const SCROLL_SOURCE: egui::scroll_area::ScrollSource = egui::scroll_area::ScrollSource {
    scroll_bar: true,
    drag: false,
    mouse_wheel: true,
};
fn border_colour() -> Color32 {
    GREY.gamma_multiply(0.93)
}

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
    let position_id = egui::Id::new(("terminal-position", title));
    let mut movement = egui::Vec2::ZERO;
    let mut window = egui::Window::new(title)
        .id(egui::Id::new(("terminal", title)))
        .title_bar(false)
        .frame(
            egui::Frame::new()
                .fill(FILL)
                .stroke((BORDER, border_colour()))
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
        .movable(false)
        .resizable(true)
        .order(egui::Order::Foreground);
    if let Some(position) = ctx.data(|data| data.get_temp::<egui::Pos2>(position_id)) {
        window = window.current_pos(position);
    }
    let shown = window.show(ctx, |ui| {
        ui.style_mut().spacing.item_spacing = vec2(8.0, 4.0);
        let mut close_rect = Rect::NOTHING;
        let header = ui.horizontal(|ui| {
            ui.label(egui::RichText::new("night-sky:~$ ").font(font()).color(DIM));
            ui.label(egui::RichText::new(title).font(font()).color(CYAN));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let response = chip_response(ui, "[ × ]".into(), TEXT);
                close = response.clicked();
                close_rect = response.rect;
            });
        });
        ui.add_space(4.0);
        // The whole header, including frame padding down to the rule, is a
        // handle. Cut out only the actual close button, not its whole column.
        let handle = Rect::from_min_max(
            header.response.rect.min - vec2(16.0, 16.0),
            pos2(ui.max_rect().right() + 16.0, ui.cursor().top()),
        );
        let content_clip = ui.clip_rect();
        ui.set_clip_rect(content_clip.union(handle));
        for (index, rect) in [
            Rect::from_min_max(handle.min, pos2(close_rect.left(), handle.bottom())),
            Rect::from_min_max(pos2(close_rect.right(), handle.top()), handle.max),
            Rect::from_min_max(
                pos2(close_rect.left(), handle.top()),
                close_rect.right_top(),
            ),
            Rect::from_min_max(
                close_rect.left_bottom(),
                pos2(close_rect.right(), handle.bottom()),
            ),
        ]
        .into_iter()
        .enumerate()
        {
            let response = ui
                .interact(rect, ui.id().with(("move-header", index)), Sense::drag())
                .on_hover_cursor(egui::CursorIcon::AllScroll);
            if response.dragged() {
                ctx.set_cursor_icon(egui::CursorIcon::AllScroll);
                movement = response.drag_delta();
            }
        }
        ui.set_clip_rect(content_clip);
        rule(ui);
        ui.add_space(4.0);
        add(ui);
    });
    if let Some(shown) = shown {
        ctx.data_mut(|data| data.insert_temp(position_id, shown.response.rect.min + movement));
    }
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

/// One hover/press state for every terminal control. A held press remains lit
/// even when egui no longer calls the active widget hovered.
fn hover_amount(ui: &Ui, response: &egui::Response, enabled: bool) -> f32 {
    // Read-only facts have hover sense, so egui never gives them an active
    // button press. Keep their highlight while pressing inside them too.
    let readout_pressed = response.sense == Sense::hover()
        && response.contains_pointer()
        && ui.input(|input| {
            input.pointer.any_down()
                && input
                    .pointer
                    .press_origin()
                    .is_some_and(|pos| response.rect.contains(pos))
        });
    ui.ctx().animate_bool_with_time(
        response.id,
        enabled && (response.hovered() || response.is_pointer_button_down_on() || readout_pressed),
        0.1,
    )
}

/// egui promotes a long press to a drag. Claim that press so it stays with
/// the control, without using the drag to move anything or trigger actions.
fn control_sense(interactive: bool) -> Sense {
    if interactive {
        Sense::click_and_drag()
    } else {
        Sense::hover()
    }
}

#[derive(Clone, Copy)]
enum Highlight {
    Whole,
    Left,
    Right,
}

/// Shared faint blue bar. Whole elements have two bright edges; split checkbox
/// halves retain their single outer edge and square corners at the divider.
fn highlight(ui: &Ui, rect: Rect, hover: f32, part: Highlight) {
    if hover > 0.0 {
        let painter = ui.painter();
        let corners = match part {
            Highlight::Whole => egui::CornerRadius::same(4),
            Highlight::Left => egui::CornerRadius {
                nw: 4,
                sw: 4,
                ne: 0,
                se: 0,
            },
            Highlight::Right => egui::CornerRadius {
                nw: 0,
                sw: 0,
                ne: 4,
                se: 4,
            },
        };
        painter.rect_filled(rect, corners, CYAN.gamma_multiply(0.12 * hover));
        let edges = [rect.min, pos2(rect.right() - 2.0, rect.top())];
        let edges = match part {
            Highlight::Whole => &edges[..],
            Highlight::Left => &edges[..1],
            Highlight::Right => &edges[1..],
        };
        for &edge in edges {
            painter.rect_filled(
                Rect::from_min_size(edge, vec2(2.0, rect.height())),
                0,
                CYAN.gamma_multiply(hover),
            );
        }
    }
}

fn row(ui: &mut Ui, interactive: bool) -> (Rect, egui::Response, f32) {
    let (rect, response) =
        ui.allocate_exact_size(vec2(ui.available_width(), ROW), control_sense(interactive));
    let hover = hover_amount(ui, &response, true);
    highlight(ui, rect, hover, Highlight::Whole);
    let response = if interactive {
        response.on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        response
    };
    (rect, response, hover)
}

/// A fixed heading, laid out like a `section` row.
pub fn heading(ui: &mut Ui, text: &str) {
    ui.add_space(6.0);
    let (rect, _, _) = row(ui, false);
    ui.painter().text(
        pos2(rect.left() + INSET, rect.center().y),
        Align2::LEFT_CENTER,
        format!("# {text}"),
        font(),
        CYAN,
    );
}

/// A persistent section toggle. Call its body only while expanded.
pub fn section(ui: &mut Ui, text: &str) -> bool {
    let id = ui.make_persistent_id(("section", text));
    let mut open = ui.data_mut(|data| data.get_persisted::<bool>(id).unwrap_or(true));
    ui.add_space(6.0);
    let (rect, response, _) = row(ui, true);
    ui.painter().text(
        pos2(rect.left() + INSET, rect.center().y),
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

/// Small dim text, inset to line up with the rows' text.
pub fn note(ui: &mut Ui, text: &str) {
    egui::Frame::new()
        .inner_margin(egui::Margin {
            left: INSET as i8,
            right: INSET as i8,
            ..egui::Margin::ZERO
        })
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new(text)
                    .font(FontId::monospace(11.0))
                    .color(DIM),
            );
        });
}

/// Split row: toggle on the left, square description button on the right.
/// Disabled switches can still explain themselves, but never toggle.
pub fn check(ui: &mut Ui, on: bool, label: &str, enabled: bool, detail: &str) -> bool {
    let id = ui.make_persistent_id(("check-detail", label));
    let mut open = ui.data(|data| data.get_temp::<bool>(id).unwrap_or(false));
    let mut height = ROW;
    let label = loop {
        let galley = ui.painter().layout(
            label.to_owned(),
            font(),
            TEXT,
            (ui.available_width() - 2.0 * INSET - 40.0 - height).max(40.0),
        );
        let needed = ROW.max(galley.size().y + 8.0);
        if needed <= height {
            break galley;
        }
        height = needed;
    };
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::hover());
    let divider = rect.right() - height;
    let toggle_rect = Rect::from_min_max(rect.min, pos2(divider, rect.bottom()));
    let detail_rect = Rect::from_min_max(pos2(divider, rect.top()), rect.max);
    let response = ui.interact(toggle_rect, id.with("toggle"), control_sense(enabled));
    let expand = ui
        .interact(detail_rect, id.with("expand"), control_sense(true))
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    let hover = hover_amount(ui, &response, enabled);
    let detail_hover = hover_amount(ui, &expand, true);
    highlight(ui, toggle_rect, hover, Highlight::Left);
    highlight(ui, detail_rect, detail_hover, Highlight::Right);
    ui.painter()
        .vline(divider, rect.y_range(), (1.0, DIM.gamma_multiply(0.4)));
    if expand.clicked() {
        open = !open;
    }
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
    let left = rect.left() + INSET;
    let galley = painter.layout_no_wrap(if on { "[x]" } else { "[ ]" }.into(), font(), mark);
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
        font(),
        TEXT.lerp_to_gamma(CYAN, detail_hover),
    );
    let toggled = enabled && response.clicked();
    if enabled {
        response.on_hover_cursor(egui::CursorIcon::PointingHand);
    }
    ui.data_mut(|data| data.insert_temp(id, open));
    if open {
        note(ui, detail);
    }
    toggled
}

#[cfg(test)]
mod tests;

/// Text sized to itself that lights up on hover. Returns whether it was clicked.
fn chip(ui: &mut Ui, text: String, colour: Color32) -> bool {
    chip_response(ui, text, colour).clicked()
}

fn chip_response(ui: &mut Ui, text: String, colour: Color32) -> egui::Response {
    let size = ui
        .painter()
        .layout_no_wrap(text.clone(), font(), colour)
        .size()
        + vec2(8.0, 6.0);
    let (rect, response) = ui.allocate_exact_size(size, control_sense(true));
    let hover = hover_amount(ui, &response, true);
    highlight(ui, rect, hover, Highlight::Whole);
    let painter = ui.painter();
    painter.text(
        rect.center(),
        Align2::CENTER_CENTER,
        text,
        font(),
        colour.lerp_to_gamma(CYAN, hover),
    );
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
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

/// Labels on the left and values flush right, joined by a dotted leader.
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
    for (label, value, detail) in rows {
        let open = fact(
            ui,
            egui::Id::new((id, *label)),
            label,
            value,
            detail.is_some(),
        );
        if let (true, Some(detail)) = (open, detail) {
            note(ui, detail);
        }
    }
}

/// Space kept between a label, its leader and its value.
const LEADER_GAP: f32 = 8.0;

/// One full-width fact row, lit on hover like every other row; a row with a
/// detail toggles it when clicked anywhere. A value too wide to share the
/// line with its label wraps, still flush right. Returns whether its detail
/// is open.
fn fact(ui: &mut Ui, id: egui::Id, label: &str, value: &str, expands: bool) -> bool {
    let mut open = expands && ui.data(|data| data.get_temp::<bool>(id).unwrap_or(false));
    let label = if expands {
        format!("{label} {}", if open { "-" } else { "+" })
    } else {
        label.to_owned()
    };
    let width = ui.available_width();
    let label = ui.painter().layout_no_wrap(label, font(), DIM);
    let wrap = (width - 2.0 * INSET - label.size().x - 2.0 * LEADER_GAP).max(120.0);
    let value = ui.painter().layout(value.to_owned(), font(), TEXT, wrap);
    // Wrapped values keep a single row's padding above and below.
    let padding = (ROW - label.size().y) / 2.0;
    let height = ROW.max(value.size().y + 2.0 * padding);
    let (rect, response) = ui.allocate_exact_size(vec2(width, height), control_sense(expands));
    let hover = hover_amount(ui, &response, true);
    highlight(ui, rect, hover, Highlight::Whole);
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
    let label_right = rect.left() + INSET + label.size().x;
    let value_left = rect.right() - INSET - value.size().x;
    let dots = egui::Rangef::new(label_right + LEADER_GAP, value_left - LEADER_GAP);
    let y = top + label.size().y * 0.62;
    let mut x = (dots.min / 6.0).ceil() * 6.0;
    while x <= dots.max {
        painter.circle_filled(pos2(x, y), 0.75, DIM.gamma_multiply(0.45 + 0.4 * hover));
        x += 6.0;
    }
    painter.galley_with_override_text_color(
        pos2(rect.left() + INSET, top),
        label,
        DIM.lerp_to_gamma(CYAN, hover),
    );
    painter.galley(pos2(value_left, top), value, TEXT);
    open
}
