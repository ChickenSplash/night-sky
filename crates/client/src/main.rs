mod actions;
#[cfg(target_arch = "wasm32")]
mod benchmark;
mod black_hole;
#[cfg(not(target_arch = "wasm32"))]
mod diagnostics;
mod flight;
mod galaxy_nav;
mod glow;
mod network;
mod render;
mod scene;
mod sky;
#[cfg(not(target_arch = "wasm32"))]
mod smoke;
mod starfield;
mod stream;
mod ui;
mod views;
#[cfg(target_arch = "wasm32")]
mod web_bridge;

use bevy::{math::DVec3, prelude::*};
use bevy_egui::{EguiPlugin, EguiPrimaryContextPass};
use big_space::prelude::*;
use space_shared::{Galaxy, StarSystem};
use space_simulation::demo_galaxy;
use space_simulation::{AU_METRES, LY_METRES, SimulationClock};
use std::sync::{Mutex, mpsc};

#[derive(Resource)]
pub struct Universe(pub Galaxy);
#[derive(Resource, Default)]
pub struct Clock(pub SimulationClock);

/// The star system the camera is at. List pages and generated systems are
/// both galactic stars; a list page adds its tasks.
#[derive(Clone, Debug, PartialEq)]
pub enum Visit {
    /// A list, by index into `Universe`, at `Navigation::page`.
    List(usize),
    Generated(Box<StarSystem>),
}

#[derive(Resource)]
pub struct Navigation {
    pub page: u32,
    pub view: views::View,
    pub viewport: Option<bevy::camera::Viewport>,
    /// `None` in open space, where the camera is relative to home.
    pub visit: Option<Visit>,
    pub selected: Option<usize>,
    /// Selected star, by click, F or page controls. Picking never moves the
    /// camera; visiting flies there.
    pub picked: Option<galaxy_nav::PickedStar>,
    pub following: bool,
    pub target: DVec3,
    /// Authoritative free-flight eye in the current page's local frame.
    /// Avoids cancelling a distant target against the camera range.
    pub eye: Option<DVec3>,
    pub distance: f64,
    /// Selected free-flight speed in metres per real second, independent of zoom.
    pub move_speed: f64,
    /// Camera orientation in the current system's local plane. Keeping this as
    /// a quaternion lets mouse turns follow the screen axes even past the poles.
    pub rotation: Quat,
    /// Vertical field of view in degrees, within `FOV_DEG`.
    pub fov_deg: f32,
    pub exaggerate: bool,
    pub labels: bool,
    pub orbits: bool,
    /// Rings marking bodies too small to see. Hover and selection rings stay.
    pub markers: bool,
    pub velocity_marker: bool,
    /// Lit and glowing bodies in HDR under the wireframes.
    pub realistic: bool,
    pub galaxy_glow: bool,
    /// One linear exposure for the frame instead of the assisted, readable one.
    pub physical: bool,
    /// Wireframes over the realistic bodies. Always drawn otherwise.
    pub wireframe: bool,
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
            visit: None,
            selected: None,
            picked: None,
            following: false,
            target: DVec3::ZERO,
            eye: None,
            distance: 120.0 * LY_METRES,
            move_speed: 42.0 * LY_METRES,
            rotation: Quat::from_rotation_y(0.3) * Quat::from_rotation_x(-0.85),
            fov_deg: 50.0,
            exaggerate: false,
            labels: true,
            orbits: true,
            markers: true,
            velocity_marker: true,
            realistic: true,
            galaxy_glow: true,
            physical: true,
            wireframe: true,
            search: String::new(),
            notice: "Connecting to local API…".into(),
            fly: false,
        }
    }
}

/// Selectable vertical field of view, in degrees.
pub const FOV_DEG: std::ops::RangeInclusive<f32> = 30.0..=130.0;

impl Navigation {
    pub fn set_fov(&mut self, degrees: f32) {
        if degrees.is_finite() {
            self.fov_deg = degrees.clamp(*FOV_DEG.start(), *FOV_DEG.end());
        }
    }

    /// Tangent of half the vertical field of view, for projecting by hand.
    pub fn tan_half_fov(&self) -> f64 {
        (f64::from(self.fov_deg).to_radians() / 2.0).tan()
    }

    pub fn system<'a>(&'a self, lists: &'a Galaxy) -> Option<&'a StarSystem> {
        match self.visit.as_ref()? {
            Visit::List(index) => lists.systems.get(*index),
            Visit::Generated(system) => Some(system),
        }
    }

    /// The visited list, if the camera is at one of its pages.
    pub fn list(&self) -> Option<usize> {
        match self.visit {
            Some(Visit::List(index)) => Some(index),
            _ => None,
        }
    }

    /// Fly to a page's star, framing its planets in orbit view.
    pub fn arrive(&mut self, visit: Visit, page: u32, lists: &Galaxy) {
        self.visit = Some(visit);
        self.page = page;
        self.frame(lists);
    }

