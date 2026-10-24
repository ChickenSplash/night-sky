//! Piloting: the keys fly the selected ship instead of the camera, which
//! follows it. Held keys become ship orders as they change, so a ship flown
//! by hand takes the same orders, keyed by step, as the CLI and autopilot give.
use crate::craft::{Fleet, Live, Order};
use crate::input::{Action, Actions, Bindings};
use crate::{Clock, Navigation, navigation::presentation::PresentationMode};
use bevy::{
    input::mouse::AccumulatedMouseMotion,
    prelude::*,
    window::{CursorGrabMode, CursorOptions, PrimaryWindow},
};
use space_simulation::craft::{WarpOrder, warp::MIN_SPEED};

/// How fast Shift and Ctrl move the throttle lever, in full throttle per
/// second.
pub const THROTTLE_RATE: f64 = 0.5;

/// The virtual joystick's ring, where it reaches full deflection, and its
/// dead zone, in mouse motion (about points).
pub const STICK_RING: f32 = 150.0;
pub const STICK_DEAD_ZONE: f32 = 8.0;

#[derive(Resource, Default)]
pub struct Pilot {
    /// The ship being flown, by ID.
    pub ship: Option<String>,
    /// The virtual joystick, while it is held.
    pub stick: Option<Stick>,
    /// The held controls last ordered, so orders go only when they change.
    sent: Held,
}

/// A virtual joystick: the mouse moves it from where it was pressed, with
/// the pointer hidden and held, so the screen's edge never stops it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Stick {
    /// Where it was pressed, in window logical pixels.
    pub origin: Vec2,
    /// How far the mouse has moved it, screen down positive, within the ring.
    pub offset: Vec2,
}

impl Stick {
    fn moved(&mut self, delta: Vec2) {
        self.offset = (self.offset + delta).clamp_length_max(STICK_RING);
    }

    /// Pitch (nose up) and yaw (nose left), from -1 to 1: none in the dead
    /// zone, full at the ring. Mouse up is nose up unless inverted.
    pub fn deflection(&self, invert_pitch: bool) -> [f32; 2] {
        let length = self.offset.length();
        if length <= STICK_DEAD_ZONE {
            return [0.0, 0.0];
        }
        let share = ((length - STICK_DEAD_ZONE) / (STICK_RING - STICK_DEAD_ZONE)).min(1.0);
        let stick = self.offset / length * share;
        let pitch = if invert_pitch { stick.y } else { -stick.y };
        [pitch, -stick.x]
    }
}

/// Holding the steering button while piloting takes the pointer for the
/// virtual joystick, which holds pitch and yaw like the arrow keys.
pub fn steer(
    mut actions: ResMut<Actions>,
    mut pilot: ResMut<Pilot>,
    bindings: Res<Bindings>,
    motion: Res<AccumulatedMouseMotion>,
    mut window: Query<(&Window, &mut CursorOptions), With<PrimaryWindow>>,
) {
    let held = pilot.flying() && actions.pressed(Action::Steer);
    let Ok((window, mut cursor)) = window.single_mut() else {
        return;
    };
    match (&mut pilot.stick, held) {
        (None, true) => {
            let origin = window.cursor_position().unwrap_or(window.size() / 2.0);
            pilot.stick = Some(Stick {
                origin,
                offset: Vec2::ZERO,
            });
            cursor.visible = false;
            cursor.grab_mode = CursorGrabMode::Locked;
        }
        (Some(stick), true) => stick.moved(motion.delta),
        (Some(_), false) => {
            pilot.stick = None;
            cursor.visible = true;
            cursor.grab_mode = CursorGrabMode::None;
        }
        (None, false) => {}
    }
    if let Some(stick) = pilot.stick {
        let [pitch, yaw] = stick.deflection(bindings.invert_pitch);
        actions.hold_axis(Action::PitchUp, Action::PitchDown, pitch);
        actions.hold_axis(Action::YawLeft, Action::YawRight, yaw);
    }
}

impl Pilot {
    pub fn flying(&self) -> bool {
        self.ship.is_some()
    }
}

/// The controls that act only while their keys are held.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Held {
    /// Fine thrusters: right, up, forward.
    pub translation: [f64; 3],
    /// Pitch (nose up), yaw (nose left) and roll (left).
    pub rotation: [f64; 3],
    /// Warp speed sweep, up positive.
    pub sweep: f64,
}

