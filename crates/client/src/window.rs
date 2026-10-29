//! The primary window's mode. F11, the debug window's `settings` tab and
//! `set_fullscreen` switch borderless fullscreen on the window's monitor.
use crate::actions::UiCommand;
use crate::input::{Action, Actions};
use bevy::prelude::*;
use bevy::window::{MonitorSelection, PrimaryWindow, WindowMode};

pub fn is_fullscreen(window: &Window) -> bool {
    window.mode != WindowMode::Windowed
}

/// The mode for `on`, so both the switch and the smoke test's start agree.
pub fn mode(on: bool) -> WindowMode {
    if on {
        WindowMode::BorderlessFullscreen(MonitorSelection::Current)
    } else {
        WindowMode::Windowed
    }
}

/// The fullscreen commands; `actions::apply` leaves them to this.
pub fn fullscreen(
    actions: Res<Actions>,
    mut commands: MessageReader<UiCommand>,
    mut windows: Query<&mut Window, With<PrimaryWindow>>,
) {
    let Ok(mut window) = windows.single_mut() else {
        return;
    };
    let mut on = is_fullscreen(&window);
    for command in commands.read() {
        if let UiCommand::SetFullscreen { on: wanted } = command {
            on = *wanted;
        }
    }
    if actions.just_pressed(Action::Fullscreen) {
        on = !on;
    }
    if is_fullscreen(&window) != on {
        window.mode = mode(on);
    }
}
