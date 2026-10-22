//! Draggable halo-text HUD controls on the 3D view: time rate, movement
//! speed, magnitude limit, exposure and compression.
use crate::Navigation;
use crate::actions::UiCommand;
use crate::format::speed_label;
use crate::ui::kit::halo::{CURRENT, HOVER, HUD_LINE_GAP, HUD_PADDING, LINK, SIZE, halo_text};
use crate::ui::kit::{Theme, TooltipExt};
use bevy::prelude::*;
use bevy_egui::egui;
use space_simulation::SimulationClock;

const YEAR: f64 = 31_557_600.0;
pub(crate) const MAX_RATE: f64 = 100.0 * YEAR;
/// A 400-point horizontal drag covers either HUD control's logarithmic range.
const HUD_DRAG_SPAN: f64 = 400.0;

/// Background-layer HUD text is deliberately not an egui window. egui's
/// `is_pointer_over_area` excludes it, so explicitly protect its hit region
/// and retain capture when a drag leaves that region.
pub(crate) fn pointer_captured(ctx: &egui::Context) -> bool {
    ctx.is_pointer_over_area()
        || ctx.wants_pointer_input()
        || [
            "time-pointer-capture",
            "speed-pointer-capture",
            "magnitude-pointer-capture",
            "exposure-pointer-capture",
            "compression-pointer-capture",
            "breadcrumb-pointer-capture",
            "icons-pointer-capture",
        ]
        .into_iter()
        .any(|id| {
            let hud = ctx.data(|data| data.get_temp::<(egui::Rect, bool)>(egui::Id::new(id)));
            hud.is_some_and(|(rect, dragging)| {
                dragging
                    || ctx.input(|input| {
                        // Hover only claims an idle pointer. While a button is held,
                        // ownership comes from the press origin, not the current
                        // position: a camera drag can pass straight over the HUD.
                        let pos = if input.pointer.any_down() {
                            input.pointer.press_origin()
                        } else {
                            input.pointer.hover_pos()
                        };
                        pos.is_some_and(|p| rect.contains(p))
                    })
            })
        })
}

pub(super) fn dragged_rate(start: f64, delta: f32) -> f64 {
    dragged_value(start, delta, 1.0, MAX_RATE)
}

pub(super) fn dragged_value(start: f64, delta: f32, min: f64, max: f64) -> f64 {
    (start.clamp(min, max).ln() + f64::from(delta) / HUD_DRAG_SPAN * (max / min).ln())
        .clamp(min.ln(), max.ln())
        .exp()
        .clamp(min, max)
}

pub(super) fn rate_text(rate: f64) -> String {
    let (unit, scale) = [
        ("years/s", YEAR),
        ("days/s", 86_400.0),
        ("hours/s", 3_600.0),
        ("minutes/s", 60.0),
        ("seconds/s", 1.0),
    ]
    .into_iter()
    .find(|(_, scale)| rate >= *scale)
    .unwrap_or(("seconds/s", 1.0));
    format!("{:.2} {unit}", rate / scale)
}

pub(super) fn time_controls(
    ctx: &egui::Context,
    rect: egui::Rect,
    clock: &SimulationClock,
    held: bool,
    drag_start: &mut Option<f64>,
    out: &mut Vec<UiCommand>,
) {
    egui::Area::new(egui::Id::new("time-controls"))
        .order(egui::Order::Background)
        .movable(false)
        .pivot(egui::Align2::CENTER_BOTTOM)
        .fixed_pos(rect.center_bottom() - egui::vec2(0.0, HUD_PADDING))
        .show(ctx, |ui| {
            ui.set_width(260.0);
            ui.spacing_mut().item_spacing.y = HUD_LINE_GAP;
            ui.vertical_centered(|ui| {
                halo_text(
                    ui,
                    &elapsed_text(clock.seconds / 86_400.0),
                    egui::Sense::hover(),
                    |_| CURRENT,
                );
                let mut label = rate_text(clock.rate);
                if clock.paused {
                    label.push_str(" · paused");
                } else if held {
                    // Craft physics cannot step this fast: time waits for it.
                    label.push_str(" · held by craft");
                }
                let response = halo_text(ui, &label, egui::Sense::click_and_drag(), |response| {
                    if response.hovered() || response.dragged() || response.has_focus() {
                        HOVER
                    } else {
                        LINK
                    }
                })
                .on_hover_cursor(egui::CursorIcon::ResizeHorizontal)
                .on_hover_text_themed(
                    &Theme::GAME,
                    if clock.paused {
                        "Click to resume · Drag left/right to adjust speed"
                    } else {
                        "Click to pause · Drag left/right to adjust speed"
                    },
                );
                ctx.data_mut(|data| {
                    data.insert_temp(
                        egui::Id::new("time-pointer-capture"),
                        (
                            response.rect,
                            response.dragged() || response.is_pointer_button_down_on(),
                        ),
                    );
                });
                if response.drag_started() {
                    *drag_start = Some(clock.rate);
                }
                if response.dragged()
                    && let Some(start) = *drag_start
                {
                    out.push(UiCommand::SetRate {
                        rate: dragged_rate(
                            start,
                            response.total_drag_delta().unwrap_or_default().x,
                        ),
                    });
                }
                if response.drag_stopped() {
                    *drag_start = None;
                }
                if response.clicked() {
                    out.push(UiCommand::SetPaused {
                        paused: !clock.paused,
                    });
                }
            });
        });
}

