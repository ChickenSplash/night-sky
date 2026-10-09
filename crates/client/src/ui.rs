use crate::actions::{Display, UiCommand};
use crate::presentation::PresentationMode;
use crate::{
    Clock, Navigation, Universe,
    scene::{CameraPose, CameraRig, CameraVelocity, Markers, speed_label},
};
use bevy::{math::DVec3, prelude::*};
use bevy_egui::{EguiContexts, egui};
use space_shared::{Galaxy, StarSystem, Task};
use space_simulation::{SimulationClock, period_seconds};
use std::collections::HashSet;

const MUTED: egui::Color32 = egui::Color32::from_rgb(118, 144, 174);
const ACCENT: egui::Color32 = egui::Color32::from_rgb(99, 214, 240);
const PANEL: egui::Color32 = egui::Color32::from_rgb(7, 15, 31);
const ROW: f32 = 30.0;

fn heading(ui: &mut egui::Ui, text: &str) {
    ui.label(egui::RichText::new(text).size(11.0).color(MUTED).strong());
}

fn muted(ui: &mut egui::Ui, text: impl Into<String>) {
    ui.label(egui::RichText::new(text).size(11.0).color(MUTED));
}

fn tint(colour: [u8; 3]) -> egui::Color32 {
    egui::Color32::from_rgb(colour[0], colour[1], colour[2])
}

type Overlay<'w> = (Res<'w, Markers>, Res<'w, CameraVelocity>);

/// Panel choices that are not part of the scene.
pub struct Sidebar {
    styled: bool,
    settings: bool,
    list_filter: String,
    list_sort: ListSort,
    task_sort: TaskSort,
    group: bool,
    /// Folded status groups, by lower-case status.
    collapsed: HashSet<String>,
}

