//! One borderless readout shared by both breadcrumb trails.
use crate::{
    Navigation,
    actions::{GalaxyReference, UiCommand},
    objects::{self, ObjectRef},
    presentation::PresentationMode,
    stars::stream::Starfield,
    ui::breadcrumb::{
        CURRENT, HOVER, HUD_LINE_GAP, Inspection, LINK, halo_text, halo_wrapped_text,
    },
};
use bevy_egui::egui;
use space_shared::Galaxy;

#[derive(Default)]
pub struct Cache {
    key: Option<ObjectRef>,
    info: Info,
}

impl Cache {
    pub fn invalidate(&mut self) {
        self.key = None;
    }
}

#[derive(Default)]
pub struct Info {
    pub title: String,
    pub rows: Vec<(String, String)>,
    pub note: String,
    description: String,
    url: Option<String>,
}

impl Info {
    fn fact(&mut self, label: &str, value: impl Into<String>) {
        self.rows.push((label.into(), value.into()));
    }
}

pub fn describe(object: &ObjectRef, galaxy: &Galaxy) -> Info {
    use crate::ui::{luminosity_text, mass_text, period_text, rotation_text, star_radius_text};
    let mut info = Info::default();
    match object {
        ObjectRef::Galaxy => {
            let model = space_simulation::galaxy::GalaxyModel::milky_way();
            info.title = "Milky Way".into();
            info.fact("Stars", format!("{:.0} billion", model.stars / 1e9));
            info.fact(
                "Model boundary radius",
                format!("{:.0} kpc", model.radius_pc / 1000.0),
            );
            info.fact(
                "Central black hole",
                format!("{:.1} million solar masses", model.black_hole_solar / 1e6),
            );
            info.note = "Simulation model · not catalogue measurements".into();
        }
        ObjectRef::List { id } => {
            if let Some(system) = galaxy.systems.iter().find(|system| system.id == *id) {
                info.title = system.name.clone();
                info.fact("Systems", system.pages().len().to_string());
                info.fact("Tasks", system.tasks().count().to_string());
                info.note = galaxy.source.clone();
            }
        }
        ObjectRef::Star { id } => {
            if let Some((system, page)) = objects::system(galaxy, id)
                && let Some(star) = system.star(page)
            {
                info.title = crate::galaxy_nav::label(galaxy, id);
                info.fact("Type", format!("{} · {}", star.kind, star.spectral));
                info.fact("Mass", format!("{:.2} solar masses", star.mass_solar));
                info.fact("Radius", star_radius_text(star.radius_solar));
                info.fact("Temperature", format!("{:.0} K", star.temperature_k));
                info.fact(
                    "Luminosity",
                    format!("{} solar", luminosity_text(star.luminosity_solar)),
                );
                info.fact("Rotation", rotation_text(star.spin.period_hours));
                info.fact("Axial tilt", crate::ui::tilt_text(star.spin.tilt_rad));
            }
        }
        ObjectRef::Planet { id } => {
            if let Some(star) = objects::parent_star(id)
                && let Some((system, _)) = objects::system(galaxy, star)
                && let Some(planet) = system.planets.iter().find(|planet| planet.id == *id)
            {
                let days =
                    space_simulation::period_seconds(&planet.orbit, system.mass(planet.page()))
                        / 86400.0;
                info.title = planet.designation.clone();
                info.fact("Type", planet.class.clone());
                info.fact("Mass", mass_text(planet.mass_earth));
                info.fact("Radius", format!("{:.0} km", planet.radius_km));
                info.fact(
                    "Orbital distance",
                    format!("{:.3} AU", planet.orbit.semi_major_au),
                );
                info.fact("Orbital period", period_text(days));
                info.fact("Eccentricity", format!("{:.3}", planet.orbit.eccentricity));
                info.fact(
                    "Rotation",
                    if planet.spin.locked {
                        "Tidally locked".into()
                    } else {
                        rotation_text(planet.spin.period_hours)
                    },
                );
                info.fact("Axial tilt", crate::ui::tilt_text(planet.spin.tilt_rad));
                if let Some([p, q]) = planet.resonance {
                    info.fact("Resonance", format!("{p}:{q} with inner neighbour"));
                }
                if let Some(task) = &planet.task {
                    info.fact("Task", task.name.clone());
                    info.fact("Status", task.status.clone());
                    info.fact(
                        "Assigned",
                        if task.assignees.is_empty() {
                            "Nobody".into()
                        } else {
                            task.assignees.join(", ")
                        },
                    );
                    info.fact(
                        "Due",
                        task.due_date_ms
                            .map_or("No due date".into(), crate::ui::due_text),
                    );
                    info.description = task.description.clone();
                    info.url = task.url.clone();
                }
            }
        }
    }
    if info.title.is_empty() {
        info.title = "Object unavailable".into();
    }
    info
}

