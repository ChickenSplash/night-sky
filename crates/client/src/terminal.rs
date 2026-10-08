//! The terminal look, after the desktop's Hyprland windows: a floating,
//! translucent black window with a cyan-to-green border, a round icon button
//! and text-mode controls.
use bevy_egui::egui::{
    self, Align2, Color32, CornerRadius, FontId, Rect, Sense, Ui, epaint, pos2, vec2,
};
use std::ops::RangeInclusive;

/// Space between the window and every edge of the screen.
pub const GAP: f32 = 12.0;
const RADIUS: u8 = 10;
const BORDER: f32 = 2.0;
const BUTTON: f32 = 40.0;
const ROW: f32 = 24.0;
const CYAN: Color32 = Color32::from_rgb(0x33, 0xcc, 0xff);
const GREEN: Color32 = Color32::from_rgb(0x00, 0xff, 0x99);
const TEXT: Color32 = Color32::from_rgb(214, 228, 244);
const DIM: Color32 = Color32::from_rgb(110, 125, 140);
const FILL: Color32 = Color32::from_black_alpha(205);

fn font() -> FontId {
    FontId::monospace(13.0)
}

/// Hyprland's active border: a 45° gradient from cyan to green.
fn gradient_border(painter: &egui::Painter, rect: Rect, radius: f32, alpha: f32) {
    let mut points = Vec::new();
    epaint::tessellator::path::rounded_rectangle(
        &mut points,
        rect.shrink(BORDER / 2.0),
        epaint::CornerRadiusF32::same(radius - BORDER / 2.0),
    );
    let stroke = epaint::PathStroke::new_uv(BORDER, move |bounds, p| {
        let t = (p.x - bounds.left() + p.y - bounds.top()) / (bounds.width() + bounds.height());
        CYAN.lerp_to_gamma(GREEN, t).gamma_multiply(alpha)
    });
    painter.add(epaint::PathShape::closed_line(points, stroke));
}

/// A round icon button in the top-right corner. `visible` fades it, from 0
/// (hidden) to 1; it only takes clicks when fully shown.
pub fn corner_button(ctx: &egui::Context, id: &str, icon: &str, visible: f32) -> bool {
    if visible <= 0.0 {
        return false;
    }
    let screen = ctx.content_rect();
    egui::Area::new(egui::Id::new(id))
        .fixed_pos(pos2(screen.right() - GAP - BUTTON, screen.top() + GAP))
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            let (rect, response) = ui.allocate_exact_size(vec2(BUTTON, BUTTON), Sense::click());
            let hover = ui
                .ctx()
                .animate_bool_with_time(response.id, response.hovered(), 0.12);
            let painter = ui.painter();
            let centre = rect.center();
            painter.circle_filled(centre, BUTTON / 2.0, FILL.gamma_multiply(visible));
            painter.circle_stroke(
                centre,
                BUTTON / 2.0 - BORDER / 2.0,
                (
                    BORDER,
                    DIM.lerp_to_gamma(CYAN, hover).gamma_multiply(visible),
                ),
            );
            painter.text(
                centre,
                Align2::CENTER_CENTER,
                icon,
                FontId::proportional(18.0),
                TEXT.lerp_to_gamma(CYAN, hover).gamma_multiply(visible),
            );
            visible >= 1.0
                && response
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .clicked()
        })
        .inner
}

