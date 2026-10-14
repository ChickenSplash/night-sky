//! Optional numeric endpoints and an optional two-handle slider in one field.
use super::interact::{focus_input_padding, held_click, input_edges};
use super::{SPACING, Theme};
use bevy_egui::egui::{self, Align2, Rect, Sense, Ui, pos2, vec2};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct NumericRange {
    pub min: Option<f64>,
    pub max: Option<f64>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RangeScale {
    Linear,
    Logarithmic,
}

/// An exploration span, not a validation bound. Typed values can extend it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RangeSlider {
    pub min: f64,
    pub max: f64,
    pub scale: RangeScale,
    pub step: Option<f64>,
}

impl RangeSlider {
    fn scale_label(self) -> String {
        if self.scale == RangeScale::Linear {
            return "Linear scale".into();
        }
        let decades = self.max.log10() - self.min.log10();
        let rounded = format!("{decades:.2}");
        let count = rounded.trim_end_matches('0').trim_end_matches('.');
        format!(
            "Log scale · {count} {}",
            if count == "1" { "decade" } else { "decades" }
        )
    }
    fn extended(mut self, range: NumericRange) -> Self {
        for value in [range.min, range.max].into_iter().flatten() {
            self.min = self.min.min(value);
            self.max = self.max.max(value);
        }
        // Logarithms cannot represent zero or negatives. Preserve valid typed
        // values by using linear exploration, never silently clamp them away.
        if self.min <= 0.0 {
            self.scale = RangeScale::Linear;
        }
        self
    }

    fn fraction(self, value: f64) -> f32 {
        let fraction = match self.scale {
            RangeScale::Linear => {
                let scale = self.min.abs().max(self.max.abs()).max(1.0);
                (value / scale - self.min / scale) / (self.max / scale - self.min / scale)
            }
            RangeScale::Logarithmic => {
                (value.ln() - self.min.ln()) / (self.max.ln() - self.min.ln())
            }
        };
        fraction.clamp(0.0, 1.0) as f32
    }

    fn value(self, fraction: f32, integer: bool) -> f64 {
        let t = f64::from(fraction.clamp(0.0, 1.0));
        let value = match self.scale {
            RangeScale::Linear => (1.0 - t) * self.min + t * self.max,
            RangeScale::Logarithmic => ((1.0 - t) * self.min.ln() + t * self.max.ln()).exp(),
        };
        let step = if integer { Some(1.0) } else { self.step };
        let rounded = match step {
            Some(step) if step > 0.0 => (value / step).round() * step,
            _ if value != 0.0 => {
                let magnitude = 10.0_f64.powf(value.abs().log10().floor() - 4.0);
                if magnitude > 0.0 {
                    (value / magnitude).round() * magnitude
                } else {
                    value
                }
            }
            _ => value,
        };
        rounded.clamp(self.min, self.max)
    }
}

/// Slider values use five significant digits, not f64's full binary-to-decimal
/// representation. Small values keep useful precision; typed text is untouched.
fn slider_text(value: f64, integer: bool) -> String {
    if integer || value == 0.0 {
        return format!("{value:.0}");
    }
    let decimals = (4.0 - value.abs().log10().floor()).max(0.0) as usize;
    if decimals > 12 || value.abs() >= 1e12 {
        return format!("{value:.4e}");
    }
    let text = format!("{value:.decimals$}");
    if decimals > 0 {
        text.trim_end_matches('0').trim_end_matches('.').to_owned()
    } else {
        text
    }
}

fn set_slider_endpoint(
    input: &mut RangeInput,
    range: NumericRange,
    lower: bool,
    value: f64,
    integer: bool,
) {
    let text = slider_text(value, integer);
    let displayed = text.parse::<f64>().unwrap_or(value);
    if lower {
        input.min = if range.max.is_some_and(|max| displayed >= max) {
            input.max.clone()
        } else {
            text
        };
    } else {
        input.max = if range.min.is_some_and(|min| displayed <= min) {
            input.min.clone()
        } else {
            text
        };
    }
}

#[derive(Clone, Debug, Default)]
pub struct RangeInput {
    pub min: String,
    pub max: String,
    // Keep the exploration span stable while dragging back inward.
    span: Option<RangeSlider>,
}

impl RangeInput {
    pub fn new(min: String, max: String) -> Self {
        Self {
            min,
            max,
            span: None,
        }
    }

    pub fn value(&self, integer: bool) -> Result<NumericRange, String> {
        let min = parse_range_end(&self.min, integer)?;
        let max = parse_range_end(&self.max, integer)?;
        if matches!((min, max), (Some(min), Some(max)) if min > max) {
            return Err("The minimum is above the maximum".into());
        }
        Ok(NumericRange { min, max })
    }
}