/// What the held keys ask of the ship. Fine thrusters do nothing in warp,
/// where thrust is refused; turning steers the bubble there, and the
/// throttle keys sweep its speed.
pub fn held(actions: &Actions, warp: bool) -> Held {
    use Action::*;
    let axis = |positive, negative| f64::from(actions.axis(positive, negative));
    Held {
        sweep: if warp {
            axis(ThrottleUp, ThrottleDown)
        } else {
            0.0
        },
        translation: if warp {
            [0.0; 3]
        } else {
            [axis(Right, Left), axis(Up, Down), axis(Forward, Backward)]
        },
        rotation: [
            axis(PitchUp, PitchDown),
            axis(YawLeft, YawRight),
            axis(RollLeft, RollRight),
        ],
    }
}

/// The throttle lever moved `seconds` at `rate` (-1 to 1) of `THROTTLE_RATE`,
/// held within the drives the ship has.
pub fn lever(throttle: f64, rate: f64, seconds: f64, reverse: bool) -> f64 {
    let low = if reverse { -1.0 } else { 0.0 };
    (throttle + rate * THROTTLE_RATE * seconds).clamp(low, 1.0)
}

fn warp(command: WarpOrder) -> Order {
    Order::Warp { command }
}

/// The orders this frame's actions give, against the held controls last
/// ordered. G drops out first, or cuts sublight thrust before engaging, so
/// every order is valid when it applies.
fn orders(actions: &Actions, live: &Live, sent: Held, seconds: f64) -> (Vec<Order>, Held) {
    let craft = &live.craft;
    let toggle = actions.just_pressed(Action::EngageWarp);
    let engaged = craft.warp.engaged;
    let in_warp = engaged != toggle;
    let now = held(actions, in_warp);
    let mut orders = Vec::new();
    let engage = toggle && !engaged;
    if toggle && engaged {
        orders.push(warp(WarpOrder::Disengage));
    }
    if engage {
        let controls = craft.input.controls;
        if controls.throttle != 0.0 || craft.input.thrust != [0.0; 3] {
            orders.push(Order::Throttle { value: 0.0 });
        }
        if controls.translation != [0.0; 3] {
            orders.push(Order::Translate {
                right: 0.0,
                up: 0.0,
                forward: 0.0,
            });
        }
        orders.push(warp(WarpOrder::Engage));
    } else if now.translation != sent.translation {
        let [right, up, forward] = now.translation;
        orders.push(Order::Translate { right, up, forward });
    }
    if now.rotation != sent.rotation {
        let [pitch, yaw, roll] = now.rotation;
        orders.push(Order::Rotate { pitch, yaw, roll });
    }
    if in_warp {
        // Against the ship, not `sent`: dropout ends a sweep by itself.
        let sweep = if engaged { craft.warp.sweep } else { 0.0 };
        if now.sweep != sweep {
            orders.push(warp(WarpOrder::Sweep { rate: now.sweep }));
        }
        if actions.just_pressed(Action::CutThrottle) && craft.warp.requested_ms != MIN_SPEED {
            orders.push(warp(WarpOrder::Set {
                speed_ms: MIN_SPEED,
            }));
        }
    } else if !toggle {
        let throttle = craft.input.controls.throttle;
        let rate = f64::from(actions.axis(Action::ThrottleUp, Action::ThrottleDown));
        let value = if actions.just_pressed(Action::FullThrottle) {
            1.0
        } else if actions.just_pressed(Action::CutThrottle) {
            0.0
        } else {
            lever(throttle, rate, seconds, craft.thrusters.reverse > 0.0)
        };
        if value != throttle {
            orders.push(Order::Throttle { value });
        }
    }
    if actions.just_pressed(Action::StopRotation) {
        orders.push(Order::Stop);
    }
    (orders, now)
}

fn give(live: &mut Live, order: Order) {
    if let Err(err) = live.voyage.order(live.craft.tick, order) {
        warn!(target: "craft", "Pilot order refused: {err}");
    }
}