    /// Frame the current page's planets in orbit view.
    pub fn frame(&mut self, lists: &Galaxy) {
        let Some(distance) = self
            .system(lists)
            .map(|system| views::fit_distance(system, self.page))
        else {
            return;
        };
        self.fly = true;
        self.view = views::View::Orbit;
        self.rotation = Quat::from_rotation_y(0.3) * Quat::from_rotation_x(-0.85);
        self.selected = None;
        self.picked = None;
        self.following = false;
        self.target = DVec3::ZERO;
        self.eye = None;
        self.distance = distance;
        self.move_speed = (distance * 0.35).clamp(scene::MIN_SPEED, scene::MAX_SPEED);
    }

    /// A list opens as a constellation regardless of the previous view.
    pub fn select_list(&mut self, index: usize, lists: &Galaxy) {
        let Some(system) = lists.systems.get(index) else {
            return;
        };
        let first_page = system.first_page();
        let picked = galaxy_nav::page_pick(system, first_page);
        self.arrive(Visit::List(index), first_page, lists);
        self.preset(views::View::Constellation, lists);
        self.picked = picked;
    }
}

const CONNECTED: &str = "Local API connected · read-only";

#[derive(Resource)]
struct Inbox(Mutex<mpsc::Receiver<network::Update>>);

fn main() {
    #[cfg(not(target_arch = "wasm32"))]
    diagnostics::install_crash_reports();
    let smoke_test = std::env::args().any(|arg| arg == "--smoke-test");
    let mut initial_nav = Navigation::default();
    #[cfg(target_arch = "wasm32")]
    let benchmark = benchmark::requested();
    #[cfg(target_arch = "wasm32")]
    if benchmark {
        benchmark::at_home(&mut initial_nav);
    }
    if smoke_test {
        // Its captures start in the first list's first system.
        initial_nav.visit = Some(Visit::List(0));
        initial_nav.distance = 20.0 * AU_METRES;
        initial_nav.move_speed = initial_nav.distance * 0.35;
    }
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
                    title: "Task Space · Mission Control".into(),
                    resolution: (1440, 900).into(),
                    #[cfg(not(target_arch = "wasm32"))]
                    mode: if smoke_test {
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
        render::RenderPlugin,
        stream::StreamPlugin,
        starfield::StarfieldRenderPlugin,
        glow::GlowPlugin,
        black_hole::BlackHolePlugin,
    ))
    .insert_resource(ClearColor(render::SPACE_BLUE))
    .insert_resource(bevy_egui::EguiGlobalSettings {
        auto_create_primary_context: false,
        ..default()
    })
    .insert_resource(Universe(demo_galaxy()))
    // After the logger, so the poller's first messages are recorded.
    .insert_resource(Inbox(Mutex::new(network::start())))
    .init_resource::<Clock>()
    .insert_resource(initial_nav)
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
            #[cfg(target_arch = "wasm32")]
            benchmark::advance,
            scene::nearby_system,
            scene::place_camera,
            scene::measure_velocity,
        )
            .chain(),
    )
    .add_systems(
        PostUpdate,
        // Before the egui pass, so markers use this frame's camera like the bodies.
        scene::draw
            .after(TransformSystems::Propagate)
            .before(bevy_egui::EguiPostUpdateSet::EndPass)
            .before(bevy::camera::visibility::VisibilitySystems::VisibilityPropagate),
    )
    .add_systems(EguiPrimaryContextPass, ui::draw_ui);
    #[cfg(not(target_arch = "wasm32"))]
    app.add_systems(Update, smoke::exercise);
    #[cfg(target_arch = "wasm32")]
    if benchmark {
        app.insert_resource(benchmark::BenchmarkFlight::default());
    }
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
            network::Update::Galaxy(mut galaxy) => {
                // Bodies are deterministic from slots, so fixtures need only slots.
                space_simulation::shape_galaxy(&mut galaxy);
                nav.notice = CONNECTED.into();
                follow_visited_star(&mut nav, &universe.0, &galaxy);
                universe.0 = galaxy;
            }
        }
    }
}

/// A visit follows its star into a new snapshot: its list may move, a list may
/// gain a page on the visited star, or the list may go and leave the star bare.
/// Planet IDs name their star, so the selected planet survives each change.
fn follow_visited_star(nav: &mut Navigation, old: &Galaxy, new: &Galaxy) {
    let Some(system) = nav.system(old) else {
        return;
    };
    let Some(star) = system.page_star(nav.page).cloned() else {
        return;
    };
    let selected = nav
        .selected
        .and_then(|i| system.planets.get(i))
        .map(|p| p.id.clone());
    if let Some((list, page)) = galaxy_nav::mapped(new, &star.id) {
        nav.visit = Some(Visit::List(list));
        nav.page = page;
    } else if nav.list().is_some() {
        nav.visit = galaxy_nav::generated(&star.id, DVec3::from_array(star.position_ly))
            .map(|system| Visit::Generated(Box::new(system)));
        nav.page = 0;
    }
    nav.selected = selected.and_then(|id| {
        nav.system(new)?
            .planets
            .iter()
            .position(|planet| planet.id == id)
    });
    if nav.selected.is_none() {
        nav.following = false;
    }
}
