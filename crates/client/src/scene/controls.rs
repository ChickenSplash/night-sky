//! Mouse and keyboard camera controls.
use super::{
    camera::{CameraPose, CameraRig},
    frames::page_frame,
};
use crate::presentation::PresentationMode;
use crate::{Clock, Navigation, Universe};
use bevy::{
    input::mouse::{AccumulatedMouseMotion, MouseWheel},
    math::{DQuat, DVec3},
    prelude::*,
};
use bevy_egui::EguiContexts;
use space_simulation::LY_METRES;

pub const MIN_SPEED: f64 = 1.0;
pub const MAX_SPEED: f64 = space_simulation::craft::warp::MAX_SPEED;
/// Closest camera zoom (Shift + scroll), in metres, whatever is in view. The
/// render scale comes down with it (`scale_floor`).
pub const MIN_DISTANCE: f64 = 1.0;

pub(super) fn scroll_speed(speed: f64, scroll: f32) -> f64 {
    (speed * (f64::from(scroll) * 0.35).exp()).clamp(MIN_SPEED, MAX_SPEED)
}

/// Right-drag moves the eye around a fixed target. That is real camera motion,
/// but should not steer the travel-direction marker. Keyboard travel can still
/// happen during the drag, so retain its precise displacement separately.
#[derive(Resource, Default)]
pub struct CameraMotion {
    pub(super) orbiting: bool,
    pub(super) travel_metres: DVec3,
    /// Whether these controls alone placed the eye this frame (free flight,
    /// not following). Velocity then uses `moved_metres`: differencing eyes
    /// far from their origin would add rounding as large as a slow step.
    pub(super) steered: bool,
    /// Every eye displacement applied here, keyboard travel and Shift + scroll
    /// zoom, in galactic axes.
    pub(super) moved_metres: DVec3,
    /// Keyboard controls, eased towards the pressed keys so starts, stops and
    /// turns take a moment: travel in camera axes as a fraction of
    /// `move_speed` (x, y, z), and roll as a fraction of `ROLL_SPEED` (w).
    pub(super) thrust: Vec4,
}

/// Time constant for keyboard controls: about 95% of a change within 0.15 s.
const THRUST_EASE_SECONDS: f32 = 0.05;
/// Q/E roll rate at full thrust, in radians per second.
const ROLL_SPEED: f32 = 1.5;

pub(super) fn ease_thrust(thrust: Vec4, wanted: Vec4, dt: f32) -> Vec4 {
    let eased = wanted + (thrust - wanted) * (-dt / THRUST_EASE_SECONDS).exp();
    if eased.distance_squared(wanted) < 1e-6 {
        wanted
    } else {
        eased
    }
}

