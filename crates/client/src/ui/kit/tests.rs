//! The debug console's controls, through `ui::terminal`.
use crate::ui::terminal::*;
use bevy_egui::egui::{self, Rect, pos2, vec2};

fn button(pos: egui::Pos2, pressed: bool) -> egui::Event {
    egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    }
}

#[test]
fn control_end_edges_ease_vertically_from_the_centre_in_both_directions() {
    let ctx = egui::Context::default();
    let theme = super::Theme::GAME;
    let frame = |time, active| {
        let mut bounds = Rect::NOTHING;
        let output = ctx.run(
            egui::RawInput {
                time: Some(time),
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let (rect, _) = ui.allocate_exact_size(vec2(120.0, 40.0), egui::Sense::hover());
                    bounds = rect;
                    super::interact::input_edges(
                        ui,
                        &theme,
                        rect,
                        ui.id().with("edge-test"),
                        active,
                    );
                });
            },
        );
        let edges = output
            .shapes
            .iter()
            .filter_map(|shape| match &shape.shape {
                egui::epaint::Shape::Rect(rect) if rect.fill == theme.accent => Some(rect.rect),
                _ => None,
            })
            .collect::<Vec<_>>();
        for (index, edge) in edges.iter().enumerate() {
            assert_eq!(edge.width(), 2.0);
            assert_eq!(edge.center().y, bounds.center().y);
            assert_eq!(
                edge.left(),
                if index == 0 {
                    bounds.left()
                } else {
                    bounds.right() - 2.0
                }
            );
        }
        edges
    };
    assert!(frame(0.0, false).is_empty());
    frame(0.01, true);
    let entering = frame(0.1, true);
    let full = frame(0.3, true);
    assert_eq!(full.len(), 2);
    assert_eq!(full[0].height(), 40.0);
    assert!(entering[0].height() > 0.0 && entering[0].height() < 40.0);
    frame(0.31, false);
    let leaving = frame(0.4, false);
    assert_eq!(leaving.len(), 2);
    assert!(leaving[0].height() > 0.0 && leaving[0].height() < 40.0);
    assert!(frame(0.6, false).is_empty());
}

#[test]
fn progress_bars_share_scrollbar_colours_and_keep_text_inside() {
    for width in [120.0, 600.0] {
        for done in [Some(-0.5), Some(0.5), Some(1.5), None] {
            let ctx = egui::Context::default();
            let theme = super::Theme::GAME;
            let output = ctx.run(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(800.0, 300.0))),
                    time: Some(1.0),
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        ui.set_width(width);
                        super::progress(
                            ui,
                            &theme,
                            done,
                            "Searching within 10,000 ly · 12 results so far",
                        );
                    });
                },
            );
            let background = output
                .shapes
                .iter()
                .find_map(|shape| match &shape.shape {
                    egui::epaint::Shape::Rect(rect)
                        if rect.fill
                            == theme
                                .accent
                                .gamma_multiply(super::Theme::SUBTLE_ACCENT_OPACITY) =>
                    {
                        assert_eq!(rect.corner_radius, egui::CornerRadius::ZERO);
                        Some(rect.rect)
                    }
                    _ => None,
                })
                .expect("progress remainder must use the idle scrollbar accent");
            assert_eq!(background.height(), theme.row);
            let fill = output
                .shapes
                .iter()
                .find_map(|shape| match &shape.shape {
                    egui::epaint::Shape::Rect(rect) if rect.fill == theme.accent => Some(rect.rect),
                    _ => None,
                })
                .expect("progress fill must use the full-strength accent");
            assert!(background.contains_rect(fill));
            assert_eq!(fill.height(), background.height());
            if let Some(done) = done {
                assert!((fill.width() - background.width() * done.clamp(0.0, 1.0)).abs() < 0.01);
            }
            let texts = output
                .shapes
                .iter()
                .filter_map(|shape| match &shape.shape {
                    egui::epaint::Shape::Text(text) => Some((text, shape.clip_rect)),
                    _ => None,
                })
                .collect::<Vec<_>>();
            assert!(!texts.is_empty());
            for (text, clip) in texts {
                assert!(
                    background.contains_rect(Rect::from_min_size(text.pos, text.galley.size()))
                );
                assert!(background.contains_rect(clip));
            }
        }
    }
}