impl Default for Sidebar {
    fn default() -> Self {
        Self {
            styled: false,
            settings: false,
            list_filter: String::new(),
            list_sort: ListSort::Default,
            task_sort: TaskSort::Default,
            group: true,
            collapsed: HashSet::new(),
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum ListSort {
    Default,
    Name,
    Tasks,
}

impl ListSort {
    const ALL: [Self; 3] = [Self::Default, Self::Name, Self::Tasks];
    fn label(self) -> &'static str {
        match self {
            Self::Default => "List order",
            Self::Name => "Name A–Z",
            Self::Tasks => "Most tasks",
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum TaskSort {
    Default,
    Name,
    Due,
}

impl TaskSort {
    const ALL: [Self; 3] = [Self::Default, Self::Name, Self::Due];
    fn label(self) -> &'static str {
        match self {
            Self::Default => "List order",
            Self::Name => "Name A–Z",
            Self::Due => "Due date",
        }
    }
}

pub fn draw_ui(
    interaction: (EguiContexts, MessageWriter<UiCommand>),
    universe: Res<Universe>,
    mut nav: ResMut<Navigation>,
    clock: Res<Clock>,
    overlay: Overlay,
    camera_state: (
        Res<CameraRig>,
        Res<CameraPose>,
        Res<crate::stream::Starfield>,
    ),
    mut sidebar: Local<Sidebar>,
) {
    let (mut contexts, mut writer) = interaction;
    let (markers, velocity) = overlay;
    let (rig, pose, starfield) = camera_state;
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };
    if !sidebar.styled {
        let mut style = (*ctx.style()).clone();
        style.visuals = egui::Visuals::dark();
        style.visuals.panel_fill = PANEL;
        style.visuals.window_fill = PANEL;
        style.visuals.override_text_color = Some(egui::Color32::from_rgb(214, 228, 244));
        style.visuals.selection.bg_fill = egui::Color32::from_rgb(24, 65, 85);
        style.visuals.widgets.inactive.bg_fill = egui::Color32::from_rgb(15, 31, 49);
        style.spacing.item_spacing = egui::vec2(10.0, 10.0);
        style.spacing.button_padding = egui::vec2(12.0, 8.0);
        ctx.set_style(style);
        sidebar.styled = true;
    }

    let mut out = Vec::new();
    let mut panels = Panels {
        galaxy: &universe.0,
        nav: &mut nav,
        clock: &clock.0,
        sidebar: &mut sidebar,
        out: &mut out,
    };
    panels.show(ctx);
    let mut rect = ctx.available_rect();
    if rect.width() < 1.0 || rect.height() < 1.0 {
        rect = egui::Rect::from_min_size(rect.min, egui::vec2(1.0, 1.0));
    }

    let pixels = ctx.pixels_per_point();
    nav.viewport = Some(bevy::camera::Viewport {
        physical_position: UVec2::new(
            (rect.min.x * pixels).max(0.0) as u32,
            (rect.min.y * pixels).max(0.0) as u32,
        ),
        physical_size: UVec2::new(
            (rect.width() * pixels).max(1.0) as u32,
            (rect.height() * pixels).max(1.0) as u32,
        ),
        ..default()
    });
    canvas_overlay(
        ctx,
        rect,
        &universe.0,
        &nav,
        &markers,
        &rig,
        &pose,
        &starfield,
        &velocity,
        &mut out,
    );
    crate::breadcrumb::show(
        ctx,
        rect,
        &crate::breadcrumb::trail(&nav, &universe.0),
        &mut out,
    );
    writer.write_batch(out);
}

/// The egui panels. They only read state; every change is a `UiCommand`.
struct Panels<'a> {
    galaxy: &'a Galaxy,
    nav: &'a mut Navigation,
    clock: &'a SimulationClock,
    sidebar: &'a mut Sidebar,
    out: &'a mut Vec<UiCommand>,
}

/// A row of the task index: a status group's heading, or a task.
enum Row<'a> {
    Group {
        key: String,
        name: &'a str,
        colour: [u8; 3],
        count: usize,
    },
    Task(&'a StarSystem, usize, &'a Task),
}

impl Panels<'_> {
    fn show(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("navigation")
            .exact_width(320.0)
            .resizable(false)
            .frame(egui::Frame::new().fill(PANEL).inner_margin(20))
            .show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("sidebar")
                    .show(ui, |ui| self.sidebar(ui));
            });
        egui::TopBottomPanel::bottom("time-controls")
            .frame(egui::Frame::new().fill(PANEL).inner_margin(14))
            .show(ctx, |ui| self.time_controls(ui));
        let mut open = self.sidebar.settings;
        egui::Window::new("Settings")
            .open(&mut open)
            .default_width(340.0)
            .default_height(640.0)
            .collapsible(false)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| self.settings(ui));
            });
        self.sidebar.settings = open;
    }

    /// Every section in one column, each collapsible.
    fn sidebar(&mut self, ui: &mut egui::Ui) {
        if ui
            .selectable_label(self.sidebar.settings, "⚙ Settings")
            .clicked()
        {
            self.sidebar.settings = !self.sidebar.settings;
        }
        let section = |title: &str| {
            egui::CollapsingHeader::new(egui::RichText::new(title).size(11.0).color(MUTED).strong())
                .default_open(true)
        };
        section("01 / LIST CONSTELLATIONS").show(ui, |ui| self.lists(ui));
        section("02 / TASK INDEX").show(ui, |ui| self.tasks(ui));
        section("03 / TELEMETRY").show(ui, |ui| self.telemetry(ui));
        section("04 / SELECTED PLANET").show(ui, |ui| self.selection(ui));
    }

    fn lists(&mut self, ui: &mut egui::Ui) {
        let (galaxy, nav, sidebar) = (self.galaxy, &mut *self.nav, &mut *self.sidebar);
        muted(
            ui,
            format!(
                "{} lists in your local universe\n{}\n{}",
                galaxy.systems.len(),
                galaxy.source,
                nav.notice
            ),
        );
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut sidebar.list_filter)
                    .hint_text("Filter lists…")
                    .desired_width(130.0),
            );
            egui::ComboBox::from_id_salt("list-sort")
                .selected_text(sidebar.list_sort.label())
                .show_ui(ui, |ui| {
                    for sort in ListSort::ALL {
                        ui.selectable_value(&mut sidebar.list_sort, sort, sort.label());
                    }
                });
        });
        let filter = sidebar.list_filter.trim().to_lowercase();
        let mut lists: Vec<_> = galaxy
            .systems
            .iter()
            .enumerate()
            .filter(|(_, s)| filter.is_empty() || s.name.to_lowercase().contains(&filter))
            .collect();
        match sidebar.list_sort {
            ListSort::Default => {}
            ListSort::Name => lists.sort_by_key(|(_, s)| s.name.to_lowercase()),
            ListSort::Tasks => lists.sort_by_key(|(_, s)| std::cmp::Reverse(s.tasks().count())),
        }
        egui::ScrollArea::vertical()
            .id_salt("systems")
            .max_height(170.0)
            .show(ui, |ui| {
                for (i, system) in lists {
                    let active = nav.list() == Some(i);
                    let text = format!("{}\n{} task planets", system.name, system.tasks().count());
                    if ui
                        .add_sized(
                            [ui.available_width(), 48.0],
                            egui::Button::new(text).selected(active),
                        )
                        .clicked()
                    {
                        self.out.push(UiCommand::Jump {
                            list: system.id.clone(),
                        });
                    }
                }
            });
    }

    fn tasks(&mut self, ui: &mut egui::Ui) {
        let (galaxy, nav, sidebar) = (self.galaxy, &mut *self.nav, &mut *self.sidebar);
        let search = ui.add(
            egui::TextEdit::singleline(&mut nav.search)
                .hint_text("Search all task planets…")
                .desired_width(f32::INFINITY),
        );
        ui.horizontal(|ui| {
            egui::ComboBox::from_id_salt("task-sort")
                .selected_text(sidebar.task_sort.label())
                .show_ui(ui, |ui| {
                    for sort in TaskSort::ALL {
                        ui.selectable_value(&mut sidebar.task_sort, sort, sort.label());
                    }
                });
            ui.checkbox(&mut sidebar.group, "Group by status");
        });
        let query = nav.search.trim().to_lowercase();
        // The current list's tasks, or every list's while searching.
        let mut matches: Vec<_> = galaxy
            .systems
            .iter()
            .enumerate()
            .filter(|(si, _)| !query.is_empty() || nav.list() == Some(*si))
            .flat_map(|(_, system)| {
                system
                    .tasks()
                    .filter(|(_, task)| {
                        query.is_empty() || task.name.to_lowercase().contains(&query)
                    })
                    .map(move |(pi, task)| (system, pi, task))
            })
            .collect();
        match sidebar.task_sort {
            TaskSort::Default => {}
            TaskSort::Name => matches.sort_by_key(|(_, _, t)| t.name.to_lowercase()),
            TaskSort::Due => matches.sort_by_key(|(_, _, t)| t.due_date_ms.unwrap_or(i64::MAX)),
        }
        let total = matches.len();
        let rows = if sidebar.group {
            group_rows(matches, &sidebar.collapsed)
        } else {
            matches
                .into_iter()
                .map(|(s, pi, t)| Row::Task(s, pi, t))
                .collect()
        };
        let shown = rows.iter().filter(|r| matches!(r, Row::Task(..))).count();
        // Enter finds the first task in view.
        if search.lost_focus()
            && ui.input(|i| i.key_pressed(egui::Key::Enter))
            && let Some(Row::Task(system, _, task)) =
                rows.iter().find(|r| matches!(r, Row::Task(..)))
        {
            self.out.push(UiCommand::FindTask {
                list: system.id.clone(),
                task: task.id.clone(),
            });
        }
        muted(
            ui,
            format!(
                "{total} task{}{}{}",
                if total == 1 { "" } else { "s" },
                if query.is_empty() { "" } else { " matching" },
                if total > shown {
                    format!(" · {} folded", total - shown)
                } else {
                    String::new()
                }
            ),
        );
        let selected_list = nav.list();
        let selected = nav.selected_planet();
        // Searching still scans names, but egui only builds widgets for the
        // visible rows. A list can hold over a thousand tasks.
        egui::ScrollArea::vertical()
            .id_salt("task-index")
            .auto_shrink([false, true])
            .max_height(360.0)
            .show_rows(ui, ROW, rows.len(), |ui, range| {
                for row in &rows[range] {
                    match row {
                        Row::Group {
                            key,
                            name,
                            colour,
                            count,
                        } => {
                            let folded = sidebar.collapsed.contains(key);
                            let text = egui::RichText::new(format!(
                                "{} {}  ({count})",
                                if folded { "⏵" } else { "⏷" },
                                name.to_uppercase()
                            ))
                            .size(11.0)
                            .strong()
                            .color(tint(*colour));
                            if ui
                                .add_sized(
                                    [ui.available_width(), ROW],
                                    egui::Button::selectable(false, text),
                                )
                                .clicked()
                                && !sidebar.collapsed.remove(key)
                            {
                                sidebar.collapsed.insert(key.clone());
                            }
                        }
                        Row::Task(system, pi, task) => {
                            let active = selected_list
                                .is_some_and(|i| galaxy.systems[i].id == system.id)
                                && selected == Some(*pi);
                            let due = task
                                .due_date_ms
                                .map(|ms| format!("   {}", due_short(ms)))
                                .unwrap_or_default();
                            let text = egui::RichText::new(format!("· {}{due}", task.name))
                                .color(tint(task.colour));
                            if ui
                                .add_sized(
                                    [ui.available_width(), ROW],
                                    egui::Button::selectable(active, text).truncate(),
                                )
                                .on_hover_text(&task.name)
                                .clicked()
                            {
                                self.out.push(UiCommand::FindTask {
                                    list: system.id.clone(),
                                    task: task.id.clone(),
                                });
                            }
                        }
                    }
                }
            });
    }

    fn settings(&mut self, ui: &mut egui::Ui) {
        let nav = &*self.nav;
        heading(ui, "DISPLAY");
        for (option, mut on, label) in [
            (Display::Exaggerate, nav.exaggerate, "Readable planet sizes"),
            (Display::Orbits, nav.orbits, "Orbital paths"),
            (Display::Labels, nav.labels, "Planet labels"),
            (Display::Markers, nav.markers, "Body markers"),
            (
                Display::VelocityMarker,
                nav.velocity_marker,
                "Velocity marker",
            ),
        ] {
            if ui.checkbox(&mut on, label).changed() {
                self.out.push(UiCommand::SetDisplay { option, on });
            }
        }
        let mut degrees = nav.fov_deg;
        if ui
            .add(
                egui::Slider::new(&mut degrees, crate::FOV_DEG)
                    .text("Field of view")
                    .suffix("°"),
            )
            .changed()
        {
            self.out.push(UiCommand::SetFov { degrees });
        }
        muted(
            ui,
            "Simulation mode uses physical distances. The task table keeps true body sizes with schematic spacing. Orbits keep running in every presentation mode.",
        );
        ui.separator();
        heading(ui, "RENDERING");
        let mut realistic = nav.realistic;
        if ui.checkbox(&mut realistic, "Realistic lighting").changed() {
            self.out.push(UiCommand::SetDisplay {
                option: Display::Realistic,
                on: realistic,
            });
        }
        muted(
            ui,
            "Realistic lighting draws stars by temperature and true brightness, and lights planets by their star.",
        );
        ui.separator();
        heading(ui, "CONTROLS");
        egui::Grid::new("controls").num_columns(2).show(ui, |ui| {
            for (key, action) in HELP {
                ui.label(egui::RichText::new(key).monospace().color(ACCENT));
                ui.label(action);
                ui.end_row();
            }
        });
    }

    fn telemetry(&mut self, ui: &mut egui::Ui) {
        let (galaxy, nav) = (self.galaxy, &*self.nav);
        let Some(system) = nav.system(galaxy) else {
            star_controls(ui, galaxy, nav, self.out);
            ui.label(
                egui::RichText::new(
                    "Click a star to select it, and again to visit it. Lists open at their first system.",
                )
                .color(MUTED),
            );
            return;
        };
        ui.heading(&system.name);
        ui.horizontal_wrapped(|ui| {
            for (label, presentation_mode) in [
                ("Simulation", PresentationMode::Simulation),
                ("Table", PresentationMode::Table),
                ("Constellation", PresentationMode::Constellation),
            ] {
                if ui
                    .selectable_label(nav.presentation_mode == presentation_mode, label)
                    .clicked()
                {
                    self.out
                        .push(UiCommand::SetPresentationMode { presentation_mode });
                }
            }
        });
        let pages = system.pages();
        let current = pages.iter().position(|p| *p == nav.page).unwrap_or(0);
        ui.horizontal(|ui| {
            if ui
                .add_enabled(current > 0, egui::Button::new("‹"))
                .clicked()
            {
                self.out.push(UiCommand::ShowPage {
                    page: pages[current - 1],
                });
            }
            ui.label(format!("System {} / {}", current + 1, pages.len().max(1)));
            if ui
                .add_enabled(current + 1 < pages.len(), egui::Button::new("›"))
                .clicked()
            {
                self.out.push(UiCommand::ShowPage {
                    page: pages[current + 1],
                });
            }
        });
        ui.label(
            egui::RichText::new(format!(
                "System {} · {} of {} tasks",
                nav.page + 1,
                system
                    .tasks()
                    .filter(|&(i, _)| system.planets[i].page() == nav.page)
                    .count(),
                system.tasks().count()
            ))
            .color(MUTED),
        );
        if let Some(star) = system.star(nav.page) {
            ui.collapsing("Star", |ui| {
                facts(
                    ui,
                    "star",
                    [
                        ("Type", format!("{} · {}", star.kind, star.spectral)),
                        ("Mass", format!("{:.2} solar masses", star.mass_solar)),
                        ("Radius", star_radius_text(star.radius_solar)),
                        ("Temperature", format!("{:.0} K", star.temperature_k)),
                        (
                            "Luminosity",
                            format!("{} solar", luminosity_text(star.luminosity_solar)),
                        ),
                        ("Rotation", rotation_text(star.spin.period_hours)),
                        ("Axial tilt", tilt_text(star.spin.tilt_rad)),
                    ],
                );
            });
        }
        if ui.button("Frame orbital system").clicked() {
            self.out.push(UiCommand::FrameSystem);
        }
        star_controls(ui, galaxy, nav, self.out);
    }

    fn selection(&mut self, ui: &mut egui::Ui) {
        let (galaxy, nav) = (self.galaxy, &*self.nav);
        let Some(system) = nav.system(galaxy) else {
            muted(ui, "Visit a system to inspect its planets.");
            return;
        };
        if let Some(planet) = nav.selected_planet().and_then(|i| system.planets.get(i)) {
            heading(
                ui,
                if planet.task.is_some() {
                    "SELECTED TASK"
                } else {
                    "SELECTED PLANET"
                },
            );
            ui.add_space(4.0);
            ui.heading(planet.label());
            if let Some(task) = &planet.task {
                ui.colored_label(tint(task.colour), task.status.to_uppercase());
                if task.description.is_empty() {
                    ui.label(egui::RichText::new("No description.").color(MUTED));
                } else {
                    ui.label(&task.description);
                }
                facts(
                    ui,
                    "task",
                    [
                        (
                            "Assigned",
                            if task.assignees.is_empty() {
                                "Nobody".into()
                            } else {
                                task.assignees.join(", ")
                            },
                        ),
                        (
                            "Due",
                            task.due_date_ms.map_or("No due date".into(), due_text),
                        ),
                        ("List", system.name.clone()),
                        ("Designation", planet.designation.clone()),
                    ],
                );
            } else {
                ui.label(egui::RichText::new(&planet.designation).color(MUTED));
                ui.label(egui::RichText::new("No task on this planet.").color(MUTED));
            }
            ui.add_space(6.0);
            heading(ui, "ORBITAL ELEMENTS");
            let period_days = period_seconds(&planet.orbit, system.mass(planet.page())) / 86400.0;
            facts(
                ui,
                "elements",
                [
                    ("Type", planet.class.clone()),
                    ("Mass", mass_text(planet.mass_earth)),
                    ("Physical radius", format!("{:.0} km", planet.radius_km)),
                    (
                        "Semi-major axis",
                        format!("{:.3} AU", planet.orbit.semi_major_au),
                    ),
                    ("Eccentricity", format!("{:.3}", planet.orbit.eccentricity)),
                    ("Orbital period", period_text(period_days)),
                    (
                        "Rotation",
                        if planet.spin.locked {
                            format!("Tidally locked · {}", period_text(period_days))
                        } else {
                            rotation_text(planet.spin.period_hours)
                        },
                    ),
                    ("Axial tilt", tilt_text(planet.spin.tilt_rad)),
                    (
                        "Resonance",
                        planet.resonance.map_or("None".into(), |[p, q]| {
                            format!("{p}:{q} with inner neighbour")
                        }),
                    ),
                ],
            );
            ui.add_space(10.0);
            if ui.button("◎  Follow this planet").clicked() {
                self.out.push(UiCommand::Follow);
            }
            if let Some(url) = planet.task.as_ref().and_then(|t| t.url.as_ref()) {
                ui.hyperlink_to("Open task link ↗", url);
            }
        } else {
            ui.add_space(16.0);
            ui.label(egui::RichText::new("Select a planet").size(19.0));
            ui.label(
                egui::RichText::new(
                    "Click a wireframe, its marker, or a task in the index to inspect it.",
                )
                .color(MUTED),
            );
            ui.add_space(16.0);
            heading(ui, "STATUS SIGNALS");
            for (text, colour) in [
                ("In progress", [92, 213, 242]),
                ("To do", [169, 154, 238]),
                ("Complete", [100, 220, 169]),
            ] {
                ui.colored_label(tint(colour), text);
            }
        }
    }

    fn time_controls(&mut self, ui: &mut egui::Ui) {
        let clock = self.clock;
        let step = rate_step(clock.rate);
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(step > 0, egui::Button::new("«"))
                .on_hover_text("Slow time down")
                .clicked()
            {
                self.out.push(UiCommand::SetRate {
                    rate: RATES[step - 1].1,
                });
            }
            if ui
                .button(if clock.paused {
                    "▶  Resume"
                } else {
                    "⏸  Pause"
                })
                .clicked()
            {
                self.out.push(UiCommand::SetPaused {
                    paused: !clock.paused,
                });
            }
            if ui
                .add_enabled(step + 1 < RATES.len(), egui::Button::new("»"))
                .on_hover_text("Speed time up")
                .clicked()
            {
                self.out.push(UiCommand::SetRate {
                    rate: RATES[step + 1].1,
                });
            }
            if ui
                .selectable_label(clock.rate == 1.0, "Real time")
                .clicked()
            {
                self.out.push(UiCommand::SetRate { rate: 1.0 });
            }
            ui.separator();
            ui.label(
                egui::RichText::new(if clock.paused {
                    "Paused"
                } else {
                    RATES[step].0
                })
                .color(MUTED),
            );
            ui.label(
                egui::RichText::new(elapsed_text(clock.seconds / 86400.0))
                    .monospace()
                    .color(ACCENT),
            );
        });
    }
}