#[allow(clippy::too_many_arguments)]
pub fn controls(
    time: Res<Time>,
    input: (Res<ButtonInput<KeyCode>>, Res<ButtonInput<MouseButton>>),
    motion: Res<AccumulatedMouseMotion>,
    mut wheel: MessageReader<MouseWheel>,
    mut contexts: EguiContexts,
    rig: Res<CameraRig>,
    pose: Res<CameraPose>,
    universe: Res<Universe>,
    mut nav: ResMut<Navigation>,
    mut clock: ResMut<Clock>,
    mut camera_motion: ResMut<CameraMotion>,
    mut ui: MessageWriter<crate::actions::UiCommand>,
) {
    camera_motion.orbiting = false;
    camera_motion.travel_metres = DVec3::ZERO;
    camera_motion.moved_metres = DVec3::ZERO;
    let (keys, buttons) = input;
    let scroll = wheel
        .read()
        .map(|e| match e.unit {
            bevy::input::mouse::MouseScrollUnit::Line => e.y,
            bevy::input::mouse::MouseScrollUnit::Pixel => e.y / 40.0,
        })
        .sum::<f32>();
    let (pointer, keyboard) = contexts
        .ctx_mut()
        .map(|ctx| (crate::ui::pointer_captured(ctx), ctx.wants_keyboard_input()))
        .unwrap_or_default();
    // Flights are short; camera input resumes when they land. Constellations
    // orbit their auto-framed centre, but cannot zoom or translate.
    let free = nav.presentation_mode == PresentationMode::Simulation && !rig.flying();
    let rotating = matches!(
        nav.presentation_mode,
        PresentationMode::Simulation | PresentationMode::Constellation
    ) && !rig.flying();
    if !pointer && rotating {
        let shift = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
        if buttons.pressed(MouseButton::Left) && shift {
            drag_fov(&mut nav, motion.delta.y);
        } else if buttons.pressed(MouseButton::Left) && free {
            let tilt = nav
                .system(&universe.0)
                .map_or(DQuat::IDENTITY, |s| page_frame(s, nav.frame_page(s)));
            drag_look(&mut nav, motion.delta, Some(tilt.inverse() * pose.eye));
        } else if buttons.pressed(MouseButton::Right) {
            nav.eye = None;
            orbit(&mut nav, motion.delta);
            camera_motion.orbiting = free;
        }
        if free && !shift {
            nav.move_speed = scroll_speed(nav.move_speed, scroll);
        } else if free && shift && scroll != 0.0 {
            let tilt = nav
                .system(&universe.0)
                .map_or(DQuat::IDENTITY, |s| page_frame(s, nav.frame_page(s)));
            let old_eye = nav.eye.unwrap_or_else(|| tilt.inverse() * pose.eye);
            let old_distance = nav.distance;
            nav.distance = (nav.distance * (-f64::from(scroll) * 0.12).exp())
                .clamp(MIN_DISTANCE, 40_000.0 * LY_METRES);
            let zoom = (nav.rotation * Vec3::Z).as_dvec3() * (nav.distance - old_distance);
            camera_motion.moved_metres += tilt * zoom;
            let eye = old_eye + zoom;
            nav.eye = Some(eye);
            nav.target = eye - (nav.rotation * Vec3::Z).as_dvec3() * nav.distance;
        }
    }
    let mut wanted = Vec4::ZERO;
    if !keyboard {
        if keys.just_pressed(KeyCode::Enter) {
            crate::galaxy_nav::visit_selected(&mut nav, &universe.0);
        }
        if keys.just_pressed(KeyCode::Space) {
            clock.0.paused = !clock.0.paused;
        }
        if keys.just_pressed(KeyCode::Escape) {
            ui.write(crate::actions::UiCommand::Up);
        }
        let axis = |positive, negative| {
            f64::from(keys.pressed(positive) as u8) - f64::from(keys.pressed(negative) as u8)
        };
        let movement = DVec3::new(
            axis(KeyCode::KeyD, KeyCode::KeyA),
            axis(KeyCode::KeyR, KeyCode::KeyF),
            axis(KeyCode::KeyS, KeyCode::KeyW),
        );
        let roll_input = axis(KeyCode::KeyQ, KeyCode::KeyE) as f32;
        if roll_input != 0.0 && free {
            nav.following = false;
            wanted.w = roll_input;
        }
        if movement.length_squared() > 0.0 && free {
            nav.following = false;
            wanted = movement.normalize().as_vec3().extend(wanted.w);
        }
    }
    // Flights, tracking and presets take the camera over: no coasting into them.
    camera_motion.steered = free && !nav.following;
    camera_motion.thrust = if camera_motion.steered {
        ease_thrust(camera_motion.thrust, wanted, time.delta_secs())
    } else {
        Vec4::ZERO
    };
    if camera_motion.thrust.w != 0.0 {
        roll(
            &mut nav,
            camera_motion.thrust.w * ROLL_SPEED * time.delta_secs(),
        );
    }
    let travel = camera_motion.thrust.truncate();
    if travel != Vec3::ZERO {
        let tilt = nav
            .system(&universe.0)
            .map_or(DQuat::IDENTITY, |s| page_frame(s, nav.frame_page(s)));
        let rotation = nav.rotation;
        let step = (rotation * travel).as_dvec3() * nav.move_speed * time.delta_secs_f64();
        camera_motion.travel_metres += tilt * step;
        camera_motion.moved_metres += tilt * step;
        let eye = nav.eye.get_or_insert_with(|| tilt.inverse() * pose.eye);
        *eye += step;
        nav.target = *eye - (rotation * Vec3::Z).as_dvec3() * nav.distance;
    }
}

/// Roll in camera-local space, not around the galaxy's vertical axis. Rolling
/// leaves the line of sight and the camera's position unchanged at any pitch.
pub(super) fn roll(nav: &mut Navigation, angle: f32) {
    nav.rotation = (nav.rotation * Quat::from_rotation_z(angle)).normalize();
    nav.following = false;
}

pub(super) fn drag_look(nav: &mut Navigation, delta: Vec2, displayed_eye: Option<DVec3>) {
    if delta == Vec2::ZERO {
        return;
    }
    let eye = *nav.eye.get_or_insert_with(|| {
        displayed_eye
            .unwrap_or_else(|| nav.target + (nav.rotation * Vec3::Z).as_dvec3() * nav.distance)
    });
    // Both axes are local to the *current camera*. At a pole or upside down,
    // screen right is not galactic right; world-axis yaw behaves like roll there.
    let turn = Quat::from_rotation_y(-delta.x * 0.005) * Quat::from_rotation_x(-delta.y * 0.005);
    nav.rotation = (nav.rotation * turn).normalize();
    nav.target = eye - (nav.rotation * Vec3::Z).as_dvec3() * nav.distance;
    nav.following = false;
}

/// Shift + left-drag: up narrows the view (zooms in), down widens it.
pub(super) fn drag_fov(nav: &mut Navigation, delta_y: f32) {
    nav.set_fov(nav.fov_deg + delta_y * 0.15);
}

/// Right-drag keeps its target fixed and turns about the camera's own screen
/// axes, so it follows the view when upside down or rolled.
pub(super) fn orbit(nav: &mut Navigation, delta: Vec2) {
    if delta == Vec2::ZERO {
        return;
    }
    let turn = Quat::from_rotation_y(-delta.x * 0.005) * Quat::from_rotation_x(-delta.y * 0.005);
    nav.rotation = (nav.rotation * turn).normalize();
}