#[test]
fn filter_removal_and_reset_use_red_feedback_without_recolouring_normal_actions() {
    let ctx = egui::Context::default();
    let theme = super::Theme::GAME;
    let chips = std::cell::RefCell::new(super::Pills::default());
    let frame = |time, pointer| {
        ctx.run(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(700.0, 400.0))),
                events: vec![egui::Event::PointerMoved(pointer)],
                time: Some(time),
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        chips
                            .borrow_mut()
                            .show(ui, &theme, &[(0, "Example filter".into())]);
                        super::destructive_button(ui, &theme, "Reset");
                    });
                });
            },
        )
    };
    let outside = pos2(0.0, 0.0);
    frame(0.0, outside);
    frame(0.3, outside);
    let idle = frame(0.31, outside);
    let position = |label| {
        idle.shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::epaint::Shape::Text(text) if text.galley.text() == label => {
                    Some(text.pos + text.galley.size() * 0.5)
                }
                _ => None,
            })
            .unwrap()
    };
    let colour_count = |output: &egui::FullOutput, colour| {
        output
            .shapes
            .iter()
            .filter(|shape| {
                matches!(&shape.shape,
        egui::epaint::Shape::Rect(rect) if rect.fill == colour && rect.rect.width() == 2.0)
            })
            .count()
    };
    frame(0.32, position("×"));
    let close_hover = frame(0.6, position("×"));
    assert_eq!(colour_count(&close_hover, super::Theme::DESTRUCTIVE), 0);
    assert_eq!(colour_count(&close_hover, theme.accent), 2);
    assert!(close_hover.shapes.iter().any(|shape| matches!(&shape.shape,
        egui::epaint::Shape::Text(text) if text.galley.text() == "×"
            && text.fallback_color == super::Theme::DESTRUCTIVE)));
    let chip = close_hover
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::epaint::Shape::Rect(rect)
                if rect.fill == theme.input_fill() && rect.rect.contains(position("×")) =>
            {
                Some(rect.rect)
            }
            _ => None,
        })
        .unwrap();
    let edges = |output: &egui::FullOutput| {
        output
            .shapes
            .iter()
            .filter_map(|shape| match &shape.shape {
                egui::epaint::Shape::Rect(rect)
                    if rect.fill == theme.accent && rect.rect.width() == 2.0 =>
                {
                    Some(rect.rect)
                }
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    let lit = edges(&close_hover);
    assert_eq!(
        lit[0],
        Rect::from_min_size(chip.min, vec2(2.0, chip.height()))
    );
    assert_eq!(
        lit[1],
        Rect::from_min_size(
            pos2(chip.right() - 2.0, chip.top()),
            vec2(2.0, chip.height())
        )
    );
    // Moving between the hit regions keeps one shared animation fully lit.
    assert_eq!(edges(&frame(0.61, position("Example filter"))), lit);
    frame(0.62, outside);
    frame(0.9, outside);
    frame(0.91, position("Reset"));
    assert_eq!(
        colour_count(&frame(1.2, position("Reset")), super::Theme::DESTRUCTIVE),
        2
    );
    frame(1.21, outside);
    frame(1.5, outside);
    frame(1.51, position("Example filter"));
    assert_eq!(
        colour_count(&frame(1.8, position("Example filter")), theme.accent),
        2
    );
}

#[test]
fn debug_window_control_rows_are_contiguous() {
    let ctx = egui::Context::default();
    let frame = || {
        ctx.run(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(1000.0, 800.0))),
                ..Default::default()
            },
            |ctx| {
                window(ctx, "contiguous-console", &mut true, |ui| {
                    assert_eq!(ui.spacing().item_spacing.x, super::SPACING);
                    check(ui, false, "First control", true, "");
                    check(ui, false, "Second control", true, "");
                })
            },
        )
    };
    frame();
    let output = frame();
    let top = |label| {
        output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::epaint::Shape::Text(text) if text.galley.text().contains(label) => {
                    Some(text.pos.y)
                }
                _ => None,
            })
            .unwrap()
    };
    assert!((top("Second control") - top("First control") - THEME.row).abs() < 0.1);
}

#[test]
fn shared_fact_rows_are_contiguous_without_changing_surrounding_spacing() {
    for details in [false, true] {
        let ctx = egui::Context::default();
        super::plain_style(&ctx);
        let theme = super::Theme::GAME;
        let output = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let spacing = ui.spacing().item_spacing.y;
                if details {
                    super::facts_with_details(
                        ui,
                        &theme,
                        "test-facts",
                        &[("First", "1".into(), None), ("Second", "2".into(), None)],
                    );
                } else {
                    super::facts(
                        ui,
                        &theme,
                        "test-facts",
                        &[("First", "1".into()), ("Second", "2".into())],
                    );
                }
                assert_eq!(ui.spacing().item_spacing.y, spacing);
            });
        });
        let top = |label| {
            output
                .shapes
                .iter()
                .find_map(|shape| match &shape.shape {
                    egui::epaint::Shape::Text(text) if text.galley.text() == label => {
                        Some(text.pos.y)
                    }
                    _ => None,
                })
                .unwrap()
        };
        assert!((top("Second") - top("First") - theme.row).abs() < 0.1);
    }
}

#[test]
fn selected_choices_keep_their_end_edges_without_hover_or_focus() {
    let ctx = egui::Context::default();
    let theme = super::Theme::GAME;
    let frame = |time, chosen| {
        ctx.run(
            egui::RawInput {
                time: Some(time),
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        super::choice(ui, &theme, "Class M", chosen);
                        super::choice(ui, &theme, "Class K", chosen);
                        super::primary_button(ui, &theme, "Add filter", true);
                    });
                });
            },
        )
    };
    let edges = |output: egui::FullOutput| {
        output
            .shapes
            .into_iter()
            .filter_map(|shape| match shape.shape {
                egui::epaint::Shape::Rect(rect)
                    if rect.fill == theme.accent && rect.rect.width() == 2.0 =>
                {
                    Some(rect.rect.height())
                }
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    assert!(edges(frame(0.0, false)).is_empty());
    frame(0.01, true);
    let appearing = edges(frame(0.1, true));
    assert_eq!(appearing.len(), 4);
    let selected = edges(frame(0.3, true));
    assert_eq!(selected.len(), 4);
    assert!(selected[0] > appearing[0]);
    frame(0.31, false);
    let leaving = edges(frame(0.4, false));
    assert_eq!(leaving.len(), 4);
    assert!(leaving[0] < selected[0]);
    assert!(edges(frame(0.6, false)).is_empty());
}

#[test]
fn layout_motion_uses_shared_easing_and_retargets_without_snapping() {
    let ctx = egui::Context::default();
    let initial = Rect::from_min_size(pos2(800.0, 0.0), vec2(300.0, 500.0));
    let side = Rect::from_min_size(pos2(480.0, 0.0), vec2(300.0, 500.0));
    let bottom = Rect::from_min_size(pos2(0.0, 260.0), vec2(600.0, 240.0));
    let sample = |time, target| {
        let mut rect = Rect::NOTHING;
        let _ = ctx.run(
            egui::RawInput {
                time: Some(time),
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    rect = super::animated_rect(ui, ui.id().with("motion-test"), initial, target);
                });
            },
        );
        rect
    };
    let seconds = f64::from(super::interact::ANIMATION_SECONDS);
    assert_eq!(sample(0.0, side), initial);
    let halfway = sample(seconds * 0.5, side);
    let eased = super::interact::ease_out(0.5);
    assert!((halfway.left() - initial.min.lerp(side.min, eased).x).abs() < 0.01);
    assert_eq!(sample(seconds * 0.5, bottom), halfway);
    let moving = sample(seconds, bottom);
    assert!(moving.left() < halfway.left() && moving.top() > halfway.top());
    assert!(moving.width() > halfway.width());
    assert_eq!(sample(seconds * 1.6, bottom), bottom);
    assert_eq!(sample(seconds * 1.6, initial), bottom);
    assert_eq!(sample(seconds * 2.7, initial), initial);
}

