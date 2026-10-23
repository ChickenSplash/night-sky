//! Keys and mouse buttons, read as named actions through one binding table.
//! Systems ask whether an action is held, never which key it is on, so the
//! table can later be saved and edited by a controls menu. The free camera
//! and piloting share the table: most actions mean the same in both.
use bevy::{
    input::mouse::{MouseScrollUnit, MouseWheel},
    prelude::*,
};
use bevy_egui::EguiContexts;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    /// Camera travel, or the ship's fine thrusters.
    Forward,
    Backward,
    Left,
    Right,
    Up,
    Down,
    RollLeft,
    RollRight,
    /// Nose up, down, left and right while piloting.
    PitchUp,
    PitchDown,
    YawLeft,
    YawRight,
    /// Held: the main drive's throttle lever up or down, or in warp the
    /// speed sweeping up or down.
    ThrottleUp,
    ThrottleDown,
    FullThrottle,
    /// Cut the throttle, or in warp go to the slowest warp speed.
    CutThrottle,
    /// Cut sublight thrust and engage warp, or drop out.
    EngageWarp,
    StopRotation,
    /// Pilot the selected ship, or stop piloting.
    Pilot,
    /// Leave piloting, text input or tracking, clear selection or move up.
    Cancel,
    /// Visit the selected star.
    Visit,
    Pause,
    /// Held mouse buttons, read with the mouse's motion.
    Look,
    Orbit,
    /// Held with Look for field of view, or with the wheel for zoom.
    Alternate,
}

/// A key or mouse button. Gamepad and flight-stick inputs can join it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Binding {
    Key(KeyCode),
    Mouse(MouseButton),
}

impl Binding {
    pub fn label(self) -> String {
        match self {
            Self::Key(key) => key_label(key),
            Self::Mouse(MouseButton::Left) => "Left mouse".into(),
            Self::Mouse(MouseButton::Right) => "Right mouse".into(),
            Self::Mouse(MouseButton::Middle) => "Middle mouse".into(),
            Self::Mouse(button) => format!("{button:?} mouse"),
        }
    }
}

fn key_label(key: KeyCode) -> String {
    let name = format!("{key:?}");
    let short = match key {
        KeyCode::ShiftLeft | KeyCode::ShiftRight => "Shift",
        KeyCode::ControlLeft | KeyCode::ControlRight => "Ctrl",
        KeyCode::AltLeft | KeyCode::AltRight => "Alt",
        KeyCode::Escape => "Esc",
        KeyCode::ArrowUp => "↑",
        KeyCode::ArrowDown => "↓",
        KeyCode::ArrowLeft => "←",
        KeyCode::ArrowRight => "→",
        _ => {
            return name
                .strip_prefix("Key")
                .or_else(|| name.strip_prefix("Digit"))
                .unwrap_or(&name)
                .to_owned();
        }
    };
    short.into()
}

/// Which bindings trigger which actions. A binding may serve several
/// actions, read by different modes, and an action may have several.
#[derive(Resource, Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Bindings(pub Vec<(Binding, Action)>);

impl Default for Bindings {
    fn default() -> Self {
        use Action::*;
        use KeyCode as K;
        let keys = [
            (K::KeyW, Forward),
            (K::KeyS, Backward),
            (K::KeyA, Left),
            (K::KeyD, Right),
            (K::KeyR, Up),
            (K::KeyF, Down),
            (K::KeyQ, RollLeft),
            (K::KeyE, RollRight),
            (K::ArrowUp, PitchUp),
            (K::ArrowDown, PitchDown),
            (K::ArrowLeft, YawLeft),
            (K::ArrowRight, YawRight),
            (K::ShiftLeft, ThrottleUp),
            (K::ShiftRight, ThrottleUp),
            (K::ControlLeft, ThrottleDown),
            (K::ControlRight, ThrottleDown),
            (K::KeyZ, FullThrottle),
            (K::KeyX, CutThrottle),
            (K::KeyG, EngageWarp),
            (K::KeyT, StopRotation),
            (K::Tab, Pilot),
            (K::Escape, Cancel),
            (K::Enter, Visit),
            (K::Space, Pause),
            (K::ShiftLeft, Alternate),
            (K::ShiftRight, Alternate),
        ];
        let buttons = [(MouseButton::Left, Look), (MouseButton::Right, Orbit)];
        Self(
            keys.into_iter()
                .map(|(key, action)| (Binding::Key(key), action))
                .chain(
                    buttons
                        .into_iter()
                        .map(|(button, action)| (Binding::Mouse(button), action)),
                )
                .collect(),
        )
    }
}

