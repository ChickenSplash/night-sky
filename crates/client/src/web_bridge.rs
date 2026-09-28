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
use space_simulation::period_seconds;
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
                        .tasks()
                        .map(|(i, t)| TaskSummary {
                            id: &t.id,
                            name: &t.name,
                            colour: t.colour,
                            page: s.planets[i].page(),
                            status: &t.status,
                            status_type: &t.status_type,
                            status_order: t.status_order,
                            due_date_ms: t.due_date_ms,
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
    markers: bool,
    realistic: bool,
    physical: bool,
    wireframe: bool,
}

/// Telemetry for the visited system and page.
#[derive(Serialize)]
struct SystemState<'a> {
    name: &'a str,
    star: Option<&'a space_shared::Star>,
    page_tasks: usize,
    tasks: usize,
    star_ly: [f64; 3],
    tilt_deg: f64,
    node_deg: f64,
}

/// The selected planet; the task fields are absent for a planet without one.
#[derive(Serialize)]
struct TaskDetail<'a> {
    name: &'a str,
    designation: &'a str,
    status: Option<&'a str>,
    colour: [u8; 3],
    description: Option<&'a str>,
    assignees: Option<&'a [String]>,
    due_date_ms: Option<i64>,
    url: Option<&'a str>,
    semi_major_au: f64,
    eccentricity: f64,
    period_days: f64,
    radius_km: f64,
    mass_earth: f64,
    class: &'a str,
    resonance: Option<[u32; 2]>,
    spin: space_shared::Spin,
}

#[derive(Serialize)]
struct State<'a> {
    list: Option<&'a str>,
    page: u32,
    pages: Vec<u32>,
    view: View,
    task: Option<&'a str>,
    following: bool,
    paused: bool,
    rate: f64,
    notice: &'a str,
    display: DisplayState,
    system: Option<SystemState<'a>>,
    selected: Option<TaskDetail<'a>>,
    picked: Option<Picked>,
}

/// The picked star. Page and page count are absent unless it is a list page.
#[derive(Serialize)]
struct Picked {
    name: String,
    page: Option<u32>,
    pages: Option<usize>,
}

fn publish_state(
    universe: Res<Universe>,
    nav: Res<Navigation>,
    clock: Res<Clock>,
    mut last: Local<String>,
) {
    let system = nav.system(&universe.0);
    let planet = system.and_then(|s| nav.selected.and_then(|i| s.planets.get(i)));
    let picked = nav.picked.as_ref().map(|star| {
        let (name, page) = crate::galaxy_nav::describe(&universe.0, &star.id);
        Picked {
            name,
            page: page.map(|(page, _)| page),
            pages: page.map(|(_, pages)| pages),
        }
    });
    let state = State {
        picked,
        list: nav
            .list()
            .and_then(|index| Some(universe.0.systems.get(index)?.id.as_str())),
        page: nav.page,
        pages: system.map(|s| s.pages()).unwrap_or_default(),
        view: nav.view,
        task: planet.and_then(|p| Some(p.task.as_ref()?.id.as_str())),
        following: nav.following,
        paused: clock.0.paused,
        rate: clock.0.rate,
        notice: &nav.notice,
        display: DisplayState {
            exaggerate: nav.exaggerate,
            orbits: nav.orbits,
            labels: nav.labels,
            markers: nav.markers,
            realistic: nav.realistic,
            physical: nav.physical,
            wireframe: nav.wireframe,
        },
        system: system.map(|s| {
            let [inclination, node, _] = crate::scene::page_tilt(s, nav.page);
            SystemState {
                name: &s.name,
                star: s.star(nav.page),
                page_tasks: s
                    .tasks()
                    .filter(|&(i, _)| s.planets[i].page() == nav.page)
                    .count(),
                tasks: s.tasks().count(),
                star_ly: crate::scene::page_origin_ly(s, nav.page).to_array(),
                tilt_deg: inclination.to_degrees(),
                node_deg: node.to_degrees(),
            }
        }),
        selected: planet.zip(system).map(|(p, s)| TaskDetail {
            name: p.label(),
            designation: &p.designation,
            status: p.task.as_ref().map(|t| t.status.as_str()),
            colour: p.colour(),
            description: p.task.as_ref().map(|t| t.description.as_str()),
            assignees: p.task.as_ref().map(|t| t.assignees.as_slice()),
            due_date_ms: p.task.as_ref().and_then(|t| t.due_date_ms),
            url: p.task.as_ref().and_then(|t| t.url.as_deref()),
            semi_major_au: p.orbit.semi_major_au,
            eccentricity: p.orbit.eccentricity,
            period_days: period_seconds(&p.orbit, s.mass(p.page())) / 86_400.0,
            radius_km: p.radius_km,
            mass_earth: p.mass_earth,
            class: &p.class,
            resonance: p.resonance,
            spin: p.spin,
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
    /// The camera's exposure in words.
    exposure: String,
}

fn publish_tick(
    time: Res<Time>,
    clock: Res<Clock>,
    pose: Res<CameraPose>,
    nav: Res<Navigation>,
    lights: Res<crate::render::Lights>,
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
            exposure: crate::render::exposure_caption(&nav, &lights),
        },
    );
}