/// Status groups follow the board: open, then custom, then done and closed.
fn group_rows<'a>(
    tasks: Vec<(&'a StarSystem, usize, &'a Task)>,
    collapsed: &HashSet<String>,
) -> Vec<Row<'a>> {
    struct Group<'a> {
        key: String,
        task: &'a Task,
        rank: u32,
        tasks: Vec<(&'a StarSystem, usize, &'a Task)>,
    }
    let mut groups: Vec<Group> = Vec::new();
    for entry in tasks {
        let key = entry.2.status.to_lowercase();
        match groups.iter_mut().find(|g| g.key == key) {
            Some(group) => group.tasks.push(entry),
            None => groups.push(Group {
                key,
                task: entry.2,
                rank: status_rank(entry.2),
                tasks: vec![entry],
            }),
        }
    }
    groups.sort_by(|a, b| {
        (a.rank, a.task.status_order, &a.task.status).cmp(&(
            b.rank,
            b.task.status_order,
            &b.task.status,
        ))
    });
    let mut rows = Vec::new();
    for group in groups {
        let folded = collapsed.contains(&group.key);
        rows.push(Row::Group {
            name: &group.task.status,
            colour: group.task.colour,
            count: group.tasks.len(),
            key: group.key,
        });
        if !folded {
            rows.extend(
                group
                    .tasks
                    .into_iter()
                    .map(|(s, pi, t)| Row::Task(s, pi, t)),
            );
        }
    }
    rows
}

