//! The debug window, kept apart from the product UI: the client log, frame and
//! star-stream timings, rendering switches, internal state and tools. ` or F3
//! toggles it on both clients. Drawn in the terminal look (`terminal.rs`).
use crate::actions::{Display, GalaxyReference, UiCommand};
use crate::command_line::{self, Source, Typed};
use crate::render::{Lights, exposure_caption};
use crate::scene::{CameraPose, CameraRig, CameraVelocity, speed_label};
use crate::stream::{Starfield, StreamMetrics};
use crate::terminal::{self, DIM, TEXT};
use crate::views::View;
use crate::{Clock, Navigation, Selection, Universe, Visit};
use bevy::log::tracing::{
    Event, Subscriber,
    field::{Field, Visit as Fields},
};
use bevy::log::tracing_subscriber::{Layer, layer::Context};
use bevy::log::{BoxedLayer, Level};
use bevy::platform::time::Instant;
use bevy::prelude::*;
use bevy_egui::{EguiContexts, egui};
use space_simulation::{AU_METRES, LY_METRES};
use std::collections::VecDeque;
use std::fmt::{self, Write};
use std::sync::{Mutex, OnceLock};

/// Log lines kept in memory for the console.
const LOG_LINES: usize = 2000;
/// Frame times kept for the graph.
const FRAMES: usize = 240;
const LEVELS: [&str; 4] = ["error", "warn", "info", "debug"];

#[derive(Clone)]
struct Line {
    seconds: f64,
    level: usize,
    target: String,
    text: String,
    /// Lower-case target and text, for the console filter.
    search: String,
}

struct Log {
    lines: VecDeque<Line>,
    /// Lines logged so far, so the console can tell which are new.
    total: u64,
}

static LOG: Mutex<Log> = Mutex::new(Log {
    lines: VecDeque::new(),
    total: 0,
});
static START: OnceLock<Instant> = OnceLock::new();

/// Bevy's extra log layers: the console's buffer, plus the log file natively.
pub fn log_layers(app: &mut App) -> Option<BoxedLayer> {
    START.get_or_init(Instant::now);
    let console: BoxedLayer = Box::new(Console);
    #[cfg(not(target_arch = "wasm32"))]
    if let Some(file) = crate::diagnostics::file_layer(app) {
        return Some(Box::new(vec![file, console]));
    }
    let _ = app;
    Some(console)
}

/// Copies each event into `LOG`. Never log while holding its lock.
struct Console;

impl<S: Subscriber> Layer<S> for Console {
    fn on_event(&self, event: &Event<'_>, _: Context<'_, S>) {
        let mut fields = Message::default();
        event.record(&mut fields);
        let meta = event.metadata();
        let target = fields.target.unwrap_or_else(|| meta.target().to_string());
        // One row per line keeps the console's rows a fixed height.
        let text = (fields.message + &fields.extra).replace('\n', " ⏎ ");
        let line = Line {
            seconds: START.get().map_or(0.0, |s| s.elapsed().as_secs_f64()),
            level: match *meta.level() {
                Level::ERROR => 0,
                Level::WARN => 1,
                Level::INFO => 2,
                _ => 3,
            },
            search: format!("{target} {text}").to_lowercase(),
            target,
            text,
        };
        if let Ok(mut log) = LOG.lock() {
            if log.lines.len() == LOG_LINES {
                log.lines.pop_front();
            }
            log.lines.push_back(line);
            log.total += 1;
        }
    }
}

#[derive(Default)]
struct Message {
    message: String,
    extra: String,
    /// The original target of a `log` crate record.
    target: Option<String>,
}

impl Message {
    fn add(&mut self, field: &Field, value: fmt::Arguments) {
        match field.name() {
            "message" => {
                let _ = self.message.write_fmt(value);
            }
            "log.target" => self.target = Some(value.to_string()),
            name if name.starts_with("log.") => {}
            name => {
                let _ = write!(self.extra, " {name}={value}");
            }
        }
    }
}

impl Fields for Message {
    fn record_str(&mut self, field: &Field, value: &str) {
        self.add(field, format_args!("{value}"));
    }

    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        self.add(field, format_args!("{value:?}"));
    }
}

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

