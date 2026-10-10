//! One borderless readout shared by both breadcrumb trails.
use crate::{
    breadcrumb::{CURRENT, LINK, halo_text, halo_wrapped_text},
    objects::{self, ObjectRef},
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
                info.note = "Procedurally generated star".into();
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
                info.note = "Procedurally generated planet".into();
            }
        }
    }
    if info.title.is_empty() {
        info.title = "Object unavailable".into();
    }
    info
}

pub fn show(ui: &mut egui::Ui, object: &ObjectRef, galaxy: &Galaxy, cache: &mut Cache) {
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
            if response.hovered() {
                crate::breadcrumb::HOVER
            } else {
                LINK
            }
        });
        if response.clicked() {
            ui.ctx().open_url(egui::OpenUrl::new_tab(url));
        }
    }
    halo_wrapped_text(ui, &cache.info.note, width, LINK);
}
