use super::*;
use egui::{Event, PointerButton, Pos2, Rect, pos2, vec2};

fn press(pos: Pos2, pressed: bool) -> Event {
    Event::PointerButton {
        pos,
        button: PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    }
}

fn text_position(output: &egui::FullOutput, needle: &str) -> Pos2 {
    output
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::epaint::Shape::Text(text) if text.galley.text().contains(needle) => {
                Some(text.pos + text.galley.size() * 0.5)
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("missing text: {needle}"))
}

#[test]
fn task_controls_preserve_filter_sort_group_and_navigation() {
    let ctx = egui::Context::default();
    kit::plain_style(&ctx);
    let galaxy = space_simulation::demo_galaxy();
    let nav = Navigation::default();
    let mut state = Tasks::default();
    let time = std::cell::Cell::new(0.0);
    let frame = |state: &mut Tasks, lists: bool, events| {
        time.set(time.get() + 0.05);
        let mut out = Vec::new();
        let output = ctx.run(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(900.0, 1000.0))),
                events,
                time: Some(time.get()),
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let mut panel = Panel {
                        galaxy: &galaxy,
                        nav: &nav,
                        state,
                        out: &mut out,
                    };
                    if lists {
                        panel.lists(ui);
                    } else {
                        panel.tasks(ui);
                    }
                });
            },
        );
        (output, out)
    };
    let click = |state: &mut Tasks, lists, pos| {
        frame(state, lists, vec![Event::PointerMoved(pos)]);
        frame(state, lists, vec![press(pos, true)]);
        frame(state, lists, vec![press(pos, false)]).1
    };
    let output = frame(&mut state, true, vec![]).0;
    let sort = text_position(&output, "Most tasks");
    click(&mut state, true, sort);
    assert!(state.list_sort == ListSort::Tasks);
    state.list_filter = galaxy.systems[0].name.clone();
    let output = frame(&mut state, true, vec![]).0;
    let list = text_position(&output, "task planets");
    assert_eq!(
        click(&mut state, true, list),
        vec![UiCommand::Jump {
            list: galaxy.systems[0].id.clone()
        }]
    );
    let (system, task) = galaxy
        .systems
        .iter()
        .find_map(|system| system.tasks().next().map(|(_, task)| (system, task)))
        .unwrap();
    state.task_filter = task.name.clone();
    let output = frame(&mut state, false, vec![]).0;
    let sort = text_position(&output, "Due date");
    click(&mut state, false, sort);
    assert!(state.task_sort == TaskSort::Due);
    let output = frame(&mut state, false, vec![]).0;
    let toggle = text_position(&output, "Group by status");
    click(&mut state, false, toggle);
    assert!(!state.group);
    let output = frame(&mut state, false, vec![]).0;
    let pos = text_position(&output, &format!("· {}", task.name));
    assert_eq!(
        click(&mut state, false, pos),
        vec![UiCommand::FindTask {
            list: system.id.clone(),
            task: task.id.clone()
        }]
    );
    let output = frame(&mut state, false, vec![]).0;
    click(&mut state, false, text_position(&output, "Group by status"));
    assert!(state.group);
    let output = frame(&mut state, false, vec![]).0;
    click(
        &mut state,
        false,
        text_position(&output, &task.status.to_uppercase()),
    );
    assert!(state.collapsed.contains(&task.status.to_lowercase()));
    let output = frame(&mut state, false, vec![]).0;
    assert!(!output.shapes.iter().any(|shape| matches!(&shape.shape,
        egui::epaint::Shape::Text(text) if text.galley.text().starts_with(&format!("· {}", task.name)))));
    click(
        &mut state,
        false,
        text_position(&output, &task.status.to_uppercase()),
    );
    assert!(!state.collapsed.contains(&task.status.to_lowercase()));
    let output = frame(&mut state, false, vec![]).0;
    // The field contains the query without the task row's leading bullet.
    let field = output
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::epaint::Shape::Text(text) if text.galley.text() == task.name => {
                Some(text.pos + text.galley.size() * 0.5)
            }
            _ => None,
        })
        .unwrap();
    click(&mut state, false, field);
    let (_, out) = frame(
        &mut state,
        false,
        vec![Event::Key {
            key: egui::Key::Enter,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }],
    );
    assert_eq!(
        out,
        vec![UiCommand::FindTask {
            list: system.id.clone(),
            task: task.id.clone()
        }]
    );
    // Searches still find remote tasks without mutating navigation directly.
    assert!(nav.visit.is_none() && nav.selection.is_none());
}

#[test]
fn task_window_uses_bounded_resizing_and_foreground_stacking() {
    for expanding in [false, true] {
        let ctx = egui::Context::default();
        kit::plain_style(&ctx);
        let galaxy = space_simulation::demo_galaxy();
        let nav = Navigation::default();
        let mut state = Tasks::default();
        let screen = Rect::from_min_size(Pos2::ZERO, vec2(1200.0, 900.0));
        let layer = egui::LayerId::new(egui::Order::Foreground, kit::window_id(WINDOW));
        ctx.data_mut(|data| {
            data.insert_temp(egui::Id::new(("window-center", WINDOW)), pos2(450.0, 450.0));
            data.insert_temp(
                egui::Id::new(("window-center", "Other")),
                pos2(700.0, 450.0),
            );
        });
        let time = std::cell::Cell::new(0.0);
        let mut frame = |events| {
            time.set(time.get() + 0.05);
            let _ = ctx.run(
                egui::RawInput {
                    screen_rect: Some(screen),
                    time: Some(time.get()),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    window(ctx, &mut true, &galaxy, &nav, &mut state, &mut Vec::new());
                    kit::Window::new("Other", "Other").show(ctx, &Theme::GAME, &mut true, |ui| {
                        ui.allocate_space(ui.available_size());
                    });
                },
            );
        };
        let bounds = || {
            ctx.memory(|memory| {
                memory
                    .area_rect(layer.id)
                    .expect("Tasks must use the shared window ID")
            })
        };
        frame(vec![]);
        frame(vec![]);
        let before = bounds();
        let header = pos2(before.left() + 70.0, before.top() + 20.0);
        frame(vec![Event::PointerMoved(header)]);
        frame(vec![press(header, true)]);
        frame(vec![]);
        let overlap = pos2(600.0, 450.0);
        assert_eq!(
            ctx.layer_id_at(overlap),
            Some(layer),
            "Tasks must rise above other foreground windows on press"
        );
        frame(vec![press(header, false)]);
        let start = pos2(before.center().x, before.top() + 0.5);
        let delta = vec2(0.0, if expanding { -1500.0 } else { 1500.0 });
        frame(vec![Event::PointerMoved(start)]);
        frame(vec![press(start, true)]);
        frame(vec![Event::PointerMoved(start + delta)]);
        frame(vec![]);
        let after = bounds();
        assert!((after.bottom() - before.bottom()).abs() < 1.0);
        assert!(screen.expand(1.0).contains_rect(after));
        if expanding {
            assert!(after.top().abs() < 1.0);
        }
        let saturated = after;
        frame(vec![Event::PointerMoved(start + delta * 1.2)]);
        frame(vec![]);
        assert!((bounds().min - saturated.min).length() < 1.0);
        assert!((bounds().max - saturated.max).length() < 1.0);
    }
}
