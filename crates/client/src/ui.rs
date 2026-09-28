use crate::actions::{Display, PanelInsets, UiCommand};
use crate::views::View;
use crate::{
    Clock, Navigation, Universe,
    render::{Lights, exposure_caption},
    scene::{CameraPose, CameraRig, Markers},
};
use bevy::prelude::*;
use bevy_egui::{EguiContexts, egui};
use space_shared::Galaxy;
use space_simulation::{AU_METRES, LY_METRES, SimulationClock, period_seconds};

const MUTED: egui::Color32 = egui::Color32::from_rgb(118, 144, 174);
const ACCENT: egui::Color32 = egui::Color32::from_rgb(99, 214, 240);
const PANEL: egui::Color32 = egui::Color32::from_rgb(7, 15, 31);

fn heading(ui: &mut egui::Ui, text: &str) {
    ui.label(egui::RichText::new(text).size(11.0).color(MUTED).strong());
}

pub fn draw_ui(
    interaction: (Commands, EguiContexts, MessageWriter<UiCommand>),
    universe: Res<Universe>,
    mut nav: ResMut<Navigation>,
    clock: Res<Clock>,
    overlay: (Res<Markers>, Res<PanelInsets>, Res<Lights>),
    camera_state: (
        Res<CameraRig>,
        Res<CameraPose>,
        Res<crate::stream::Starfield>,
    ),
    mut styled: Local<bool>,
) {
    let (mut commands, mut contexts, mut writer) = interaction;
    #[cfg(target_arch = "wasm32")]
    let _ = &mut commands;
    let (markers, insets, lights) = overlay;
    let (rig, pose, starfield) = camera_state;
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };
    if !*styled {
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
        *styled = true;
    }

    let mut out = Vec::new();
    let mut rect = ctx.available_rect();
    if let Some([top, right, bottom, left]) = insets.0 {
        // HTML panels cover the canvas edges; the 3D view fits the space between.
        let scale = ctx.zoom_factor();
        rect = egui::Rect::from_min_max(
            rect.min + egui::vec2(left, top) / scale,
            rect.max - egui::vec2(right, bottom) / scale,
        );
    } else {
        let mut panels = Panels {
            galaxy: &universe.0,
            nav: &mut nav,
            clock: &clock.0,
            out: &mut out,
            report: false,
        };
        panels.show(ctx);
        let report = panels.report;
        rect = ctx.available_rect();
        #[cfg(not(target_arch = "wasm32"))]
        if report {
            nav.notice = save_bug_report(
                &mut commands,
                &universe.0,
                &nav,
                &clock.0,
                (&rig, &pose),
                &starfield,
            );
        }
        #[cfg(target_arch = "wasm32")]
        let _ = report;
    }
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
        &exposure_caption(&nav, &lights),
        insets.0.is_none(),
        &mut out,
    );
    writer.write_batch(out);
}

/// The egui panels: the native client's UI, and the browser's until HTML panels
/// take over. They only read state; every change is a `UiCommand`.
struct Panels<'a> {
    galaxy: &'a Galaxy,
    nav: &'a mut Navigation,
    clock: &'a SimulationClock,
    out: &'a mut Vec<UiCommand>,
    /// Save bug report was clicked.
    report: bool,
}