/// The piloted ship's lines, centred above the time controls' two lines.
pub(super) fn pilot_readout(ctx: &egui::Context, rect: egui::Rect, lines: &[String]) {
    let height = ctx.fonts_mut(|fonts| fonts.row_height(&egui::FontId::proportional(SIZE)));
    let above = HUD_PADDING + 2.0 * (height + HUD_LINE_GAP) + 2.0 * HUD_LINE_GAP;
    egui::Area::new(egui::Id::new("pilot-readout"))
        .order(egui::Order::Background)
        .movable(false)
        .interactable(false)
        .pivot(egui::Align2::CENTER_BOTTOM)
        .fixed_pos(rect.center_bottom() - egui::vec2(0.0, above))
        .show(ctx, |ui| {
            ui.set_width(420.0);
            ui.spacing_mut().item_spacing.y = HUD_LINE_GAP;
            ui.vertical_centered(|ui| {
                for line in lines {
                    halo_text(ui, line, egui::Sense::hover(), |_| CURRENT);
                }
            });
        });
}

fn elapsed_text(days: f64) -> String {
    if days >= 365.25 {
        format!("T + {:.2} yr", days / 365.25)
    } else {
        format!("T + {days:.2} days")
    }
}

/// What the camera moves with, then measured velocity above a draggable set
/// speed, like the time controls.
#[allow(clippy::too_many_arguments)]
pub(super) fn speed_readout(
    ctx: &egui::Context,
    rect: egui::Rect,
    moving_with: Option<&str>,
    set_speed: f64,
    velocity: f64,
    drag_start: &mut Option<f64>,
    out: &mut Vec<UiCommand>,
) {
    egui::Area::new(egui::Id::new("speed-readout"))
        .order(egui::Order::Background)
        .movable(false)
        .pivot(egui::Align2::LEFT_BOTTOM)
        .fixed_pos(rect.left_bottom() + egui::vec2(HUD_PADDING, -HUD_PADDING))
        .show(ctx, |ui| {
            ui.spacing_mut().item_spacing.y = HUD_LINE_GAP;
            if let Some(anchor) = moving_with {
                halo_text(
                    ui,
                    &format!("Moving with {anchor}"),
                    egui::Sense::hover(),
                    |_| CURRENT,
                );
            }
            halo_text(
                ui,
                &format!("Velocity {}", speed_label(velocity)),
                egui::Sense::hover(),
                |_| CURRENT,
            );
            let response = halo_text(
                ui,
                &format!("Set speed {}", speed_label(set_speed)),
                egui::Sense::click_and_drag(),
                |response| {
                    if response.hovered() || response.dragged() || response.has_focus() {
                        HOVER
                    } else {
                        LINK
                    }
                },
            )
            .on_hover_cursor(egui::CursorIcon::ResizeHorizontal)
            .on_hover_text_themed(&Theme::GAME, "Drag left/right to adjust movement speed");
            ctx.data_mut(|data| {
                data.insert_temp(
                    egui::Id::new("speed-pointer-capture"),
                    (
                        response.rect,
                        response.dragged() || response.is_pointer_button_down_on(),
                    ),
                );
            });
            if response.drag_started() {
                *drag_start = Some(set_speed);
            }
            if response.dragged()
                && let Some(start) = *drag_start
            {
                out.push(UiCommand::SetMoveSpeed {
                    speed: dragged_value(
                        start,
                        response.total_drag_delta().unwrap_or_default().x,
                        crate::scene::MIN_SPEED,
                        crate::scene::MAX_SPEED,
                    ),
                });
            }
            if response.drag_stopped() {
                *drag_start = None;
            }
        });
}

/// The magnitude limit `start` dragged `delta` points, to a tenth: the same
/// span as the other HUD controls covers its whole range.
pub(super) fn dragged_magnitude(start: f64, delta: f32) -> f64 {
    let (min, max) = (*crate::MAGNITUDE.start(), *crate::MAGNITUDE.end());
    ((start + f64::from(delta) / HUD_DRAG_SPAN * (max - min)).clamp(min, max) * 10.0).round() / 10.0
}

