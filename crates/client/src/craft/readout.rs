//! Live craft facts for the developer API and inspector. These are derived
//! from the displayed state, never cached as simulation state.
use super::*;
use schemars::JsonSchema;
use serde::Serialize;

/// Every craft's developer readout, as `ns ship state` shows it. Its schema
/// drives the TUI's state panel: every top-level craft field names a display
/// group, and every number its unit (`""` when it has none).
#[derive(Serialize, JsonSchema)]
pub struct FleetReadout {
    pub craft: Vec<CraftReadout>,
    /// The clock is held while physics catches up or systems load.
    pub held_clock: bool,
    /// The fixed physics step.
    #[schemars(extend("x-unit" = "s"))]
    pub step_seconds: f64,
    pub system_loading_error: Option<String>,
    pub system_loading: super::travel::LoadingStatus,
}

/// An order kept for a later step; `ends_from` marks a timed order's end.
#[derive(Serialize, JsonSchema)]
pub struct PendingOrder {
    /// Seconds until it applies.
    #[schemars(extend("x-unit" = "s"))]
    pub in_s: f64,
    #[serde(flatten)]
    pub scheduled: space_simulation::craft::voyage::ScheduledOrder,
}

#[derive(Serialize, JsonSchema)]
pub struct CraftReadout {
    #[schemars(extend("x-group" = "ship"))]
    pub id: String,
    #[schemars(extend("x-group" = "ship"))]
    pub name: String,
    #[schemars(extend("x-group" = "ship"))]
    pub kind: space_simulation::craft::CraftKind,
    /// What the position is measured from.
    #[schemars(extend("x-group" = "position"))]
    pub frame: Frame,
    /// Simulated time at the craft's latest physics step.
    #[schemars(extend("x-group" = "ship", "x-unit" = "s"))]
    pub seconds: f64,
    /// The world step the state describes.
    #[schemars(extend("x-group" = "ship"))]
    pub tick: i64,
    /// Position in frame axes.
    #[schemars(extend("x-group" = "position", "x-unit" = "m"))]
    pub position_m: [f64; 3],
    /// Ordinary velocity in frame axes; warp is separate.
    #[schemars(extend("x-group" = "position", "x-unit" = "m/s"))]
    pub velocity_ms: [f64; 3],
    /// Ordinary speed in the frame; warp is separate.
    #[schemars(extend("x-group" = "position", "x-unit" = "km/s"))]
    pub speed_kms: f64,
    /// Held drive inputs.
    #[schemars(extend("x-group" = "drives"))]
    pub input: space_simulation::craft::Input,
    /// The body the orbit is described around, inside a system.
    #[schemars(extend("x-group" = "position"))]
    pub around: Option<Around>,
    /// Attitude quaternion in frame axes.
    #[schemars(extend("x-group" = "attitude", "x-unit" = ""))]
    pub orientation_xyzw: [f64; 4],
    /// Nose direction in frame axes.
    #[schemars(extend("x-group" = "attitude", "x-unit" = ""))]
    pub forward: [f64; 3],
    /// Where the pointing pilot is aiming, in frame axes.
    #[schemars(extend("x-group" = "attitude", "x-unit" = ""))]
    pub target_direction: Option<[f64; 3]>,
    /// Angle between the nose and the pointing target.
    #[schemars(extend("x-group" = "attitude", "x-unit" = "°"))]
    pub pointing_error_deg: Option<f64>,
    /// Roof direction in frame axes.
    #[schemars(extend("x-group" = "attitude", "x-unit" = ""))]
    pub up: [f64; 3],
    /// Turn rate about pitch, yaw and roll; in warp, the bubble's.
    #[schemars(extend("x-group" = "attitude", "x-unit" = "rad/s"))]
    pub spin_rads: [f64; 3],
    /// Thruster limits.
    #[schemars(extend("x-group" = "drives"))]
    pub thrusters: space_simulation::craft::Thrusters,
    /// The attitude pilot.
    #[schemars(extend("x-group" = "attitude"))]
    pub autopilot: space_simulation::craft::AttitudePilot,
    /// The shown coast path, outside warp in a system.
    #[schemars(extend("x-group" = "prediction"))]
    pub prediction: PredictionReadout,
    #[schemars(extend("x-group" = "warp"))]
    pub warp: space_simulation::craft::warp::Warp,
    /// The plotted warp route, kept after it ends.
    #[schemars(extend("x-group" = "route"))]
    pub route: Option<space_simulation::craft::Route>,
    /// Orders scheduled for later steps.
    #[schemars(extend("x-group" = "drives"))]
    pub pending_orders: Vec<PendingOrder>,
    /// Which assists limit warp; manual warp has none.
    #[schemars(extend("x-group" = "warp"))]
    pub warp_mode: &'static str,
    /// Entering systems, or why the clock is held.
    #[schemars(extend("x-group" = "position"))]
    pub arrival: String,
    /// Galactic position from the galactic centre.
    #[schemars(title = "galactic position")]
    #[schemars(extend("x-group" = "position", "x-unit" = "pc"))]
    pub position_pc: [f64; 3],
}

