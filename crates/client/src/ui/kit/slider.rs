//! A full-width slider row, split like `check`: the label, a text-mode track
//! (`[=====|-----]`) and the value on the left, a square description button
//! on the right.
use super::Theme;
use super::interact::{Highlight, control_sense, held_click, highlight, hover_amount};
use super::widgets::note;
use bevy_egui::egui::{self, Align2, Rect, Sense, Ui, pos2, vec2};
use std::ops::RangeInclusive;

/// Labels shorter than this many characters share one column, so the
/// tracks of neighbouring rows line up.
const LABEL_CHARS: f32 = 15.0;
/// Room for the value and its unit, such as `0.0001 bar`.
const VALUE_CHARS: f32 = 10.0;

pub struct Slider<'a> {
    pub label: &'a str,
    pub detail: &'a str,
    pub range: RangeInclusive<f64>,
    pub unit: &'a str,
    pub logarithmic: bool,
    pub integer: bool,
}

impl Slider<'_> {
    /// Where `value` sits along the track, 0 to 1.
    fn fraction(&self, value: f64) -> f32 {
        let (min, max) = (*self.range.start(), *self.range.end());
        let fraction = if self.logarithmic {
            let low = self.log_low();
            if value <= low {
                0.0
            } else {
                (value / low).ln() / (max / low).ln()
            }
        } else {
            (value - min) / (max - min)
        };
        fraction.clamp(0.0, 1.0) as f32
    }

    /// The value at `fraction` along the track, rounded to what the row shows.
    fn value(&self, fraction: f32) -> f64 {
        let (min, max) = (*self.range.start(), *self.range.end());
        let fraction = f64::from(fraction.clamp(0.0, 1.0));
        let value = if self.logarithmic && fraction > 0.0 {
            let low = self.log_low();
            low * (max / low).powf(fraction)
        } else {
            min + fraction * (max - min)
        };
        let value = if self.integer {
            value.round()
        } else {
            significant(value)
        };
        value.clamp(min, max)
    }

    /// A logarithmic track starting at zero starts its scale five decades
    /// below the top, leaving zero at the very left.
    fn log_low(&self) -> f64 {
        let min = *self.range.start();
        if min > 0.0 {
            min
        } else {
            *self.range.end() * 1e-5
        }
    }

    fn text(&self, value: f64) -> String {
        let number = if self.integer {
            format!("{value:.0}")
        } else if value == 0.0 {
            "0".into()
        } else {
            let decimals = (2 - value.abs().log10().floor() as i32).clamp(0, 6) as usize;
            let text = format!("{value:.decimals$}");
            if text.contains('.') {
                text.trim_end_matches('0').trim_end_matches('.').into()
            } else {
                text
            }
        };
        format!("{number}{}", self.unit)
    }

    /// Draws the row, returning the new value while it is dragged or clicked.
    pub fn show(&self, ui: &mut Ui, theme: &Theme, value: f64) -> Option<f64> {
        let id = ui.make_persistent_id(("slider", self.label));
        let mut open = ui.data(|data| data.get_temp::<bool>(id).unwrap_or(false));
        let font = theme.font.id();
        let height = theme.row;
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::hover());
        let divider = rect.right() - height;
        let left = Rect::from_min_max(rect.min, pos2(divider, rect.bottom()));
        let detail_rect = Rect::from_min_max(pos2(divider, rect.top()), rect.max);
        let cell = ui
            .painter()
            .layout_no_wrap("0".into(), font.clone(), theme.text)
            .size()
            .x;
        let label = ui
            .painter()
            .layout_no_wrap(self.label.to_owned(), font.clone(), theme.text);
        let track_left = left.left() + theme.inset + label.size().x.max(cell * LABEL_CHARS) + cell;
        let value_right = divider - theme.inset;
        let track_right = value_right - cell * (VALUE_CHARS + 1.0);
        // Whole characters between the brackets, at least a few.
        let cells = (((track_right - track_left) / cell).floor() - 2.0).max(4.0) as usize;
        let inner = Rect::from_min_max(
            pos2(track_left + cell, rect.top()),
            pos2(track_left + cell * (cells as f32 + 1.0), rect.bottom()),
        );

        let row = ui.interact(left, id.with("row"), Sense::hover());
        let track = held_click(ui.interact(
            Rect::from_min_max(
                pos2(track_left, rect.top()),
                pos2(inner.right() + cell, rect.bottom()),
            ),
            id.with("track"),
            Sense::click_and_drag(),
        ))
        .on_hover_cursor(egui::CursorIcon::ResizeHorizontal);
        let expand = held_click(ui.interact(detail_rect, id.with("expand"), control_sense(true)))
            .on_hover_cursor(egui::CursorIcon::PointingHand);
        let hover = hover_amount(ui, &row, true).max(hover_amount(ui, &track, true));
        let detail_hover = hover_amount(ui, &expand, true);
        highlight(ui, theme, left, hover, Highlight::Left);
        highlight(ui, theme, detail_rect, detail_hover, Highlight::Right);
        ui.painter()
            .vline(divider, rect.y_range(), (1.0, theme.line()));
        if expand.clicked() {
            open = !open;
        }

        let mut changed = None;
        if track.is_pointer_button_down_on()
            && let Some(pointer) = track.interact_pointer_pos()
        {
            // Each character is a step; its centre holds the step's value.
            let fraction =
                (pointer.x - inner.left() - cell * 0.5) / (inner.width() - cell).max(1.0);
            let new = self.value(fraction);
            if new != value {
                changed = Some(new);
            }
        }
        let shown = changed.unwrap_or(value);

        let painter = ui.painter();
        let text = theme.text.lerp_to_gamma(theme.accent, hover);
        let y = rect.center().y;
        painter.galley_with_override_text_color(
            pos2(left.left() + theme.inset, y - label.size().y / 2.0),
            label,
            text,
        );
        let handle = (self.fraction(shown) * (cells - 1) as f32).round() as usize;
        let mut job = egui::text::LayoutJob::default();
        let mut add = |part: &str, colour| {
            job.append(part, 0.0, egui::TextFormat::simple(font.clone(), colour));
        };
        add("[", theme.dim);
        add(&"=".repeat(handle), theme.accent);
        add("|", theme.text.lerp_to_gamma(theme.accent, hover));
        add(
            &"-".repeat(cells - 1 - handle),
            theme.dim.gamma_multiply(0.6),
        );
        add("]", theme.dim);
        let galley = painter.layout_job(job);
        painter.galley(
            pos2(track_left, y - galley.size().y / 2.0),
            galley,
            theme.text,
        );
        painter.text(
            pos2(value_right, y),
            Align2::RIGHT_CENTER,
            self.text(shown),
            font.clone(),
            theme.text,
        );
        painter.text(
            detail_rect.center(),
            Align2::CENTER_CENTER,
            if open { "-" } else { "+" },
            font,
            theme.text.lerp_to_gamma(theme.accent, detail_hover),
        );
        ui.data_mut(|data| data.insert_temp(id, open));
        if open {
            note(ui, theme, self.detail);
        }
        changed
    }
}

