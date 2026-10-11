use super::canvas::project;
use super::hud::{MAX_RATE, dragged_magnitude, dragged_rate, dragged_value, rate_text};
use super::*;
use crate::format::civil_date;
use space_simulation::SimulationClock;

#[test]
fn travel_directions_project_through_the_camera_perspective() {
    let rect = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(1600.0, 900.0));
    let half = |degrees: f32| (degrees.to_radians() / 2.0).tan();
    let project = |camera| project(rect, camera, half(50.0));
    assert_eq!(project(-Vec3::Z), Some(rect.center()));
    // Pure sideways or backwards travel has no on-screen vanishing point.
    assert_eq!(project(-Vec3::X), None);
    assert_eq!(project(Vec3::Z), None);
    // Forward and left at 45° is off the left edge of a 50° view.
    let diagonal = Vec3::new(-1.0, 0.0, -1.0).normalize();
    assert!(!rect.contains(project(diagonal).unwrap()));
    // 25° up is the top edge.
    let up = project(Vec3::new(0.0, half(50.0), -1.0)).unwrap();
    assert!((up.y - rect.top()).abs() < 1e-3);
    // A 120° view (about 144° wide at 16:9) brings the diagonal on screen.
    let wide = super::canvas::project(rect, diagonal, half(120.0)).unwrap();
    assert!(rect.contains(wide));
}

#[test]
fn civil_dates_and_rates() {
    // 2026-10-08T00:00:00Z
    assert_eq!(civil_date(1_791_417_600_000), (8, "Oct", 2026));
    assert_eq!(civil_date(0), (1, "Jan", 1970));
    assert_eq!(rate_text(1.0), "1.00 seconds/s");
    assert_eq!(rate_text(60.0), "1.00 minutes/s");
    assert_eq!(rate_text(3600.0), "1.00 hours/s");
    assert_eq!(rate_text(89.9 * 86_400.0), "89.90 days/s");
    assert_eq!(rate_text(MAX_RATE), "100.00 years/s");
}

#[test]
fn time_drag_is_continuous_logarithmic_and_clamped() {
    assert!((dragged_rate(1.0, 200.0) - MAX_RATE.sqrt()).abs() < 1e-6);
    assert!((dragged_rate(86_400.0, 0.0) - 86_400.0).abs() < 1e-6);
    assert!(dragged_rate(86_400.0, 0.1) > 86_400.0);
    assert!(dragged_rate(86_400.0, -0.1) < 86_400.0);
    assert_eq!(dragged_rate(1.0, -1000.0), 1.0);
    assert!((dragged_rate(MAX_RATE, 1000.0) - MAX_RATE).abs() < 1e-5);
}

#[test]
fn time_control_click_pauses_but_a_multi_frame_drag_does_not() {
    let ctx = egui::Context::default();
    let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0));
    let mut clock = SimulationClock {
        paused: true,
        ..Default::default()
    };
    let mut drag = None;
    let mut frame = |events: Vec<egui::Event>| {
        let mut commands = Vec::new();
        let output = ctx.run(
            egui::RawInput {
                screen_rect: Some(rect),
                events,
                ..Default::default()
            },
            |ctx| time_controls(ctx, rect, &clock, &mut drag, &mut commands),
        );
        if !commands.is_empty() {
            assert!(
                pointer_captured(&ctx),
                "time interactions must not reach the scene"
            );
        }
        for command in &commands {
            match command {
                UiCommand::SetRate { rate } => clock.rate = *rate,
                UiCommand::SetPaused { paused } => clock.paused = *paused,
                _ => panic!("unexpected time command"),
            }
        }
        (output, commands)
    };
    // Areas settle their position after their first sizing frame.
    frame(vec![]);
    let (output, _) = frame(vec![]);
    let pos = output
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::epaint::Shape::Text(text) if text.galley.job.text.contains("days/s") => {
                Some(text.pos + text.galley.size() * 0.5)
            }
            _ => None,
        })
        .expect("speed text is painted");
    let button = |pos, pressed| egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    frame(vec![egui::Event::PointerMoved(pos), button(pos, true)]);
    let (_, commands) = frame(vec![button(pos, false)]);
    assert!(matches!(
        commands.as_slice(),
        [UiCommand::SetPaused { paused: false }]
    ));

    frame(vec![button(pos, true)]);
    let first = pos + egui::vec2(40.0, 0.0);
    let (_, commands) = frame(vec![egui::Event::PointerMoved(first)]);
    let [UiCommand::SetRate { rate: first_rate }] = commands.as_slice() else {
        panic!("drag must only change the rate");
    };
    let first_rate = *first_rate;
    let end = pos + egui::vec2(80.0, 0.0);
    let (_, commands) = frame(vec![egui::Event::PointerMoved(end)]);
    let [UiCommand::SetRate { rate: end_rate }] = commands.as_slice() else {
        panic!("continued drag must only change the rate");
    };
    assert!(*end_rate > first_rate);
    assert!((*end_rate - dragged_rate(86_400.0, 80.0)).abs() < 1e-5);
    let (_, commands) = frame(vec![button(end, false)]);
    assert!(
        commands.is_empty(),
        "releasing a drag must not toggle pause"
    );
}