#[test]
fn selected_list_rows_highlight_both_edges_without_hover() {
    let ctx = egui::Context::default();
    let theme = super::Theme::GAME;
    let mut row = Rect::NOTHING;
    let output = ctx.run(
        egui::RawInput {
            screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(600.0, 400.0))),
            ..Default::default()
        },
        |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                row = super::list_row(ui, &theme, 46.0, true).0;
            });
        },
    );
    assert!(output.shapes.iter().any(|shape| matches!(
        &shape.shape,
        egui::epaint::Shape::Rect(rect)
            if rect.rect == row && rect.fill == theme.accent.gamma_multiply(0.12)
    )));
    let edges = output
        .shapes
        .into_iter()
        .filter_map(|shape| match shape.shape {
            egui::epaint::Shape::Rect(rect) if rect.fill == theme.accent => Some(rect.rect),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        edges,
        vec![
            Rect::from_min_size(row.min, vec2(2.0, row.height())),
            Rect::from_min_size(pos2(row.right() - 2.0, row.top()), vec2(2.0, row.height())),
        ]
    );
}

#[test]
fn list_rows_ease_between_selections_and_keep_hover_on_deselection() {
    let ctx = egui::Context::default();
    let theme = super::Theme::GAME;
    let sample = |time, selected: Option<usize>, pointer| {
        let mut amounts = [0.0; 2];
        let mut rects = [Rect::NOTHING; 2];
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(600.0, 400.0))),
                time: Some(time),
                events: vec![egui::Event::PointerMoved(pointer)],
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    for index in 0..2 {
                        ui.push_id(index, |ui| {
                            let (rect, _, amount) =
                                super::list_row(ui, &theme, 46.0, selected == Some(index));
                            amounts[index] = amount;
                            rects[index] = rect;
                        });
                    }
                });
            },
        );
        (amounts, rects)
    };
    let outside = pos2(590.0, 390.0);
    assert_eq!(sample(0.0, None, outside).0, [0.0, 0.0]);
    sample(0.01, Some(0), outside);
    let appearing = sample(0.1, Some(0), outside).0;
    assert!(appearing[0] > 0.0 && appearing[0] < 1.0);
    assert_eq!(sample(0.3, Some(0), outside).0, [1.0, 0.0]);
    sample(0.31, Some(1), outside);
    let switching = sample(0.4, Some(1), outside).0;
    assert!(
        switching
            .iter()
            .all(|amount| *amount > 0.0 && *amount < 1.0)
    );
    let (settled, rects) = sample(0.6, Some(1), outside);
    assert_eq!(settled, [0.0, 1.0]);
    let pointer = rects[1].center();
    sample(0.61, Some(1), pointer);
    assert_eq!(
        sample(0.7, None, pointer).0[1],
        1.0,
        "hover keeps the deselected row lit"
    );
    sample(0.71, None, outside);
    let leaving = sample(0.8, None, outside).0[1];
    assert!(leaving > 0.0 && leaving < 1.0);
    assert_eq!(sample(1.0, None, outside).0, [0.0, 0.0]);
}

#[test]
fn split_checkbox_toggles_on_left_and_expands_only_on_right() {
    for enabled in [true, false] {
        let ctx = egui::Context::default();
        let frame = |time, events| {
            let mut toggled = false;
            let output = ctx.run(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(600.0, 400.0))),
                    time: Some(time),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        toggled = check(ui, false, "Test switch", enabled, "Explanation");
                    });
                },
            );
            let explained = output.shapes.iter().any(|shape| matches!(
                &shape.shape, egui::epaint::Shape::Text(text) if text.galley.job.text == "Explanation"
            ));
            (toggled, explained)
        };
        let pos = pos2(100.0, 20.0);
        frame(0.0, vec![]);
        frame(0.1, vec![egui::Event::PointerMoved(pos), button(pos, true)]);
        assert_eq!(frame(0.2, vec![button(pos, false)]), (enabled, false));
        frame(1.0, vec![button(pos, true)]);
        assert_eq!(
            frame(1.6, vec![]),
            (false, false),
            "holding does not expand"
        );
        frame(1.7, vec![button(pos, false)]);
        // The full square, including its corners, is clickable, not just the glyph.
        let expand = pos2(569.0, 9.0);
        frame(
            2.0,
            vec![egui::Event::PointerMoved(expand), button(expand, true)],
        );
        assert_eq!(frame(2.1, vec![button(expand, false)]), (false, true));
        frame(2.2, vec![egui::Event::PointerMoved(pos), button(pos, true)]);
        assert_eq!(frame(2.3, vec![button(pos, false)]), (enabled, true));
        let collapse = pos2(591.0, 31.0);
        frame(
            3.0,
            vec![egui::Event::PointerMoved(collapse), button(collapse, true)],
        );
        assert_eq!(frame(3.1, vec![button(collapse, false)]), (false, false));
    }
}