/// A one-click equivalent of a scene gesture on the inspected object.
pub struct Action {
    pub label: String,
    /// Already showing this, so it would only reframe.
    pub current: bool,
    pub command: UiCommand,
}

/// Unreachable actions are left out rather than shown doing nothing.
pub fn actions(
    object: &ObjectRef,
    nav: &Navigation,
    galaxy: &Galaxy,
    field: &Starfield,
) -> Vec<Action> {
    let visited_star = nav
        .location(galaxy)
        .map(|system| objects::star_id(system, nav.page));
    match object {
        ObjectRef::Galaxy => vec![Action {
            label: "View from above".into(),
            current: false,
            command: UiCommand::GalaxyReference {
                view: GalaxyReference::FaceOn,
            },
        }],
        ObjectRef::List { id } => {
            let mut actions = vec![Action {
                label: "Constellation view".into(),
                current: nav.presentation_mode == PresentationMode::Constellation
                    && nav
                        .list()
                        .is_some_and(|index| galaxy.systems[index].id == *id),
                command: UiCommand::Jump { list: id.clone() },
            }];
            if let Some((pages, Some(current))) = list_pages(id, nav, galaxy) {
                for (label, step) in [("Previous system", -1), ("Next system", 1)] {
                    if let Some(page) = current
                        .checked_add_signed(step)
                        .and_then(|index| pages.get(index))
                    {
                        actions.push(Action {
                            label: label.into(),
                            current: false,
                            command: page.command.clone(),
                        });
                    }
                }
            }
            actions
        }
        ObjectRef::Star { id } => {
            if !crate::galaxy_nav::reachable(nav, galaxy, field, id) {
                return Vec::new();
            }
            let here = visited_star.as_deref() == Some(id.as_str());
            let showing = |mode| {
                nav.presentation_mode == mode
                    && nav
                        .system(galaxy)
                        .is_some_and(|system| objects::star_id(system, nav.page) == *id)
            };
            let visit = |label: &str, presentation_mode| Action {
                label: label.into(),
                current: presentation_mode != PresentationMode::Simulation
                    && showing(presentation_mode),
                command: UiCommand::VisitStar {
                    id: id.clone(),
                    presentation_mode,
                },
            };
            let mut actions = vec![
                visit(
                    if here {
                        "Frame system"
                    } else {
                        "Fly to system"
                    },
                    PresentationMode::Simulation,
                ),
                visit("Table view", PresentationMode::Table),
            ];
            if objects::known_system(galaxy, id).is_some() {
                actions.push(visit("Constellation view", PresentationMode::Constellation));
            }
            actions
        }
        ObjectRef::Planet { id } => {
            let local = nav
                .system(galaxy)
                .is_some_and(|system| system.planets.iter().any(|planet| planet.id == *id));
            let reachable = local
                || objects::parent_star(id)
                    .is_some_and(|star| crate::galaxy_nav::reachable(nav, galaxy, field, star));
            if !reachable {
                return Vec::new();
            }
            let following = nav.following
                && matches!(&nav.selection, Some(crate::Selection::Planet(planet)) if planet == id);
            vec![Action {
                label: "Fly to planet".into(),
                current: following,
                command: UiCommand::FlyToPlanet { planet: id.clone() },
            }]
        }
    }
}