#[derive(Serialize, JsonSchema)]
pub struct Around {
    pub id: String,
    pub kind: space_simulation::craft::ReferenceKind,
    #[schemars(extend("x-unit" = "km"))]
    pub altitude_km: f64,
    /// Speed relative to the body.
    #[schemars(extend("x-unit" = "km/s"))]
    pub speed_kms: f64,
    /// The two-body orbit around this body alone.
    pub two_body_orbit: TwoBodyOrbit,
}

#[derive(Serialize, JsonSchema)]
pub struct TwoBodyOrbit {
    #[schemars(extend("x-unit" = ""))]
    pub eccentricity: f64,
    /// Periapsis and apoapsis altitude; none when escaping.
    #[schemars(extend("x-unit" = "km"))]
    pub apsides_altitude_km: Option<[f64; 2]>,
    #[schemars(extend("x-unit" = "s"))]
    pub period_seconds: Option<f64>,
}

/// Absent fields do not apply: during warp or galactic flight only
/// `current`, `updating` and `status` are given, and while the first
/// prediction works only `updating`.
#[derive(Default, Serialize, JsonSchema)]
pub struct PredictionReadout {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reference: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(extend("x-unit" = "s"))]
    pub age_seconds: Option<f64>,
    /// The shown prediction starts from the craft's current state.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current: Option<bool>,
    pub updating: bool,
    /// Planned for about one period of a bound orbit.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bound: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(extend("x-unit" = "s"))]
    pub horizon_seconds: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(extend("x-unit" = "s"))]
    pub achieved_seconds: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(extend("x-unit" = "s"))]
    pub ahead_seconds: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub samples: Option<usize>,
    /// horizon, truncated, impact:<body> or running.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub events: Option<Vec<PredictedEvent>>,
}

#[derive(Serialize, JsonSchema)]
pub struct PredictedEvent {
    /// periapsis, apoapsis, exit or encounter.
    pub event: &'static str,
    #[schemars(extend("x-unit" = "s"))]
    pub in_seconds: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(extend("x-unit" = "km"))]
    pub altitude_km: Option<f64>,
    /// The body a reference change goes to.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub to: Option<String>,
}

