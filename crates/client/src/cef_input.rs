//! Native ownership is decided before camera and egui input, not after a JS event.
use bevy::{input::mouse::MouseWheel, prelude::*, window::PrimaryWindow};
use bevy_cef::prelude::*;
#[cfg(not(target_os = "windows"))]
use bevy_cef_core::prelude::Browsers;
#[cfg(target_os = "windows")]
use bevy_cef_core::prelude::BrowsersProxy;
use std::collections::HashSet;

#[derive(Resource, Default)]
pub struct HtmlInput {
    pub rects: Vec<[f32; 4]>,
    pub ready: bool,
    pub pointer: bool,
    pub keyboard: bool,
    focus: bool,
    drag: Option<bool>,
    held: HashSet<MouseButton>,
    blocked: HashSet<KeyCode>,
    last_pos: Vec2,
    hover: bool,
}

impl HtmlInput {
    fn keyboard_handoff(&mut self, keys: &ButtonInput<KeyCode>, old_focus: bool) {
        self.blocked.retain(|k| keys.pressed(*k));
        if self.keyboard || old_focus != self.focus {
            self.blocked.extend(keys.get_pressed().copied());
        }
        self.keyboard |= !self.blocked.is_empty();
    }

    fn hit(&self, pos: Vec2) -> bool {
        !self.ready
            || self
                .rects
                .iter()
                .any(|[x, y, w, h]| pos.x >= *x && pos.y >= *y && pos.x < x + w && pos.y < y + h)
    }

    fn own(&mut self, hover: bool, pressed: bool, down: bool, focused: bool, escape: bool) {
        if pressed && self.drag.is_none() {
            self.drag = Some(hover);
            self.focus = hover;
        }
        self.pointer = self.drag.unwrap_or(hover);
        self.keyboard = self.focus || escape;
        if escape || !focused {
            self.focus = false;
        }
        // Keep ownership for the release frame. Clear it only after routing it.
        if !down || !focused {
            self.drag = None;
        }
    }
}

pub fn install(app: &mut App) {
    app.init_resource::<HtmlInput>().add_systems(
        PreUpdate,
        (route, filter_egui)
            .chain()
            .after(bevy_egui::EguiPreUpdateSet::ProcessInput)
            .before(bevy_egui::EguiPreUpdateSet::BeginPass),
    );
}

#[allow(clippy::too_many_arguments)]
fn route(
    mut state: ResMut<HtmlInput>,
    windows: Query<&Window, With<PrimaryWindow>>,
    webviews: Query<(Entity, &WebviewSize), With<crate::cef_overlay::HtmlPanel>>,
    keys: Res<ButtonInput<KeyCode>>,
    buttons: Res<ButtonInput<MouseButton>>,
    mut wheel: MessageReader<MouseWheel>,
    mut focus: ResMut<FocusedWebview>,
    mut commands: Commands,
    #[cfg(not(target_os = "windows"))] browsers: NonSend<Browsers>,
    #[cfg(target_os = "windows")] browsers: Res<BrowsersProxy>,
) {
    let Ok(window) = windows.single() else { return };
    let Ok((entity, size)) = webviews.single() else {
        return;
    };
    let pos = window
        .cursor_position()
        .map(|p| p * size.0 / Vec2::new(window.width(), window.height()));
    let moved = pos.is_some_and(|p| p != state.last_pos);
    if let Some(pos) = pos {
        state.last_pos = pos;
    }
    let hover = pos.is_some_and(|p| state.hit(p));
    let pressed = buttons.get_just_pressed().next().is_some();
    let down = buttons.get_pressed().next().is_some();
    let escape = state.focus && keys.just_pressed(KeyCode::Escape);
    let old_focus = state.focus;
    state.own(hover, pressed, down, window.focused, escape);
    state.keyboard_handoff(&keys, old_focus);
    let target = state.focus.then_some(entity);
    if focus.0 != target {
        focus.0 = target;
    }
    if old_focus != state.focus {
        browsers.set_focus(&entity, state.focus);
        if !state.focus {
            commands.trigger(HostEmitEvent::new(entity, "space:blur", &true));
        }
    }
    let cursor = state.last_pos;
    let held: Vec<_> = state.held.iter().copied().collect();
    if moved || state.hover != hover {
        #[cfg(not(target_os = "windows"))]
        browsers.send_mouse_move(&entity, held.iter(), cursor, !hover && held.is_empty());
        #[cfg(target_os = "windows")]
        browsers.send_mouse_move(&entity, &held, cursor, !hover && held.is_empty());
    }
    state.hover = hover;
    if state.pointer && window.focused {
        for button in buttons.get_just_pressed() {
            if let Some(pointer_button) = pointer_button(*button) {
                browsers.send_mouse_click(&entity, cursor, pointer_button, false);
                state.held.insert(*button);
            }
        }
    }
    // Releases are never alpha-filtered, even outside the panel or the window.
    for button in state.held.iter().copied().collect::<Vec<_>>() {
        if !buttons.pressed(button) || !window.focused {
            if let Some(pointer_button) = pointer_button(button) {
                browsers.send_mouse_click(&entity, cursor, pointer_button, true);
            }
            state.held.remove(&button);
        }
    }
    for event in wheel.read() {
        if state.pointer && window.focused {
            let scale = match event.unit {
                bevy::input::mouse::MouseScrollUnit::Line => 120.0,
                bevy::input::mouse::MouseScrollUnit::Pixel => 1.0,
            };
            browsers.send_mouse_wheel(&entity, cursor, Vec2::new(event.x, event.y) * scale);
        }
    }
}