/// Blank means unbounded. Accept the separators used by the filter editor.
pub fn parse_range_end(text: &str, integer: bool) -> Result<Option<f64>, String> {
    let text = text.trim().replace([',', '_', ' '], "");
    if text.is_empty() {
        return Ok(None);
    }
    let value = text
        .parse::<f64>()
        .ok()
        .filter(|value| value.is_finite())
        .ok_or_else(|| format!("`{text}` is not a number"))?;
    if integer && value.fract() != 0.0 {
        return Err(format!("{text} is not a whole number"));
    }
    Ok(Some(value))
}

pub struct RangeField<'a> {
    pub unit: &'a str,
    pub integer: bool,
    pub slider: Option<RangeSlider>,
}

pub struct RangeResponse {
    pub response: egui::Response,
    pub value: Result<NumericRange, String>,
}

fn endpoint(
    ui: &mut Ui,
    theme: &Theme,
    rect: Rect,
    id: egui::Id,
    label: &str,
    text: &mut String,
) -> egui::Response {
    let label_width = ui
        .painter()
        .layout_no_wrap(label.into(), theme.font.id(), theme.dim)
        .size()
        .x;
    ui.painter().text(
        pos2(rect.left(), rect.center().y),
        Align2::LEFT_CENTER,
        label,
        theme.font.id(),
        theme.dim,
    );
    let edit_rect = Rect::from_min_max(
        pos2(rect.left() + label_width + SPACING, rect.top()),
        rect.max,
    );
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(edit_rect)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    child.add(
        egui::TextEdit::singleline(text)
            .id(id)
            .font(theme.font.id())
            .text_color(theme.text)
            .hint_text("Any")
            .frame(false)
            .margin(egui::Margin::ZERO)
            .desired_width(edit_rect.width().max(1.0)),
    )
}

/// The two text edits, units and optional slider share one background and
/// animated end edges. Text remains editable even when the range is invalid.
pub fn range_field(
    ui: &mut Ui,
    theme: &Theme,
    id: &str,
    input: &mut RangeInput,
    config: RangeField<'_>,
) -> RangeResponse {
    let id = ui.make_persistent_id(("range-field", id));
    let before = (input.min.clone(), input.max.clone());
    let height = theme.field_height()
        + if config.slider.is_some() {
            SPACING + 48.0
        } else {
            0.0
        };
    let (outer, mut response) =
        ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::hover());
    ui.painter().rect_filled(outer, 0, theme.input_fill());
    let inner = outer.shrink2(vec2(12.0, 0.0));
    let unit_width = if config.unit.is_empty() {
        0.0
    } else {
        ui.painter()
            .layout_no_wrap(config.unit.into(), theme.font.id(), theme.dim)
            .size()
            .x
            + SPACING
    };
    let fields = Rect::from_min_size(
        inner.min,
        vec2((inner.width() - unit_width).max(0.0), theme.field_height()),
    );
    let half = ((fields.width() - SPACING) * 0.5).max(0.0);
    let min_rect = Rect::from_min_size(fields.min, vec2(half, fields.height()));
    let max_rect = Rect::from_min_size(
        pos2(fields.left() + half + SPACING, fields.top()),
        min_rect.size(),
    );
    let min = endpoint(ui, theme, min_rect, id.with("min"), "Min:", &mut input.min);
    let max = endpoint(ui, theme, max_rect, id.with("max"), "Max:", &mut input.max);
    focus_input_padding(
        ui,
        &min,
        Rect::from_min_max(outer.min, min_rect.max),
        &input.min,
    );
    focus_input_padding(
        ui,
        &max,
        Rect::from_min_max(max_rect.min, pos2(outer.right(), fields.bottom())),
        &input.max,
    );
    ui.painter().text(
        pos2(inner.right(), fields.center().y),
        Align2::RIGHT_CENTER,
        config.unit,
        theme.font.id(),
        theme.dim,
    );
    let mut active = min.has_focus() || max.has_focus();
    if let Some(default_span) = config.slider {
        let range = input.value(config.integer);
        let span = input
            .span
            .unwrap_or(default_span)
            .extended(range.clone().unwrap_or_default());
        input.span = Some(span);
        let track = Rect::from_min_max(
            pos2(inner.left() + 5.0, fields.bottom() + SPACING),
            pos2(inner.right() - 5.0, fields.bottom() + SPACING + 24.0),
        );
        let slider = slider(
            ui,
            theme,
            id.with("slider"),
            track,
            input,
            span,
            config.integer,
        );
        active |= slider.has_focus() || slider.is_pointer_button_down_on();
        response = response.union(slider);
        for (x, align, value) in [
            (track.left(), Align2::LEFT_TOP, span.min),
            (track.right(), Align2::RIGHT_TOP, span.max),
        ] {
            ui.painter().text(
                pos2(x, track.bottom() + 3.0),
                align,
                value.to_string(),
                theme.small.id(),
                theme.dim,
            );
        }
        ui.painter().text(
            pos2(track.center().x, track.bottom() + 3.0),
            Align2::CENTER_TOP,
            span.scale_label(),
            theme.small.id(),
            theme.dim,
        );
    }
    input_edges(
        ui,
        theme,
        outer,
        id,
        active || ui.rect_contains_pointer(outer),
    );
    response = response.union(min).union(max);
    response.flags.set(
        egui::response::Flags::CHANGED,
        before != (input.min.clone(), input.max.clone()),
    );
    RangeResponse {
        response,
        value: input.value(config.integer),
    }
}

