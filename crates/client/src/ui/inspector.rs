//! One borderless readout shared by both breadcrumb trails.
mod atmosphere;
use crate::{
    Navigation,
    actions::{GalaxyReference, UiCommand},
    craft::Now,
    navigation::objects::{self, ObjectRef},
    navigation::presentation::PresentationMode,
    stars::stream::Starfield,
    ui::breadcrumb::Inspection,
    ui::kit::halo::{CURRENT, HOVER, HUD_LINE_GAP, LINK, halo_text, halo_wrapped_text},
};
use bevy::math::DVec3;
use bevy_egui::egui;
use space_shared::Galaxy;
use space_simulation::{
    LY_METRES,
    galaxy::{GalaxyModel, PC_METRES},
};

#[derive(Default)]
pub struct Cache {
    key: Option<(ObjectRef, Option<DVec3>)>,
    info: Info,
}

impl Cache {
    pub fn invalidate(&mut self) {
        self.key = None;
    }

    /// A star's facts include its motion once its position is known. A
    /// craft's change every frame, so are never kept.
    pub(crate) fn get(
        &mut self,
        object: &ObjectRef,
        galaxy: &Galaxy,
        position_ly: Option<DVec3>,
        now: Now,
    ) -> &Info {
        if self.key.as_ref() != Some(&(object.clone(), position_ly))
            || matches!(object, ObjectRef::Craft { .. })
        {
            self.info = describe(object, galaxy, position_ly, now);
            self.key = Some((object.clone(), position_ly));
        }
        &self.info
    }
}

#[derive(Default)]
pub struct Info {
    pub title: String,
    pub rows: Vec<(String, String)>,
    pub note: String,
    description: String,
    url: Option<String>,
    /// Galactocentric, km/s, for a located star or system.
    velocity_kms: Option<[f64; 3]>,
}

impl Info {
    fn fact(&mut self, label: &str, value: impl Into<String>) {
        self.rows.push((label.into(), value.into()));
    }
}

/// Where an inspected star or system is, in light years from home, if known.
pub(crate) fn position_ly(
    object: &ObjectRef,
    nav: &Navigation,
    galaxy: &Galaxy,
    field: &Starfield,
) -> Option<DVec3> {
    match object {
        ObjectRef::Star { id } | ObjectRef::Barycentre { id } => {
            crate::navigation::galaxy_nav::selectable_star(nav, galaxy, field, id)
                .map(|star| star.position_ly)
        }
        _ => None,
    }
}