impl Bindings {
    /// The controls for some actions, as help shows them: each action's
    /// bindings in table order, the same label once.
    pub fn label(&self, actions: &[Action]) -> String {
        let mut labels: Vec<String> = Vec::new();
        for action in actions {
            for (binding, _) in self.0.iter().filter(|(_, bound)| bound == action) {
                let label = binding.label();
                if !labels.contains(&label) {
                    labels.push(label);
                }
            }
        }
        labels.join(" ")
    }
}

/// This frame's actions. Keys typed into the interface and mouse input over
/// it are not actions.
#[derive(Resource, Default)]
pub struct Actions {
    pressed: HashSet<Action>,
    just_pressed: HashSet<Action>,
    /// Mouse wheel lines this frame, upwards positive.
    pub wheel: f32,
    /// Whether the pointer is over the 3D view rather than the interface.
    pub pointer_free: bool,
}

impl Actions {
    pub fn pressed(&self, action: Action) -> bool {
        self.pressed.contains(&action)
    }

    pub fn just_pressed(&self, action: Action) -> bool {
        self.just_pressed.contains(&action)
    }

    /// How far an action is held, from 0 to 1. Keys are all or nothing; an
    /// analogue axis bound to it later gives the part between.
    pub fn value(&self, action: Action) -> f32 {
        f32::from(u8::from(self.pressed(action)))
    }

    /// A pair of actions as one axis, from -1 to 1.
    pub fn axis(&self, positive: Action, negative: Action) -> f32 {
        (self.value(positive) - self.value(negative)).clamp(-1.0, 1.0)
    }

    /// Handle a press, so a later system this frame does not act on it too.
    pub fn consume(&mut self, action: Action) {
        self.just_pressed.remove(&action);
    }

    /// Read every binding: keys unless the interface takes typing, mouse
    /// buttons and the wheel unless the pointer is the interface's.
    pub fn update(
        &mut self,
        bindings: &Bindings,
        keys: &ButtonInput<KeyCode>,
        buttons: &ButtonInput<MouseButton>,
        typing: bool,
        pointer_free: bool,
    ) {
        self.pressed.clear();
        self.just_pressed.clear();
        self.pointer_free = pointer_free;
        for (binding, action) in &bindings.0 {
            let (pressed, just) = match binding {
                Binding::Key(_) if typing => (false, false),
                Binding::Mouse(_) if !pointer_free => (false, false),
                Binding::Key(key) => (keys.pressed(*key), keys.just_pressed(*key)),
                Binding::Mouse(button) => (buttons.pressed(*button), buttons.just_pressed(*button)),
            };
            if pressed {
                self.pressed.insert(*action);
            }
            if just {
                self.just_pressed.insert(*action);
            }
        }
    }
}

pub fn read(
    keys: Res<ButtonInput<KeyCode>>,
    buttons: Res<ButtonInput<MouseButton>>,
    mut wheel: MessageReader<MouseWheel>,
    mut contexts: EguiContexts,
    bindings: Res<Bindings>,
    mut actions: ResMut<Actions>,
) {
    let (pointer, typing) = contexts
        .ctx_mut()
        .map(|ctx| (crate::ui::pointer_captured(ctx), ctx.wants_keyboard_input()))
        .unwrap_or_default();
    actions.update(&bindings, &keys, &buttons, typing, !pointer);
    let lines = wheel
        .read()
        .map(|e| match e.unit {
            MouseScrollUnit::Line => e.y,
            MouseScrollUnit::Pixel => e.y / 40.0,
        })
        .sum::<f32>();
    actions.wheel = if pointer { 0.0 } else { lines };
}

/// egui moves keyboard focus between widgets with Tab and the arrow keys, and
/// a focused widget takes every key from the game. Those keys are the game's,
/// so egui sees them only once a widget has been focused some other way.
pub fn keep_navigation_keys(
    mut contexts: Query<(&mut bevy_egui::EguiInput, &mut bevy_egui::EguiContext)>,
) {
    for (mut input, mut context) in &mut contexts {
        keep_from(context.get_mut(), &mut input.events);
    }
}

fn keep_from(ctx: &bevy_egui::egui::Context, events: &mut Vec<bevy_egui::egui::Event>) {
    if ctx.memory(|memory| memory.focused().is_none()) {
        events.retain(|event| !navigates(event));
    }
}

fn navigates(event: &bevy_egui::egui::Event) -> bool {
    use bevy_egui::egui::{Event, Key};
    matches!(
        event,
        Event::Key {
            key: Key::Tab | Key::ArrowUp | Key::ArrowDown | Key::ArrowLeft | Key::ArrowRight,
            ..
        }
    )
}

#[cfg(test)]
mod tests;