#[test]
fn checkbox_halves_highlight_independently_with_square_divider_edges() {
    let ctx = egui::Context::default();
    let frame = |time, pos| {
        ctx.run(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(600.0, 400.0))),
                time: Some(time),
                events: vec![egui::Event::PointerMoved(pos)],
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    check(ui, false, "Test switch", true, "Explanation");
                });
            },
        )
    };
    let highlights = |output: egui::FullOutput| {
        output
            .shapes
            .into_iter()
            .filter_map(|shape| match shape.shape {
                egui::epaint::Shape::Rect(rect) if rect.fill == CYAN.gamma_multiply(0.12) => {
                    if rect.rect.left() == 568.0 {
                        assert_eq!(rect.corner_radius.nw, 0);
                        assert_eq!(rect.corner_radius.sw, 0);
                    } else {
                        assert_eq!(rect.corner_radius.ne, 0);
                        assert_eq!(rect.corner_radius.se, 0);
                    }
                    Some(rect.rect)
                }
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    let left = pos2(100.0, 20.0);
    frame(0.0, left);
    frame(1.0, left);
    let highlighted = highlights(frame(2.0, left));
    assert_eq!(highlighted.len(), 1);
    assert_eq!(highlighted[0].right(), 568.0);
    let right = pos2(580.0, 20.0);
    frame(3.0, right);
    let highlighted = highlights(frame(4.0, right));
    assert_eq!(highlighted.len(), 1);
    assert_eq!(
        highlighted[0],
        Rect::from_min_size(pos2(568.0, 8.0), vec2(24.0, 24.0))
    );
}

#[test]
fn all_terminal_rows_keep_the_same_bright_edge_during_long_presses() {
    for kind in 0..8 {
        let ctx = egui::Context::default();
        let frame = |time, events| {
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(600.0, 400.0))),
                    time: Some(time),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        super::ScrollArea::vertical()
                            .max_height(160.0)
                            .show(ui, |ui| {
                                match kind {
                                    0 => facts_with_details(
                                        ui,
                                        "held-facts",
                                        &[("Fact", "Value".into(), Some("Explanation"))],
                                    ),
                                    1 => {
                                        facts(ui, "held-controls", &[("Control", "Action".into())])
                                    }
                                    2 => {
                                        section(ui, "Section");
                                    }
                                    3 => heading(ui, "Heading"),
                                    4 | 5 => {
                                        check(ui, false, "Setting", true, "Explanation");
                                    }
                                    6 => {
                                        crate::ui::terminal::button(ui, "Button");
                                    }
                                    _ => {
                                        tab(ui, "Tab", false);
                                    }
                                }
                                ui.allocate_space(vec2(100.0, 1000.0));
                            });
                    });
                },
            )
        };
        frame(0.0, vec![]);
        let output = frame(0.1, vec![]);
        let needle = [
            "Fact +",
            "Control",
            "# Section -",
            "# Heading",
            "Setting",
            "+",
            "[ Button ]",
            " Tab ",
        ][kind];
        let pos = output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::epaint::Shape::Text(text) if text.galley.job.text == needle => {
                    Some(text.pos + text.galley.size() * 0.5)
                }
                _ => None,
            })
            .unwrap_or_else(|| panic!("missing {needle}"));
        frame(0.2, vec![egui::Event::PointerMoved(pos)]);
        frame(0.4, vec![button(pos, true)]);
        for time in [0.6, 1.6, 3.0] {
            let output = frame(time, vec![]);
            let edges: Vec<_> = output
                .shapes
                .iter()
                .filter_map(|shape| match &shape.shape {
                    egui::epaint::Shape::Rect(rect)
                        if rect.fill == CYAN && rect.rect.width() == 2.0 =>
                    {
                        Some(rect.rect)
                    }
                    _ => None,
                })
                .collect();
            let split = matches!(kind, 4 | 5);
            assert_eq!(
                edges.len(),
                if split { 1 } else { 2 },
                "{needle} has incorrect bright edges while held at {time}s"
            );
            let bar = output
                .shapes
                .iter()
                .find_map(|shape| match &shape.shape {
                    egui::epaint::Shape::Rect(rect) if rect.fill == CYAN.gamma_multiply(0.12) => {
                        Some(rect.rect)
                    }
                    _ => None,
                })
                .expect("shared hover bar");
            if kind != 5 {
                assert_eq!(edges[0].left(), bar.left());
            }
            if kind != 4 {
                assert_eq!(edges.last().unwrap().right(), bar.right());
            }
        }
    }
}

#[test]
fn long_presses_click_when_released_over_the_control() {
    let ctx = egui::Context::default();
    let frame = |time, events| {
        let mut clicked = false;
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(600.0, 400.0))),
                time: Some(time),
                events,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    clicked = tab(ui, "Tab", false);
                });
            },
        );
        clicked
    };
    let pos = pos2(30.0, 18.0);
    frame(0.0, vec![egui::Event::PointerMoved(pos)]);
    // A second between frames, as under software rendering.
    frame(0.1, vec![button(pos, true)]);
    assert!(frame(1.9, vec![button(pos, false)]), "released over it");
    frame(2.0, vec![button(pos, true)]);
    frame(3.0, vec![]);
    let away = pos2(300.0, 300.0);
    assert!(
        !frame(
            3.1,
            vec![egui::Event::PointerMoved(away), button(away, false)]
        ),
        "released elsewhere"
    );
    // A press and release that arrive together, behind one slow frame.
    frame(4.0, vec![egui::Event::PointerMoved(pos)]);
    assert!(
        frame(5.8, vec![button(pos, true), button(pos, false)]),
        "pressed and released in one frame"
    );
}

#[test]
fn reserved_button_sizes_match_rendered_buttons_in_both_themes() {
    for theme in [super::Theme::GAME, super::Theme::TERMINAL] {
        let ctx = egui::Context::default();
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let expected = super::button_size(ui, &theme, "Reset filters");
                let response = super::button(ui, &theme, "Reset filters");
                assert_eq!(response.rect.size(), expected);
            });
        });
    }
}

