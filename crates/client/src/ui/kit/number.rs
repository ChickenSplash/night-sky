//! A prefix, editable number and optional unit inside one shared frame.
use super::interact::{focus_input_padding, input_underline};
use super::{SPACING, Theme};
use bevy_egui::egui::{self, Ui, emath::Numeric};
use std::ops::RangeInclusive;

#[derive(Clone)]
struct Edit {
    text: String,
    value: f64,
}

fn committed<Num: Numeric>(text: &str, range: &RangeInclusive<Num>) -> Option<Num> {
    let value = text.trim().parse::<f64>().ok()?;
    if !value.is_finite()
        || (Num::INTEGRAL && value.fract() != 0.0)
        || (!Num::INTEGRAL && value < range.start().to_f64())
    {
        return None;
    }
    Some(Num::from_f64(
        value.clamp(range.start().to_f64(), range.end().to_f64()),
    ))
}

/// Applies valid edits on Enter or focus loss, clamping to the given range.
/// Invalid edits revert to the last value. Prefixes identify fields within a UI.
pub fn number_field<Num: Numeric>(
    ui: &mut Ui,
    theme: &Theme,
    prefix: &str,
    value: &mut Num,
    range: RangeInclusive<Num>,
    suffix: &str,
) -> egui::Response {
    let id = ui.make_persistent_id(("number-field", prefix));
    let mut edit = ui
        .data_mut(|data| data.get_temp::<Edit>(id))
        .unwrap_or(Edit {
            text: value.to_f64().to_string(),
            value: value.to_f64(),
        });
    // Queries arriving from commands or Reset also update the displayed value.
    if edit.value != value.to_f64() {
        edit.value = value.to_f64();
        edit.text = edit.value.to_string();
    }
    let label = format!("{prefix}:");
    let label_width = |text: &str| {
        ui.painter()
            .layout_no_wrap(text.to_owned(), theme.font.id(), theme.dim)
            .size()
            .x
    };
    let number_width = ui
        .painter()
        .layout_no_wrap(edit.text.clone(), theme.font.id(), theme.text)
        .size()
        .x
        .max(1.0);
    let width = label_width(&label)
        + number_width
        + SPACING
        + if suffix.is_empty() {
            0.0
        } else {
            label_width(suffix) + SPACING
        };
    let shown = egui::Frame::new()
        .fill(theme.input_fill())
        .stroke(egui::Stroke::NONE)
        .corner_radius(0)
        .inner_margin(theme.input_padding())
        .show(ui, |ui| {
            ui.set_width(width);
            ui.spacing_mut().item_spacing.x = SPACING;
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(&label)
                        .font(theme.font.id())
                        .color(theme.dim),
                );
                let response = ui.add(
                    egui::TextEdit::singleline(&mut edit.text)
                        .id(id)
                        .font(theme.font.id())
                        .text_color(theme.text)
                        .frame(false)
                        .margin(egui::Margin::ZERO)
                        .clip_text(false)
                        .desired_width(number_width),
                );
                if !suffix.is_empty() {
                    ui.label(
                        egui::RichText::new(suffix)
                            .font(theme.font.id())
                            .color(theme.dim),
                    );
                }
                response
            })
            .inner
        });
    let mut response = shown.inner;
    let outer = shown.response.rect;
    focus_input_padding(ui, &response, outer, &edit.text);
    input_underline(
        ui,
        theme,
        outer,
        id,
        ui.memory(|memory| memory.has_focus(id)) || ui.rect_contains_pointer(outer),
    );
    let enter = response.has_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));
    // changed() describes a committed numeric change, not unfinished typing.
    let before = *value;
    if response.lost_focus() || enter {
        if let Some(next) = committed(&edit.text, &range) {
            *value = next;
        }
        edit.value = value.to_f64();
        edit.text = edit.value.to_string();
    }
    response
        .flags
        .set(egui::response::Flags::CHANGED, *value != before);
    ui.data_mut(|data| data.insert_temp(id, edit));
    response
}

#[cfg(test)]
mod tests {
    use super::committed;