#[test]
fn hud_capture_survives_leaving_text_but_does_not_cover_empty_sky() {
    let ctx = egui::Context::default();
    let hit = egui::Rect::from_min_size(egui::pos2(350.0, 550.0), egui::vec2(100.0, 20.0));
    let frame = |pos, dragging| {
        ctx.data_mut(|data| {
            data.insert_temp(egui::Id::new("time-pointer-capture"), (hit, dragging));
        });
        let mut captured = false;
        let _ = ctx.run(
            egui::RawInput {
                events: vec![egui::Event::PointerMoved(pos)],
                ..Default::default()
            },
            |ctx| captured = pointer_captured(ctx),
        );
        captured
    };
    assert!(frame(hit.center(), false));
    assert!(frame(egui::pos2(700.0, 300.0), true));
    assert!(!frame(egui::pos2(700.0, 300.0), false));
}

#[test]
fn set_speed_drag_is_continuous_and_captures_input_without_click_actions() {
    let ctx = egui::Context::default();
    let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0));
    let mut speed = 1000.0;
    let mut drag = None;
    let mut frame = |events| {
        let mut commands = Vec::new();
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(rect),
                events,
                ..Default::default()
            },
            |ctx| speed_readout(ctx, rect, speed, 0.0, &mut drag, &mut commands),
        );
        for command in &commands {
            let UiCommand::SetMoveSpeed { speed: value } = command else {
                panic!("movement speed must not affect the clock or selection");
            };
            assert!(pointer_captured(&ctx));
            speed = *value;
        }
        let (hit, _) = ctx
            .data(|data| {
                data.get_temp::<(egui::Rect, bool)>(egui::Id::new("speed-pointer-capture"))
            })
            .unwrap();
        (commands, hit)
    };
    frame(vec![]);
    let (_, hit) = frame(vec![]);
    let start = hit.center();
    let button = |pos, pressed| egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    frame(vec![egui::Event::PointerMoved(start), button(start, true)]);
    assert!(frame(vec![button(start, false)]).0.is_empty());
    frame(vec![button(start, true)]);
    let (commands, _) = frame(vec![egui::Event::PointerMoved(
        start + egui::vec2(40.0, 0.0),
    )]);
    let [UiCommand::SetMoveSpeed { speed: first }] = commands.as_slice() else {
        panic!("expected speed adjustment");
    };
    let first = *first;
    let end = start + egui::vec2(80.0, 0.0);
    let (commands, _) = frame(vec![egui::Event::PointerMoved(end)]);
    let [UiCommand::SetMoveSpeed { speed: last }] = commands.as_slice() else {
        panic!("expected continued adjustment");
    };
    assert!(*last > first);
    assert!(
        (*last
            - dragged_value(
                1000.0,
                80.0,
                crate::scene::MIN_SPEED,
                crate::scene::MAX_SPEED
            ))
        .abs()
            < 1e-5
    );
    assert!(frame(vec![button(end, false)]).0.is_empty());
}