#[test]
fn expandable_fact_state_is_scoped_to_its_owning_panel() {
    let ctx = egui::Context::default();
    super::plain_style(&ctx);
    let frame = |events| {
        ctx.run(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(600.0, 400.0))),
                events,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    for (panel, detail) in [
                        ("first", "First panel detail"),
                        ("second", "Second panel detail"),
                    ] {
                        ui.push_id(panel, |ui| {
                            super::facts_with_details(
                                ui,
                                &super::Theme::GAME,
                                "facts",
                                &[("Shared label", "Value".into(), Some(detail))],
                            )
                        });
                    }
                });
            },
        )
    };
    frame(vec![]);
    let output = frame(vec![]);
    let pos = output
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::epaint::Shape::Text(text) if text.galley.text() == "Shared label +" => {
                Some(text.pos + text.galley.size() * 0.5)
            }
            _ => None,
        })
        .unwrap();
    frame(vec![egui::Event::PointerMoved(pos)]);
    frame(vec![button(pos, true)]);
    let output = frame(vec![button(pos, false)]);
    let contains = |label| {
        output.shapes.iter().any(|shape| {
            matches!(&shape.shape,
        egui::epaint::Shape::Text(text) if text.galley.text() == label)
        })
    };
    assert!(contains("First panel detail"));
    assert!(
        !contains("Second panel detail"),
        "panels must not share expansion state"
    );
}

#[test]
fn window_close_glyph_aligns_with_title_ink_in_both_themes() {
    for theme in [super::Theme::GAME, super::Theme::TERMINAL] {
        for eyebrow in ["", "EXPLORER"] {
            let ctx = egui::Context::default();
            super::plain_style(&ctx);
            let mut open = true;
            let mut frame = || {
                ctx.run(
                    egui::RawInput {
                        screen_rect: Some(Rect::from_min_size(
                            egui::Pos2::ZERO,
                            vec2(1200.0, 900.0),
                        )),
                        ..Default::default()
                    },
                    |ctx| {
                        let window =
                            super::Window::new("Alignment test", "Alignment test").eyebrow(eyebrow);
                        window.show(ctx, &theme, &mut open, |ui| {
                            ui.label("Content");
                        });
                    },
                )
            };
            frame();
            let output = frame();
            let centre = |label| {
                output
                    .shapes
                    .iter()
                    .find_map(|shape| match &shape.shape {
                        egui::epaint::Shape::Text(text) if text.galley.text() == label => {
                            Some(text.pos.y + text.galley.mesh_bounds.center().y)
                        }
                        _ => None,
                    })
                    .unwrap()
            };
            assert!(
                (centre("×") - centre("Alignment test")).abs() < 1.0,
                "close glyph must align with title, text_mode={}, eyebrow={eyebrow}",
                theme.text_mode
            );
        }
    }
}

#[test]
fn window_moves_only_from_header_and_shows_move_cursor() {
    let ctx = egui::Context::default();
    let frame = |events| {
        ctx.run(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(1000.0, 800.0))),
                events,
                ..Default::default()
            },
            |ctx| {
                window(ctx, "test", &mut true, |ui| {
                    check(ui, false, "Test switch", true, "Explanation");
                    ui.allocate_space(vec2(300.0, 200.0));
                });
            },
        )
    };
    frame(vec![]);
    frame(vec![]);
    let position = || ctx.memory(|memory| memory.area_rect(super::window_id("test")).unwrap().min);
    let original = position();
    let content = original + vec2(100.0, 90.0);
    frame(vec![
        egui::Event::PointerMoved(content),
        button(content, true),
    ]);
    frame(vec![egui::Event::PointerMoved(content + vec2(-50.0, 20.0))]);
    frame(vec![button(content + vec2(-50.0, 20.0), false)]);
    assert_eq!(position(), original, "content cannot drag the window");
    for offset in [
        vec2(100.0, 8.0),
        vec2(100.0, 28.0),
        vec2(100.0, 42.0),
        vec2(8.0, 28.0),
    ] {
        let start = position();
        let header = start + offset;
        let output = frame(vec![egui::Event::PointerMoved(header)]);
        assert_eq!(
            output.platform_output.cursor_icon,
            egui::CursorIcon::AllScroll,
            "header offset {offset:?}"
        );
        frame(vec![button(header, true)]);
        frame(vec![egui::Event::PointerMoved(header + vec2(-50.0, 20.0))]);
        frame(vec![button(header + vec2(-50.0, 20.0), false)]);
        assert_eq!(
            position(),
            start + vec2(-50.0, 20.0),
            "header padding is draggable"
        );
    }
}

#[test]
fn windows_resize_from_top_and_left_without_moving_the_opposite_edge() {
    for left in [true, false] {
        let ctx = egui::Context::default();
        let time = std::cell::Cell::new(0.0);
        let frame = |events| {
            time.set(time.get() + 0.1);
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(1000.0, 800.0))),
                    events,
                    time: Some(time.get()),
                    ..Default::default()
                },
                |ctx| {
                    window(ctx, "resize-test", &mut true, |ui| {
                        ui.label("Content");
                        ui.allocate_space(ui.available_size());
                    });
                },
            )
        };
        let bounds =
            || ctx.memory(|memory| memory.area_rect(super::window_id("resize-test")).unwrap());
        frame(vec![]);
        frame(vec![]);
        let before = bounds();
        let (start, delta) = if left {
            (
                pos2(before.left() + 1.0, before.center().y),
                vec2(-30.0, 0.0),
            )
        } else {
            (
                pos2(before.center().x, before.top() + 1.0),
                vec2(0.0, -10.0),
            )
        };
        let hover = frame(vec![egui::Event::PointerMoved(start)]);
        assert_eq!(
            hover.platform_output.cursor_icon,
            if left {
                egui::CursorIcon::ResizeHorizontal
            } else {
                egui::CursorIcon::ResizeVertical
            }
        );
        frame(vec![button(start, true)]);
        frame(vec![egui::Event::PointerMoved(start + delta)]);
        frame(vec![]);
        frame(vec![button(start + delta, false)]);
        let after = bounds();
        if left {
            assert!(
                after.left() < before.left(),
                "before {before:?}, after {after:?}"
            );
            assert!((after.right() - before.right()).abs() < 1.0);
        } else {
            assert!(after.top() < before.top());
            assert!((after.bottom() - before.bottom()).abs() < 1.0);
        }
    }
}