/// A floating window docked to the right edge, with a gap on every side.
/// `shown` slides it from 0 (off screen) to 1. Returns whether its close
/// button was clicked.
pub fn side_window(
    ctx: &egui::Context,
    id: &str,
    title: &str,
    shown: f32,
    add: impl FnOnce(&mut Ui),
) -> bool {
    if shown <= 0.0 {
        return false;
    }
    let screen = ctx.content_rect();
    let size = vec2(
        (screen.width() * 0.15).clamp(300.0, 460.0),
        screen.height() - 2.0 * GAP,
    );
    // Off screen at 0, shadow included.
    let x = screen.right() - GAP - size.x + (1.0 - shown) * (size.x + GAP + 32.0);
    egui::Area::new(egui::Id::new(id))
        .fixed_pos(pos2(x, screen.top() + GAP))
        .order(egui::Order::Foreground)
        .constrain(false)
        .show(ctx, |ui| {
            let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
            let painter = ui.painter().clone();
            let radius = CornerRadius::same(RADIUS);
            painter.add(
                epaint::Shadow {
                    offset: [0, 6],
                    blur: 24,
                    spread: 0,
                    color: Color32::from_black_alpha(140),
                }
                .as_shape(rect, radius),
            );
            painter.rect_filled(rect, radius, FILL);
            let inner = rect.shrink(16.0);
            let mut ui = ui.new_child(egui::UiBuilder::new().max_rect(inner));
            ui.style_mut().spacing.item_spacing = vec2(8.0, 4.0);
            let mut close = false;
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("night-sky:~$ ").font(font()).color(DIM));
                ui.label(egui::RichText::new(title).font(font()).color(CYAN));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    close = button(ui, "×");
                });
            });
            ui.add_space(4.0);
            let line = ui.cursor().top();
            ui.painter()
                .hline(inner.x_range(), line, (1.0, DIM.gamma_multiply(0.4)));
            ui.add_space(8.0);
            egui::ScrollArea::vertical()
                .id_salt(id)
                .auto_shrink(false)
                .show(&mut ui, add);
            gradient_border(&painter, rect, f32::from(RADIUS), 0.93);
            close
        })
        .inner
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

/// `label [=====-----] 50°`: click or drag along the bar, or scroll over the
/// row. Returns a new whole-number value when it changes.
pub fn slider(
    ui: &mut Ui,
    label: &str,
    value: f32,
    range: RangeInclusive<f32>,
    unit: &str,
) -> Option<f32> {
    let (rect, response, hover) = row(ui, Sense::click_and_drag());
    let painter = ui.painter();
    let (lo, hi) = (*range.start(), *range.end());
    let text = TEXT.lerp_to_gamma(CYAN, hover);
    let left = rect.left() + 8.0;
    let label_width = painter
        .text(
            pos2(left, rect.center().y),
            Align2::LEFT_CENTER,
            label,
            font(),
            text,
        )
        .width();
    let value_width = painter
        .text(
            pos2(rect.right() - 8.0, rect.center().y),
            Align2::RIGHT_CENTER,
            format!("{value:>3.0}{unit}"),
            font(),
            text,
        )
        .width();
    let glyph = painter.layout_no_wrap("=".into(), font(), text).size().x;
    let start = left + label_width + 10.0;
    let cells =
        (((rect.right() - 8.0 - value_width - 10.0 - start) / glyph) as usize).saturating_sub(2);
    let fraction = ((value - lo) / (hi - lo)).clamp(0.0, 1.0);
    let filled = (fraction * cells as f32).round() as usize;
    let mut x = start;
    for (part, colour) in [
        ("[".to_string(), text),
        ("=".repeat(filled), CYAN),
        ("-".repeat(cells - filled), DIM),
        ("]".to_string(), text),
    ] {
        let galley = painter.layout_no_wrap(part, font(), colour);
        let width = galley.size().x;
        painter.galley(
            pos2(x, rect.center().y - galley.size().y / 2.0),
            galley,
            colour,
        );
        x += width;
    }
    let mut next = value;
    if (response.clicked() || response.dragged())
        && let Some(pointer) = response.interact_pointer_pos()
        && cells > 0
    {
        let t = ((pointer.x - start - glyph) / (cells as f32 * glyph)).clamp(0.0, 1.0);
        next = lo + t * (hi - lo);
    }
    if response.hovered() {
        let scroll = ui.input_mut(|i| {
            let delta = i.raw_scroll_delta.y;
            i.raw_scroll_delta = egui::Vec2::ZERO;
            i.smooth_scroll_delta = egui::Vec2::ZERO;
            delta
        });
        if scroll != 0.0 {
            next = value + scroll.signum();
        }
    }
    let next = next.round().clamp(lo, hi);
    (next != value.round()).then_some(next)
}

/// `[ label ]`, sized to its text.
pub fn button(ui: &mut Ui, label: &str) -> bool {
    let text = format!("[ {label} ]");
    let size = ui
        .painter()
        .layout_no_wrap(text.clone(), font(), TEXT)
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
        TEXT.lerp_to_gamma(CYAN, hover),
    );
    response
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .clicked()
}

/// A key and what it does, as one wrapped line.
pub fn key(ui: &mut Ui, key: &str, action: &str) {
    ui.horizontal_wrapped(|ui| {
        ui.label(egui::RichText::new(key).font(font()).color(CYAN));
        ui.label(egui::RichText::new(action).font(font()).color(TEXT));
    });
}