fn slider(
    ui: &mut Ui,
    theme: &Theme,
    id: egui::Id,
    rect: Rect,
    input: &mut RangeInput,
    span: RangeSlider,
    integer: bool,
) -> egui::Response {
    let enabled = input.value(integer).is_ok();
    let response = held_click(ui.interact(
        rect,
        id,
        if enabled {
            Sense::click_and_drag()
        } else {
            Sense::hover()
        },
    ));
    let range = input.value(integer).unwrap_or_default();
    let min = span.fraction(range.min.unwrap_or(span.min));
    let max = span.fraction(range.max.unwrap_or(span.max));
    let x = |fraction: f32| rect.left() + fraction * rect.width();
    let origin_id = id.with("handle");
    let selected_id = id.with("selected-handle");
    if enabled && response.is_pointer_button_down_on() {
        if let Some(pointer) = response.interact_pointer_pos() {
            let lower = ui.data_mut(|data| {
                let min_distance = (pointer.x - x(min)).abs();
                let max_distance = (pointer.x - x(max)).abs();
                let nearest = if min_distance == max_distance {
                    pointer.x < x(min)
                } else {
                    min_distance < max_distance
                };
                *data.get_temp_mut_or(origin_id, nearest)
            });
            ui.data_mut(|data| data.insert_temp(selected_id, lower));
            let fraction = ((pointer.x - rect.left()) / rect.width().max(1.0)).clamp(0.0, 1.0);
            let value = span.value(fraction, integer);
            set_slider_endpoint(input, range, lower, value, integer);
            response.request_focus();
        }
    } else {
        ui.data_mut(|data| data.remove::<bool>(origin_id));
    }
    if enabled && response.has_focus() {
        let mut lower = ui.data_mut(|data| data.get_temp::<bool>(selected_id).unwrap_or(true));
        let delta = ui.input(|keys| {
            if keys.key_pressed(egui::Key::ArrowDown) {
                lower = true;
            }
            if keys.key_pressed(egui::Key::ArrowUp) {
                lower = false;
            }
            let direction = i32::from(keys.key_pressed(egui::Key::ArrowRight))
                - i32::from(keys.key_pressed(egui::Key::ArrowLeft));
            direction as f32 * if keys.modifiers.shift { 0.1 } else { 0.01 }
        });
        ui.data_mut(|data| data.insert_temp(selected_id, lower));
        if delta != 0.0 {
            let fraction = if lower { min } else { max };
            let value = span.value(fraction + delta, integer);
            set_slider_endpoint(input, range, lower, value, integer);
        }
    }
    let range = input.value(integer).unwrap_or_default();
    let left = x(span.fraction(range.min.unwrap_or(span.min)));
    let right = x(span.fraction(range.max.unwrap_or(span.max)));
    let y = rect.center().y;
    let accent = if enabled { theme.accent } else { theme.dim };
    ui.painter()
        .hline(rect.x_range(), y, (2.0, theme.dim.gamma_multiply(0.3)));
    ui.painter()
        .line_segment([pos2(left, y), pos2(right, y)], (2.0, accent));
    for (x, bounded) in [(left, range.min.is_some()), (right, range.max.is_some())] {
        let handle = Rect::from_center_size(pos2(x, y), vec2(8.0, 14.0));
        ui.painter().rect(
            handle,
            0,
            if bounded { accent } else { theme.input_fill() },
            (1.0, accent),
            egui::StrokeKind::Inside,
        );
    }
    response.on_hover_cursor(if enabled {
        egui::CursorIcon::ResizeHorizontal
    } else {
        egui::CursorIcon::Default
    })
}

#[cfg(test)]
mod tests;