#[test]
fn window_edges_stop_at_minimum_size_and_screen_without_moving_opposite_edge() {
    for side in 0..4 {
        for expanding in [false, true] {
            let ctx = egui::Context::default();
            let screen = Rect::from_min_size(egui::Pos2::ZERO, vec2(1000.0, 800.0));
            let title = "bounded-resize-test";
            ctx.data_mut(|data| {
                data.insert_temp(egui::Id::new(("window-center", title)), screen.center())
            });
            let time = std::cell::Cell::new(0.0);
            let frame = |events| {
                time.set(time.get() + 0.05);
                let _ = ctx.run(
                    egui::RawInput {
                        screen_rect: Some(screen),
                        time: Some(time.get()),
                        events,
                        ..Default::default()
                    },
                    |ctx| {
                        super::Window::new(title, title).show(
                            ctx,
                            &super::Theme::GAME,
                            &mut true,
                            |ui| {
                                ui.label("Content");
                                ui.allocate_space(ui.available_size());
                            },
                        );
                    },
                );
            };
            let bounds = || ctx.memory(|memory| memory.area_rect(super::window_id(title)).unwrap());
            frame(vec![]);
            frame(vec![]);
            let before = bounds();
            let start = match side {
                0 => pos2(before.left() + 0.5, before.center().y),
                1 => pos2(before.right() - 0.5, before.center().y),
                2 => pos2(before.center().x, before.top() + 0.5),
                _ => pos2(before.center().x, before.bottom() - 0.5),
            };
            let sign = if (side == 0 || side == 2) == expanding {
                -1.0
            } else {
                1.0
            };
            let delta = if side < 2 {
                vec2(sign * 1500.0, 0.0)
            } else {
                vec2(0.0, sign * 1500.0)
            };
            frame(vec![egui::Event::PointerMoved(start)]);
            frame(vec![button(start, true)]);
            frame(vec![egui::Event::PointerMoved(start + delta)]);
            frame(vec![]);
            let saturated = bounds();
            frame(vec![egui::Event::PointerMoved(start + delta * 1.2)]);
            frame(vec![]);
            let after = bounds();
            let opposite = |rect: Rect| match side {
                0 => rect.right(),
                1 => rect.left(),
                2 => rect.bottom(),
                _ => rect.top(),
            };
            assert!(
                (opposite(after) - opposite(before)).abs() < 1.0,
                "side {side}, expanding {expanding}: opposite edge moved: {before:?} -> {after:?}"
            );
            assert!(
                (after.min - saturated.min).length() < 1.0
                    && (after.max - saturated.max).length() < 1.0,
                "side {side}, expanding {expanding}: kept changing past its limit"
            );
            assert!(screen.expand(1.0).contains_rect(after));
            let axis_size = if side < 2 {
                after.width()
            } else {
                after.height()
            };
            let theme = super::Theme::GAME;
            let minimum = if side < 2 { 320.0 } else { 200.0 }
                + 2.0 * (f32::from(theme.padding) + theme.border_width);
            if !expanding {
                assert!((axis_size - minimum).abs() < 1.0);
            } else {
                let edge = |rect: Rect| match side {
                    0 => rect.left(),
                    1 => rect.right(),
                    2 => rect.top(),
                    _ => rect.bottom(),
                };
                assert!(
                    (edge(after) - edge(screen)).abs() < 1.0,
                    "dragged edge should stop at the viewport"
                );
            }
            frame(vec![button(start + delta * 1.2, false)]);
        }
    }
}

#[test]
fn corner_resize_stops_both_axes_without_moving_the_opposite_corner() {
    for (name, edges) in [
        ("left_top", [true, false, true, false]),
        ("right_top", [false, true, true, false]),
        ("left_bottom", [true, false, false, true]),
        ("right_bottom", [false, true, false, true]),
    ] {
        let start = Rect::from_min_max(pos2(200.0, 150.0), pos2(700.0, 650.0));
        let screen = Rect::from_min_max(pos2(0.0, 0.0), pos2(1000.0, 800.0));
        for expanding in [false, true] {
            let x = if edges[0] == expanding {
                -1500.0
            } else {
                1500.0
            };
            let y = if edges[2] == expanding {
                -1500.0
            } else {
                1500.0
            };
            let rect =
                super::window::bounded_resize(start, edges, vec2(x, y), vec2(320.0, 200.0), screen);
            assert_eq!(
                if edges[0] { rect.right() } else { rect.left() },
                if edges[0] {
                    start.right()
                } else {
                    start.left()
                },
                "{name}"
            );
            assert_eq!(
                if edges[2] { rect.bottom() } else { rect.top() },
                if edges[2] {
                    start.bottom()
                } else {
                    start.top()
                },
                "{name}"
            );
            assert!(screen.contains_rect(rect));
            if !expanding {
                assert_eq!(rect.size(), vec2(320.0, 200.0));
            }
        }
    }
}