/// The craft's developer readout, as `ns ship state` shows it.
pub fn readout(fleet: &Fleet, seconds: f64) -> serde_json::Value {
    let craft = fleet
        .craft
        .iter()
        .map(|live| {
            let (state, reference) = live.surroundings(seconds);
            let target_direction = match &live.craft.autopilot {
                space_simulation::craft::AttitudePilot::Point { target, .. } => target
                    .direction(
                        live.craft.state(),
                        live.craft.seconds(),
                        &live.field,
                        live.craft.reference.as_deref(),
                    )
                    .map(|v| v.to_array()),
                _ => None,
            };
            let attitude = bevy::math::DQuat::from_array(live.craft.orientation);
            let forward = attitude * -bevy::math::DVec3::Z;
            let pointing_error_deg = target_direction.map(|v| {
                forward
                    .angle_between(bevy::math::DVec3::from_array(v))
                    .to_degrees()
            });
            let around = reference.map(|body| {
                let relative = relative(state, body.state);
                let orbit = space_simulation::craft::Elements::of(relative, body.gm);
                Around {
                    altitude_km: (distance(relative.position, [0.0; 3]) - body.radius_m) / 1_000.0,
                    speed_kms: distance(relative.velocity, [0.0; 3]) / 1_000.0,
                    two_body_orbit: TwoBodyOrbit {
                        eccentricity: orbit.eccentricity,
                        apsides_altitude_km: orbit
                            .apsides()
                            .map(|r| r.map(|v| (v - body.radius_m) / 1000.0)),
                        period_seconds: orbit.period(),
                    },
                    id: body.id,
                    kind: body.kind,
                }
            });
            CraftReadout {
                id: live.craft.id.clone(),
                name: live.craft.name.clone(),
                kind: live.craft.kind,
                frame: live.craft.frame.clone(),
                seconds: live.craft.seconds(),
                tick: live.craft.tick,
                position_m: state.position,
                velocity_ms: state.velocity,
                speed_kms: distance(state.velocity, [0.0; 3]) / 1_000.0,
                input: live.craft.input,
                around,
                orientation_xyzw: live.craft.orientation,
                forward: forward.to_array(),
                target_direction,
                pointing_error_deg,
                up: (attitude * bevy::math::DVec3::Y).to_array(),
                spin_rads: live.craft.spin,
                thrusters: live.craft.thrusters,
                autopilot: live.craft.autopilot.clone(),
                prediction: prediction_readout(live, seconds),
                warp: live.craft.warp.clone(),
                route: live.craft.route.clone(),
                pending_orders: live
                    .pending_orders()
                    .into_iter()
                    .map(|scheduled| PendingOrder {
                        in_s: (scheduled.tick - live.craft.tick) as f64 * STEP_SECONDS,
                        scheduled,
                    })
                    .collect(),
                warp_mode: "unsafe_manual",
                arrival: live.arrival_status(),
                position_pc: live.position_pc(seconds),
            }
        })
        .collect();
    serde_json::to_value(FleetReadout {
        craft,
        held_clock: fleet.held,
        step_seconds: STEP_SECONDS,
        system_loading_error: fleet.charts.error().clone(),
        system_loading: fleet.charts.loading_status(),
    })
    .expect("craft readout")
}

