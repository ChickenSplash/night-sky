//! One hover and held-press state, and one highlight painter, for every
//! kit control.
use super::Theme;
use bevy_egui::egui::{self, Rect, Sense, Ui, pos2, vec2};

/// Content never owns drags: only scrollbars and the wheel scroll.
pub const SCROLL_SOURCE: egui::scroll_area::ScrollSource = egui::scroll_area::ScrollSource {
    scroll_bar: true,
    drag: false,
    mouse_wheel: true,
};

/// One timing and easing curve for hover edges and input underlines.
pub(super) const ANIMATION_SECONDS: f32 = 0.18;

pub(super) fn ease_out(progress: f32) -> f32 {
    egui::emath::easing::cubic_out(progress)
}

fn animate_highlight(ui: &Ui, id: egui::Id, active: bool) -> f32 {
    ui.ctx().animate_bool_with_time_and_easing(
        id.with("highlight-animation"),
        active,
        ANIMATION_SECONDS,
        ease_out,
    )
}

fn growing_edge(ui: &Ui, theme: &Theme, rect: Rect, amount: f32, vertical: bool) {
    if amount <= 0.0 {
        return;
    }
    let size = if vertical {
        vec2(rect.width(), rect.height() * amount)
    } else {
        vec2(rect.width() * amount, rect.height())
    };
    ui.painter()
        .rect_filled(Rect::from_center_size(rect.center(), size), 0, theme.accent);
}

/// How lit a control is, 0 to 1. A held press remains lit even when egui no
/// longer calls the active widget hovered.
pub(super) fn hover_amount(ui: &Ui, response: &egui::Response, enabled: bool) -> f32 {
    selection_amount(ui, response, enabled, false)
}

/// Hover, held presses and persistent selection share one animation target,
/// so handing the highlight between these states never jumps or restarts it.
pub(super) fn selection_amount(
    ui: &Ui,
    response: &egui::Response,
    enabled: bool,
    selected: bool,
) -> f32 {
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
    animate_highlight(
        ui,
        response.id,
        enabled
            && (selected
                || response.hovered()
                || response.is_pointer_button_down_on()
                || readout_pressed),
    )
}

/// egui promotes a long press to a drag. Claim that press so it stays with
/// the control, without using the drag to move anything or trigger actions.
pub(super) fn control_sense(interactive: bool) -> Sense {
    if interactive {
        Sense::click_and_drag()
    } else {
        Sense::hover()
    }
}

/// egui reports no click for a press held past its click time, even across a
/// single slow frame (a press and release a second apart). Counts a release
/// as a click when the press began on the control and ends over it, as
/// ordinary buttons do.
pub(super) fn held_click(mut response: egui::Response) -> egui::Response {
    let ctx = response.ctx.clone();
    // egui forgets where the press began by the frame it ends.
    let id = response.id.with("held-press");
    if response.is_pointer_button_down_on() {
        ctx.data_mut(|data| data.insert_temp(id, ()));
    }
    let (released, over) = ctx.input(|input| {
        (
            input.pointer.primary_released(),
            input
                .pointer
                .latest_pos()
                .is_some_and(|pos| response.interact_rect.contains(pos)),
        )
    });
    if !released {
        return response;
    }
    let pressed = ctx.data_mut(|data| data.remove_temp::<()>(id)).is_some();
    if pressed
        && over
        && response.sense.senses_click()
        && response.contains_pointer()
        && !response.clicked()
    {
        response
            .flags
            .insert(egui::response::Flags::FAKE_PRIMARY_CLICKED);
    }
    response
}

#[derive(Clone, Copy)]
pub(super) enum Highlight {
    Whole,
    Left,
    Right,
}

/// The shared faint accent bar. Whole elements have two bright edges; split
/// halves keep their single outer edge and square corners at the divider.
pub(super) fn highlight(ui: &Ui, theme: &Theme, rect: Rect, hover: f32, part: Highlight) {
    if hover <= 0.0 {
        return;
    }
    let painter = ui.painter();
    let r = theme.control_radius;
    let corners = match part {
        Highlight::Whole => egui::CornerRadius::same(r),
        Highlight::Left => egui::CornerRadius {
            nw: r,
            sw: r,
            ne: 0,
            se: 0,
        },
        Highlight::Right => egui::CornerRadius {
            nw: 0,
            sw: 0,
            ne: r,
            se: r,
        },
    };
    painter.rect_filled(
        rect,
        corners,
        theme
            .accent
            .gamma_multiply(Theme::SUBTLE_ACCENT_OPACITY * hover),
    );
    let edges = [rect.min, pos2(rect.right() - 2.0, rect.top())];
    let edges = match part {
        Highlight::Whole => &edges[..],
        Highlight::Left => &edges[..1],
        Highlight::Right => &edges[1..],
    };
    for &edge in edges {
        growing_edge(
            ui,
            theme,
            Rect::from_min_size(edge, vec2(2.0, rect.height())),
            hover,
            true,
        );
    }
}

/// Clicking a field's padding focuses and selects its text for replacement.
pub(super) fn focus_input_padding(ui: &Ui, response: &egui::Response, outer: Rect, text: &str) {
    let inner = response.rect;
    // Keep hit areas separate from TextEdit so text clicks and drags retain
    // their native caret/selection behaviour.
    let zones = [
        Rect::from_min_max(outer.min, pos2(inner.left(), outer.bottom())),
        Rect::from_min_max(pos2(inner.right(), outer.top()), outer.max),
        Rect::from_min_max(
            pos2(inner.left(), outer.top()),
            pos2(inner.right(), inner.top()),
        ),
        Rect::from_min_max(
            pos2(inner.left(), inner.bottom()),
            pos2(inner.right(), outer.bottom()),
        ),
    ];
    let mut select_all = false;
    for (index, rect) in zones.into_iter().enumerate() {
        let click = ui
            .interact(rect, response.id.with(("padding", index)), Sense::click())
            .on_hover_cursor(egui::CursorIcon::Text);
        select_all |= click.clicked();
    }
    if select_all {
        response.request_focus();
        if let Some(mut state) = egui::TextEdit::load_state(ui.ctx(), response.id) {
            state
                .cursor
                .set_char_range(Some(egui::text::CCursorRange::two(
                    egui::text::CCursor::new(0),
                    egui::text::CCursor::new(text.chars().count()),
                )));
            egui::TextEdit::store_state(ui.ctx(), response.id, state);
        }
    }
}

/// Shared hover/focus underline: expands from the centre and retracts to it.
pub(super) fn input_underline(
    ui: &Ui,
    theme: &Theme,
    rect: Rect,
    id: egui::Id,
    active: bool,
) -> f32 {
    let amount = animate_highlight(ui, id, active);
    growing_edge(
        ui,
        theme,
        Rect::from_min_max(pos2(rect.left(), rect.bottom() - 2.0), rect.max),
        amount,
        false,
    );
    amount
}

/// A full-width row of `height`, lit on hover.
pub(super) fn row(
    ui: &mut Ui,
    theme: &Theme,
    height: f32,
    interactive: bool,
) -> (Rect, egui::Response, f32) {
    let (rect, response) = ui.allocate_exact_size(
        vec2(ui.available_width(), height),
        control_sense(interactive),
    );
    let response = held_click(response);
    let hover = hover_amount(ui, &response, true);
    highlight(ui, theme, rect, hover, Highlight::Whole);
    let response = if interactive {
        response.on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        response
    };
    (rect, response, hover)
}
