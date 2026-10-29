//! The debug window, kept apart from the product UI: the client log, frame and
//! star-stream timings, display settings, controls, internal state and tools.
//! The cog, ` or F3 toggles it. Drawn in the terminal look.
pub mod atmosphere_test;
mod capture;
pub mod command_line;
pub mod core_load;
pub mod diagnostics;
pub mod gpu_test;
mod http;
pub mod smoke;
pub mod snapshot;
mod tabs;

use self::command_line::{Source, Typed};
use crate::actions::UiCommand;
use crate::devtools::atmosphere_test::AtmosphereTest;
use crate::devtools::core_load::CoreLoad;
use crate::devtools::gpu_test::GpuTest;
use crate::render::debug::RenderDebug;
use crate::scene::{CameraPose, CameraRig, CameraVelocity};
use crate::stars::stream::{Starfield, StreamMetrics};
use crate::ui::terminal;
use crate::{Clock, Navigation, Universe};
use bevy::diagnostic::DiagnosticsStore;
use bevy::prelude::*;
use bevy_egui::{EguiContexts, egui};
use std::collections::VecDeque;

use capture::Line;
pub use capture::log_layers;
pub use http::install as install_http;
use tabs::tools::save_bug_report;
use tabs::{
    atmosphere::atmosphere,
    controls::controls,
    log::{log, unseen_badges},
    perf::perf,
    settings::settings,
    state::state,
    terminal::terminal_tab,
    tools::tools_tab,
};

/// Frame times kept for the graph.
const FRAMES: usize = 240;
/// The debug console's window key and title.
pub const WINDOW: &str = "debug";

#[derive(Clone, Copy, PartialEq)]
enum Tab {
    Log,
    Terminal,
    Perf,
    Settings,
    Atmosphere,
    Controls,
    State,
    Tools,
}

impl Tab {
    const ALL: [(Self, &str); 8] = [
        (Self::Log, "log"),
        (Self::Terminal, "terminal"),
        (Self::Perf, "perf"),
        (Self::Settings, "settings"),
        (Self::Atmosphere, "atmosphere"),
        (Self::Controls, "controls"),
        (Self::State, "state"),
        (Self::Tools, "tools"),
    ];
}

#[derive(Resource)]
pub struct DevTools {
    pub(crate) open: bool,
    tab: Tab,
    lines: VecDeque<Line>,
    seen: u64,
    levels: [bool; 4],
    grep: String,
    /// Errors and warnings logged since the log tab was last shown.
    unseen: [usize; 2],
    /// The terminal tab's output: the `console` target's log lines.
    terminal: VecDeque<Line>,
    /// Focus the command line on the next frame.
    focus: bool,
    /// The command line, and earlier commands, oldest first.
    command: String,
    history: Vec<String>,
    /// The history entry the arrow keys have recalled.
    recalled: Option<usize>,
    /// Recent frame times in milliseconds.
    frames: VecDeque<f32>,
    /// The last bug report's outcome.
    report: String,
}

impl Default for DevTools {
    fn default() -> Self {
        Self {
            open: false,
            tab: Tab::Log,
            lines: VecDeque::new(),
            seen: 0,
            levels: [true; 4],
            grep: String::new(),
            unseen: [0; 2],
            terminal: VecDeque::new(),
            focus: false,
            command: String::new(),
            history: Vec::new(),
            recalled: None,
            frames: VecDeque::new(),
            report: String::new(),
        }
    }
}

type Scene<'w> = (
    Res<'w, Universe>,
    Res<'w, Navigation>,
    Res<'w, Clock>,
    Res<'w, Time>,
);
type Camera<'w> = (
    Res<'w, CameraRig>,
    Res<'w, CameraPose>,
    Res<'w, CameraVelocity>,
);
type Rendering<'w> = (
    Res<'w, crate::search::Searches>,
    Res<'w, Starfield>,
    ResMut<'w, StreamMetrics>,
    Res<'w, RenderDebug>,
    Option<Res<'w, DiagnosticsStore>>,
    ResMut<'w, CoreLoad>,
    (ResMut<'w, GpuTest>, ResMut<'w, AtmosphereTest>),
    Res<'w, crate::render::atmosphere::AtmosphereTuning>,
);

