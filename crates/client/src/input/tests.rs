use super::*;

fn read(keys: &[KeyCode], buttons: &[MouseButton], typing: bool, pointer_free: bool) -> Actions {
    let mut key_input = ButtonInput::<KeyCode>::default();
    for key in keys {
        key_input.press(*key);
    }
    let mut button_input = ButtonInput::<MouseButton>::default();
    for button in buttons {
        button_input.press(*button);
    }
    let mut actions = Actions::default();
    actions.update(
        &Bindings::default(),
        &key_input,
        &button_input,
        typing,
        pointer_free,
    );
    actions
}

#[test]
fn one_key_can_serve_several_actions() {
    let actions = read(&[KeyCode::ShiftRight], &[], false, true);
    assert!(actions.pressed(Action::ThrottleUp) && actions.pressed(Action::Alternate));
    assert!(actions.just_pressed(Action::ThrottleUp));
}

#[test]
fn opposite_keys_make_an_axis() {
    let actions = read(&[KeyCode::KeyW], &[], false, true);
    assert_eq!(actions.axis(Action::Forward, Action::Backward), 1.0);
    let both = read(&[KeyCode::KeyW, KeyCode::KeyS], &[], false, true);
    assert_eq!(both.axis(Action::Forward, Action::Backward), 0.0);
}

#[test]
fn the_interface_keeps_its_typing_and_pointer() {
    let typing = read(&[KeyCode::KeyW], &[MouseButton::Left], true, true);
    assert!(!typing.pressed(Action::Forward) && typing.pressed(Action::Look));
    let over_ui = read(&[KeyCode::KeyW], &[MouseButton::Left], false, false);
    assert!(over_ui.pressed(Action::Forward) && !over_ui.pressed(Action::Look));
}

#[test]
fn a_consumed_press_is_not_seen_again() {
    let mut actions = read(&[KeyCode::Escape], &[], false, true);
    actions.consume(Action::Cancel);
    assert!(!actions.just_pressed(Action::Cancel) && actions.pressed(Action::Cancel));
}

#[test]
fn labels_name_each_key_once() {
    let bindings = Bindings::default();
    assert_eq!(
        bindings.label(&[
            Action::Forward,
            Action::Left,
            Action::Backward,
            Action::Right
        ]),
        "W A S D"
    );
    assert_eq!(
        bindings.label(&[Action::ThrottleUp, Action::ThrottleDown]),
        "Shift Ctrl"
    );
    assert_eq!(bindings.label(&[Action::Cancel]), "Esc");
    assert_eq!(bindings.label(&[Action::Look]), "Left mouse");
}

#[test]
fn bindings_round_trip_as_data() {
    let bindings = Bindings::default();
    let json = serde_json::to_string(&bindings).unwrap();
    assert_eq!(serde_json::from_str::<Bindings>(&json).unwrap(), bindings);
}

#[test]
fn tab_does_not_hand_the_keyboard_to_a_widget() {
    use bevy_egui::egui;
    let tab = egui::Event::Key {
        key: egui::Key::Tab,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    };
    let frame = |ctx: &egui::Context, events: Vec<egui::Event>, filter: bool| {
        let mut events = events;
        if filter {
            keep_from(ctx, &mut events);
        }
        let input = egui::RawInput {
            events,
            ..Default::default()
        };
        let _ = ctx.run(input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let _ = ui.button("Focusable");
            });
        });
    };
    for filter in [false, true] {
        let ctx = egui::Context::default();
        frame(&ctx, Vec::new(), filter);
        frame(&ctx, vec![tab.clone()], filter);
        frame(&ctx, Vec::new(), filter);
        assert_eq!(ctx.wants_keyboard_input(), !filter, "filtered: {filter}");
    }
}
