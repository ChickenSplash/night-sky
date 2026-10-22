//! Piloting: the keys fly the selected ship instead of the camera, which
//! follows it. Held keys become ship orders as they change, so a ship flown
//! by hand takes the same orders, keyed by step, as the CLI and autopilot give.
use crate::craft::{Fleet, Live, Order};
use crate::input::{Action, Actions, Bindings};
use crate::{Clock, Navigation, presentation::PresentationMode};
use bevy::prelude::*;

/// How fast Shift and Ctrl move the throttle lever, in full throttle per
/// second.
pub const THROTTLE_RATE: f64 = 0.5;

#[derive(Resource, Default)]
pub struct Pilot {
    /// The ship being flown, by ID.
    pub ship: Option<String>,
    /// The held controls last ordered, so orders go only when they change.
    sent: Held,
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
}

/// What the held keys ask of the ship. Fine thrusters do nothing in warp,
/// where thrust is refused; turning steers the bubble there.
pub fn held(actions: &Actions, warp: bool) -> Held {
    use Action::*;
    let axis = |positive, negative| f64::from(actions.axis(positive, negative));
    Held {
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

/// The orders this frame's actions give, against the held controls last
/// ordered.
fn orders(actions: &Actions, live: &Live, sent: Held, seconds: f64) -> (Vec<Order>, Held) {
    let craft = &live.craft;
    let warp = craft.warp.engaged;
    let now = held(actions, warp);
    let mut orders = Vec::new();
    if now.translation != sent.translation {
        let [right, up, forward] = now.translation;
        orders.push(Order::Translate { right, up, forward });
    }
    if now.rotation != sent.rotation {
        let [pitch, yaw, roll] = now.rotation;
        orders.push(Order::Rotate { pitch, yaw, roll });
    }
    if !warp {
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
    if craft.warp.engaged {
        lines.push(format!(
            "Warp {} · thrusters off",
            crate::format::speed_label(craft.warp.actual_ms)
        ));
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
    }
    lines
}

#[cfg(test)]
mod tests;