impl DevTools {
    /// Takes the lines logged since the last call.
    fn sync(&mut self) {
        let Ok(log) = LOG.lock() else {
            return;
        };
        let new = (log.total - self.seen).min(log.lines.len() as u64) as usize;
        for line in log.lines.iter().skip(log.lines.len() - new) {
            if let Some(unseen) = self.unseen.get_mut(line.level) {
                *unseen += 1;
            }
            if line.target == command_line::TARGET {
                self.terminal.push_back(line.clone());
            }
            self.lines.push_back(line.clone());
        }
        self.seen = log.total;
        drop(log);
        for lines in [&mut self.lines, &mut self.terminal] {
            while lines.len() > LOG_LINES {
                lines.pop_front();
            }
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
    Res<'w, Lights>,
    Res<'w, crate::sky::Sky>,
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
    let (starfield, metrics, lights, sky) = rendering;
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
                        Tab::Perf => perf(ui, &tools.frames, &metrics, &starfield),
                        Tab::Render => render(ui, &nav, &lights, &sky, &mut out),
                        Tab::State => state(ui, &universe, &nav, &clock, &rig, &pose, &velocity),
                        _ => report = tools_tab(ui, &tools),
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

/// Errors and warnings logged out of sight, such as `2 err 5 warn`.
fn unseen_badges(ui: &mut egui::Ui, unseen: [usize; 2]) {
    for (level, (count, label)) in unseen.into_iter().zip(["err", "warn"]).enumerate() {
        if count > 0 {
            let count = if count > 99 {
                "99+".to_string()
            } else {
                count.to_string()
            };
            ui.label(
                egui::RichText::new(format!("{count} {label}"))
                    .font(egui::FontId::monospace(11.0))
                    .color(level_colour(level)),
            );
        }
    }
}

fn log(ui: &mut egui::Ui, tools: &mut DevTools) {
    ui.horizontal(|ui| {
        for (i, level) in LEVELS.iter().enumerate() {
            if terminal::toggle(ui, tools.levels[i], level) {
                tools.levels[i] = !tools.levels[i];
            }
        }
        ui.add_space(8.0);
        terminal::input(ui, "grep", &mut tools.grep, 160.0);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if terminal::button(ui, "clear") {
                tools.lines.clear();
            }
        });
    });
    let grep = tools.grep.trim().to_lowercase();
    let rows: Vec<&Line> = tools
        .lines
        .iter()
        .filter(|l| tools.levels[l.level] && (grep.is_empty() || l.search.contains(&grep)))
        .collect();
    terminal::note(
        ui,
        &format!("{} of {} lines", rows.len(), tools.lines.len()),
    );
    let font = egui::FontId::monospace(12.0);
    let height = ui.fonts_mut(|f| f.row_height(&font));
    egui::ScrollArea::both()
        .id_salt("debug-log")
        .auto_shrink(false)
        .stick_to_bottom(true)
        .show_rows(ui, height, rows.len(), |ui, range| {
            ui.style_mut().spacing.item_spacing.y = 0.0;
            for line in &rows[range] {
                let mut job = egui::text::LayoutJob::default();
                let mut add = |text: &str, colour| {
                    job.append(text, 0.0, egui::TextFormat::simple(font.clone(), colour))
                };
                add(&format!("{:>9.3} ", line.seconds), DIM);
                add(
                    &format!("{:<5} ", LEVELS[line.level].to_uppercase()),
                    level_colour(line.level),
                );
                add(&format!("{}: ", line.target), DIM);
                add(&line.text, TEXT);
                ui.add(egui::Label::new(job).wrap_mode(egui::TextWrapMode::Extend));
            }
        });
}

/// Commands and their output, with the command line below.
fn terminal_tab(ui: &mut egui::Ui, tools: &mut DevTools, lines: &mut Vec<String>) {
    let font = egui::FontId::monospace(12.0);
    let height = ui.fonts_mut(|f| f.row_height(&font));
    let prompt = ui.fonts_mut(|f| f.row_height(&terminal::font())) + 12.0;
    if tools.terminal.is_empty() {
        terminal::note(
            ui,
            "Type help for the commands. On the desktop, the terminal that ran ./scripts/run.sh takes them too.",
        );
    }
    egui::ScrollArea::both()
        .id_salt("debug-terminal")
        .auto_shrink(false)
        .max_height((ui.available_height() - prompt).max(height))
        .stick_to_bottom(true)
        .show_rows(ui, height, tools.terminal.len(), |ui, range| {
            ui.style_mut().spacing.item_spacing.y = 0.0;
            for line in tools.terminal.range(range) {
                let colour = if line.text.starts_with("> ") {
                    terminal::CYAN
                } else if line.level < 2 {
                    level_colour(line.level)
                } else {
                    TEXT
                };
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(&line.text)
                            .font(font.clone())
                            .color(colour),
                    )
                    .wrap_mode(egui::TextWrapMode::Extend),
                );
            }
        });
    ui.add_space(4.0);
    if let Some(line) = command_line(ui, tools) {
        // Clearing is the window's own business; everything else runs as typed.
        if line.trim() == "clear" {
            tools.terminal.clear();
        } else {
            lines.push(line);
        }
    }
}