fn pointer_button(button: MouseButton) -> Option<PointerButton> {
    match button {
        MouseButton::Left => Some(PointerButton::Primary),
        MouseButton::Right => Some(PointerButton::Secondary),
        MouseButton::Middle => Some(PointerButton::Middle),
        _ => None,
    }
}

fn filter_egui(
    state: Res<HtmlInput>,
    mut inputs: Query<(&mut bevy_egui::EguiInput, &mut bevy_egui::EguiContext)>,
) {
    use bevy_egui::egui::Event;
    for (mut input, mut context) in &mut inputs {
        input.events.retain(|event| match event {
            // Always clear previously-held egui keys/buttons, even when the
            // matching press belonged to a previous focus owner.
            Event::Key { pressed: false, .. } | Event::PointerButton { pressed: false, .. } => true,
            Event::PointerMoved(_)
            | Event::MouseMoved(_)
            | Event::PointerButton { .. }
            | Event::MouseWheel { .. } => !state.pointer,
            Event::Key { .. }
            | Event::Text(_)
            | Event::Paste(_)
            | Event::Copy
            | Event::Cut
            | Event::Ime(_) => !state.keyboard,
            _ => true,
        });
        if state.pointer {
            input.events.push(Event::PointerGone);
        }
        if state.keyboard {
            input.modifiers = default();
            context.get_mut().memory_mut(|m| {
                if let Some(id) = m.focused() {
                    m.surrender_focus(id);
                }
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn held_typing_keys_do_not_turn_into_camera_movement_after_escape() {
        let mut state = HtmlInput::default();
        let mut keys = ButtonInput::default();
        keys.press(KeyCode::KeyW);
        state.own(true, true, false, true, false);
        state.keyboard_handoff(&keys, false);
        state.own(false, false, false, true, true);
        state.keyboard_handoff(&keys, true);
        state.own(false, false, false, true, false);
        state.keyboard_handoff(&keys, false);
        assert!(state.keyboard);
        keys.release(KeyCode::KeyW);
        state.own(false, false, false, true, false);
        state.keyboard_handoff(&keys, false);
        assert!(!state.keyboard);
    }

    #[test]
    fn fast_clicks_keep_ui_ownership_for_the_release_frame() {
        let mut state = HtmlInput::default();
        state.own(true, true, false, true, false);
        assert!(state.pointer);
        assert!(state.drag.is_none());
        state.own(false, false, false, true, false);
        assert!(!state.pointer);
    }

    #[test]
    fn egui_filter_drops_ui_events_but_keeps_releases() {
        use bevy_egui::{EguiContext, EguiInput, egui};
        let mut app = App::new();
        app.insert_resource(HtmlInput {
            pointer: true,
            keyboard: true,
            ..default()
        })
        .add_systems(Update, filter_egui);
        let release = egui::Event::Key {
            key: egui::Key::W,
            physical_key: None,
            pressed: false,
            repeat: false,
            modifiers: default(),
        };
        let entity = app
            .world_mut()
            .spawn((
                EguiContext::default(),
                EguiInput(egui::RawInput {
                    events: vec![
                        egui::Event::Text("w".into()),
                        egui::Event::PointerMoved(egui::pos2(10.0, 10.0)),
                        release.clone(),
                    ],
                    ..default()
                }),
            ))
            .id();
        app.update();
        assert_eq!(
            app.world()
                .entity(entity)
                .get::<EguiInput>()
                .unwrap()
                .events,
            vec![release, egui::Event::PointerGone]
        );
    }
    #[test]
    fn drags_keep_their_owner_in_both_directions_through_release() {
        for ui in [true, false] {
            let mut state = HtmlInput::default();
            state.own(ui, true, true, true, false);
            state.own(!ui, false, true, true, false);
            assert_eq!(state.pointer, ui);
            state.own(!ui, false, false, true, false);
            assert_eq!(state.pointer, ui);
            state.own(!ui, false, false, true, false);
            assert_eq!(state.pointer, !ui);
        }
    }
    #[test]
    fn escape_and_window_blur_release_focus() {
        let mut state = HtmlInput::default();
        state.own(true, true, true, true, false);
        assert!(state.keyboard);
        state.own(true, false, false, true, true);
        assert!(!state.focus);
        assert!(state.keyboard); // Escape is consumed, not a scene deselection.
        state.own(true, false, false, true, false);
        assert!(!state.keyboard);
        state.own(true, true, true, true, false);
        state.own(false, false, false, false, false);
        assert!(!state.focus);
        assert!(state.drag.is_none());
    }
    #[test]
    fn logical_rectangles_not_transparency_define_ui_ownership() {
        let state = HtmlInput {
            ready: true,
            rects: vec![[20.0, 30.0, 100.0, 50.0]],
            ..default()
        };
        assert!(state.hit(Vec2::new(25.0, 35.0)));
        assert!(!state.hit(Vec2::new(120.0, 35.0)));
        assert!(!state.hit(Vec2::ZERO));
    }
}
