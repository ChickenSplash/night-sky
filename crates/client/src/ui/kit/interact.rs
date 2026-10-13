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

/// How lit a control is, 0 to 1. A held press remains lit even when egui no
/// longer calls the active widget hovered.
pub(super) fn hover_amount(ui: &Ui, response: &egui::Response, enabled: bool) -> f32 {
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
pub(super) fn control_sense(interactive: bool) -> Sense {
    if interactive {
        Sense::click_and_drag()
    } else {
        Sense::hover()
    }
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
    painter.rect_filled(rect, corners, theme.accent.gamma_multiply(0.12 * hover));
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
            theme.accent.gamma_multiply(hover),
        );
    }
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
    let hover = hover_amount(ui, &response, true);
    highlight(ui, theme, rect, hover, Highlight::Whole);
    let response = if interactive {
        response.on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        response
    };
    (rect, response, hover)
}
