//! Live craft facts for the developer API and inspector. These are derived
//! from the displayed state, never cached as simulation state.
use super::*;

/// The craft's developer readout, as `ns ship state` shows it.
pub fn readout(fleet: &Fleet, seconds: f64) -> serde_json::Value {
    use serde_json::json;
    let craft: Vec<_> = fleet
        .craft
        .iter()
        .map(|live| {
            let (state, reference) = live.surroundings(seconds);
            let target_direction = match &live.craft.autopilot {
                space_simulation::craft::AttitudePilot::Point { target, .. } => target
                    .direction(live.craft.state(), live.craft.seconds(), &live.field, live.craft.reference.as_deref())
                    .map(|v| v.to_array()),
                _ => None,
            };
            let forward = bevy::math::DQuat::from_array(live.craft.orientation) * -bevy::math::DVec3::Z;
            let pointing_error_deg = target_direction.map(|v| forward.angle_between(bevy::math::DVec3::from_array(v)).to_degrees());
            let around = reference.map(|body| {
                let relative = relative(state, body.state);
                let orbit = space_simulation::craft::Elements::of(relative, body.gm);
                json!({
                    "id": body.id, "kind": body.kind,
                    "altitude_km": (distance(relative.position, [0.0; 3]) - body.radius_m) / 1_000.0,
                    "speed_kms": distance(relative.velocity, [0.0; 3]) / 1_000.0,
                    "two_body_orbit": {"eccentricity": orbit.eccentricity,
                        "apsides_altitude_km": orbit.apsides().map(|r| r.map(|v| (v - body.radius_m) / 1000.0)),
                        "period_seconds": orbit.period()},
                })
            });
            json!({
                "id": live.craft.id, "name": live.craft.name, "kind": live.craft.kind,
                "frame": live.craft.frame, "seconds": live.craft.seconds(), "tick": live.craft.tick,
                "position_m": state.position, "velocity_ms": state.velocity,
                "speed_kms": distance(state.velocity, [0.0; 3]) / 1_000.0,
                "input": live.craft.input, "around": around,
                "orientation_xyzw": live.craft.orientation,
                "forward": forward.to_array(),
                "target_direction": target_direction, "pointing_error_deg": pointing_error_deg,
                "up": (bevy::math::DQuat::from_array(live.craft.orientation) * bevy::math::DVec3::Y).to_array(),
                "spin_rads": live.craft.spin,
                "thrusters": live.craft.thrusters,
                "autopilot": live.craft.autopilot,
                "prediction": prediction_readout(live, seconds),
                "warp": live.craft.warp,
                "pending_orders": live.pending_orders(),
                "warp_mode": "unsafe_manual",
                "arrival": live.arrival_status(),
                "position_pc": live.position_pc(seconds),
            })
        })
        .collect();
    json!({"craft": craft, "held_clock": fleet.held, "step_seconds": STEP_SECONDS,
        "system_loading_error": fleet.charts.error})
}

/// The shown coast prediction, for `ns ship state`.
fn prediction_readout(live: &Live, seconds: f64) -> serde_json::Value {
    use serde_json::json;
    use space_simulation::craft::predict::End;
    let forecast = &live.forecast;
    if live.craft.warp.engaged || live.star().is_empty() {
        return json!({"current": false, "updating": false, "status": "unavailable during warp or galactic flight"});
    }
    let Some(p) = &forecast.shown else {
        return json!({"updating": forecast.updating()});
    };
    let end = match &p.end {
        Some(End::Horizon) => "horizon".to_owned(),
        Some(End::Truncated) => "truncated".to_owned(),
        Some(End::Impact { body }) => format!("impact:{body}"),
        None => "running".to_owned(),
    };
    let events: Vec<_> = upcoming(live, p, seconds)
        .into_iter()
        .map(|event| match event {
            Upcoming::Apsis {
                periapsis,
                seconds: at,
                altitude_m,
            } => json!({
                "event": if periapsis { "periapsis" } else { "apoapsis" },
                "in_seconds": at - seconds, "altitude_km": altitude_m / 1_000.0,
            }),
            Upcoming::Reference {
                exit,
                seconds: at,
                id,
            } => json!({
                "event": if exit { "exit" } else { "encounter" },
                "in_seconds": at - seconds, "to": id,
            }),
        })
        .collect();
    json!({
        "reference": p.reference,
        "age_seconds": seconds - p.start_seconds,
        "current": forecast.current(&live.craft),
        "updating": forecast.updating(),
        "bound": p.bound,
        "horizon_seconds": p.horizon_seconds,
        "achieved_seconds": p.achieved_seconds(),
        "ahead_seconds": (p.shown_end(seconds) - seconds).max(0.0),
        "samples": p.samples.len(),
        "end": end,
        "events": events,
    })
}

