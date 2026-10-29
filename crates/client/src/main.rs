mod actions;
mod craft;
mod devtools;
mod format;
mod input;
mod navigation;
mod network;
mod planet_loading;
mod render;
mod scene;
mod search;
mod stars;
mod ui;
mod window;

use bevy::prelude::*;
use bevy_egui::{EguiPlugin, EguiPrimaryContextPass};
use big_space::prelude::*;
use space_shared::Galaxy;
use space_simulation::demo_galaxy;
use space_simulation::{AU_METRES, SimulationClock};
use std::sync::{Mutex, mpsc};

use navigation::follow_visited_star;
pub use navigation::{Anchor, FOV_DEG, MAGNITUDE, Navigation, Selection, Visit};

#[derive(Resource)]
pub struct Universe(pub Galaxy);
#[derive(Resource, Default)]
pub struct Clock(pub SimulationClock);

const CONNECTED: &str = "Local API connected · read-only";

#[derive(Resource)]
struct Inbox(Mutex<mpsc::Receiver<network::Update>>);

fn main() {
    devtools::diagnostics::install_crash_reports();
    let smoke_test = std::env::args().any(|arg| arg == "--smoke-test");
    let mut initial_nav = Navigation::default();
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
                    mode: window::mode(smoke_test),
                    ..default()
                }),
                ..default()
            })
            .build()
            .disable::<TransformPlugin>(),
        BigSpaceDefaultPlugins,
        EguiPlugin::default(),
        render::RenderPlugin,
        render::debug::RenderDebugPlugin,
        stars::stream::StreamPlugin,
        stars::starfield::StarfieldRenderPlugin,
        render::glow::GlowPlugin,
        render::black_hole::BlackHolePlugin,
        devtools::command_line::CommandLinePlugin,
        search::SearchPlugin,
        ui::browser::BrowserPlugin,
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
    .init_resource::<planet_loading::PlanetLoading>()
    .init_resource::<craft::Fleet>()
    .init_resource::<input::Bindings>()
    .init_resource::<input::Actions>()
    .init_resource::<input::pilot::Pilot>()
    .init_resource::<devtools::DevTools>()
    .init_resource::<devtools::core_load::CoreLoad>()
    .init_resource::<devtools::gpu_test::GpuTest>()
    .init_resource::<devtools::atmosphere_test::AtmosphereTest>()
    .insert_resource(initial_nav)
    .add_message::<actions::UiCommand>()
    .add_systems(Startup, scene::setup)
    .add_systems(
        PreUpdate,
        input::keep_navigation_keys
            .after(bevy_egui::EguiPreUpdateSet::ProcessInput)
            .before(bevy_egui::EguiPreUpdateSet::BeginPass),
    )
    .add_systems(
        Update,
        (
            receive_galaxy,
            planet_loading::restore,
            actions::apply,
            input::read,
            input::pilot::steer,
            input::pilot::fly,
            tick,
            craft::step,
            craft::forecast,
            scene::controls,
            devtools::core_load::run,
            devtools::gpu_test::run,
            devtools::atmosphere_test::run,
            scene::locate,
            planet_loading::restore_visit,
            scene::place_camera,
            planet_loading::update,
            scene::locate_body,
            scene::locate_anchor,
            scene::measure_velocity,
        )
            .chain(),
    )
    .add_systems(Update, window::fullscreen.after(input::read))
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
    app.add_systems(Update, devtools::smoke::exercise);
    devtools::snapshot::install(&mut app);
    devtools::install_http(&mut app);
    app.run();
}

fn tick(time: Res<Time>, mut clock: ResMut<Clock>, fleet: Res<craft::Fleet>) {
    let requested = clock.0.rate;
    clock.0.rate = fleet.safe_rate(requested);
    clock.0.advance(time.delta_secs_f64());
    clock.0.rate = requested;
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
