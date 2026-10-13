use super::*;

fn linear() -> RangeSlider {
    RangeSlider {
        min: 1000.0,
        max: 50000.0,
        scale: RangeScale::Linear,
        step: Some(1.0),
    }
}

#[test]
fn log_caption_reports_the_current_span_in_decades() {
    let span = RangeSlider {
        min: 0.01,
        max: 1000.0,
        scale: RangeScale::Logarithmic,
        step: None,
    };
    assert_eq!(span.scale_label(), "Log scale · 5 decades");
    assert_eq!(
        span.extended(NumericRange {
            min: None,
            max: Some(10000.0)
        })
        .scale_label(),
        "Log scale · 6 decades"
    );
    assert_eq!(
        RangeSlider {
            min: 1.0,
            max: 10.0,
            ..span
        }
        .scale_label(),
        "Log scale · 1 decade"
    );
    assert_eq!(
        RangeSlider {
            min: 1.0,
            max: 50.0,
            ..span
        }
        .scale_label(),
        "Log scale · 1.7 decades"
    );
    assert_eq!(linear().scale_label(), "Linear scale");
}

#[test]
fn slider_text_removes_float_noise_without_losing_small_value_precision() {
    assert_eq!(slider_text(19.737000000000002, false), "19.737");
    assert_eq!(slider_text(1.2345000000000002, false), "1.2345");
    assert_eq!(slider_text(0.012345000000000002, false), "0.012345");
    assert_eq!(slider_text(25500.0, false), "25500");
    let span = RangeSlider {
        min: 0.01,
        max: 1000.0,
        scale: RangeScale::Logarithmic,
        step: None,
    };
    for index in 0..=1000 {
        let text = slider_text(span.value(index as f32 / 1000.0, false), false);
        assert!(
            text.split('.')
                .nth(1)
                .is_none_or(|decimals| decimals.len() <= 6),
            "no noisy tail: {text}"
        );
    }
    let mut input = RangeInput::new("".into(), "19.73654321".into());
    let range = input.value(false).unwrap();
    set_slider_endpoint(&mut input, range, true, 19.73654, false);
    assert_eq!(
        input.min, input.max,
        "formatting cannot cross a precisely typed bound"
    );
    assert_eq!(input.max, "19.73654321", "manual precision is retained");
}

#[test]
fn optional_ranges_preserve_validation_and_open_ends() {
    assert_eq!(
        RangeInput::default().value(false),
        Ok(NumericRange::default())
    );
    assert_eq!(
        RangeInput::new("12,000".into(), "".into()).value(false),
        Ok(NumericRange {
            min: Some(12000.0),
            max: None
        })
    );
    assert_eq!(
        RangeInput::new("".into(), "2_000".into()).value(true),
        Ok(NumericRange {
            min: None,
            max: Some(2000.0)
        })
    );
    for (min, max, integer) in [
        ("2", "1", false),
        ("NaN", "", false),
        ("inf", "", false),
        ("1.5", "", true),
        ("bad", "", false),
    ] {
        assert!(
            RangeInput::new(min.into(), max.into())
                .value(integer)
                .is_err()
        );
    }
}

#[test]
fn slider_scales_and_typed_extensions_are_not_validation_bounds() {
    let linear = linear();
    assert_eq!(linear.value(0.5, false), 25500.0);
    let logarithmic = RangeSlider {
        min: 0.01,
        max: 1000.0,
        scale: RangeScale::Logarithmic,
        step: None,
    };
    let value = logarithmic.value(0.5, false);
    assert!((value - 10.0_f64.sqrt()).abs() < 0.0001);
    assert!((logarithmic.fraction(value) - 0.5).abs() < 0.0001);
    let extended = linear.extended(NumericRange {
        min: Some(90000.0),
        max: None,
    });
    assert_eq!(extended.max, 90000.0);
    let extended = logarithmic.extended(NumericRange {
        min: None,
        max: Some(0.001),
    });
    assert_eq!(extended.min, 0.001);
    let zero = logarithmic.extended(NumericRange {
        min: Some(0.0),
        max: None,
    });
    assert_eq!(zero.scale, RangeScale::Linear);
    let huge = linear.extended(NumericRange {
        min: Some(-1e308),
        max: Some(1e308),
    });
    assert_eq!(huge.fraction(0.0), 0.5);
    assert!(huge.value(0.5, false).is_finite());
}