/// `position_ly` is the system's, from home, when known.
pub fn describe(object: &ObjectRef, galaxy: &Galaxy, position_ly: Option<DVec3>, now: Now) -> Info {
    use crate::format::{luminosity_text, mass_text, period_text, rotation_text, star_radius_text};
    let mut info = Info::default();
    match object {
        ObjectRef::Galaxy => {
            let model = space_simulation::galaxy::GalaxyModel::milky_way();
            info.title = "Milky Way".into();
            info.fact("Star systems", format!("{:.0} billion", model.stars / 1e9));
            info.fact(
                "Model boundary radius",
                format!("{:.0} kpc", model.radius_pc / 1000.0),
            );
            info.fact(
                "Central black hole",
                format!("{:.1} million solar masses", model.black_hole_solar / 1e6),
            );
            info.fact(
                "Rotation at home",
                format!(
                    "{:.0} km/s",
                    model.circular_speed_kms(model.home_pc[0].hypot(model.home_pc[2]))
                ),
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
        ObjectRef::Star { id } | ObjectRef::Barycentre { id } => {
            let key = space_simulation::star::multiple::system_key(id);
            if let Some((system, page)) = objects::system(galaxy, key) {
                let name = crate::navigation::galaxy_nav::label(galaxy, key);
                let velocity = position_ly.map(|at| velocity_kms(key, at));
                // The system as a whole is its outermost barycentre.
                let member = if id == key {
                    system.root(page).unwrap_or(key).to_owned()
                } else {
                    id.clone()
                };
                if let Some(pair) = system.barycentre(&member) {
                    info.title = objects::member_label(&name, &pair.component);
                    pair_facts(&mut info, &system, page, pair, id == key);
                } else if let Some(star) = system
                    .stars_on(page)
                    .find(|star| star.id == member)
                    .or_else(|| system.star(page))
                {
                    info.title = objects::member_label(&name, &star.component);
                    info.fact("Type", format!("{} · {}", star.kind, star.spectral));
                    info.fact("Mass", format!("{:.2} solar masses", star.mass_solar));
                    info.fact("Radius", star_radius_text(star.radius_solar));
                    info.fact(
                        "Flattening",
                        crate::format::flattening_text(
                            space_simulation::star_shape(star).flattening(),
                        ),
                    );
                    info.fact("Temperature", format!("{:.0} K", star.temperature_k));
                    info.fact(
                        "Luminosity",
                        format!("{} solar", luminosity_text(star.luminosity_solar)),
                    );
                    info.fact("Rotation", rotation_text(star.spin.period_hours));
                    info.fact("Axial tilt", crate::format::tilt_text(star.spin.tilt_rad));
                    if let Some(pair) = system.pair_of(page, &star.id) {
                        let other = pair.members.iter().find(|m| **m != star.id);
                        let partner = other.map_or("", |m| member_component(&system, m));
                        info.fact("Companion", objects::member_label(&name, partner));
                        info.fact(
                            "Separation",
                            crate::format::au_text(pair.orbit.semi_major_au),
                        );
                    }
                }
                if let Some(velocity) = velocity {
                    motion_facts(&mut info, velocity);
                }
            }
        }
        ObjectRef::Planet { id } => {
            if let Some(star) = objects::parent_star(id)
                && let Some((system, _)) = objects::system(galaxy, star)
                && let Some(planet) = system.planets.iter().find(|planet| planet.id == *id)
            {
                let days = space_simulation::planet_period(&system, planet) / 86400.0;
                info.title = planet.designation.clone();
                info.fact("Type", planet.class.clone());
                if system.barycentres_on(planet.page()).next().is_some() {
                    let host: Vec<_> = member_component(&system, &planet.host)
                        .chars()
                        .map(String::from)
                        .collect();
                    info.fact(
                        "Orbits",
                        match host.split_last() {
                            Some((last, rest)) if !rest.is_empty() => {
                                format!("Stars {} and {last}", rest.join(", "))
                            }
                            _ => format!("Star {}", host.concat()),
                        },
                    );
                }
                info.fact("Mass", mass_text(planet.mass_earth));
                info.fact("Radius", format!("{:.0} km", planet.radius_km));
                info.fact(
                    "Flattening",
                    crate::format::flattening_text(
                        space_simulation::planet_shape(planet).flattening(),
                    ),
                );
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
                info.fact("Axial tilt", crate::format::tilt_text(planet.spin.tilt_rad));
                if let Some([p, q]) = planet.resonance {
                    info.fact("Resonance", format!("{p}:{q} with inner neighbour"));
                }
                atmosphere::facts(&mut info, planet.environment.as_ref());
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
                            .map_or("No due date".into(), crate::format::due_text),
                    );
                    info.description = task.description.clone();
                    info.url = task.url.clone();
                }
            }
        }
        ObjectRef::Craft { id } => {
            if let Some(live) = now.get(id) {
                (info.title, info.rows) = crate::craft::facts(live, galaxy, now.seconds);
                info.note = "Player-made · simulated live".into();
            }
        }
    }
    if info.title.is_empty() {
        info.title = "Object unavailable".into();
    }
    info
}