    #[test]
    fn counts_are_whole_and_bounded() {
        assert_eq!(committed("50", &(1_u32..=1000)), Some(50));
        assert_eq!(committed("123", &(1_u32..=1000)), Some(123));
        assert_eq!(committed("2000", &(1_u32..=1000)), Some(1000));
        assert_eq!(committed("0", &(1_u32..=1000)), Some(1));
        assert_eq!(committed("2.5", &(1_u32..=1000)), None);
    }

    #[test]
    fn number_width_grows_and_shrinks_with_its_text() {
        use super::{Theme, number_field};
        use bevy_egui::egui;

        let ctx = egui::Context::default();
        let width = |mut value: u32| {
            let mut width = 0.0;
            let _ = ctx.run(egui::RawInput::default(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    width = number_field(ui, &Theme::GAME, "Show", &mut value, 1..=1000, "")
                        .rect
                        .width();
                });
            });
            width
        };
        let short = width(1);
        let long = width(1000);
        let medium = width(50);
        assert!(long > medium && medium > short);
    }

    #[test]
    fn labels_units_and_padding_focus_and_select_the_number() {
        use super::{Theme, number_field};
        use bevy_egui::egui;

        for zone in 0..5 {
            let ctx = egui::Context::default();
            let mut value = 50.0;
            let mut frame = |events| {
                let mut rect = egui::Rect::NOTHING;
                let mut id = egui::Id::NULL;
                let _ = ctx.run(
                    egui::RawInput {
                        events,
                        ..Default::default()
                    },
                    |ctx| {
                        egui::CentralPanel::default().show(ctx, |ui| {
                            let response = number_field(
                                ui,
                                &Theme::GAME,
                                "Within",
                                &mut value,
                                f64::MIN_POSITIVE..=1000.0,
                                "ly",
                            );
                            rect = response.rect;
                            id = response.id;
                        });
                    },
                );
                (rect, id)
            };
            let (rect, id) = frame(vec![]);
            let pos = match zone {
                0 => egui::pos2(rect.left() - 25.0, rect.center().y),
                1 => egui::pos2(rect.right() + 15.0, rect.center().y),
                2 => egui::pos2(rect.left() - 5.0, rect.center().y),
                3 => egui::pos2(rect.center().x, rect.top() - 3.0),
                _ => egui::pos2(rect.center().x, rect.bottom() + 3.0),
            };
            frame(vec![egui::Event::PointerMoved(pos)]);
            for pressed in [true, false] {
                frame(vec![egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                }]);
            }
            assert!(ctx.memory(|memory| memory.has_focus(id)), "zone {zone}");
            let state = egui::TextEdit::load_state(&ctx, id).unwrap();
            let selection = state.cursor.char_range().unwrap().sorted_cursors();
            assert_eq!(
                [selection[0].index, selection[1].index],
                [0, 2],
                "zone {zone}"
            );
        }
    }

    #[test]
    fn distances_allow_decimals_but_not_invalid_numbers() {
        let range = f64::MIN_POSITIVE..=1000.0;
        assert_eq!(committed("12.5", &range), Some(12.5));
        for text in ["", "no", "NaN", "inf"] {
            assert_eq!(committed(text, &range), None);
        }
        assert_eq!(committed("0", &range), None);
        assert_eq!(committed("-10", &range), None);
    }

    #[test]
    fn typing_commits_only_when_the_field_loses_focus() {
        use super::{Theme, number_field};
        use bevy_egui::egui;

        let ctx = egui::Context::default();
        let mut value = 50_u32;
        let mut frame = |events, focus: Option<bool>| {
            let mut changed = false;
            let _ = ctx.run(
                egui::RawInput {
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        let other = ui.button("Other");
                        if focus == Some(false) {
                            other.request_focus();
                        }
                        let response =
                            number_field(ui, &Theme::GAME, "Show", &mut value, 1..=1000, "");
                        if focus == Some(true) {
                            response.request_focus();
                        }
                        changed = response.changed();
                    });
                },
            );
            (value, changed)
        };
        frame(vec![], Some(true));
        assert_eq!(frame(vec![egui::Event::Text("7".into())], None).0, 50);
        let (value, changed) = frame(vec![], Some(false));
        assert_ne!(value, 50);
        assert!(changed);
    }
}