impl Panels<'_> {
    fn show(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("header")
            .frame(egui::Frame::new().fill(PANEL).inner_margin(18))
            .show(ctx, |ui| self.header(ui));
        egui::TopBottomPanel::bottom("footer")
            .frame(egui::Frame::new().fill(PANEL).inner_margin(12))
            .show(ctx, |ui| self.footer(ui));
        egui::SidePanel::left("navigation")
            .exact_width(270.0)
            .resizable(false)
            .frame(egui::Frame::new().fill(PANEL).inner_margin(20))
            .show(ctx, |ui| self.navigation(ui));
        egui::SidePanel::right("inspector")
            .exact_width(280.0)
            .resizable(false)
            .frame(egui::Frame::new().fill(PANEL).inner_margin(20))
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| self.inspector(ui));
            });
        egui::TopBottomPanel::bottom("time-controls")
            .frame(egui::Frame::new().fill(PANEL).inner_margin(14))
            .show(ctx, |ui| self.time_controls(ui));
    }

    fn header(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new("C L I C K U P   /   S P A C E")
                    .size(19.0)
                    .strong(),
            );
            ui.separator();
            ui.label(
                egui::RichText::new("MISSION CONTROL")
                    .color(ACCENT)
                    .size(11.0),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(
                    egui::RichText::new(&self.galaxy.source)
                        .color(MUTED)
                        .size(12.0),
                );
            });
        });
    }

    fn footer(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            ui.label(egui::RichText::new("Click: select a star, again to visit    C: centre star    Enter: visit    LMB drag: look    RMB drag: orbit    Scroll: zoom    WASD: move    Q/E: roll    R/F: up/down    Shift: boost    Space: pause").size(11.0).color(MUTED));
            ui.separator();
            ui.label(egui::RichText::new(&self.nav.notice).size(11.0).color(MUTED));
        });
    }

    fn navigation(&mut self, ui: &mut egui::Ui) {
        let (galaxy, nav) = (self.galaxy, &mut *self.nav);
        heading(ui, "01 / NAVIGATION");
        ui.add_space(8.0);
        ui.heading("List constellations");
        ui.label(
            egui::RichText::new(format!(
                "{} lists in your local universe",
                galaxy.systems.len()
            ))
            .color(MUTED),
        );
        ui.add_space(8.0);
        egui::ScrollArea::vertical()
            .id_salt("systems")
            .max_height(180.0)
            .show(ui, |ui| {
                for (i, system) in galaxy.systems.iter().enumerate() {
                    let active = nav.list() == Some(i);
                    let text = format!("{}\n{} task planets", system.name, system.tasks().count());
                    if ui
                        .add_sized(
                            [ui.available_width(), 54.0],
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
        ui.add_space(14.0);
        ui.separator();
        heading(ui, "02 / TASK INDEX");
        ui.add(
            egui::TextEdit::singleline(&mut nav.search)
                .hint_text("Search all task planets…")
                .desired_width(f32::INFINITY),
        );
        let query = nav.search.to_lowercase();
        // Searching still scans names, but egui only builds widgets for the
        // visible rows. Some live lists have over a thousand tasks.
        let matches: Vec<_> = galaxy
            .systems
            .iter()
            .enumerate()
            .filter(|(si, _)| !query.is_empty() || nav.list() == Some(*si))
            .flat_map(|(si, system)| {
                system
                    .tasks()
                    .filter(|(_, task)| {
                        query.is_empty() || task.name.to_lowercase().contains(&query)
                    })
                    .map(move |(pi, task)| (si, pi, system, task))
            })
            .collect();
        egui::ScrollArea::vertical()
            .id_salt("task-index")
            .max_height((ui.available_height() - 180.0).clamp(60.0, 240.0))
            .show_rows(ui, 30.0, matches.len(), |ui, range| {
                for &(si, pi, system, task) in &matches[range] {
                    let c = task.colour;
                    let tint = egui::Color32::from_rgb(c[0], c[1], c[2]);
                    let text = egui::RichText::new(format!("· {}", task.name)).color(tint);
                    if ui
                        .add_sized(
                            [ui.available_width(), 30.0],
                            egui::Button::selectable(
                                nav.list() == Some(si) && nav.selected == Some(pi),
                                text,
                            ),
                        )
                        .clicked()
                    {
                        self.out.push(UiCommand::FindTask {
                            list: system.id.clone(),
                            task: task.id.clone(),
                        });
                    }
                }
            });
        ui.add_space(12.0);
        ui.separator();
        heading(ui, "DISPLAY");
        for (option, mut on, label) in [
            (Display::Exaggerate, nav.exaggerate, "Readable planet sizes"),
            (Display::Orbits, nav.orbits, "Orbital paths"),
            (Display::Labels, nav.labels, "Planet labels"),
            (Display::Markers, nav.markers, "Body markers"),
        ] {
            if ui.checkbox(&mut on, label).changed() {
                self.out.push(UiCommand::SetDisplay { option, on });
            }
        }
        ui.label(
            egui::RichText::new(
                "Orbit view uses physical distances. The task table keeps true body sizes with schematic spacing. Orbits keep running in every view.",
            )
            .size(11.0)
            .color(MUTED),
        );
        ui.add_space(8.0);
        heading(ui, "RENDERING");
        let mut realistic = nav.realistic;
        if ui.checkbox(&mut realistic, "Realistic lighting").changed() {
            self.out.push(UiCommand::SetDisplay {
                option: Display::Realistic,
                on: realistic,
            });
        }
        for (option, mut on, label) in [
            (Display::Physical, nav.physical, "Physical exposure"),
            (Display::Wireframe, nav.wireframe, "Wireframes over bodies"),
        ] {
            if ui
                .add_enabled(nav.realistic, egui::Checkbox::new(&mut on, label))
                .changed()
            {
                self.out.push(UiCommand::SetDisplay { option, on });
            }
        }
        ui.label(
            egui::RichText::new(
                "Realistic lighting draws stars by temperature and true brightness, and lights planets by their star. Assisted exposure keeps faint planets and stars readable; physical exposure shows true contrast.",
            )
            .size(11.0)
            .color(MUTED),
        );
    }

    fn inspector(&mut self, ui: &mut egui::Ui) {
        let (galaxy, nav) = (self.galaxy, &*self.nav);
        heading(ui, "03 / TELEMETRY");
        ui.add_space(8.0);
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
            for (label, view) in [
                ("Orbit", View::Orbit),
                ("Planet table", View::Table),
                ("Constellation", View::Constellation),
            ] {
                if ui.selectable_label(nav.view == view, label).clicked() {
                    self.out.push(UiCommand::SetView { view });
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
            ui.label(format!("Page {} / {}", current + 1, pages.len()));
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
            ui.label(
                egui::RichText::new(format!(
                    "{} · {}\n{:.2} solar masses · {:.0} K",
                    star.kind, star.spectral, star.mass_solar, star.temperature_k
                ))
                .color(MUTED),
            );
        }
        let p = crate::scene::page_origin_ly(system, nav.page);
        ui.label(
            egui::RichText::new(format!(
                "Page star position (ly)\n{:+.2}  {:+.2}  {:+.2}",
                p.x, p.y, p.z
            ))
            .monospace()
            .size(11.0)
            .color(MUTED),
        );
        let [inclination, node, _] = crate::scene::page_tilt(system, nav.page);
        ui.label(
            egui::RichText::new(format!(
                "Page {} plane tilt {:.0}°, node {:.0}°",
                nav.page + 1,
                inclination.to_degrees(),
                node.to_degrees()
            ))
            .monospace()
            .size(11.0)
            .color(MUTED),
        );
        if ui.button("Frame orbital system").clicked() {
            self.out.push(UiCommand::FrameSystem);
        }
        #[cfg(not(target_arch = "wasm32"))]
        if ui.button("Save bug report").clicked() {
            self.report = true;
        }
        star_controls(ui, galaxy, nav, self.out);
        ui.separator();
        if let Some(planet) = nav.selected.and_then(|i| system.planets.get(i)) {
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
                let c = task.colour;
                ui.colored_label(
                    egui::Color32::from_rgb(c[0], c[1], c[2]),
                    task.status.to_uppercase(),
                );
                ui.label(&task.description);
                ui.separator();
                ui.label(format!(
                    "Assigned: {}",
                    if task.assignees.is_empty() {
                        "Nobody".into()
                    } else {
                        task.assignees.join(", ")
                    }
                ));
                if let Some(due) = task.due_date_ms {
                    let now = crate::network::now_ms();
                    let days = (due - now) as f64 / 86_400_000.0;
                    ui.label(if days < 0.0 {
                        format!("Due date: {:.1} days ago", -days)
                    } else {
                        format!("Due in {days:.1} days")
                    });
                }
            } else {
                ui.label(egui::RichText::new(&planet.designation).color(MUTED));
                ui.label(egui::RichText::new("No task on this planet.").color(MUTED));
            }
            ui.add_space(6.0);
            heading(ui, "ORBITAL ELEMENTS");
            egui::Grid::new("elements").num_columns(2).show(ui, |ui| {
                for (label, value) in [
                    (
                        "Semi-major axis",
                        format!("{:.3} AU", planet.orbit.semi_major_au),
                    ),
                    ("Eccentricity", format!("{:.3}", planet.orbit.eccentricity)),
                    (
                        "Orbital period",
                        format!(
                            "{:.1} days",
                            period_seconds(&planet.orbit, system.mass(planet.page())) / 86400.0
                        ),
                    ),
                    ("Physical radius", format!("{:.0} km", planet.radius_km)),
                    ("Type", planet.class.clone()),
                    ("Mass", format!("{:.4} Earth masses", planet.mass_earth)),
                    (
                        "Rotation",
                        if planet.spin.locked {
                            "Tidally locked".into()
                        } else {
                            format!("{:.1} hours", planet.spin.period_hours)
                        },
                    ),
                    (
                        "Axial tilt",
                        format!("{:.1}°", planet.spin.tilt_rad.to_degrees()),
                    ),
                    (
                        "Resonance",
                        planet.resonance.map_or("None".into(), |[p, q]| {
                            format!("{p}:{q} with inner neighbour")
                        }),
                    ),
                ] {
                    ui.label(egui::RichText::new(label).color(MUTED));
                    ui.label(value);
                    ui.end_row();
                }
            });
            ui.add_space(10.0);
            if ui.button("◎  Follow this planet").clicked() {
                self.out.push(UiCommand::Follow);
            }
            if let Some(url) = planet.task.as_ref().and_then(|t| t.url.as_ref()) {
                ui.hyperlink_to("Open task in ClickUp ↗", url);
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
                ui.colored_label(
                    egui::Color32::from_rgb(colour[0], colour[1], colour[2]),
                    text,
                );
            }
            ui.label(
                egui::RichText::new("Live lists use their ClickUp status colours.")
                    .color(MUTED)
                    .size(11.0),
            );
        }
    }

    fn time_controls(&mut self, ui: &mut egui::Ui) {
        let clock = self.clock;
        ui.horizontal_wrapped(|ui| {
            if ui
                .button(if clock.paused {
                    "▶  Resume"
                } else {
                    "Ⅱ  Pause"
                })
                .clicked()
            {
                self.out.push(UiCommand::SetPaused {
                    paused: !clock.paused,
                });
            }
            for (label, rate) in RATES {
                if ui.selectable_label(clock.rate == rate, label).clicked() {
                    self.out.push(UiCommand::SetRate { rate });
                }
            }
            ui.separator();
            ui.label(
                egui::RichText::new(format!("T + {:.2} days", clock.seconds / 86400.0))
                    .monospace()
                    .color(ACCENT),
            );
        });
    }
}

/// The selected star, from anywhere in the galaxy, and how to visit it.
fn star_controls(ui: &mut egui::Ui, galaxy: &Galaxy, nav: &Navigation, out: &mut Vec<UiCommand>) {
    if let Some(star) = &nav.picked {
        ui.label(format!(
            "Selected {}",
            crate::galaxy_nav::label(galaxy, &star.id)
        ));
        if ui.button("Visit star (Enter)").clicked() {
            out.push(UiCommand::FrameSystem);
        }
    }
    if ui.button("Select centre star (F)").clicked() {
        out.push(UiCommand::PickCentre);
    }
}

const RATES: [(&str, f64); 4] = [
    ("Real time", 1.0),
    ("1 hr/s", 3600.0),
    ("1 day/s", 86400.0),
    ("30 days/s", 2_592_000.0),
];

#[cfg(not(target_arch = "wasm32"))]
fn save_bug_report(
    commands: &mut Commands,
    galaxy: &Galaxy,
    nav: &Navigation,
    clock: &SimulationClock,
    (rig, pose): (&CameraRig, &CameraPose),
    starfield: &crate::stream::Starfield,
) -> String {
    let system = nav.system(galaxy);
    let state = format!(
        "System ID: {}\nList: {}\nPage: {}\nView: {:?}\nFollowing: {}\nSelected task index: {:?}\nSource: {}\nSystems: {}\nTasks: {}\nSimulated seconds: {:.3}\nRate: {:.3}\nPaused: {}\nDesired target (m): {:?}\nDesired range (m): {:.3}\nDisplayed eye relative to page star (m): {:?}\nDisplayed range (m): {:.3}\nFlight progress: {:?}\nGalaxy: {}\n",
        system.map_or("none", |s| s.id.as_str()),
        nav.list().is_some(),
        nav.page,
        nav.view,
        nav.following,
        nav.selected,
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
            format!("Bug report and screenshot: {}", path.display())
        }
        Err(err) => format!("Could not save bug report: {err}"),
    }
}

/// Labels, hover rings and clicks on the 3D view itself. These stay in egui on
/// every target: they move every frame and there can be hundreds of them.
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
    exposure: &str,
    // HTML panels show the loading state and view captions themselves.
    captions: bool,
    out: &mut Vec<UiCommand>,
) {
    if captions && galaxy.systems.is_empty() {
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
    if captions {
        painter.text(
            rect.left_top() + egui::vec2(24.0, 24.0),
            egui::Align2::LEFT_TOP,
            if nav.visit.is_none() {
                "GALAXY / LIGHT YEARS"
            } else if nav.view == View::Table {
                "TASK TABLE / TRUE SIZES · ORBITS CONTINUE"
            } else if nav.view == View::Constellation {
                "LIST CONSTELLATION / SELECT A SYSTEM"
            } else {
                "SYSTEM VIEW / ASTRONOMICAL UNITS"
            },
            egui::FontId::monospace(11.0),
            MUTED,
        );
        let distance = if pose.distance > LY_METRES * 0.01 {
            format!("{:.3} ly", pose.distance / LY_METRES)
        } else {
            format!("{:.4} AU", pose.distance / AU_METRES)
        };
        painter.text(
            rect.left_bottom() + egui::vec2(24.0, -20.0),
            egui::Align2::LEFT_BOTTOM,
            format!(
                "{}\nCAMERA RANGE {distance}   /   {}",
                exposure.to_uppercase(),
                if nav.following {
                    "TRACKING"
                } else if nav.view != View::Orbit {
                    "AUTO-FRAMED PRESET"
                } else {
                    "FREE NAVIGATION"
                }
            ),
            egui::FontId::monospace(11.0),
            MUTED,
        );
    }

    let pointer = ctx.input(|i| i.pointer.interact_pos());
    let mut click = ctx.input(|i| i.pointer.primary_clicked());
    if let Some(star) = &nav.picked {
        let camera =
            rig.rotation().inverse() * (star.position_ly - rig.galactic_ly(pose)).as_vec3();
        if camera.z < 0.0 {
            let half = (50.0_f32.to_radians() / 2.0).tan();
            let size = rect.height() / (2.0 * half * -camera.z);
            let point = egui::pos2(
                rect.center().x + camera.x * size,
                rect.center().y - camera.y * size,
            );
            if rect.contains(point) {
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
        let selected = nav.selected == Some(marker.planet);
        if selected || hovered {
            painter.circle_stroke(point, radius + 5.0, egui::Stroke::new(1.0, tint));
        }
        // Markers remain visible at true physical body sizes.
        if nav.markers && marker.radius < 6.0 {
            painter.circle_stroke(point, 4.0, egui::Stroke::new(1.0, tint));
        }
        // Task labels would smear across the screen mid-flight.
        if selected || hovered || ((nav.labels || nav.view != View::Orbit) && !rig.flying()) {
            let table = nav.view == View::Table && !rig.flying();
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
        )
    {
        out.push(UiCommand::PickStar { id: star.id });
    }
}
