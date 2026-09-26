//! Browser bridge for the HTML panels. JavaScript sends `UiCommand`s as JSON
//! through `send_command`, and Rust reports back with window events whose detail is
//! a JSON string:
//! - `space:galaxy` when list data arrives,
//! - `space:state` when navigation or the clock settings change,
//! - `space:tick` about ten times a second, for fast-moving readouts.
use crate::actions::UiCommand;
use crate::scene::CameraPose;
use crate::views::View;
use crate::{Clock, Navigation, Universe};
use bevy::prelude::*;
use serde::Serialize;
use space_simulation::{page_tilt, period_seconds};
use std::cell::RefCell;
use wasm_bindgen::prelude::*;

const TICK_SECONDS: f64 = 0.1;

thread_local! {
    static QUEUE: RefCell<Vec<UiCommand>> = const { RefCell::new(Vec::new()) };
}

/// Queues a command for the next frame. Called from JavaScript between frames.
#[wasm_bindgen]
pub fn send_command(json: &str) -> Result<(), JsError> {
    let command: UiCommand = serde_json::from_str(json)?;
    QUEUE.with_borrow_mut(|queue| queue.push(command));
    Ok(())
}

pub fn install(app: &mut App) {
    app.add_systems(PreUpdate, receive)
        .add_systems(Last, (publish_galaxy, publish_state, publish_tick));
}

fn receive(mut writer: MessageWriter<UiCommand>) {
    QUEUE.with_borrow_mut(|queue| {
        writer.write_batch(queue.drain(..));
    });
}

fn emit(name: &str, detail: &impl Serialize) {
    if let Ok(json) = serde_json::to_string(detail) {
        dispatch(name, &json);
    }
}

fn dispatch(name: &str, json: &str) {
    let Some(window) = web_sys::window() else {
        return;
    };
    let init = web_sys::CustomEventInit::new();
    init.set_detail(&JsValue::from_str(json));
    if let Ok(event) = web_sys::CustomEvent::new_with_event_init_dict(name, &init) {
        let _ = window.dispatch_event(&event);
    }
}

#[derive(Serialize)]
struct TaskSummary<'a> {
    id: &'a str,
    name: &'a str,
    colour: [u8; 3],
    page: u32,
    status: &'a str,
    status_type: &'a str,
    status_order: u32,
    due_date_ms: Option<i64>,
}

#[derive(Serialize)]
struct ListSummary<'a> {
    id: &'a str,
    name: &'a str,
    tasks: Vec<TaskSummary<'a>>,
}

#[derive(Serialize)]
struct GalaxySummary<'a> {
    source: &'a str,
    lists: Vec<ListSummary<'a>>,
}

fn publish_galaxy(universe: Res<Universe>) {
    if !universe.is_changed() {
        return;
    }
    let galaxy = &universe.0;
    emit(
        "space:galaxy",
        &GalaxySummary {
            source: &galaxy.source,
            lists: galaxy
                .systems
                .iter()
                .map(|s| ListSummary {
                    id: &s.id,
                    name: &s.name,
                    tasks: s
                        .tasks
                        .iter()
                        .map(|p| TaskSummary {
                            id: &p.id,
                            name: &p.name,
                            colour: p.colour,
                            page: p.page(),
                            status: &p.status,
                            status_type: &p.status_type,
                            status_order: p.status_order,
                            due_date_ms: p.due_date_ms,
                        })
                        .collect(),
                })
                .collect(),
        },
    );
}

#[derive(Serialize)]
struct DisplayState {
    exaggerate: bool,
    orbits: bool,
    labels: bool,
}

/// Telemetry for the current list and page.
#[derive(Serialize)]
struct SystemState<'a> {
    name: &'a str,
    mass_solar: f64,
    page_tasks: usize,
    tasks: usize,
    star_ly: [f64; 3],
    tilt_deg: f64,
    node_deg: f64,
}

#[derive(Serialize)]
struct TaskDetail<'a> {
    name: &'a str,
    status: &'a str,
    colour: [u8; 3],
    description: &'a str,
    assignees: &'a [String],
    due_date_ms: Option<i64>,
    url: Option<&'a str>,
    semi_major_au: f64,
    eccentricity: f64,
    period_days: f64,
    radius_km: f64,
}

#[derive(Serialize)]
struct State<'a> {
    list: Option<&'a str>,
    page: u32,
    pages: Vec<u32>,
    view: View,
    atlas: bool,
    task: Option<&'a str>,
    following: bool,
    paused: bool,
    rate: f64,
    notice: &'a str,
    display: DisplayState,
    system: Option<SystemState<'a>>,
    selected: Option<TaskDetail<'a>>,
}

fn publish_state(
    universe: Res<Universe>,
    nav: Res<Navigation>,
    clock: Res<Clock>,
    mut last: Local<String>,
) {
    let system = universe.0.systems.get(nav.system);
    let planet = system.and_then(|s| nav.selected.and_then(|i| s.tasks.get(i)));
    let state = State {
        list: system.map(|s| s.id.as_str()),
        page: nav.page,
        pages: system.map(|s| s.pages()).unwrap_or_default(),
        view: nav.view,
        atlas: nav.atlas,
        task: planet.map(|p| p.id.as_str()),
        following: nav.following,
        paused: clock.0.paused,
        rate: clock.0.rate,
        notice: &nav.notice,
        display: DisplayState {
            exaggerate: nav.exaggerate,
            orbits: nav.orbits,
            labels: nav.labels,
        },
        system: system.map(|s| {
            let [inclination, node, _] = page_tilt(&s.id, nav.page);
            SystemState {
                name: &s.name,
                mass_solar: s.mass_solar,
                page_tasks: s.on_page(nav.page).count(),
                tasks: s.tasks.len(),
                star_ly: crate::scene::page_origin_ly(s, nav.page).to_array(),
                tilt_deg: inclination.to_degrees(),
                node_deg: node.to_degrees(),
            }
        }),
        selected: planet.zip(system).map(|(p, s)| TaskDetail {
            name: &p.name,
            status: &p.status,
            colour: p.colour,
            description: &p.description,
            assignees: &p.assignees,
            due_date_ms: p.due_date_ms,
            url: p.url.as_deref(),
            semi_major_au: p.orbit.semi_major_au,
            eccentricity: p.orbit.eccentricity,
            period_days: period_seconds(&p.orbit, s.mass_solar) / 86_400.0,
            radius_km: p.radius_km,
        }),
    };
    // Compare the serialised form, so the event fires only on a real change.
    let Ok(json) = serde_json::to_string(&state) else {
        return;
    };
    if *last != json {
        dispatch("space:state", &json);
        *last = json;
    }
}

#[derive(Serialize)]
struct Tick {
    /// Simulated days since launch.
    days: f64,
    /// Displayed camera distance from its target, in metres.
    range: f64,
    /// Destination camera target relative to the page star, in metres.
    target: [f64; 3],
}

fn publish_tick(
    time: Res<Time>,
    clock: Res<Clock>,
    pose: Res<CameraPose>,
    nav: Res<Navigation>,
    mut next: Local<f64>,
) {
    let now = time.elapsed_secs_f64();
    if now < *next {
        return;
    }
    *next = now + TICK_SECONDS;
    emit(
        "space:tick",
        &Tick {
            days: clock.0.seconds / 86_400.0,
            range: pose.distance,
            target: nav.target.to_array(),
        },
    );
}