fn status_rank(task: &Task) -> u32 {
    match task.status_type.as_str() {
        "open" => 0,
        "custom" => 1,
        "done" => 2,
        "closed" => 3,
        _ => {
            let status = task.status.to_lowercase();
            if ["complete", "done", "closed"]
                .iter()
                .any(|word| status.contains(word))
            {
                2
            } else {
                1
            }
        }
    }
}

fn facts<const N: usize>(ui: &mut egui::Ui, id: &str, rows: [(&str, String); N]) {
    egui::Grid::new(id).num_columns(2).show(ui, |ui| {
        for (label, value) in rows {
            ui.label(egui::RichText::new(label).color(MUTED));
            ui.label(value);
            ui.end_row();
        }
    });
}

/// The selected star, from anywhere in the galaxy, and how to visit it.
fn star_controls(ui: &mut egui::Ui, galaxy: &Galaxy, nav: &Navigation, out: &mut Vec<UiCommand>) {
    if let Some(star) = nav.selected_star() {
        ui.label(format!(
            "Selected {}",
            crate::galaxy_nav::label(galaxy, &star.id)
        ));
        if ui.button("Visit star (Enter)").clicked() {
            out.push(UiCommand::FrameSystem);
        }
    }
    if ui.button("Select centre star (C)").clicked() {
        out.push(UiCommand::PickCentre);
    }
}