#[test]
fn manual_sliders_apply_immediately_and_capture_drags() {
    let ctx = egui::Context::default();
    let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1000.0, 700.0));
    let mut nav = Navigation::default();
    let mut drags = [None; 2];
    let mut frame = |events| {
        let mut commands = Vec::new();
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(rect),
                events,
                ..Default::default()
            },
            |ctx| exposure_controls(ctx, rect, &nav, &mut drags, &mut commands),
        );
        for command in &commands {
            assert!(pointer_captured(&ctx));
            match command {
                UiCommand::SetExposure { stops } => nav.set_exposure(*stops),
                UiCommand::SetCompression { amount } => nav.set_compression(*amount),
                _ => panic!("manual sliders must not change camera, clock or selection"),
            }
        }
        let hits = ctx.data(|data| {
            let hit = |id| {
                data.get_temp::<(egui::Rect, bool)>(egui::Id::new(id))
                    .unwrap()
                    .0
            };
            (
                hit("exposure-pointer-capture"),
                hit("compression-pointer-capture"),
            )
        });
        (commands, hits)
    };
    frame(vec![]);
    let (_, (exposure, compression)) = frame(vec![]);
    assert!(exposure.bottom() < compression.top());
    let button = |pos, pressed| egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    for (hit, is_exposure) in [(exposure, true), (compression, false)] {
        let start = hit.left_center() + egui::vec2(hit.width() * 0.25, 0.0);
        frame(vec![egui::Event::PointerMoved(start), button(start, true)]);
        assert!(frame(vec![button(start, false)]).0.is_empty());
        frame(vec![button(start, true)]);
        let (commands, _) = frame(vec![egui::Event::PointerMoved(hit.center())]);
        assert!(commands.iter().any(|c| matches!(
            (c, is_exposure),
            (UiCommand::SetExposure { .. }, true) | (UiCommand::SetCompression { .. }, false)
        )));
        frame(vec![egui::Event::PointerMoved(egui::pos2(200.0, 200.0))]);
        assert!(pointer_captured(&ctx));
        frame(vec![button(egui::pos2(200.0, 200.0), false)]);
    }
}

#[test]
fn manual_controls_are_inactive_in_wireframe_mode() {
    let ctx = egui::Context::default();
    let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1000.0, 700.0));
    let nav = Navigation {
        realistic: false,
        ..Default::default()
    };
    let mut drags = [None; 2];
    let mut commands = Vec::new();
    let _ = ctx.run(
        egui::RawInput {
            screen_rect: Some(rect),
            ..Default::default()
        },
        |ctx| exposure_controls(ctx, rect, &nav, &mut drags, &mut commands),
    );
    let hit = ctx
        .data(|data| data.get_temp::<(egui::Rect, bool)>(egui::Id::new("exposure-pointer-capture")))
        .unwrap()
        .0;
    for events in [
        vec![
            egui::Event::PointerMoved(hit.center()),
            egui::Event::PointerButton {
                pos: hit.center(),
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            },
        ],
        vec![egui::Event::PointerMoved(
            hit.center() + egui::vec2(40.0, 0.0),
        )],
    ] {
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(rect),
                events,
                ..Default::default()
            },
            |ctx| exposure_controls(ctx, rect, &nav, &mut drags, &mut commands),
        );
    }
    assert!(commands.is_empty());
    assert_eq!(drags, [None; 2]);
}

#[test]
fn magnitude_drag_applies_once_on_release() {
    let ctx = egui::Context::default();
    let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0));
    let mut drag = None;
    let mut frame = |events| {
        let mut commands = Vec::new();
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(rect),
                events,
                ..Default::default()
            },
            |ctx| magnitude_control(ctx, rect, 6.5, &mut drag, &mut commands),
        );
        let (hit, _) = ctx
            .data(|data| {
                data.get_temp::<(egui::Rect, bool)>(egui::Id::new("magnitude-pointer-capture"))
            })
            .unwrap();
        (commands, hit)
    };
    frame(vec![]);
    let (_, hit) = frame(vec![]);
    let start = hit.center();
    let button = |pos, pressed| egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    frame(vec![egui::Event::PointerMoved(start), button(start, true)]);
    let end = start - egui::vec2(100.0, 0.0);
    assert!(frame(vec![egui::Event::PointerMoved(end)]).0.is_empty());
    assert!(pointer_captured(&ctx));
    let (commands, _) = frame(vec![button(end, false)]);
    assert_eq!(
        commands,
        [UiCommand::SetMagnitude {
            magnitude: dragged_magnitude(6.5, -100.0)
        }]
    );
    assert_eq!(dragged_magnitude(6.5, -100.0), 5.1);
    assert_eq!(dragged_magnitude(6.5, 1000.0), 8.5);
}

