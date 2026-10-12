use super::*;

fn button(pos: egui::Pos2, pressed: bool) -> egui::Event {
    egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    }
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
                        egui::ScrollArea::vertical()
                            .scroll_source(SCROLL_SOURCE)
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
                                        super::button(ui, "Button");
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
    let position = || {
        ctx.data(|data| {
            data.get_temp::<egui::Pos2>(egui::Id::new(("terminal-position", "test")))
                .unwrap()
        })
    };
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