#[test]
fn terminal_theme_matches_the_browser_border_and_keeps_its_background() {
    assert_eq!(THEME.border, super::Theme::GAME.border);
    assert_eq!(THEME.border_width, super::Theme::GAME.border_width);
    assert_eq!(THEME.fill, egui::Color32::from_black_alpha(205));
}

#[test]
fn resize_highlight_grows_from_pointer_in_blue_without_changing_thickness() {
    for vertical in [true, false] {
        let ctx = egui::Context::default();
        super::plain_style(&ctx);
        let theme = super::Theme::GAME;
        let original = ctx.style().visuals.widgets.hovered.bg_stroke;
        let frame = |time, events| {
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(1000.0, 800.0))),
                    time: Some(time),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    super::Window::new("edge-animation-test", "edge-animation-test").show(
                        ctx,
                        &theme,
                        &mut true,
                        |ui| {
                            ui.label("Content");
                            ui.allocate_space(ui.available_size());
                        },
                    );
                },
            )
        };
        frame(0.0, vec![]);
        frame(0.01, vec![]);
        let bounds = ctx.memory(|memory| {
            memory
                .area_rect(super::window_id("edge-animation-test"))
                .unwrap()
        });
        let border = bounds.shrink(theme.border_width * 0.5);
        let pointer = if vertical {
            pos2(border.right(), border.top() + border.height() * 0.3)
        } else {
            pos2(border.left() + border.width() * 0.3, border.bottom())
        };
        frame(0.02, vec![egui::Event::PointerMoved(pointer)]);
        let lines = |output: egui::FullOutput| {
            output
                .shapes
                .into_iter()
                .filter_map(|shape| match shape.shape {
                    egui::epaint::Shape::LineSegment { points, stroke }
                        if stroke.color == theme.accent =>
                    {
                        assert_eq!(stroke.width, original.width);
                        Some(points)
                    }
                    _ => None,
                })
                .collect::<Vec<_>>()
        };
        let midway = lines(frame(0.11, vec![]));
        assert_eq!(midway.len(), 1);
        let coordinate = |point: egui::Pos2| if vertical { point.y } else { point.x };
        let start = if vertical {
            border.top()
        } else {
            border.left()
        };
        let end = if vertical {
            border.bottom()
        } else {
            border.right()
        };
        assert!(coordinate(midway[0][0]) > start && coordinate(midway[0][0]) < coordinate(pointer));
        assert!(coordinate(midway[0][1]) < end && coordinate(midway[0][1]) > coordinate(pointer));
        // Unequal growth on either side proves the origin is the pointer,
        // not the centre used by button and input highlights.
        let before = coordinate(pointer) - coordinate(midway[0][0]);
        let after = coordinate(midway[0][1]) - coordinate(pointer);
        assert!((before / after - 3.0 / 7.0).abs() < 0.02);
        let full = lines(frame(0.3, vec![]));
        assert!((coordinate(full[0][0]) - start).abs() < 0.1);
        assert!((coordinate(full[0][1]) - end).abs() < 0.1);
        frame(0.31, vec![egui::Event::PointerMoved(pos2(0.0, 0.0))]);
        let fading = lines(frame(0.4, vec![]));
        assert_eq!(fading.len(), 1);
        assert!(coordinate(fading[0][0]) > start && coordinate(fading[0][1]) < end);
        assert!(lines(frame(0.6, vec![])).is_empty());
        assert_eq!(ctx.style().visuals.widgets.hovered.bg_stroke, original);
    }
}

#[test]
fn shared_scrollbars_reserve_their_expanded_width() {
    let ctx = egui::Context::default();
    super::plain_style(&ctx);
    let style = ctx.style();
    let scroll = &style.spacing.scroll;
    assert!(scroll.floating);
    assert_eq!(scroll.floating_width, scroll.bar_width);
    assert_eq!(
        scroll.allocated_width(),
        scroll.bar_width + scroll.bar_inner_margin + scroll.bar_outer_margin
    );
}

#[test]
fn scrollbars_are_square_full_width_accent_handles_without_tracks() {
    for theme in [super::Theme::GAME, super::Theme::TERMINAL] {
        let ctx = egui::Context::default();
        super::plain_style(&ctx);
        // Ignore visibility fade-in when comparing idle/hover handle widths.
        ctx.style_mut(|style| style.animation_time = 0.0);
        let mut bar_width = 0.0;
        let mut frame = |events| {
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(600.0, 400.0))),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        theme.style_scrollbars(ui.style_mut());
                        let scroll = &ui.spacing().scroll;
                        bar_width = scroll.bar_width;
                        assert_eq!(scroll.floating_width, bar_width);
                        assert_eq!(
                            [
                                scroll.dormant_background_opacity,
                                scroll.active_background_opacity,
                                scroll.interact_background_opacity
                            ],
                            [0.0; 3]
                        );
                        egui::ScrollArea::vertical()
                            .max_height(140.0)
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                ui.allocate_space(vec2(ui.available_width(), 1000.0));
                            });
                    });
                },
            )
        };
        let handles = |output: egui::FullOutput, colour| {
            output
                .shapes
                .into_iter()
                .filter_map(|shape| match shape.shape {
                    egui::epaint::Shape::Rect(rect) if rect.fill == colour => {
                        assert_eq!(rect.corner_radius, egui::CornerRadius::ZERO);
                        Some(rect.rect)
                    }
                    _ => None,
                })
                .collect::<Vec<_>>()
        };
        frame(vec![]);
        let idle = handles(frame(vec![]), theme.accent.gamma_multiply(0.12));
        assert_eq!(idle.len(), 1);
        let content_hover = handles(
            frame(vec![egui::Event::PointerMoved(pos2(50.0, 50.0))]),
            theme.accent.gamma_multiply(0.12),
        );
        assert_eq!(
            content_hover.len(),
            1,
            "hovering content must not brighten the scrollbar"
        );
        let hovered = handles(
            frame(vec![egui::Event::PointerMoved(idle[0].center())]),
            theme.accent,
        );
        assert_eq!(hovered.len(), 1);
        assert_eq!(idle[0].width(), hovered[0].width());
        // Release the frame closure's mutable borrow before reading its output.
        let actual_width = idle[0].width();
        assert!((actual_width - bar_width).abs() < 0.1);
    }
}