/// A system's galactocentric velocity in km/s, from its position from home.
fn velocity_kms(key: &str, position_ly: DVec3) -> [f64; 3] {
    let home = DVec3::from_array(GalaxyModel::milky_way().home_pc);
    let position_pc = home + position_ly * (LY_METRES / PC_METRES);
    space_simulation::galaxy::kinematics::system_velocity_kms(key, position_pc.to_array())
}

fn motion_facts(info: &mut Info, velocity: [f64; 3]) {
    info.fact(
        "Galactic velocity",
        format!("{:.0} km/s", DVec3::from_array(velocity).length()),
    );
    info.velocity_kms = Some(velocity);
}

/// Letters of a star or barycentre within its system.
fn member_component<'a>(system: &'a space_shared::StarSystem, id: &'a str) -> &'a str {
    system.barycentre(id).map_or_else(
        || space_simulation::star::multiple::member_letters(id),
        |pair| pair.component.as_str(),
    )
}

/// A pair's orbit, and for a whole system what it holds.
fn pair_facts(
    info: &mut Info,
    system: &space_shared::StarSystem,
    page: u32,
    pair: &space_shared::Barycentre,
    whole: bool,
) {
    use crate::format::{luminosity_text, period_text};
    let stars: Vec<_> = system
        .stars_on(page)
        .filter(|star| pair.component.contains(star.component.as_str()))
        .collect();
    let kind = match stars.len() {
        2 => "Binary",
        3 => "Triple",
        _ => "Quadruple",
    };
    info.fact(
        "Type",
        if whole {
            format!("{kind} star system")
        } else {
            "Pair within its system".into()
        },
    );
    for star in &stars {
        info.fact(
            &format!("Star {}", star.component),
            format!("{} · {}", star.kind, star.spectral),
        );
    }
    let [first, second] = pair.members.each_ref().map(|m| member_component(system, m));
    info.fact("Pair", format!("{first} and {second}"));
    let mass = system.member_mass(page, &pair.id);
    info.fact("Total mass", format!("{mass:.2} solar masses"));
    let light: f64 = stars.iter().map(|s| s.luminosity_solar).sum();
    info.fact("Luminosity", format!("{} solar", luminosity_text(light)));
    info.fact(
        "Separation",
        crate::format::au_text(pair.orbit.semi_major_au),
    );
    let days = space_simulation::period_seconds(&pair.orbit, mass) / 86_400.0;
    info.fact("Orbital period", period_text(days));
    info.fact("Eccentricity", format!("{:.3}", pair.orbit.eccentricity));
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
    now: Now,
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
        ObjectRef::Star { id } | ObjectRef::Barycentre { id } => {
            // Any member acts for its whole system.
            let id = &space_simulation::star::multiple::system_key(id).to_owned();
            if !crate::navigation::galaxy_nav::reachable(nav, galaxy, field, id) {
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
                || objects::parent_star(id).is_some_and(|star| {
                    crate::navigation::galaxy_nav::reachable(nav, galaxy, field, star)
                });
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
        ObjectRef::Craft { id } => {
            let Some(live) = now.get(id) else {
                return Vec::new();
            };
            vec![Action {
                label: format!("Follow {}", live.craft.kind.label().to_lowercase()),
                current: nav.following && nav.selected_craft() == Some(id.as_str()),
                command: UiCommand::FlyToCraft { id: id.clone() },
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
        .and_then(|star| crate::navigation::galaxy_nav::mapped(galaxy, &star.id))
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

/// Preserve the list tab exactly as a click in the inspector does.
fn action_commands(inspection: &Inspection, command: UiCommand) -> Vec<UiCommand> {
    let keep = matches!(inspection.object, ObjectRef::List { .. })
        && matches!(command, UiCommand::PickStar { .. });
    let mut commands = vec![command];
    if keep {
        commands.push(UiCommand::InspectObject {
            inspection: inspection.clone(),
        });
    }
    commands
}

/// Read-only developer view of the very same facts and available UI actions.
/// Unreachable actions remain omitted; task links are URLs, not app commands.
pub fn readout(
    inspection: &Inspection,
    nav: &Navigation,
    galaxy: &Galaxy,
    field: &Starfield,
    cache: &mut Cache,
    now: Now,
) -> serde_json::Value {
    use serde_json::json;
    let position = position_ly(&inspection.object, nav, galaxy, field);
    let info = cache.get(&inspection.object, galaxy, position, now);
    let export = |action: Action, group: &str| {
        json!({
            "label": action.label, "current": action.current, "available": true,
            "group": group, "kind": "command",
            "commands": action_commands(inspection, action.command),
        })
    };
    let mut buttons = Vec::new();
    if let Some(url) = &info.url {
        buttons.push(
            json!({"label": "Open task link ↗", "kind": "link", "group": "task",
            "current": false, "available": true, "url": url, "commands": []}),
        );
    }
    buttons.extend(
        actions(&inspection.object, nav, galaxy, field, now)
            .into_iter()
            .map(|action| export(action, "navigation")),
    );
    if let ObjectRef::List { id } = &inspection.object
        && let Some((systems, _)) = list_pages(id, nav, galaxy)
    {
        buttons.extend(systems.into_iter().map(|action| export(action, "systems")));
    }
    json!({
        "inspection": inspection, "object": inspection.object,
        "title": info.title,
        "facts": info.rows.iter().map(|(label, value)| json!({"label": label, "value": value})).collect::<Vec<_>>(),
        "description": info.description, "note": info.note, "actions": buttons,
        "velocity_kms": info.velocity_kms,
    })
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

#[allow(clippy::too_many_arguments)]
pub fn show(
    ui: &mut egui::Ui,
    inspection: &Inspection,
    nav: &Navigation,
    galaxy: &Galaxy,
    field: &Starfield,
    cache: &mut Cache,
    now: Now,
    out: &mut Vec<UiCommand>,
) {
    let object = &inspection.object;
    cache.get(object, galaxy, position_ly(object, nav, galaxy, field), now);
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
    let actions = actions(object, nav, galaxy, field, now);
    // Stepping through a list keeps its tab, though selecting or visiting a
    // star would otherwise move the inspector to it.
    let step = |out: &mut Vec<UiCommand>, command: UiCommand| {
        out.extend(action_commands(inspection, command));
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::breadcrumb::TrailSource;

    fn inspection(object: ObjectRef) -> Inspection {
        Inspection {
            source: TrailSource::Selection,
            object,
        }
    }

    #[test]
    fn developer_readout_matches_facts_and_action_commands_for_each_object_kind() {
        let galaxy = space_simulation::demo_galaxy();
        let nav = Navigation::default();
        let field = Starfield::default();
        let fleet = crate::craft::Fleet::default();
        let now = Now {
            fleet: &fleet,
            seconds: 0.0,
        };
        let system = &galaxy.systems[0];
        let star = objects::star_id(system, 0);
        let mut objects = vec![
            ObjectRef::Galaxy,
            ObjectRef::List {
                id: system.id.clone(),
            },
            ObjectRef::Star { id: star },
            ObjectRef::Planet {
                id: system.planets[0].id.clone(),
            },
        ];
        if let Some(pair) = system.barycentres_on(0).next() {
            objects.push(ObjectRef::Barycentre {
                id: pair.id.clone(),
            });
        }
        let mut cache = Cache::default();
        for object in objects {
            let inspection = inspection(object.clone());
            let exported = readout(&inspection, &nav, &galaxy, &field, &mut cache, now);
            let info = describe(
                &object,
                &galaxy,
                position_ly(&object, &nav, &galaxy, &field),
                now,
            );
            assert_eq!(exported["title"], info.title);
            // A list page's star is located, so its motion is known.
            if matches!(object, ObjectRef::Star { .. }) {
                assert_eq!(exported["velocity_kms"].as_array().unwrap().len(), 3);
                assert!(
                    info.rows
                        .iter()
                        .any(|(label, _)| label == "Galactic velocity")
                );
            }
            assert_eq!(exported["facts"].as_array().unwrap().len(), info.rows.len());
            for (fact, (label, value)) in
                exported["facts"].as_array().unwrap().iter().zip(info.rows)
            {
                assert_eq!(fact["label"], label);
                assert_eq!(fact["value"], value);
            }
            let buttons: Vec<_> = exported["actions"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|button| button["group"] == "navigation")
                .collect();
            for (button, action) in buttons
                .iter()
                .zip(actions(&object, &nav, &galaxy, &field, now))
            {
                assert_eq!(button["label"], action.label);
                assert_eq!(button["current"], action.current);
                assert_eq!(button["available"], true);
                let parsed =
                    crate::devtools::command_line::parse(&button["commands"][0].to_string())
                        .unwrap();
                assert_eq!(parsed, action.command);
            }
            assert_eq!(
                buttons.len(),
                actions(&object, &nav, &galaxy, &field, now).len()
            );
        }
    }

    #[test]
    fn list_buttons_preserve_inspection_and_cached_facts_invalidate() {
        let mut galaxy = space_simulation::demo_galaxy();
        let nav = Navigation::default();
        let field = Starfield::default();
        let fleet = crate::craft::Fleet::default();
        let now = Now {
            fleet: &fleet,
            seconds: 0.0,
        };
        let inspection = inspection(ObjectRef::List {
            id: galaxy.systems[0].id.clone(),
        });
        let mut cache = Cache::default();
        let exported = readout(&inspection, &nav, &galaxy, &field, &mut cache, now);
        let systems: Vec<_> = exported["actions"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|button| button["group"] == "systems")
            .collect();
        assert_eq!(systems.len(), galaxy.systems[0].pages().len());
        for button in systems {
            let commands: Vec<UiCommand> =
                serde_json::from_value(button["commands"].clone()).unwrap();
            assert!(matches!(commands[0], UiCommand::PickStar { .. }));
            assert_eq!(
                commands[1],
                UiCommand::InspectObject {
                    inspection: inspection.clone()
                }
            );
        }
        galaxy.systems[0].name = "Updated list".into();
        assert_eq!(
            readout(&inspection, &nav, &galaxy, &field, &mut cache, now)["title"],
            exported["title"]
        );
        cache.invalidate();
        assert_eq!(
            readout(&inspection, &nav, &galaxy, &field, &mut cache, now)["title"],
            "Updated list"
        );
    }

    #[test]
    fn task_links_are_urls_and_unreachable_actions_are_omitted() {
        let mut galaxy = space_simulation::demo_galaxy();
        let nav = Navigation::default();
        let field = Starfield::default();
        let fleet = crate::craft::Fleet::default();
        let now = Now {
            fleet: &fleet,
            seconds: 0.0,
        };
        let planet = galaxy.systems[0]
            .planets
            .iter_mut()
            .find(|planet| planet.task.is_some())
            .unwrap();
        planet.task.as_mut().unwrap().url = Some("https://example.com/task".into());
        let inspection = inspection(ObjectRef::Planet {
            id: planet.id.clone(),
        });
        let mut cache = Cache::default();
        let exported = readout(&inspection, &nav, &galaxy, &field, &mut cache, now);
        let link = &exported["actions"][0];
        assert_eq!(link["kind"], "link");
        assert_eq!(link["url"], "https://example.com/task");
        assert_eq!(link["commands"], serde_json::json!([]));
        let unreachable = inspection_for_unreachable();
        let exported = readout(&unreachable, &nav, &galaxy, &field, &mut cache, now);
        assert_eq!(exported["actions"], serde_json::json!([]));
    }

    fn inspection_for_unreachable() -> Inspection {
        inspection(ObjectRef::Star {
            id: "g100.0.0".into(),
        })
    }
}
