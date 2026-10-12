mod actions;
mod command_line;
mod devtools;
mod flight;
mod format;
mod galaxy_nav;
mod navigation;
mod network;
mod objects;
mod presentation;
mod render;
mod scene;
mod stars;
mod ui;

use bevy::prelude::*;
use bevy_egui::{EguiPlugin, EguiPrimaryContextPass};
use big_space::prelude::*;
use space_shared::Galaxy;
use space_simulation::demo_galaxy;
use space_simulation::{AU_METRES, SimulationClock};
use std::sync::{Mutex, mpsc};

use navigation::follow_visited_star;
pub use navigation::{FOV_DEG, MAGNITUDE, Navigation, Selection, Visit};

#[derive(Resource)]
pub struct Universe(pub Galaxy);
#[derive(Resource, Default)]
pub struct Clock(pub SimulationClock);

const CONNECTED: &str = "Local API connected · read-only";

#[derive(Resource)]
struct Inbox(Mutex<mpsc::Receiver<network::Update>>);

fn main() {
    #[cfg(not(target_arch = "wasm32"))]
    devtools::diagnostics::install_crash_reports();
    let smoke_test = std::env::args().any(|arg| arg == "--smoke-test");
    let mut initial_nav = Navigation::default();
    #[cfg(target_arch = "wasm32")]
    let benchmark = devtools::benchmark::requested();
    #[cfg(target_arch = "wasm32")]
    if benchmark {
        devtools::benchmark::at_home(&mut initial_nav);
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
        render::debug::RenderDebugPlugin,
        stars::stream::StreamPlugin,
        stars::starfield::StarfieldRenderPlugin,
        render::glow::GlowPlugin,
        render::black_hole::BlackHolePlugin,
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
    .init_resource::<devtools::core_load::CoreLoad>()
    .init_resource::<devtools::gpu_test::GpuTest>()
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
            #[cfg(not(target_arch = "wasm32"))]
            devtools::core_load::run,
            #[cfg(not(target_arch = "wasm32"))]
            devtools::gpu_test::run,
            #[cfg(target_arch = "wasm32")]
            devtools::benchmark::advance,
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
    app.add_systems(Update, devtools::smoke::exercise);
    #[cfg(target_arch = "wasm32")]
    if benchmark {
        app.insert_resource(devtools::benchmark::BenchmarkFlight::default())
            .add_systems(Last, devtools::benchmark::record)
            .add_systems(
                EguiPrimaryContextPass,
                devtools::benchmark::results_window.after(ui::draw_ui),
            );
    }
    #[cfg(target_arch = "wasm32")]
    devtools::automation::install(&mut app);
    devtools::snapshot::install(&mut app);
    #[cfg(not(target_arch = "wasm32"))]
    devtools::install_http(&mut app);
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