#[test]
fn inputs_share_square_backgrounds_and_focus_end_edges() {
    for numeric in [false, true] {
        let ctx = egui::Context::default();
        let theme = super::Theme::GAME;
        let mut text = String::new();
        let mut number = 50_u32;
        let mut frame = |focus| {
            ctx.run(egui::RawInput::default(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let response = if numeric {
                        super::number_field(ui, &theme, "Show", &mut number, 1..=1000, "")
                    } else {
                        super::field(ui, &theme, "test-field", "Search…", &mut text, 200.0)
                    };
                    if focus {
                        response.request_focus();
                    }
                });
            })
        };
        let idle = frame(false);
        frame(true);
        frame(false);
        let focused = frame(false);
        for (output, expected_edges) in [(idle, 0), (focused, 2)] {
            let mut backgrounds = 0;
            let mut edges = 0;
            for shape in output.shapes {
                if let egui::epaint::Shape::Rect(rect) = shape.shape {
                    if rect.fill == theme.input_fill() {
                        backgrounds += 1;
                        assert_eq!(rect.corner_radius, egui::CornerRadius::ZERO);
                        assert_eq!(rect.stroke, egui::Stroke::NONE);
                    }
                    if rect.fill == theme.accent && rect.rect.width() == 2.0 {
                        edges += 1;
                    }
                }
            }
            assert_eq!(backgrounds, 1);
            assert_eq!(edges, expected_edges);
        }
    }
}

#[test]
fn input_buttons_match_the_number_fields_height() {
    let ctx = egui::Context::default();
    let theme = super::Theme::GAME;
    let output = ctx.run(egui::RawInput::default(), |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            super::button(ui, &theme, "Back");
            super::number_field(ui, &theme, "Show", &mut 50_u32, 1..=1000, "");
        });
    });
    let heights = output
        .shapes
        .into_iter()
        .filter_map(|shape| match shape.shape {
            egui::epaint::Shape::Rect(rect) if rect.fill == theme.input_fill() => {
                Some(rect.rect.height())
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(heights.len(), 2);
    assert_eq!(heights[0], heights[1]);
}

#[test]
fn filter_chips_reveal_and_remove_with_eased_horizontal_sweeps() {
    let ctx = egui::Context::default();
    let theme = super::Theme::GAME;
    let mut chips = super::Pills::default();
    let mut values = vec![(0, "Planet radius".into())];
    let mut frame = |time, events| {
        let mut result = super::pill::Pill::default();
        let output = ctx.run(
            egui::RawInput {
                time: Some(time),
                events,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    result = chips
                        .show(ui, &theme, &values)
                        .into_iter()
                        .next()
                        .map(|(_, action)| action)
                        .unwrap_or_default();
                    if result.remove {
                        values.clear();
                    }
                });
            },
        );
        let visible = output
            .shapes
            .into_iter()
            .find_map(|shape| match shape.shape {
                egui::epaint::Shape::Rect(rect) if rect.fill == theme.input_fill() => {
                    Some((rect.rect, shape.clip_rect))
                }
                _ => None,
            });
        (result, visible)
    };
    assert!(frame(0.0, vec![]).1.is_none());
    let (full, clip) = frame(0.09, vec![]).1.unwrap();
    assert!(clip.width() > full.width() * 0.5 && clip.width() < full.width());
    assert_eq!(clip.left(), full.left());
    frame(0.2, vec![]);
    let pos = pos2(full.right() - 10.0, full.center().y);
    frame(0.29, vec![egui::Event::PointerMoved(pos)]);
    frame(0.30, vec![button(pos, true)]);
    assert!(frame(0.31, vec![button(pos, false)]).0.remove);
    frame(0.32, vec![]);
    let (result, visible) = frame(0.41, vec![]);
    assert!(!result.remove);
    let (full, clip) = visible.unwrap();
    assert!(clip.width() < full.width() * 0.5);
    assert_eq!(clip.left(), full.left());
    assert!(frame(0.52, vec![]).1.is_none());
}

#[test]
fn short_content_keeps_the_window_size_and_its_header_buttons_inside() {
    let ctx = egui::Context::default();
    super::plain_style(&ctx);
    let screen = Rect::from_min_size(egui::Pos2::ZERO, vec2(2560.0, 1400.0));
    let mut output = None;
    for _ in 0..3 {
        output = Some(ctx.run(
            egui::RawInput {
                screen_rect: Some(screen),
                ..Default::default()
            },
            |ctx| {
                super::Window::new("short", "Short")
                    .min_size(vec2(380.0, 320.0))
                    .show(ctx, &super::Theme::GAME, &mut true, |ui| {
                        super::ScrollArea::vertical().show(ui, |ui| {
                            super::note(ui, &super::Theme::GAME, "Select an object.");
                        });
                    });
            },
        ));
    }
    let bounds = ctx.memory(|memory| memory.area_rect(super::window_id("short")).unwrap());
    assert!(
        bounds.width() >= 380.0 && bounds.height() >= 320.0,
        "a single note must not shrink the window: {bounds:?}"
    );
    let close = output
        .unwrap()
        .shapes
        .into_iter()
        .find_map(|shape| match shape.shape {
            egui::epaint::Shape::Text(text) if text.galley.text() == "×" => {
                Some(text.pos + text.galley.size() * 0.5)
            }
            _ => None,
        })
        .unwrap();
    assert!(bounds.contains(close), "× at {close:?} outside {bounds:?}");
}