/// Three significant figures, so dragged values read cleanly.
fn significant(value: f64) -> f64 {
    if value == 0.0 {
        return 0.0;
    }
    let scale = 10f64.powi(2 - value.abs().log10().floor() as i32);
    (value * scale).round() / scale
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slider(range: RangeInclusive<f64>, logarithmic: bool) -> Slider<'static> {
        Slider {
            label: "test",
            detail: "A detail.",
            range,
            unit: " bar",
            logarithmic,
            integer: false,
        }
    }

    #[test]
    fn tracks_map_both_ways_and_show_clean_values() {
        let linear = slider(0.0..=10.0, false);
        assert_eq!(linear.value(0.5), 5.0);
        assert_eq!(linear.fraction(2.5), 0.25);
        let log = slider(1e-4..=1000.0, true);
        assert_eq!(log.value(0.0), 1e-4);
        assert_eq!(log.value(1.0), 1000.0);
        assert!((log.fraction(1.0) - 4.0 / 7.0).abs() < 1e-6);
        // A logarithmic track from zero keeps zero at its left end.
        let haze = slider(0.0..=1000.0, true);
        assert_eq!(haze.value(0.0), 0.0);
        assert_eq!(haze.fraction(0.0), 0.0);
        assert!(haze.value(0.01) > 0.0);
        assert_eq!(log.text(1.01325), "1.01 bar");
        assert_eq!(log.text(92.0), "92 bar");
        assert_eq!(log.text(0.0001), "0.0001 bar");
        assert_eq!(log.text(1000.0), "1000 bar");
    }

    #[test]
    fn pressing_the_track_sets_its_value_and_the_square_opens_the_detail() {
        let ctx = egui::Context::default();
        let theme = Theme::TERMINAL;
        let row = slider(0.0..=10.0, false);
        let frame = |events| {
            let mut result = (None, 0.0);
            let _ = ctx.run(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(600.0, 200.0))),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        ui.spacing_mut().item_spacing.y = 0.0;
                        let changed = row.show(ui, &theme, 5.0);
                        result = (changed, ui.next_widget_position().y);
                    });
                },
            );
            result
        };
        let press = |pos, pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        let (_, closed) = frame(vec![]);
        // Near the right end of the track: close to the top of the range.
        let end = pos2(470.0, 20.0);
        frame(vec![egui::Event::PointerMoved(end)]);
        let (changed, _) = frame(vec![press(end, true)]);
        assert!(changed.is_some_and(|v| v > 8.0), "{changed:?}");
        frame(vec![press(end, false)]);
        let square = pos2(580.0, 20.0);
        frame(vec![egui::Event::PointerMoved(square)]);
        frame(vec![press(square, true)]);
        frame(vec![press(square, false)]);
        let (_, open) = frame(vec![]);
        assert!(open > closed);
    }
}