const HELP: [(&str, &str); 13] = [
    ("Click", "Select a star, again to visit"),
    ("C", "Select the centre star"),
    ("Enter", "Visit the selected star"),
    ("Left drag", "Look around"),
    ("Right drag", "Orbit"),
    ("Shift + left drag", "Field of view"),
    ("Scroll", "Zoom"),
    ("Shift + scroll", "Adjust movement speed"),
    ("W A S D", "Move"),
    ("Q / E", "Roll left / right"),
    ("R / F", "Up / down"),
    ("Space", "Pause"),
    ("Esc", "Leave a text field"),
];

const YEAR: f64 = 31_557_600.0;
/// Simulated seconds per real second, from real time up to a century a second.
const RATES: [(&str, f64); 16] = [
    ("Real time", 1.0),
    ("10 s/s", 10.0),
    ("1 min/s", 60.0),
    ("10 min/s", 600.0),
    ("1 hr/s", 3600.0),
    ("6 hr/s", 21_600.0),
    ("1 day/s", 86_400.0),
    ("3 days/s", 259_200.0),
    ("1 week/s", 604_800.0),
    ("30 days/s", 2_592_000.0),
    ("90 days/s", 7_776_000.0),
    ("1 yr/s", YEAR),
    ("3 yr/s", 3.0 * YEAR),
    ("10 yr/s", 10.0 * YEAR),
    ("30 yr/s", 30.0 * YEAR),
    ("100 yr/s", 100.0 * YEAR),
];

