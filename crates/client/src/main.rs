mod actions;
#[cfg(not(target_arch = "wasm32"))]
mod diagnostics;
mod flight;
mod network;
mod scene;
#[cfg(not(target_arch = "wasm32"))]
mod smoke;
mod ui;
mod views;
#[cfg(target_arch = "wasm32")]
mod web_bridge;

use bevy::{math::DVec3, prelude::*};
use bevy_egui::{EguiPlugin, EguiPrimaryContextPass};
use big_space::prelude::*;
use space_shared::{Galaxy, StarSystem, demo_galaxy};
use space_simulation::{AU_METRES, SimulationClock};
use std::sync::{Mutex, mpsc};

#[derive(Resource)]
pub struct Universe(pub Galaxy);
#[derive(Resource, Default)]
pub struct Clock(pub SimulationClock);

#[derive(Resource)]
pub struct Navigation {
    pub page: u32,
    pub view: views::View,
    pub viewport: Option<bevy::camera::Viewport>,
    pub system: usize,
    pub selected: Option<usize>,
    /// A star clicked on screen: its list index and page. Picking never moves
    /// the camera; framing the system flies there.
    pub picked: Option<(usize, u32)>,
    pub following: bool,
    pub atlas: bool,
    pub target: DVec3,
    pub distance: f64,
    pub yaw: f32,
    pub pitch: f32,
    pub exaggerate: bool,
    pub labels: bool,
    pub orbits: bool,
    pub search: String,
    pub notice: String,
    /// Animate the camera to the new target instead of cutting to it.
    pub fly: bool,
}

impl Default for Navigation {
    fn default() -> Self {
        Self {
            page: 0,
            view: views::View::Orbit,
            viewport: None,
            system: 0,
            selected: None,
            picked: None,
            following: false,
            atlas: false,
            target: DVec3::ZERO,
            distance: 20.0 * AU_METRES,
            yaw: 0.3,
            pitch: 0.85,
            exaggerate: false,
            labels: true,
            orbits: true,
            search: String::new(),
            notice: "Connecting to local API…".into(),
            fly: false,
        }
    }
}

impl Navigation {
    pub fn show_atlas(&mut self, galaxy: &Galaxy) {
        self.fly = true;
        self.view = views::View::Orbit;
        self.yaw = 0.3;
        self.pitch = 0.85;
        self.atlas = true;
        self.following = false;
        self.selected = None;
        let Some(current) = galaxy.systems.get(self.system) else {
            return;
        };
        let base = DVec3::from_array(current.position_ly);
        let mut low = DVec3::ZERO;
        let mut high = DVec3::ZERO;
        for system in &galaxy.systems {
            let p = DVec3::from_array(system.position_ly) - base;
            low = low.min(p);
            high = high.max(p);
        }
        self.target = (low + high) * 0.5 * space_simulation::LY_METRES;
        self.distance = (high - low).length().max(2.0) * 2.8 * space_simulation::LY_METRES;
    }

    pub fn jump(&mut self, system: usize, galaxy: &Galaxy) {
        if self.system != system {
            self.page = galaxy
                .systems
                .get(system)
                .map(StarSystem::first_page)
                .unwrap_or(0);
        }
        self.fly = true;
        self.view = views::View::Orbit;
        self.yaw = 0.3;
        self.pitch = 0.85;
        self.system = system;
        self.selected = None;
        self.picked = None;
        self.following = false;
        self.atlas = false;
        self.target = DVec3::ZERO;
        let extent = galaxy
            .systems
            .get(system)
            .map(|s| {
                s.on_page(self.page)
                    .map(|p| p.orbit.semi_major_au * (1.0 + p.orbit.eccentricity))
                    .fold(3.0, f64::max)
            })
            .unwrap_or(6.0);
        self.distance = extent * 3.1 * AU_METRES;
    }
}

const CONNECTED: &str = "Local API connected · read-only";

#[derive(Resource)]
struct Inbox(Mutex<mpsc::Receiver<network::Update>>);