/// The inspector's title, facts and note for a craft, live at `seconds`.
pub fn facts(live: &Live, galaxy: &Galaxy, seconds: f64) -> (String, Vec<(String, String)>) {
    use crate::format::{duration_text, km_text, speed_label};
    let (state, reference) = live.surroundings(seconds);
    let mut rows = vec![("Type".to_owned(), live.craft.kind.label().to_owned())];
    rows.push((
        "Warp".into(),
        if live.craft.warp.engaged {
            "Engaged".into()
        } else {
            "Off".into()
        },
    ));
    rows.push((
        "Warp requested / actual".into(),
        format!(
            "{} / {}",
            speed_label(live.craft.warp.requested_ms),
            speed_label(live.craft.warp.actual_ms)
        ),
    ));
    rows.push((
        "Warp gravity".into(),
        format!("{:?}", live.craft.warp.gravity),
    ));
    rows.push((
        "Warp mode".into(),
        "Unsafe manual (no collision protection)".into(),
    ));
    rows.push(("Arrival".into(), live.arrival_status()));
    if let Some(reason) = &live.craft.warp.stop_reason {
        rows.push(("Warp stopped".into(), reason.clone()));
    }
    let key = space_simulation::multiple::system_key(live.star());
    rows.push((
        "System".into(),
        if key.is_empty() {
            "Galactic space".into()
        } else {
            crate::galaxy_nav::label(galaxy, key)
        },
    ));
    if let Some(body) = reference {
        let relative = relative(state, body.state);
        let r = distance(relative.position, [0.0; 3]);
        let v = distance(relative.velocity, [0.0; 3]);
        rows.push(("Around".into(), body_label(live, galaxy, &body.id)));
        rows.push(("Altitude".into(), km_text((r - body.radius_m) / 1_000.0)));
        rows.push(("Speed".into(), speed_label(v)));
        let orbit = space_simulation::craft::Elements::of(relative, body.gm);
        match orbit.apsides() {
            Some([low, high]) => {
                let height = |r: f64| km_text((r - body.radius_m) / 1_000.0);
                rows.push(("Two-body periapsis".into(), height(low)));
                rows.push(("Two-body apoapsis".into(), height(high)));
                let period = orbit.period().unwrap_or(f64::INFINITY);
                rows.push(("Two-body period".into(), duration_text(period)));
            }
            None => rows.push(("Two-body orbit".into(), "Escaping".into())),
        }
    }
    rows.extend(prediction_facts(live, galaxy, seconds));
    let thrust = distance(live.craft.input.thrust, [0.0; 3]);
    rows.push((
        "Thrust".into(),
        if thrust > 0.0 {
            format!("{thrust:.2} m/s²")
        } else {
            "Off".into()
        },
    ));
    rows.push((
        "Throttle".into(),
        format!("{:+.1}%", live.craft.input.controls.throttle * 100.0),
    ));
    rows.push((
        "Spin (pitch/yaw/roll)".into(),
        live.craft
            .spin
            .map(|v| format!("{:+.2}", v.to_degrees()))
            .join(" / ")
            + " °/s",
    ));
    rows.push((
        "Attitude pilot".into(),
        match &live.craft.autopilot {
            space_simulation::craft::AttitudePilot::Manual => "Manual".into(),
            space_simulation::craft::AttitudePilot::Stop => "Stop rotation".into(),
            space_simulation::craft::AttitudePilot::Point { target, .. } => format!("{target:?}"),
        },
    ));
    (live.craft.name.clone(), rows)
}

/// The inspector's prediction facts: reference, age and upcoming events.
fn prediction_facts(live: &Live, galaxy: &Galaxy, seconds: f64) -> Vec<(String, String)> {
    use crate::format::{duration_text, km_text};
    use space_simulation::craft::predict::End;
    if live.craft.warp.engaged || live.star().is_empty() {
        return vec![(
            "Prediction".into(),
            "Unavailable during warp or galactic flight".into(),
        )];
    }
    let forecast = &live.forecast;
    let Some(p) = &forecast.shown else {
        return vec![("Prediction".into(), "Working…".into())];
    };
    let state = if !forecast.current(&live.craft) {
        " · out of date"
    } else if forecast.updating() {
        " · updating"
    } else {
        ""
    };
    let mut rows = vec![
        (
            "Prediction".into(),
            format!(
                "{} ahead, around {}{state}",
                duration_text((p.shown_end(seconds) - seconds).max(0.0)),
                body_label(live, galaxy, &p.reference)
            ),
        ),
        (
            "Prediction age".into(),
            duration_text((seconds - p.start_seconds).max(0.0)),
        ),
    ];
    for event in upcoming(live, p, seconds) {
        rows.push(match event {
            Upcoming::Apsis {
                periapsis,
                seconds: at,
                altitude_m,
            } => (
                format!(
                    "Predicted {}",
                    if periapsis { "periapsis" } else { "apoapsis" }
                ),
                format!(
                    "{} in {}",
                    km_text(altitude_m / 1_000.0),
                    duration_text(at - seconds)
                ),
            ),
            Upcoming::Reference {
                exit,
                seconds: at,
                id,
            } => (
                format!("Predicted {}", if exit { "exit" } else { "encounter" }),
                format!(
                    "{} in {}",
                    body_label(live, galaxy, &id),
                    duration_text(at - seconds)
                ),
            ),
        });
    }
    if let Some(End::Impact { body }) = &p.end
        && p.end_seconds() >= seconds
    {
        rows.push((
            "Predicted impact".into(),
            format!(
                "{} in {}",
                body_label(live, galaxy, body),
                duration_text(p.end_seconds() - seconds)
            ),
        ));
    }
    if let Some(End::Truncated) = &p.end {
        rows.push(("Prediction ends".into(), "Step budget reached".into()));
    }
    rows
}
