mod actions;
#[cfg(target_arch = "wasm32")]
mod automation;
#[cfg(target_arch = "wasm32")]
mod benchmark;
mod black_hole;
mod breadcrumb;
mod command_line;
mod devtools;
#[cfg(not(target_arch = "wasm32"))]
mod diagnostics;
mod flight;
mod galaxy_nav;
mod glow;
mod inspector;
mod network;
mod objects;
mod presentation;
mod render;
mod scene;
mod sky;
#[cfg(not(target_arch = "wasm32"))]
mod smoke;
mod starfield;
mod stream;
mod tasks;
mod terminal;
mod ui;

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

/// The one clicked object. A second click activates it: a star is visited, a
/// planet followed.
#[derive(Clone, Debug, PartialEq)]
pub enum Selection {
    /// Any star in the galaxy, by stable ID.
    Star(galaxy_nav::PickedStar),
    /// Stable planet ID, even when its parent system is not visited.
    Planet(String),
}

impl Selection {
    pub fn planet(system: &StarSystem, index: usize) -> Self {
        Self::Planet(system.planets[index].id.clone())
    }

    pub fn object(&self) -> objects::ObjectRef {
        match self {
            Self::Star(star) => objects::ObjectRef::Star {
                id: star.id.clone(),
            },
            Self::Planet(id) => objects::ObjectRef::Planet { id: id.clone() },
        }
    }
}

#[derive(Resource)]
pub struct Navigation {
    pub page: u32,
    pub presentation_mode: presentation::PresentationMode,
    pub viewport: Option<bevy::camera::Viewport>,
    /// The system the camera is anchored to: the one it is at, is flying to,
    /// or whose list is shown as a constellation. `None` in open space, where
    /// the camera is relative to home. See `location` for where it actually is.
    pub visit: Option<Visit>,
    /// Whether the camera is at the visited star, by physical distance. Set
    /// each frame by `scene::locate`.
    pub arrived: bool,
    /// Set by clicks, C or page controls. Selecting never moves the camera;
    /// visiting a star or following a planet does.
    pub selection: Option<Selection>,
    /// Which trail and object currently drive the floating inspector.
    pub inspection: Option<breadcrumb::Inspection>,
    /// Physical planet neighbourhood, unrelated to selection or following.
    pub location_planet: Option<String>,
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
            presentation_mode: presentation::PresentationMode::Simulation,
            viewport: None,
            visit: None,
            arrived: false,
            selection: None,
            inspection: None,
            location_planet: None,
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

    pub fn selected_star(&self) -> Option<&galaxy_nav::PickedStar> {
        match &self.selection {
            Some(Selection::Star(star)) => Some(star),
            _ => None,
        }
    }

    pub fn selected_planet(&self, galaxy: &Galaxy) -> Option<usize> {
        match &self.selection {
            Some(Selection::Planet(id)) => self
                .system(galaxy)?
                .planets
                .iter()
                .position(|planet| planet.id == *id),
            _ => None,
        }
    }

    /// Replace the selection. Following carries over from planet to planet only.
    pub fn select(&mut self, selection: Option<Selection>) {
        if !matches!(selection, Some(Selection::Planet(_))) {
            self.following = false;
        }
        self.selection = selection;
        self.inspection = self
            .selection
            .as_ref()
            .map(|selection| breadcrumb::Inspection {
                source: breadcrumb::TrailSource::Selection,
                object: selection.object(),
            });
    }

    /// The system the camera is physically at, on `page`. `None` in open space,
    /// in constellation view and while a flight is still on its way in.
    pub fn location<'a>(&'a self, lists: &'a Galaxy) -> Option<&'a StarSystem> {
        self.system(lists).filter(|_| self.arrived)
    }

    /// The open list: the visited star's list, whether the camera is at it,
    /// flying to it or showing its constellation. It closes in open space.
    pub fn list(&self) -> Option<usize> {
        match self.visit {
            Some(Visit::List(index)) => Some(index),
            _ => None,
        }
    }

    /// Fly to a page's star, framing its planets in simulation mode.
    pub fn arrive(&mut self, visit: Visit, page: u32, lists: &Galaxy) {
        self.visit = Some(visit);
        self.page = page;
        self.frame(lists);
    }

    /// Frame the current page's planets in simulation mode.
    pub fn frame(&mut self, lists: &Galaxy) {
        let Some(distance) = self
            .system(lists)
            .map(|system| presentation::fit_distance(system, self.page))
        else {
            return;
        };
        self.fly = true;
        self.presentation_mode = presentation::PresentationMode::Simulation;
        self.rotation = Quat::from_rotation_y(0.3) * Quat::from_rotation_x(-0.85);
        self.selection = None;
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
        let star = galaxy_nav::page_pick(system, first_page);
        self.arrive(Visit::List(index), first_page, lists);
        self.preset(presentation::PresentationMode::Constellation, lists);
        self.selection = star.map(Selection::Star);
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
                custom_layer: devtools::log_layers,
                ..default()
            })
            .set(WindowPlugin {
                primary_window: Some(Window {
                    title: "Night Sky".into(),
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
        command_line::CommandLinePlugin,
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
            scene::locate,
            scene::place_camera,
            scene::locate_planet,
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
    .add_systems(
        EguiPrimaryContextPass,
        (ui::draw_ui, devtools::draw.after(ui::draw_ui)),
    );
    #[cfg(not(target_arch = "wasm32"))]
    app.add_systems(Update, smoke::exercise);
    #[cfg(target_arch = "wasm32")]
    if benchmark {
        app.insert_resource(benchmark::BenchmarkFlight::default())
            .add_systems(Last, benchmark::record)
            .add_systems(
                EguiPrimaryContextPass,
                benchmark::results_window.after(ui::draw_ui),
            );
    }
    #[cfg(target_arch = "wasm32")]
    automation::install(&mut app);
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
    let planet = nav
        .selected_planet(old)
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
    if let Some(id) = planet {
        nav.selection = Some(Selection::Planet(id));
    }
    if nav.selected_planet(new).is_none() {
        nav.following = false;
    }
}