/// A list's systems in page order, and which one is current: the selected
/// star when it belongs to this list, else the open list's page. Each acts
/// like clicking its star: select it, or visit it once selected.
fn list_pages(id: &str, nav: &Navigation, galaxy: &Galaxy) -> Option<(Vec<Action>, Option<usize>)> {
    let index = galaxy.systems.iter().position(|list| list.id == id)?;
    let system = &galaxy.systems[index];
    let pages = system.pages();
    let current_page = nav
        .selected_star()
        .and_then(|star| crate::galaxy_nav::mapped(galaxy, &star.id))
        .filter(|(list, _)| *list == index)
        .map(|(_, page)| page)
        .or_else(|| (nav.list() == Some(index)).then_some(nav.page));
    let mut tasks = std::collections::HashMap::<u32, usize>::new();
    for (planet, _) in system.tasks() {
        *tasks.entry(system.planets[planet].page()).or_default() += 1;
    }
    let actions = pages
        .iter()
        .enumerate()
        .map(|(number, &page)| {
            let count = tasks.get(&page).copied().unwrap_or(0);
            Action {
                label: format!(
                    "System {} · {count} task{}",
                    number + 1,
                    if count == 1 { "" } else { "s" }
                ),
                current: current_page == Some(page),
                command: UiCommand::PickStar {
                    id: objects::star_id(system, page),
                },
            }
        })
        .collect();
    let current = current_page.and_then(|page| pages.iter().position(|p| *p == page));
    Some((actions, current))
}

/// Clickable halo text, white when already showing what it would do.
fn action_text(ui: &mut egui::Ui, action: &Action) -> bool {
    halo_text(ui, &action.label, egui::Sense::click(), |response| {
        if response.hovered() {
            HOVER
        } else if action.current {
            CURRENT
        } else {
            LINK
        }
    })
    .on_hover_cursor(egui::CursorIcon::PointingHand)
    .clicked()
}

pub fn show(
    ui: &mut egui::Ui,
    inspection: &Inspection,
    nav: &Navigation,
    galaxy: &Galaxy,
    field: &Starfield,
    cache: &mut Cache,
    out: &mut Vec<UiCommand>,
) {
    let object = &inspection.object;
    if cache.key.as_ref() != Some(object) {
        cache.info = describe(object, galaxy);
        cache.key = Some(object.clone());
    }
    let width = ui.available_width().min(480.0);
    halo_wrapped_text(ui, &cache.info.title, width, CURRENT);
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing.y = 4.0;
        for (label, value) in &cache.info.rows {
            ui.horizontal(|ui| {
                let label_width = (width * 0.4).min(150.0);
                ui.allocate_ui_with_layout(
                    egui::vec2(label_width, 0.0),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        ui.set_min_width(label_width);
                        halo_wrapped_text(ui, label, label_width, LINK);
                    },
                );
                halo_wrapped_text(ui, value, width - label_width - 16.0, CURRENT);
            });
        }
    });
    if !cache.info.description.is_empty() {
        halo_wrapped_text(ui, &cache.info.description, width, CURRENT);
    }
    if let Some(url) = &cache.info.url {
        let response = halo_text(ui, "Open task link ↗", egui::Sense::click(), |response| {
            if response.hovered() { HOVER } else { LINK }
        });
        if response.clicked() {
            ui.ctx().open_url(egui::OpenUrl::new_tab(url));
        }
    }
    if !cache.info.note.is_empty() {
        halo_wrapped_text(ui, &cache.info.note, width, LINK);
    }
    let actions = actions(object, nav, galaxy, field);
    // Stepping through a list keeps its tab, though selecting or visiting a
    // star would otherwise move the inspector to it.
    let step = |out: &mut Vec<UiCommand>, command: UiCommand| {
        let keep = matches!(object, ObjectRef::List { .. })
            && matches!(command, UiCommand::PickStar { .. });
        out.push(command);
        if keep {
            out.push(UiCommand::InspectObject {
                inspection: inspection.clone(),
            });
        }
    };
    if !actions.is_empty() {
        ui.add_space(HUD_LINE_GAP);
        ui.horizontal_wrapped(|ui| {
            ui.set_max_width(width);
            for (index, action) in actions.into_iter().enumerate() {
                if index > 0 {
                    halo_text(ui, "·", egui::Sense::hover(), |_| LINK);
                }
                if action_text(ui, &action) {
                    step(out, action.command);
                }
            }
        });
    }
    if let ObjectRef::List { id } = object
        && let Some((systems, _)) = list_pages(id, nav, galaxy)
    {
        ui.add_space(HUD_LINE_GAP);
        ui.scope(|ui| {
            ui.spacing_mut().item_spacing.y = 4.0;
            for action in systems {
                if action_text(ui, &action) {
                    step(out, action.command);
                }
            }
        });
    }
}