/// The step on the rate ladder nearest to a rate, on a log scale.
fn rate_step(rate: f64) -> usize {
    let distance = |r: f64| (r / rate).ln().abs();
    (0..RATES.len())
        .min_by(|&a, &b| distance(RATES[a].1).total_cmp(&distance(RATES[b].1)))
        .unwrap_or(0)
}

fn elapsed_text(days: f64) -> String {
    if days >= 365.25 {
        format!("T + {:.2} yr", days / 365.25)
    } else {
        format!("T + {days:.2} days")
    }
}

const JUPITER_MASSES: f64 = 317.83;

fn mass_text(earths: f64) -> String {
    if earths >= 0.1 * JUPITER_MASSES {
        format!("{:.2} Jupiter masses", earths / JUPITER_MASSES)
    } else if earths < 0.1 {
        format!("{earths:.4} Earth masses")
    } else {
        format!("{earths:.2} Earth masses")
    }
}

fn period_text(days: f64) -> String {
    if days >= 730.0 {
        format!("{:.1} years", days / 365.25)
    } else {
        format!("{days:.1} days")
    }
}

fn rotation_text(hours: f64) -> String {
    if hours >= 48.0 {
        period_text(hours / 24.0)
    } else if hours >= 2.0 {
        format!("{hours:.1} hours")
    } else {
        format!("{:.0} minutes", hours * 60.0)
    }
}

fn tilt_text(radians: f64) -> String {
    let degrees = radians.to_degrees();
    format!(
        "{degrees:.1}°{}",
        if degrees > 90.0 { " · retrograde" } else { "" }
    )
}

fn star_radius_text(solar: f64) -> String {
    if solar < 0.05 {
        format!("{:.0} km", solar * 695_700.0)
    } else {
        format!("{solar:.2} solar radii")
    }
}

fn luminosity_text(solar: f64) -> String {
    if solar < 0.01 {
        format!("{solar:.1e}")
    } else if solar < 10.0 {
        format!("{solar:.3}")
    } else {
        format!("{solar:.0}")
    }
}

/// Day, month and year of a Unix time in milliseconds (UTC).
fn civil_date(ms: i64) -> (i64, &'static str, i64) {
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    // Howard Hinnant's days-to-civil algorithm.
    let z = ms.div_euclid(86_400_000) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    (day, MONTHS[month as usize - 1], year)
}