fn main() {
    #[cfg(not(target_arch = "wasm32"))]
    diagnostics::install_crash_reports();
    let mut app = App::new();
    app.add_plugins((
        DefaultPlugins
            .set(bevy::log::LogPlugin {
                #[cfg(not(target_arch = "wasm32"))]
                custom_layer: diagnostics::file_layer,
                ..default()
            })
            .set(WindowPlugin {
                primary_window: Some(Window {
                    title: "ClickUp Space · Mission Control".into(),
                    resolution: (1440, 900).into(),
                    #[cfg(not(target_arch = "wasm32"))]
                    mode: if std::env::args().any(|arg| arg == "--smoke-test") {
                        bevy::window::WindowMode::BorderlessFullscreen(
                            bevy::window::MonitorSelection::Current,
                        )
                    } else {
                        bevy::window::WindowMode::Windowed
                    },
                    #[cfg(target_arch = "wasm32")]
                    canvas: Some("#space-canvas".into()),
                    #[cfg(target_arch = "wasm32")]
                    fit_canvas_to_parent: true,
                    #[cfg(target_arch = "wasm32")]
                    prevent_default_event_handling: true,
                    ..default()
                }),
                ..default()
            })
            .build()
            .disable::<TransformPlugin>(),
        BigSpaceDefaultPlugins,
        EguiPlugin::default(),
    ))
    .insert_resource(ClearColor(Color::srgb(0.015, 0.035, 0.09)))
    .insert_resource(bevy_egui::EguiGlobalSettings {
        auto_create_primary_context: false,
        ..default()
    })
    .insert_resource(Universe(demo_galaxy()))
    // After the logger, so the poller's first messages are recorded.
    .insert_resource(Inbox(Mutex::new(network::start())))
    .init_resource::<Clock>()
    .init_resource::<Navigation>()
    .init_resource::<actions::PanelInsets>()
    .add_message::<actions::UiCommand>()
    .add_systems(Startup, scene::setup)
    .add_systems(
        Update,
        (
            receive_galaxy,
            actions::apply,
            tick,
            scene::controls,
            scene::place_camera,
        )
            .chain(),
    )
    .add_systems(PostUpdate, scene::draw.after(TransformSystems::Propagate))
    .add_systems(EguiPrimaryContextPass, ui::draw_ui);
    #[cfg(not(target_arch = "wasm32"))]
    app.add_systems(Update, smoke::exercise);
    #[cfg(target_arch = "wasm32")]
    web_bridge::install(&mut app);
    app.run();
}

fn tick(time: Res<Time>, mut clock: ResMut<Clock>) {
    clock.0.advance(time.delta_secs_f64());
}

fn receive_galaxy(inbox: Res<Inbox>, mut universe: ResMut<Universe>, mut nav: ResMut<Navigation>) {
    let Ok(receiver) = inbox.0.lock() else {
        return;
    };
    while let Ok(update) = receiver.try_recv() {
        match update {
            network::Update::Unchanged => nav.notice = CONNECTED.into(),
            network::Update::Offline(message) => nav.notice = message,
            network::Update::Galaxy(galaxy) => {
                let old_system = universe.0.systems.get(nav.system).map(|s| s.id.clone());
                let old_task = universe
                    .0
                    .systems
                    .get(nav.system)
                    .and_then(|s| nav.selected.and_then(|i| s.tasks.get(i)))
                    .map(|p| p.id.clone());
                let same_system = old_system
                    .as_ref()
                    .and_then(|id| galaxy.systems.iter().position(|s| &s.id == id));
                nav.notice = CONNECTED.into();
                if let Some(index) = same_system {
                    nav.system = index;
                    nav.selected = old_task
                        .and_then(|id| galaxy.systems[index].tasks.iter().position(|p| p.id == id));
                    if nav.selected.is_none() {
                        nav.following = false;
                    }
                    let system = &galaxy.systems[index];
                    if let Some(task) = nav.selected.map(|i| &system.tasks[i]) {
                        nav.page = task.page();
                    } else if system.on_page(nav.page).next().is_none() {
                        nav.page = system.first_page();
                    }
                } else {
                    nav.jump(0, &galaxy);
                }
                universe.0 = galaxy;
            }
        }
    }
}