/// `> command`: Enter returns it, the arrow keys recall earlier ones.
fn command_line(ui: &mut egui::Ui, tools: &mut DevTools) -> Option<String> {
    let response = ui
        .horizontal(|ui| {
            let width = ui.available_width() - 24.0;
            terminal::input(ui, ">", &mut tools.command, width)
        })
        .inner;
    if std::mem::take(&mut tools.focus) {
        response.request_focus();
    }
    if response.has_focus() {
        // Up steps back through the history; down past the newest clears the line.
        let steps = ui.input(|i| {
            i.num_presses(egui::Key::ArrowDown) as isize
                - i.num_presses(egui::Key::ArrowUp) as isize
        });
        let last = tools.history.len() as isize;
        if steps != 0 && last > 0 {
            let from = tools.recalled.map_or(last, |i| i as isize);
            let to = (from + steps).clamp(0, last);
            let recalled = (to < last).then_some(to as usize);
            tools.recalled = recalled;
            tools.command = recalled.map_or(String::new(), |i| tools.history[i].clone());
            // Put the cursor after the recalled text, not where the arrow left it.
            if let Some(mut state) = egui::TextEdit::load_state(ui.ctx(), response.id) {
                let end = egui::text::CCursor::new(tools.command.chars().count());
                state
                    .cursor
                    .set_char_range(Some(egui::text::CCursorRange::one(end)));
                state.store(ui.ctx(), response.id);
            }
        }
    }
    if !(response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))) {
        return None;
    }
    response.request_focus();
    let line = std::mem::take(&mut tools.command);
    tools.recalled = None;
    if line.trim().is_empty() {
        return None;
    }
    if tools.history.last() != Some(&line) {
        tools.history.push(line.clone());
    }
    Some(line)
}

fn level_colour(level: usize) -> egui::Color32 {
    match level {
        0 => egui::Color32::from_rgb(255, 110, 110),
        1 => egui::Color32::from_rgb(255, 196, 100),
        2 => TEXT,
        _ => DIM,
    }
}

fn perf(ui: &mut egui::Ui, frames: &VecDeque<f32>, metrics: &StreamMetrics, field: &Starfield) {
    terminal::heading(ui, "frames");
    let recent: Vec<f32> = frames.iter().rev().take(60).copied().collect();
    let mean = recent.iter().sum::<f32>() / recent.len().max(1) as f32;
    let worst = recent.iter().copied().fold(0.0, f32::max);
    terminal::facts(
        ui,
        "debug-frames",
        &[
            ("fps", format!("{:.0}", 1000.0 / mean.max(0.001))),
            ("frame", format!("{mean:.1} ms mean, {worst:.1} ms worst")),
        ],
    );
    frame_graph(ui, frames);
    terminal::heading(ui, "star stream");
    terminal::note(ui, &metrics.caption);
    terminal::note(ui, &field.summary());
}

/// Frame times, newest on the right, against 60 and 30 fps guides.
fn frame_graph(ui: &mut egui::Ui, frames: &VecDeque<f32>) {
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 80.0), egui::Sense::hover());
    let painter = ui.painter();
    // Spikes past 15 fps clip, so the guides stay apart.
    let top = frames.iter().copied().fold(40.0, f32::max).min(66.7);
    let y = |ms: f32| rect.bottom() - rect.height() * (ms / top).min(1.0);
    for (ms, label) in [(1000.0 / 60.0, "60 fps"), (1000.0 / 30.0, "30 fps")] {
        painter.hline(rect.x_range(), y(ms), (1.0, DIM.gamma_multiply(0.4)));
        painter.text(
            egui::pos2(rect.left() + 4.0, y(ms) - 2.0),
            egui::Align2::LEFT_BOTTOM,
            label,
            egui::FontId::monospace(10.0),
            DIM,
        );
    }
    let step = rect.width() / (FRAMES - 1) as f32;
    let start = rect.right() - step * (frames.len().max(1) - 1) as f32;
    let points = frames
        .iter()
        .enumerate()
        .map(|(i, ms)| egui::pos2(start + step * i as f32, y(*ms)))
        .collect();
    painter.add(egui::Shape::line(points, (1.0, TEXT)));
}