/// Stop piloting. The ship keeps its throttle, warp and spin; only the
/// controls held by key let go.
fn leave(pilot: &mut Pilot, fleet: &mut Fleet) {
    let sent = std::mem::take(&mut pilot.sent);
    if let Some(id) = pilot.ship.take()
        && let Some(live) = fleet.get_mut(&id)
    {
        info!(target: "craft", "Stopped piloting {id}");
        if sent.translation != [0.0; 3] {
            give(
                live,
                Order::Translate {
                    right: 0.0,
                    up: 0.0,
                    forward: 0.0,
                },
            );
        }
        if sent.rotation != [0.0; 3] {
            give(
                live,
                Order::Rotate {
                    pitch: 0.0,
                    yaw: 0.0,
                    roll: 0.0,
                },
            );
        }
        if sent.sweep != 0.0 && live.craft.warp.sweep != 0.0 {
            give(live, warp(WarpOrder::Sweep { rate: 0.0 }));
        }
    }
}

/// Tab pilots the selected ship and following it, at 1× time; Tab or Esc
/// stops. Losing the ship, its selection or simulation view stops too.
pub fn fly(
    time: Res<Time>,
    mut actions: ResMut<Actions>,
    mut pilot: ResMut<Pilot>,
    mut nav: ResMut<Navigation>,
    mut clock: ResMut<Clock>,
    mut fleet: ResMut<Fleet>,
) {
    let selected = nav
        .selected_craft()
        .filter(|id| fleet.get(id).is_some())
        .map(str::to_owned);
    if pilot.flying()
        && (pilot.ship != selected || nav.presentation_mode != PresentationMode::Simulation)
    {
        leave(&mut pilot, &mut fleet);
    }
    if actions.just_pressed(Action::Pilot) {
        actions.consume(Action::Pilot);
        if pilot.flying() {
            leave(&mut pilot, &mut fleet);
        } else if let Some(id) = selected {
            if !nav.following {
                nav.follow_craft();
            }
            info!(target: "craft", "Piloting {id}");
            pilot.ship = Some(id);
        }
    } else if pilot.flying() && actions.just_pressed(Action::Cancel) {
        actions.consume(Action::Cancel);
        leave(&mut pilot, &mut fleet);
    }
    let Some(id) = pilot.ship.clone() else {
        return;
    };
    clock.0.rate = 1.0;
    let Some(live) = fleet.get_mut(&id) else {
        return;
    };
    let (orders, sent) = orders(&actions, live, pilot.sent, time.delta_secs_f64());
    pilot.sent = sent;
    for order in orders {
        give(live, order);
    }
}

/// The pilot's readout lines: what is flown and how to stop, then its drives.
pub fn readout(live: &Live, bindings: &Bindings) -> Vec<String> {
    let craft = &live.craft;
    let mut lines = vec![format!(
        "Piloting {} · {} or {} to stop",
        craft.name,
        bindings.label(&[Action::Pilot]),
        bindings.label(&[Action::Cancel]),
    )];
    let controls = craft.input.controls;
    let speed = crate::format::speed_label;
    let warp_key = bindings.label(&[Action::EngageWarp]);
    if craft.warp.engaged {
        let sweep = match craft.warp.sweep {
            s if s > 0.0 => " · rising",
            s if s < 0.0 => " · falling",
            _ => "",
        };
        lines.push(format!(
            "Warp {} · set {}{sweep}",
            speed(craft.warp.actual_ms),
            speed(craft.warp.requested_ms)
        ));
        lines.push(format!("{warp_key} drops out · thrusters off"));
    } else {
        let throttle = match controls.throttle {
            t if t < 0.0 => format!("Reverse {:.0}%", -t * 100.0),
            t => format!("Throttle {:.0}%", t * 100.0),
        };
        let fine = if controls.translation == [0.0; 3] {
            ""
        } else {
            " · fine thrust"
        };
        lines.push(format!("{throttle}{fine}"));
        let spin = craft.spin.iter().map(|v| v * v).sum::<f64>().sqrt();
        lines.push(format!("Spin {:.1} °/s", spin.to_degrees()));
        let cuts = if craft.check_engage().is_ok() {
            ""
        } else {
            " cuts thrust and"
        };
        lines.push(format!(
            "{warp_key}{cuts} warps at {}",
            speed(craft.warp.requested_ms)
        ));
    }
    lines
}

#[cfg(test)]
mod tests;