#[test]
fn text_huds_share_edge_padding_and_bottom_baseline() {
    use crate::ui::breadcrumb::{CURRENT, HUD_LINE_GAP, HUD_PADDING, LINK};
    for size in [egui::vec2(1000.0, 700.0), egui::vec2(1600.0, 1000.0)] {
        let ctx = egui::Context::default();
        let rect = egui::Rect::from_min_max(egui::pos2(320.0, 0.0), size.to_pos2());
        let clock = SimulationClock::default();
        let mut drag = None;
        let mut speed_drag = None;
        let mut magnitude_drag = None;
        let mut commands = Vec::new();
        let nav = Navigation::default();
        let galaxy = space_simulation::demo_galaxy();
        let mut inspector = crate::ui::inspector::Cache::default();
        let mut output = egui::FullOutput::default();
        for frame in 0..3 {
            output = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
                    time: Some(f64::from(frame)),
                    ..Default::default()
                },
                |ctx| {
                    crate::ui::breadcrumb::show(
                        ctx,
                        rect,
                        &nav,
                        &galaxy,
                        &crate::stars::stream::Starfield::default(),
                        &mut inspector,
                        &mut commands,
                    );
                    time_controls(ctx, rect, &clock, &mut drag, &mut commands);
                    speed_readout(ctx, rect, 1000.0, 0.0, &mut speed_drag, &mut commands);
                    magnitude_control(ctx, rect, 6.5, &mut magnitude_drag, &mut commands);
                },
            );
        }
        let bounds = |prefix: &str, colour| {
            output
                .shapes
                .iter()
                .find_map(|shape| match &shape.shape {
                    egui::epaint::Shape::Text(text)
                        if text.galley.job.text.starts_with(prefix)
                            && text.fallback_color == colour =>
                    {
                        Some(egui::Rect::from_min_size(text.pos, text.galley.size()))
                    }
                    _ => None,
                })
                .unwrap_or_else(|| panic!("HUD text {prefix} painted with shared colour"))
        };
        let crumb = bounds("Location", LINK);
        let speed = bounds("Set speed", LINK);
        let velocity = bounds("Velocity", CURRENT);
        let time_rate = bounds("1.00 days/s", LINK);
        assert!((crumb.left() - rect.left() - HUD_PADDING).abs() < 1.0);
        assert!(
            (crumb.top() - rect.top() - HUD_PADDING).abs() < 1.0,
            "crumb: {crumb:?}"
        );
        assert!((speed.left() - crumb.left()).abs() < 1.0);
        assert!((velocity.left() - crumb.left()).abs() < 1.0);
        assert!((rect.bottom() - speed.bottom() - HUD_PADDING).abs() < 1.0);
        assert!((speed.bottom() - time_rate.bottom()).abs() < 1.0);
        let magnitude = bounds("Magnitude limit", LINK);
        assert!((rect.right() - magnitude.right() - HUD_PADDING).abs() < 1.0);
        assert!((magnitude.bottom() - speed.bottom()).abs() < 1.0);
        assert!((speed.top() - velocity.bottom() - HUD_LINE_GAP).abs() < 1.0);
        assert!((time_rate.center().x - rect.center().x).abs() < 1.0);
    }
}

