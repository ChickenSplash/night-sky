//! The debug window, kept apart from the product UI: the client log, frame and
//! star-stream timings, rendering switches, internal state and tools. ` or F3
//! toggles it on both clients. Drawn in the terminal look (`terminal.rs`).
#[cfg(target_arch = "wasm32")]
pub mod automation;
#[cfg(target_arch = "wasm32")]
pub mod benchmark;
mod capture;
pub mod core_load;
#[cfg(not(target_arch = "wasm32"))]
pub mod diagnostics;
pub mod gpu_test;
#[cfg(not(target_arch = "wasm32"))]
pub mod smoke;
mod tabs;

use crate::actions::UiCommand;
use crate::command_line::{Source, Typed};
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
#[cfg(not(target_arch = "wasm32"))]
use tabs::tools::save_bug_report;
use tabs::{
    log::{log, unseen_badges},
    perf::perf,
    render::render,
    state::state,
    terminal::terminal_tab,
    tools::tools_tab,
};

/// Frame times kept for the graph.
const FRAMES: usize = 240;

#[derive(Clone, Copy, PartialEq)]
enum Tab {
    Log,
    Terminal,
    Perf,
    Render,
    State,
    Tools,
}

impl Tab {
    const ALL: [(Self, &str); 6] = [
        (Self::Log, "log"),
        (Self::Terminal, "terminal"),
        (Self::Perf, "perf"),
        (Self::Render, "render"),
        (Self::State, "state"),
        (Self::Tools, "tools"),
    ];
}

pub struct DevTools {
    open: bool,
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
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
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
    Res<'w, Starfield>,
    Res<'w, StreamMetrics>,
    Res<'w, RenderDebug>,
    Option<Res<'w, DiagnosticsStore>>,
    ResMut<'w, CoreLoad>,
    ResMut<'w, GpuTest>,
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
    mut tools: Local<DevTools>,
) {
    let (mut contexts, mut writer, mut typed, mut commands) = interaction;
    #[cfg(target_arch = "wasm32")]
    let _ = &mut commands;
    let (universe, nav, clock, time) = scene;
    let (rig, pose, velocity) = camera;
    let (starfield, metrics, debug, diagnostics, mut core_load, mut gpu_test) = rendering;
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
        tools.open = !tools.open;
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
    terminal::window(ctx, "debug", &mut open, |ui| {
        ui.horizontal(|ui| {
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
        ui.add_space(4.0);
        match tools.tab {
            Tab::Log => {
                tools.unseen = [0; 2];
                log(ui, &mut tools);
            }
            Tab::Terminal => terminal_tab(ui, &mut tools, &mut lines),
            tab => {
                egui::ScrollArea::vertical()
                    .id_salt("debug-tab")
                    .auto_shrink(false)
                    .show(ui, |ui| match tab {
                        Tab::Perf => perf(
                            ui,
                            &tools.frames,
                            &metrics,
                            &starfield,
                            diagnostics.as_deref(),
                        ),
                        Tab::Render => render(ui, &nav, &debug, &mut out),
                        Tab::State => state(ui, &universe, &nav, &clock, &rig, &pose, &velocity),
                        _ => {
                            report = tools_tab(
                                ui,
                                &tools,
                                (&mut core_load, &mut gpu_test),
                                &starfield,
                                &metrics,
                            )
                        }
                    });
            }
        }
    });
    tools.open = open;
    #[cfg(not(target_arch = "wasm32"))]
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
    #[cfg(target_arch = "wasm32")]
    let _ = report;
    writer.write_batch(out);
    typed.write_batch(lines.into_iter().map(|line| Typed {
        line,
        source: Source::Window,
    }));
}