pub fn draw(
    interaction: (
        EguiContexts,
        MessageWriter<UiCommand>,
        MessageWriter<Typed>,
        Commands,
    ),
    scene: Scene,
    camera: Camera,
    rendering: Rendering,
    mut bindings: ResMut<crate::input::Bindings>,
    mut tools: ResMut<DevTools>,
    (mut planets, window): (
        ResMut<crate::planet_loading::PlanetLoading>,
        Query<&Window, With<bevy::window::PrimaryWindow>>,
    ),
) {
    let (mut contexts, mut writer, mut typed, mut commands) = interaction;
    let (universe, nav, clock, time) = scene;
    let (rig, pose, velocity) = camera;
    let (searches, starfield, mut metrics, debug, diagnostics, mut core_load, tests, tuning) =
        rendering;
    let (mut gpu_test, mut atmosphere_test) = tests;
    metrics.instrumentation_latest.debug_window_ms = 0.0;
    if tools.frames.len() == FRAMES {
        tools.frames.pop_front();
    }
    tools.frames.push_back(time.delta_secs() * 1000.0);
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };
    if !ctx.wants_keyboard_input()
        && ctx.input(|i| i.key_pressed(egui::Key::Backtick) || i.key_pressed(egui::Key::F3))
    {
        crate::ui::kit::toggle_window(ctx, WINDOW, &mut tools.open);
    }
    // Kept up to date while closed, so the log tab can count what was missed.
    tools.sync();
    if !tools.open {
        return;
    }
    let mut out = Vec::new();
    let mut lines = Vec::new();
    let mut report = false;
    let mut open = true;
    let window_start = bevy::platform::time::Instant::now();
    terminal::window(ctx, WINDOW, &mut open, |ui| {
        ui.horizontal_wrapped(|ui| {
            for (tab, label) in Tab::ALL {
                if terminal::tab(ui, label, tools.tab == tab) {
                    tools.focus = tab == Tab::Terminal && tools.tab != tab;
                    tools.tab = tab;
                }
                if tab == Tab::Log && tools.tab != Tab::Log {
                    unseen_badges(ui, tools.unseen);
                }
            }
        });
        terminal::rule(ui);
        match tools.tab {
            Tab::Log => {
                tools.unseen = [0; 2];
                log(ui, &mut tools);
            }
            Tab::Terminal => terminal_tab(ui, &mut tools, &mut lines),
            tab => {
                terminal::ScrollArea::vertical()
                    .scroll_source(terminal::SCROLL_SOURCE)
                    .id_salt("debug-tab")
                    .auto_shrink(false)
                    .show(ui, |ui| match tab {
                        Tab::Perf => {
                            perf(
                                ui,
                                &tools.frames,
                                &metrics,
                                &starfield,
                                diagnostics.as_deref(),
                                &searches,
                                &mut out,
                            );
                            tabs::planets::planets(ui, &mut planets);
                        }
                        Tab::Settings => settings(
                            ui,
                            &nav,
                            &debug,
                            window.single().is_ok_and(crate::window::is_fullscreen),
                            &mut out,
                        ),
                        Tab::Atmosphere => {
                            atmosphere(ui, &tuning, diagnostics.as_deref(), &mut out)
                        }
                        Tab::Controls => controls(ui, &mut bindings),
                        Tab::State => state(
                            ui,
                            &universe,
                            &nav,
                            &clock,
                            (&rig, &pose, &velocity),
                            &searches,
                        ),
                        _ => {
                            report = tools_tab(
                                ui,
                                &tools,
                                (&mut core_load, &mut gpu_test, &mut atmosphere_test),
                                &starfield,
                                &metrics,
                            )
                        }
                    });
            }
        }
    });
    metrics.instrumentation_latest.debug_window_ms = window_start.elapsed().as_secs_f64() * 1000.0;
    tools.open = open;
    if report {
        tools.report = save_bug_report(
            &mut commands,
            &universe,
            &nav,
            &clock,
            (&rig, &pose),
            &starfield,
        );
    }
    writer.write_batch(out);
    typed.write_batch(lines.into_iter().map(|line| Typed {
        line,
        source: Source::Window,
        reply: None,
    }));
}