#[test]
fn floating_inspector_grows_after_tab_changes_and_wraps_in_a_narrow_viewport() {
    let ctx = egui::Context::default();
    let galaxy = space_simulation::demo_galaxy();
    let mut nav = Navigation::default();
    let mut inspector = crate::ui::inspector::Cache::default();
    let mut commands = Vec::new();
    let mut output = egui::FullOutput::default();
    for (stage, size) in [
        egui::vec2(1600.0, 1000.0),
        egui::vec2(1600.0, 1000.0),
        egui::vec2(760.0, 600.0),
    ]
    .into_iter()
    .enumerate()
    {
        if stage == 1 {
            nav.select(Some(crate::Selection::planet(&galaxy.systems[1], 0)));
        }
        let rect = egui::Rect::from_min_max(egui::pos2(320.0, 0.0), size.to_pos2());
        for _ in 0..4 {
            output = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
                    ..Default::default()
                },
                |ctx| {
                    crate::ui::breadcrumb::show(
                        ctx,
                        rect,
                        &nav,
                        &galaxy,
                        &crate::stars::stream::Starfield::default(),
                        &mut inspector,
                        &mut commands,
                    );
                },
            );
        }
        if stage == 1 {
            for prefix in ["Orbital period", "Fly to planet"] {
                let shape = output
                    .shapes
                    .iter()
                    .find_map(|shape| match &shape.shape {
                        egui::epaint::Shape::Text(text)
                            if text.galley.job.text.starts_with(prefix)
                                && text.fallback_color != egui::Color32::from_black_alpha(130)
                                && text.fallback_color != egui::Color32::from_black_alpha(45) =>
                        {
                            Some((
                                shape.clip_rect,
                                egui::Rect::from_min_size(text.pos, text.galley.size()),
                            ))
                        }
                        _ => None,
                    })
                    .unwrap_or_else(|| panic!("inspector must paint {prefix}"));
                assert!(
                    shape.0.contains_rect(shape.1),
                    "{prefix} is clipped: {shape:?}"
                );
            }
        }
        if stage == 2 {
            let area = ctx
                .memory(|memory| memory.area_rect(egui::Id::new("breadcrumb")))
                .unwrap();
            assert!(
                area.right() <= rect.right() - crate::ui::breadcrumb::HUD_PADDING + 1.0,
                "inspector exceeds viewport: {area:?}"
            );
        }
    }
}

#[test]
fn floating_inspector_expands_instead_of_scrolling() {
    let ctx = egui::Context::default();
    let galaxy = space_simulation::demo_galaxy();
    let mut nav = Navigation::default();
    nav.select(Some(crate::Selection::planet(&galaxy.systems[1], 0)));
    let mut inspector = crate::ui::inspector::Cache::default();
    let mut commands = Vec::new();
    let viewport = egui::Rect::from_min_max(egui::pos2(320.0, 0.0), egui::pos2(1000.0, 200.0));
    let mut output = egui::FullOutput::default();
    for _ in 0..4 {
        output = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1000.0, 1000.0),
                )),
                ..Default::default()
            },
            |ctx| {
                crate::ui::breadcrumb::show(
                    ctx,
                    viewport,
                    &nav,
                    &galaxy,
                    &crate::stars::stream::Starfield::default(),
                    &mut inspector,
                    &mut commands,
                )
            },
        );
    }
    let area = ctx
        .memory(|memory| memory.area_rect(egui::Id::new("breadcrumb")))
        .unwrap();
    assert!(
        area.height() > viewport.height(),
        "information must use its natural height: {area:?}"
    );
    assert!((area.top() - crate::ui::breadcrumb::HUD_PADDING).abs() < 1.0);
    assert!(
        output.shapes.iter().any(|shape| match &shape.shape {
            egui::epaint::Shape::Text(text) =>
                text.galley.job.text == "Fly to planet"
                    && shape
                        .clip_rect
                        .contains_rect(egui::Rect::from_min_size(text.pos, text.galley.size())),
            _ => false,
        }),
        "final information row must remain visible, not inside a short scroll viewport"
    );
}

#[test]
fn camera_drag_crossing_time_text_stays_with_the_scene() {
    let ctx = egui::Context::default();
    let hit = egui::Rect::from_min_size(egui::pos2(350.0, 550.0), egui::vec2(100.0, 20.0));
    ctx.data_mut(|data| {
        data.insert_temp(egui::Id::new("time-pointer-capture"), (hit, false));
    });
    let frame = |events| {
        let mut captured = false;
        let _ = ctx.run(
            egui::RawInput {
                events,
                ..Default::default()
            },
            |ctx| captured = pointer_captured(ctx),
        );
        captured
    };
    let start = egui::pos2(475.0, 560.0);
    assert!(!frame(vec![
        egui::Event::PointerMoved(start),
        egui::Event::PointerButton {
            pos: start,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: egui::Modifiers::NONE,
        },
    ]));
    assert!(!frame(vec![egui::Event::PointerMoved(hit.center())]));
    assert!(!frame(vec![egui::Event::PointerMoved(egui::pos2(
        325.0, 560.0
    ))]));
    // Once released, hovering the text captures normally again.
    assert!(frame(vec![
        egui::Event::PointerMoved(hit.center()),
        egui::Event::PointerButton {
            pos: hit.center(),
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        },
    ]));
}