fn due_short(ms: i64) -> String {
    let days = (ms - crate::network::now_ms()) as f64 / 86_400_000.0;
    if days.abs() < 1.0 {
        "Today".into()
    } else if days < 0.0 {
        format!("{:.0}d late", -days)
    } else if days > 45.0 {
        let (day, month, _) = civil_date(ms);
        format!("{day} {month}")
    } else {
        format!("in {days:.0}d")
    }
}

fn due_text(ms: i64) -> String {
    let (day, month, year) = civil_date(ms);
    format!("{day} {month} {year} · {}", due_short(ms).to_lowercase())
}

/// Captions, labels, hover rings and clicks on the 3D view itself.
#[allow(clippy::too_many_arguments)]
fn canvas_overlay(
    ctx: &egui::Context,
    rect: egui::Rect,
    galaxy: &Galaxy,
    nav: &Navigation,
    markers: &Markers,
    rig: &CameraRig,
    pose: &CameraPose,
    field: &crate::stream::Starfield,
    velocity: &CameraVelocity,
    out: &mut Vec<UiCommand>,
) {
    if galaxy.systems.is_empty() {
        egui::Area::new(egui::Id::new("loading-state"))
            .fixed_pos(rect.center() - egui::vec2(160.0, 45.0))
            .show(ctx, |ui| {
                ui.set_width(320.0);
                ui.spinner();
                ui.heading("Loading your universe");
                ui.label(&galaxy.source);
                ui.label("Systems appear as each list finishes loading.");
            });
    }
    let painter = ctx
        .layer_painter(egui::LayerId::new(
            egui::Order::Background,
            egui::Id::new("annotations"),
        ))
        .with_clip_rect(rect);
    painter.text(
        rect.left_bottom() + egui::vec2(24.0, -20.0),
        egui::Align2::LEFT_BOTTOM,
        format!(
            "SET SPEED {}   /   VELOCITY {}",
            speed_label(nav.move_speed).to_uppercase(),
            speed_label(velocity.metres_per_second.length()).to_uppercase(),
        ),
        egui::FontId::monospace(11.0),
        MUTED,
    );

    if nav.velocity_marker {
        draw_velocity_marker(
            &painter,
            rect,
            rig,
            velocity.marker_metres_per_second,
            nav.tan_half_fov() as f32,
        );
    }

    // Windows such as Settings float over the view; clicks there are theirs.
    let pointer = ctx
        .input(|i| i.pointer.interact_pos())
        .filter(|_| !ctx.is_pointer_over_area());
    let mut click = pointer.is_some() && ctx.input(|i| i.pointer.primary_clicked());
    if let Some(star) = nav.selected_star() {
        let camera =
            rig.rotation().inverse() * (star.position_ly - rig.galactic_ly(pose)).as_vec3();
        if let Some(point) =
            project(rect, camera, nav.tan_half_fov() as f32).filter(|p| rect.contains(*p))
        {
            painter.circle_stroke(point, 10.0, egui::Stroke::new(1.5, ACCENT));
            painter.text(
                point + egui::vec2(16.0, -8.0),
                egui::Align2::LEFT_CENTER,
                crate::galaxy_nav::label(galaxy, &star.id),
                egui::FontId::proportional(12.0),
                ACCENT,
            );
        }
    }
    // Bevy viewport coordinates are window logical pixels; egui has a further UI scale.
    let scale = ctx.zoom_factor();
    let system = nav.system(galaxy);
    // A horizontal table needs its captions below each planet: putting them to
    // the right covers the next planet, especially on pages with many slots.
    let table_column = markers
        .0
        .windows(2)
        .map(|pair| (pair[1].point.x - pair[0].point.x).abs() / scale)
        .filter(|gap| *gap > 1.0)
        .fold(f32::INFINITY, f32::min);
    for marker in &markers.0 {
        let point = egui::pos2(marker.point.x / scale, marker.point.y / scale);
        if !rect.contains(point) {
            continue;
        }
        let c = marker.colour;
        let tint = egui::Color32::from_rgb(c[0], c[1], c[2]);
        let radius = (marker.radius / scale).max(7.0);
        let hovered = pointer.is_some_and(|p| rect.contains(p) && p.distance(point) < radius + 8.0);
        let selected = nav.selected_planet() == Some(marker.planet);
        if selected || hovered {
            painter.circle_stroke(point, radius + 5.0, egui::Stroke::new(1.0, tint));
        }
        // Markers remain visible at true physical body sizes.
        if nav.markers && marker.radius < 6.0 {
            painter.circle_stroke(point, 4.0, egui::Stroke::new(1.0, tint));
        }
        // Task labels would smear across the screen mid-flight.
        if selected
            || hovered
            || ((nav.labels || nav.presentation_mode != PresentationMode::Simulation)
                && !rig.flying())
        {
            let table = nav.presentation_mode == PresentationMode::Table && !rig.flying();
            let width = if table {
                (table_column - 12.0).clamp(40.0, 180.0)
            } else {
                220.0
            };
            let text = painter.layout(
                marker.label.clone(),
                egui::FontId::proportional(12.0),
                tint,
                width,
            );
            let (x, y) = if table {
                (
                    (point.x - text.size().x * 0.5).clamp(
                        rect.left() + 10.0,
                        (rect.right() - 10.0 - text.size().x).max(rect.left() + 10.0),
                    ),
                    point.y + radius + 10.0,
                )
            } else {
                let right = point.x + radius + 10.0;
                let x = if right + text.size().x > rect.right() - 10.0 {
                    (point.x - radius - 10.0 - text.size().x).max(rect.left() + 10.0)
                } else {
                    right
                };
                (x, point.y - text.size().y * 0.5)
            };
            painter.galley(egui::pos2(x, y), text, tint);
        }
        // A second click on the selected planet follows it.
        if hovered && click {
            click = false;
            if selected {
                out.push(UiCommand::Follow);
            } else if let Some(planet) = system.and_then(|s| s.planets.get(marker.planet)) {
                out.push(UiCommand::Inspect {
                    planet: planet.id.clone(),
                });
            }
        }
    }
    // Any other click picks from the streamed galaxy, list pages included.
    if click
        && let Some(pointer) = pointer.filter(|p| rect.contains(*p))
        && let Some(star) = crate::galaxy_nav::pick_pointer(
            field,
            rig,
            pose,
            rig.rotation(),
            [
                f64::from(pointer.x - rect.min.x),
                f64::from(pointer.y - rect.min.y),
            ],
            [f64::from(rect.width()), f64::from(rect.height())],
            nav.tan_half_fov(),
        )
    {
        out.push(UiCommand::PickStar { id: star.id });
    }
}