fn render(
    ui: &mut egui::Ui,
    nav: &Navigation,
    lights: &Lights,
    sky: &crate::sky::Sky,
    out: &mut Vec<UiCommand>,
) {
    terminal::heading(ui, "exposure");
    terminal::note(ui, &exposure_caption(nav, lights, sky));
    terminal::heading(ui, "realistic rendering");
    for (option, on, label) in [
        (
            Display::GalaxyGlow,
            nav.galaxy_glow,
            "Smooth galaxy glow (prototype)",
        ),
        (Display::Physical, nav.physical, "Physical exposure"),
        (Display::Wireframe, nav.wireframe, "Wireframes over bodies"),
    ] {
        if terminal::check(ui, on, label, nav.realistic) {
            out.push(UiCommand::SetDisplay { option, on: !on });
        }
    }
    terminal::note(
        ui,
        if nav.realistic {
            "Physical exposure shows true contrast, as one camera exposure adapting like an eye; assisted exposure keeps faint planets and stars readable."
        } else {
            "Turn on realistic lighting in Settings to use these."
        },
    );
    terminal::heading(ui, "galaxy reference views");
    ui.horizontal_wrapped(|ui| {
        for (view, label) in [
            (GalaxyReference::Inside, "inside"),
            (GalaxyReference::FaceOn, "face-on"),
            (GalaxyReference::EdgeOn, "edge-on"),
        ] {
            if terminal::button(ui, label) {
                out.push(UiCommand::GalaxyReference { view });
            }
        }
    });
    terminal::note(ui, "Smooth glow only: no dust or spiral detail yet.");
}

fn state(
    ui: &mut egui::Ui,
    universe: &Universe,
    nav: &Navigation,
    clock: &Clock,
    rig: &CameraRig,
    pose: &CameraPose,
    velocity: &CameraVelocity,
) {
    let galaxy = &universe.0;
    let ly = |v: bevy::math::DVec3| format!("{:+.3}  {:+.3}  {:+.3}", v.x, v.y, v.z);
    let range = if pose.distance > LY_METRES * 0.01 {
        format!("{:.3} ly", pose.distance / LY_METRES)
    } else {
        format!("{:.4} AU", pose.distance / AU_METRES)
    };
    terminal::heading(ui, "camera");
    terminal::facts(
        ui,
        "debug-camera",
        &[
            (
                "mode",
                if nav.following {
                    "tracking"
                } else if nav.view != View::Orbit {
                    "auto-framed preset"
                } else {
                    "free navigation"
                }
                .into(),
            ),
            ("range", range),
            ("set speed", speed_label(nav.move_speed)),
            ("velocity", speed_label(velocity.metres_per_second.length())),
            ("galactic (ly)", ly(rig.galactic_ly(pose))),
            ("field of view", format!("{:.0}°", nav.fov_deg)),
            (
                "flight",
                rig.progress()
                    .map_or("none".into(), |p| format!("{:.0}%", p * 100.0)),
            ),
        ],
    );
    terminal::heading(ui, "navigation");
    let system = nav.system(galaxy);
    let star = |system: Option<&space_shared::StarSystem>| {
        system
            .and_then(|s| s.page_star(nav.page))
            .map(|star| crate::galaxy_nav::label(galaxy, &star.id))
    };
    terminal::facts(
        ui,
        "debug-navigation",
        &[
            (
                "location",
                star(nav.location(galaxy)).unwrap_or_else(|| "open space".into()),
            ),
            (
                "list",
                nav.list()
                    .and_then(|i| galaxy.systems.get(i))
                    .map_or("none".into(), |s| s.name.clone()),
            ),
            (
                "anchor",
                match &nav.visit {
                    None => "home".into(),
                    Some(Visit::List(_)) => star(system).unwrap_or_else(|| "?".into()),
                    Some(Visit::Generated(s)) => s.name.clone(),
                },
            ),
            ("page", nav.page.to_string()),
            ("view", format!("{:?}", nav.view)),
            (
                "selected",
                match &nav.selection {
                    None => "none".into(),
                    Some(Selection::Star(star)) => format!("star {}", star.id),
                    Some(Selection::Planet(i)) => format!(
                        "planet {}",
                        system
                            .and_then(|s| s.planets.get(*i))
                            .map_or("?", |p| p.id.as_str())
                    ),
                },
            ),
        ],
    );
    if let Some(system) = system {
        let [inclination, node, _] = crate::scene::page_tilt(system, nav.page);
        terminal::heading(ui, "anchor star");
        terminal::facts(
            ui,
            "debug-star",
            &[
                (
                    "position (ly)",
                    ly(crate::scene::page_origin_ly(system, nav.page)),
                ),
                (
                    "plane tilt",
                    format!(
                        "{:.0}°, node {:.0}°",
                        inclination.to_degrees(),
                        node.to_degrees()
                    ),
                ),
            ],
        );
    }
    terminal::heading(ui, "clock");
    terminal::facts(
        ui,
        "debug-clock",
        &[
            ("simulated", format!("{:.3} s", clock.0.seconds)),
            ("rate", format!("{} s/s", clock.0.rate)),
            ("paused", clock.0.paused.to_string()),
        ],
    );
}