/// The shown coast prediction, for `ns ship state`.
fn prediction_readout(live: &Live, seconds: f64) -> PredictionReadout {
    use space_simulation::craft::predict::End;
    let forecast = &live.forecast;
    if live.craft.warp.engaged || live.star().is_empty() {
        return PredictionReadout {
            current: Some(false),
            status: Some("unavailable during warp or galactic flight"),
            ..Default::default()
        };
    }
    let Some(p) = &forecast.shown else {
        return PredictionReadout {
            updating: forecast.updating(),
            ..Default::default()
        };
    };
    let end = match &p.end {
        Some(End::Horizon) => "horizon".to_owned(),
        Some(End::Truncated) => "truncated".to_owned(),
        Some(End::Impact { body }) => format!("impact:{body}"),
        None => "running".to_owned(),
    };
    let events = upcoming(live, p, seconds)
        .into_iter()
        .map(|event| match event {
            Upcoming::Apsis {
                periapsis,
                seconds: at,
                altitude_m,
            } => PredictedEvent {
                event: if periapsis { "periapsis" } else { "apoapsis" },
                in_seconds: at - seconds,
                altitude_km: Some(altitude_m / 1_000.0),
                to: None,
            },
            Upcoming::Reference {
                exit,
                seconds: at,
                id,
            } => PredictedEvent {
                event: if exit { "exit" } else { "encounter" },
                in_seconds: at - seconds,
                altitude_km: None,
                to: Some(id),
            },
        })
        .collect();
    PredictionReadout {
        status: None,
        reference: Some(p.reference.clone()),
        age_seconds: Some(seconds - p.start_seconds),
        current: Some(forecast.current(&live.craft)),
        updating: forecast.updating(),
        bound: Some(p.bound),
        horizon_seconds: Some(p.horizon_seconds),
        achieved_seconds: Some(p.achieved_seconds()),
        ahead_seconds: Some((p.shown_end(seconds) - seconds).max(0.0)),
        samples: Some(p.samples.len()),
        end: Some(end),
        events: Some(events),
    }
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
    let turning = bevy::math::DVec3::from_array(live.craft.spin)
        .length()
        .to_degrees();
    rows.push((
        "Warp turn rate".into(),
        if live.craft.warp.engaged {
            format!("{turning:.1} / {} °/s", live.craft.warp.turn_rate_dps)
        } else {
            format!("{} °/s at full steering", live.craft.warp.turn_rate_dps)
        },
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
    if let Some(route) = &live.craft.route {
        rows.extend(route_facts(live, galaxy, route));
    }
    if let Some(exit) = &live.craft.warp.last_match {
        rows.push((
            "Last matched dropout".into(),
            format!(
                "{} · {}",
                body_label(live, galaxy, &exit.body),
                if exit.orbit {
                    "rough orbit"
                } else {
                    "body velocity"
                }
            ),
        ));
    }
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

/// The inspector's route facts: target, stage and progress, or how it ended.
fn route_facts(
    live: &Live,
    galaxy: &Galaxy,
    route: &space_simulation::craft::Route,
) -> Vec<(String, String)> {
    use crate::format::{duration_text, km_text};
    use space_simulation::craft::{ArrivalMode, route::Stage};
    let arrival = match route.arrival {
        ArrivalMode::Keep => "keep velocity",
        ArrivalMode::Match => "match velocity",
        ArrivalMode::Orbit => "rough orbit",
    };
    let mut rows = vec![(
        "Route".into(),
        format!("{} · {arrival}", body_label(live, galaxy, &route.target)),
    )];
    if !route.active() {
        rows.push(("Route ended".into(), route.end.clone().unwrap_or_default()));
        return rows;
    }
    let stage = match route.stage {
        Stage::Plotting => "Loading the destination",
        Stage::Align => "Turning to target",
        Stage::Cruise => "Cruise",
        Stage::Approach => "Approach",
        Stage::Arrived | Stage::Released => "",
    };
    rows.push(("Route stage".into(), stage.into()));
    let mut distance =
        km_text((route.distance_m - route.standoff_m).max(0.0) / 1_000.0) + " to standoff";
    if route.detour_m > 0.0 {
        distance += &format!(" · detour {}", km_text(route.detour_m / 1_000.0));
    }
    rows.push(("Route distance".into(), distance));
    if let Some(around) = &route.around {
        rows.push(("Route around".into(), body_label(live, galaxy, around)));
    }
    let mut eta = format!(
        "{} · trip {}",
        duration_text(route.eta_s),
        duration_text(route.trip_s)
    );
    if route.late_s > space_simulation::craft::route::LATE_S {
        eta += &format!(" · {} late", duration_text(route.late_s));
        if let Some(cause) = &route.slowed_by {
            eta += &format!(", {cause}");
        }
    }
    if route.capped {
        eta += " · top speed";
    }
    rows.push(("Route ETA".into(), eta));
    rows
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
    } else if forecast.refreshing(&live.craft) {
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