/// Screen point of a camera-space position or direction, through the 3D
/// camera's perspective (`tan_half` of its vertical field of view). `None`
/// behind the camera.
fn project(rect: egui::Rect, camera: Vec3, tan_half: f32) -> Option<egui::Pos2> {
    if camera.z >= 0.0 {
        return None;
    }
    let size = rect.height() / (2.0 * tan_half * -camera.z);
    Some(egui::pos2(
        rect.center().x + camera.x * size,
        rect.center().y - camera.y * size,
    ))
}

/// Prograde (dot) and retrograde (cross), placed where the direction of travel
/// vanishes in the 3D view, so they sit over the scene's own motion. Each is
/// hidden when its direction is off-screen, including pure sideways travel.
fn draw_velocity_marker(
    painter: &egui::Painter,
    rect: egui::Rect,
    rig: &CameraRig,
    velocity: DVec3,
    tan_half: f32,
) {
    if velocity.length_squared() < 1.0 || rect.width() < 60.0 || rect.height() < 60.0 {
        return;
    }
    let direction = rig.rotation().inverse() * velocity.normalize().as_vec3();
    let stroke = egui::Stroke::new(1.5, ACCENT);
    for (vector, prograde) in [(direction, true), (-direction, false)] {
        let Some(point) = project(rect, vector, tan_half).filter(|p| rect.contains(*p)) else {
            continue;
        };
        painter.circle_stroke(point, 9.0, stroke);
        if prograde {
            painter.circle_filled(point, 2.5, ACCENT);
        } else {
            for sign in [-1.0, 1.0] {
                painter.line_segment(
                    [
                        point + egui::vec2(-4.0, 4.0 * sign),
                        point + egui::vec2(4.0, -4.0 * sign),
                    ],
                    stroke,
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn travel_directions_project_through_the_camera_perspective() {
        let rect = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(1600.0, 900.0));
        let half = |degrees: f32| (degrees.to_radians() / 2.0).tan();
        let project = |camera| project(rect, camera, half(50.0));
        assert_eq!(project(-Vec3::Z), Some(rect.center()));
        // Pure sideways or backwards travel has no on-screen vanishing point.
        assert_eq!(project(-Vec3::X), None);
        assert_eq!(project(Vec3::Z), None);
        // Forward and left at 45° is off the left edge of a 50° view.
        let diagonal = Vec3::new(-1.0, 0.0, -1.0).normalize();
        assert!(!rect.contains(project(diagonal).unwrap()));
        // 25° up is the top edge.
        let up = project(Vec3::new(0.0, half(50.0), -1.0)).unwrap();
        assert!((up.y - rect.top()).abs() < 1e-3);
        // A 120° view (about 144° wide at 16:9) brings the diagonal on screen.
        let wide = super::project(rect, diagonal, half(120.0)).unwrap();
        assert!(rect.contains(wide));
    }

    #[test]
    fn civil_dates_and_rates() {
        // 2026-10-08T00:00:00Z
        assert_eq!(civil_date(1_791_417_600_000), (8, "Oct", 2026));
        assert_eq!(civil_date(0), (1, "Jan", 1970));
        assert_eq!(rate_step(1.0), 0);
        assert_eq!(rate_step(86_400.0), 6);
        assert_eq!(rate_step(1e12), RATES.len() - 1);
    }
}