/// Returns whether Save bug report was clicked.
fn tools_tab(ui: &mut egui::Ui, tools: &DevTools) -> bool {
    #[cfg(not(target_arch = "wasm32"))]
    {
        terminal::heading(ui, "bug report");
        let report = terminal::button(ui, "save bug report");
        terminal::note(
            ui,
            "Saves state, recent log lines and a screenshot under data/logs/bug-*.",
        );
        if !tools.report.is_empty() {
            ui.label(
                egui::RichText::new(&tools.report)
                    .font(terminal::font())
                    .color(terminal::CYAN),
            );
        }
        report
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = tools;
        terminal::heading(ui, "benchmark");
        if terminal::button(ui, "run star-stream benchmark") {
            crate::benchmark::restart();
        }
        terminal::note(
            ui,
            "Reloads with no stars loaded, then flies 15 seconds from home towards the galactic centre at 1,000 ly/s. Results appear on screen and in the browser console.",
        );
        false
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn save_bug_report(
    commands: &mut Commands,
    universe: &Universe,
    nav: &Navigation,
    clock: &Clock,
    (rig, pose): (&CameraRig, &CameraPose),
    starfield: &Starfield,
) -> String {
    let (galaxy, clock) = (&universe.0, &clock.0);
    let system = nav.system(galaxy);
    let state = format!(
        "System ID: {}\nArrived: {}\nList: {}\nPage: {}\nView: {:?}\nFollowing: {}\nSelection: {:?}\nSource: {}\nSystems: {}\nTasks: {}\nSimulated seconds: {:.3}\nRate: {:.3}\nPaused: {}\nDesired target (m): {:?}\nDesired range (m): {:.3}\nDisplayed eye relative to page star (m): {:?}\nDisplayed range (m): {:.3}\nFlight progress: {:?}\nGalaxy: {}\n",
        system.map_or("none", |s| s.id.as_str()),
        nav.arrived,
        nav.list().is_some(),
        nav.page,
        nav.view,
        nav.following,
        nav.selection,
        galaxy.source,
        galaxy.systems.len(),
        galaxy
            .systems
            .iter()
            .map(|s| s.planets.len())
            .sum::<usize>(),
        clock.seconds,
        clock.rate,
        clock.paused,
        nav.target,
        nav.distance,
        pose.eye,
        pose.distance,
        rig.progress(),
        starfield.summary(),
    );
    match crate::diagnostics::save_bug_report(&state) {
        Ok(path) => {
            info!("Bug report saved to {}", path.display());
            commands
                .spawn(bevy::render::view::screenshot::Screenshot::primary_window())
                .observe(bevy::render::view::screenshot::save_to_disk(
                    path.with_extension("png"),
                ));
            format!("Saved {}", path.display())
        }
        Err(err) => format!("Could not save bug report: {err}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::log::tracing_subscriber::{Registry, layer::SubscriberExt};

    #[test]
    fn console_keeps_messages_and_fields_on_one_line() {
        let subscriber = Registry::default().with(Console);
        bevy::log::tracing::subscriber::with_default(subscriber, || {
            warn!(cells = 175, "Console test\nsecond line");
        });
        let log = LOG.lock().unwrap();
        let line = log
            .lines
            .iter()
            .find(|l| l.text.starts_with("Console test"))
            .unwrap();
        assert_eq!(line.text, "Console test ⏎ second line cells=175");
        assert_eq!(line.level, 1);
        assert!(line.search.contains("console test"));
    }
}