fn press(pos: egui::Pos2, pressed: bool) -> egui::Event {
    egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    }
}

#[test]
fn unified_field_drags_both_handles_without_crossing_and_keeps_other_end_open() {
    let ctx = egui::Context::default();
    let mut input = RangeInput::default();
    let mut rect = Rect::NOTHING;
    let time = std::cell::Cell::new(0.0);
    let frame = |input: &mut RangeInput, rect: &mut Rect, events| {
        time.set(time.get() + 0.05);
        let mut changed = false;
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(500.0, 400.0))),
                events,
                time: Some(time.get()),
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let output = range_field(
                        ui,
                        &Theme::GAME,
                        "test",
                        input,
                        RangeField {
                            unit: "K",
                            integer: false,
                            slider: Some(linear()),
                        },
                    );
                    *rect = output.response.rect;
                    changed = output.response.changed();
                    assert_eq!(output.value, input.value(false));
                });
            },
        );
        changed
    };
    frame(&mut input, &mut rect, vec![]);
    let track = Rect::from_min_max(
        pos2(
            rect.left() + 17.0,
            rect.top() + Theme::GAME.field_height() + SPACING,
        ),
        pos2(
            rect.right() - 17.0,
            rect.top() + Theme::GAME.field_height() + SPACING + 24.0,
        ),
    );
    let point = |t| pos2(track.left() + track.width() * t, track.center().y);
    frame(
        &mut input,
        &mut rect,
        vec![egui::Event::PointerMoved(point(0.2))],
    );
    assert!(frame(&mut input, &mut rect, vec![press(point(0.2), true)]));
    assert_eq!(
        input.value(false).unwrap(),
        NumericRange {
            min: Some(10800.0),
            max: None
        }
    );
    frame(&mut input, &mut rect, vec![press(point(0.2), false)]);
    frame(
        &mut input,
        &mut rect,
        vec![egui::Event::PointerMoved(point(0.8))],
    );
    frame(&mut input, &mut rect, vec![press(point(0.8), true)]);
    assert_eq!(input.value(false).unwrap().max, Some(40200.0));
    frame(
        &mut input,
        &mut rect,
        vec![egui::Event::PointerMoved(point(0.0))],
    );
    let value = input.value(false).unwrap();
    assert_eq!(
        value.min, value.max,
        "upper handle cannot cross lower handle"
    );
    frame(&mut input, &mut rect, vec![press(point(0.0), false)]);
}

#[test]
fn fields_only_and_slider_share_one_background_and_label_focus() {
    for slider in [None, Some(linear())] {
        let ctx = egui::Context::default();
        let mut input = RangeInput::new("1000".into(), "5000".into());
        let frame = |input: &mut RangeInput, events| {
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(500.0, 400.0))),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        range_field(
                            ui,
                            &Theme::GAME,
                            "test",
                            input,
                            RangeField {
                                unit: "K",
                                integer: false,
                                slider,
                            },
                        );
                    });
                },
            )
        };
        let output = frame(&mut input, vec![]);
        assert_eq!(
            output
                .shapes
                .iter()
                .filter(|shape| matches!(&shape.shape,
            egui::epaint::Shape::Rect(rect) if rect.fill == Theme::GAME.input_fill()))
                .count(),
            1
        );
        let label = output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::epaint::Shape::Text(text) if text.galley.text() == "Min:" => {
                    Some(text.pos + text.galley.size() * 0.5)
                }
                _ => None,
            })
            .unwrap();
        frame(
            &mut input,
            vec![egui::Event::PointerMoved(label), press(label, true)],
        );
        frame(&mut input, vec![press(label, false)]);
        frame(&mut input, vec![egui::Event::Text("2000".into())]);
        assert_eq!(input.min, "2000", "label clicks select all for replacement");
        assert_eq!(input.max, "5000");
    }
}