/// The draggable star point magnitude limit, at the bottom right. It applies
/// on release, as each change reloads the stars.
pub(super) fn magnitude_control(
    ctx: &egui::Context,
    rect: egui::Rect,
    magnitude: f64,
    drag: &mut Option<(f64, f64)>,
    out: &mut Vec<UiCommand>,
) {
    egui::Area::new(egui::Id::new("magnitude-control"))
        .order(egui::Order::Background)
        .movable(false)
        .pivot(egui::Align2::RIGHT_BOTTOM)
        .fixed_pos(rect.right_bottom() + egui::vec2(-HUD_PADDING, -HUD_PADDING))
        .show(ctx, |ui| {
            let shown = drag.map_or(magnitude, |(_, now)| now);
            let response = halo_text(
                ui,
                &format!("Magnitude limit {shown:.1}"),
                egui::Sense::click_and_drag(),
                |response| {
                    if response.hovered() || response.dragged() || response.has_focus() {
                        HOVER
                    } else {
                        LINK
                    }
                },
            )
            .on_hover_cursor(egui::CursorIcon::ResizeHorizontal)
            .on_hover_text_themed(&Theme::GAME,
                "Faintest star drawn as a point; fainter ones join the galaxy glow · Drag left/right, stars reload on release",
            );
            ctx.data_mut(|data| {
                data.insert_temp(
                    egui::Id::new("magnitude-pointer-capture"),
                    (
                        response.rect,
                        response.dragged() || response.is_pointer_button_down_on(),
                    ),
                );
            });
            if response.drag_started() {
                *drag = Some((magnitude, magnitude));
            }
            if response.dragged()
                && let Some((start, now)) = drag
            {
                *now = dragged_magnitude(*start, response.total_drag_delta().unwrap_or_default().x);
            }
            if response.drag_stopped()
                && let Some((_, now)) = drag.take()
                && now != magnitude
            {
                out.push(UiCommand::SetMagnitude { magnitude: now });
            }
        });
}

/// Two immediate draggable text controls, aligned above the magnitude limit.
pub(super) fn exposure_controls(
    ctx: &egui::Context,
    rect: egui::Rect,
    nav: &Navigation,
    drags: &mut [Option<f64>; 2],
    out: &mut Vec<UiCommand>,
) {
    let height = ctx.fonts_mut(|fonts| fonts.row_height(&egui::FontId::proportional(SIZE)));
    for (index, (id, value, text, help)) in [
        ("exposure", nav.exposure, format!("Exposure {:+.2} stops", nav.exposure), "Drag left/right: 20 pixels per stop. +1 stop doubles light, -1 halves it. Applies immediately, with no automatic adaptation."),
        ("compression", nav.compression, format!("Dynamic Range Compression {:.0}%", nav.compression * 100.0), "Drag left/right: 400 pixels covers 0 to 100%. Exposes each star, corona and planet for itself: 0% is one physical exposure, 100% shows every body correctly exposed. The galaxy always follows Exposure. Applies immediately."),
    ].into_iter().enumerate() {
        egui::Area::new(egui::Id::new(("manual-control", id)))
            .order(egui::Order::Background)
            .movable(false)
            .pivot(egui::Align2::RIGHT_BOTTOM)
            .fixed_pos(rect.right_bottom() + egui::vec2(-HUD_PADDING, -HUD_PADDING - (2 - index) as f32 * (height + HUD_LINE_GAP)))
            .show(ctx, |ui| {
                let response = halo_text(ui, &text, egui::Sense::click_and_drag(), |r| {
                    if nav.realistic && (r.hovered() || r.dragged() || r.has_focus()) { HOVER } else { LINK }
                }).on_hover_cursor(egui::CursorIcon::ResizeHorizontal).on_hover_text_themed(&Theme::GAME,
                    if nav.realistic { help } else { "Turn on realistic lighting in the debug console's render tab to adjust this." });
                ctx.data_mut(|data| data.insert_temp(egui::Id::new(format!("{id}-pointer-capture")),
                    (response.rect, response.dragged() || response.is_pointer_button_down_on())));
                if response.drag_started() && nav.realistic { drags[index] = Some(value); }
                if nav.realistic && response.dragged() && let Some(start) = drags[index] {
                    let delta = f64::from(response.total_drag_delta().unwrap_or_default().x);
                    out.push(if index == 0 {
                        UiCommand::SetExposure { stops: (start + delta / 20.0).clamp(*crate::render::exposure::STOPS.start(), *crate::render::exposure::STOPS.end()) }
                    } else {
                        UiCommand::SetCompression { amount: (start + delta / HUD_DRAG_SPAN).clamp(0.0, 1.0) }
                    });
                }
                if response.drag_stopped() || !nav.realistic { drags[index] = None; }
            });
    }
}
