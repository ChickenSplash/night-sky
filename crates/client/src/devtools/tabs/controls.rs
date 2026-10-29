//! The `controls` tab: camera, piloting, selection, time and console
//! shortcuts. Bound actions are labelled from the binding table.
use crate::input::{Action::*, Bindings};
use crate::ui::terminal;
use bevy_egui::egui;

pub fn controls(ui: &mut egui::Ui, bindings: &mut Bindings) {
    controls_help(ui, bindings);
    if terminal::check(
        ui,
        bindings.invert_pitch,
        "Invert mouse pitch",
        true,
        "Mouse steering: pull the mouse back to raise the nose, as on a flight stick. Off, mouse up is nose up. Reset on restart.",
    ) {
        bindings.invert_pitch = !bindings.invert_pitch;
    }
}

fn controls_help(ui: &mut egui::Ui, bindings: &Bindings) {
    let keys = |actions: &[crate::input::Action]| bindings.label(actions);
    let with = |a, b| format!("{} + {}", keys(&[a]), keys(&[b]));
    terminal::heading(ui, "controls");
    terminal::facts(
        ui,
        "debug-controls",
        &[
            ("Click", "Select a body, again to visit/follow".into()),
            (&keys(&[Visit]), "Visit the selected star".into()),
            (&keys(&[Look]), "Drag to look around".into()),
            (&keys(&[Orbit]), "Drag to orbit".into()),
            (&with(Alternate, Look), "Drag for field of view".into()),
            ("Scroll", "Adjust movement speed".into()),
            (&format!("{} + scroll", keys(&[Alternate])), "Zoom".into()),
            (&keys(&[Forward, Left, Backward, Right]), "Move".into()),
            (&keys(&[RollLeft, RollRight]), "Roll left / right".into()),
            (&keys(&[Up, Down]), "Up / down".into()),
            (&keys(&[Pause]), "Pause / resume".into()),
            (
                &keys(&[Cancel]),
                "Leave text input, reframe, clear or move up".into(),
            ),
            (&keys(&[Pilot]), "Pilot the selected ship".into()),
            ("F3 / ` / cog", "Toggle debug console".into()),
            (&keys(&[Fullscreen]), "Toggle fullscreen".into()),
            ("Drag set speed", "Adjust movement speed".into()),
            ("Click time rate", "Pause / resume".into()),
            ("Drag time rate", "Adjust simulation speed".into()),
            ("Drag Exposure", "Adjust manual exposure".into()),
            (
                "Drag Compression",
                "Adjust dynamic range compression".into(),
            ),
            (
                "Drag Magnitude limit",
                "Adjust visible stars on release".into(),
            ),
        ],
    );
    terminal::heading(ui, "piloting");
    terminal::facts(
        ui,
        "debug-piloting",
        &[
            (
                &format!("{} or {}", keys(&[Pilot]), keys(&[Cancel])),
                "Stop piloting; the ship keeps its throttle".into(),
            ),
            (
                &keys(&[Forward, Left, Backward, Right]),
                "Fine thrusters".into(),
            ),
            (&keys(&[Up, Down]), "Fine thrusters up / down".into()),
            (
                &keys(&[ThrottleUp, ThrottleDown]),
                "Hold: throttle up / down, below zero is reverse; in warp, speed".into(),
            ),
            (
                &keys(&[FullThrottle, CutThrottle]),
                format!(
                    "Full / cut throttle; {} in warp: slowest speed",
                    keys(&[CutThrottle])
                ),
            ),
            (
                &keys(&[EngageWarp]),
                "Cut thrust and engage warp, or drop out".into(),
            ),
            (
                &keys(&[Steer]),
                "Hold and move: steer, a virtual joystick".into(),
            ),
            (
                &keys(&[PitchUp, PitchDown, YawLeft, YawRight]),
                "Pitch and yaw".into(),
            ),
            (&keys(&[RollLeft, RollRight]), "Roll left / right".into()),
            (&keys(&[StopRotation]), "Stop rotation".into()),
            (&keys(&[Look]), "Drag to look round the ship".into()),
            ("Scroll", "Camera distance".into()),
        ],
    );
}
