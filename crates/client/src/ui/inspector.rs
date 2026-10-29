//! One borderless readout shared by both breadcrumb trails.
mod atmosphere;
mod composition;
mod layered;
mod planet_layout;
mod presentation;
use crate::{
    Navigation,
    actions::{GalaxyReference, UiCommand},
    craft::Now,
    navigation::objects::{self, ObjectRef},
    navigation::presentation::PresentationMode,
    planet_loading::{Layered, PlanetLoading},
    stars::stream::Starfield,
    ui::kit::halo::{CURRENT, HOVER, HUD_LINE_GAP, LINK, halo_text, halo_wrapped_text},
};
use bevy::math::DVec3;
use bevy_egui::egui;
use presentation::{Fact, Section, Value};
use space_shared::Galaxy;
use space_simulation::{
    LY_METRES,
    galaxy::{GalaxyModel, PC_METRES},
};

#[derive(Default)]
pub struct Cache {
    adoption: Option<String>,
    key: Option<(ObjectRef, Option<DVec3>, u8)>,
    physical_revision: Option<(
        space_shared::PhysicalRevision,
        space_shared::RefinementStatus,
    )>,
    info: Info,
    model_details_open: bool,
}

impl Cache {
    pub fn invalidate(&mut self) {
        self.key = None;
    }

    /// A star's facts include its motion once its position is known. A
    /// craft's change every frame, so are never kept. A planet's change with
    /// its active revision and background refinement status.
    pub(crate) fn get(
        &mut self,
        object: &ObjectRef,
        galaxy: &Galaxy,
        position_ly: Option<DVec3>,
        now: Now,
        loading: Option<&PlanetLoading>,
        active: Option<&space_shared::StarSystem>,
    ) -> &Info {
        let revision = match (object, loading) {
            (ObjectRef::Planet { id }, Some(loading)) => loading.status(id).key(),
            _ => 0,
        };
        // Loaded records may change without changing the object ID or worker
        // status. Remote inspection remains on deterministic synchronous facts.
        let physical_revision = match object {
            ObjectRef::Planet { id } => active
                .into_iter()
                .chain(galaxy.systems.iter())
                .flat_map(|s| &s.planets)
                .find(|p| p.id == *id)
                .and_then(|p| p.initial_result.as_ref())
                .or_else(|| now.fleet.charts.refinement(id).map(|r| &r.result))
                .map(|r| (r.revision.clone(), r.refinement.clone())),
            _ => None,
        };
        let adoption = match (object, loading) {
            (ObjectRef::Planet { id }, Some(loading)) => loading.adoption(id).map(str::to_owned),
            _ => None,
        };
        if self.key.as_ref() != Some(&(object.clone(), position_ly, revision))
            || self.adoption != adoption
            || self.physical_revision != physical_revision
            || matches!(object, ObjectRef::Craft { .. })
        {
            self.info = describe_with(object, galaxy, position_ly, now, loading, active);
            self.key = Some((object.clone(), position_ly, revision));
            self.physical_revision = physical_revision;
            self.adoption = adoption;
        }
        &self.info
    }
}

#[derive(Default)]
pub struct Info {
    pub title: String,
    pub fields: Vec<Fact>,
    section: Section,
    pub note: String,
    description: String,
    url: Option<String>,
    /// Galactocentric, km/s, for a located star or system.
    velocity_kms: Option<[f64; 3]>,
}

impl Info {
    fn fact(&mut self, label: &str, value: impl Into<String>) {
        // Compatibility adapter for non-planet readouts with dynamic labels.
        self.field(&label.to_lowercase().replace(' ', "_"), label, None, value);
    }
    fn field(
        &mut self,
        id: &str,
        label: &str,
        unit: Option<&'static str>,
        value: impl Into<String>,
    ) {
        self.fields.push(Fact {
            id: id.into(),
            section: self.section,
            label: label.into(),
            unit,
            value: Value::Known(value.into()),
        });
    }
    fn unknown(&mut self, id: &str, label: &str, reason: &str) {
        self.fields.push(Fact {
            id: id.into(),
            section: self.section,
            label: label.into(),
            unit: None,
            value: Value::Unknown(reason.into()),
        });
    }
    pub fn rows(&self) -> impl Iterator<Item = (&str, &str)> {
        self.fields
            .iter()
            .map(|f| (f.label.as_str(), f.value.text()))
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
/// Without background refinement diagnostics.
#[cfg(test)]
pub fn describe(object: &ObjectRef, galaxy: &Galaxy, position_ly: Option<DVec3>, now: Now) -> Info {
    describe_with(object, galaxy, position_ly, now, None, None)
}

fn describe_with(
    object: &ObjectRef,
    galaxy: &Galaxy,
    position_ly: Option<DVec3>,
    now: Now,
    loading: Option<&PlanetLoading>,
    active: Option<&space_shared::StarSystem>,
) -> Info {
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
                && let Some((system, _)) = active
                    .filter(|s| s.planets.iter().any(|p| p.id == *id))
                    .map(|s| (std::borrow::Cow::Borrowed(s), 0))
                    .or_else(|| objects::system(galaxy, star))
                    .map(|(mut system, page)| {
                        if system
                            .planets
                            .iter()
                            .any(|p| now.fleet.charts.refinement(&p.id).is_some())
                        {
                            now.fleet.charts.restore_system(system.to_mut());
                        }
                        (system, page)
                    })
                && let Some(planet) = system.planets.iter().find(|planet| planet.id == *id)
            {
                let days = space_simulation::planet_period(&system, planet) / 86400.0;
                info.title = planet.designation.clone();
                info.field("planet.class", "Type", None, planet.class.clone());
                if system.barycentres_on(planet.page()).next().is_some() {
                    let host: Vec<_> = member_component(&system, &planet.host)
                        .chars()
                        .map(String::from)
                        .collect();
                    info.field(
                        "orbit.host",
                        "Orbits",
                        None,
                        match host.split_last() {
                            Some((last, rest)) if !rest.is_empty() => {
                                format!("Stars {} and {last}", rest.join(", "))
                            }
                            _ => format!("Star {}", host.concat()),
                        },
                    );
                }
                info.field(
                    "planet.mass",
                    "Mass",
                    Some(if planet.mass_earth >= 31.783 {
                        "Jupiter masses"
                    } else {
                        "Earth masses"
                    }),
                    mass_text(planet.mass_earth),
                );
                info.field(
                    "planet.radius",
                    "Radius",
                    Some("km"),
                    format!("{:.0} km", planet.radius_km),
                );
                composition::facts(&mut info, planet);
                info.section = Section::Orbit;
                info.field(
                    "rotation.flattening",
                    "Flattening",
                    Some("fraction"),
                    crate::format::flattening_text(
                        space_simulation::planet_shape(planet).flattening(),
                    ),
                );
                info.section = Section::Orbit;
                info.field(
                    "orbit.distance",
                    "Orbital distance",
                    Some("AU"),
                    format!("{:.3} AU", planet.orbit.semi_major_au),
                );
                info.field(
                    "orbit.period",
                    "Orbital period",
                    Some(if days >= 730.0 { "years" } else { "days" }),
                    period_text(days),
                );
                info.field(
                    "orbit.eccentricity",
                    "Eccentricity",
                    Some("fraction"),
                    format!("{:.3}", planet.orbit.eccentricity),
                );
                info.section = Section::Orbit;
                info.field(
                    "rotation.period",
                    "Rotation",
                    if planet.spin.locked {
                        None
                    } else {
                        Some(if planet.spin.period_hours < 2.0 {
                            "minutes"
                        } else if planet.spin.period_hours < 48.0 {
                            "hours"
                        } else if planet.spin.period_hours / 24.0 < 730.0 {
                            "days"
                        } else {
                            "years"
                        })
                    },
                    if planet.spin.locked {
                        "Tidally locked".into()
                    } else {
                        rotation_text(planet.spin.period_hours)
                    },
                );
                info.field(
                    "rotation.tilt",
                    "Axial tilt",
                    Some("degrees"),
                    crate::format::tilt_text(planet.spin.tilt_rad),
                );
                info.section = Section::Orbit;
                if let Some([p, q]) = planet.resonance {
                    info.field(
                        "orbit.resonance",
                        "Resonance",
                        None,
                        format!("{p}:{q} with inner neighbour"),
                    );
                }
                atmosphere::facts(&mut info, planet.environment.as_ref());
                planet_layout::organise(&mut info, planet);
                let status = loading.map_or(Layered::Off, |l| l.status(&planet.id));
                layered::facts(&mut info, planet, status);
                if let Some(reason) = loading.and_then(|l| l.adoption(id)) {
                    info.field("layered.adoption", "Adoption", None, reason);
                }
                info.section = Section::Overview;
                if let Some(task) = &planet.task {
                    info.field("task.name", "Task", None, task.name.clone());
                    info.field("task.status", "Status", None, task.status.clone());
                    info.field(
                        "task.assignees",
                        "Assigned",
                        None,
                        if task.assignees.is_empty() {
                            "Nobody".into()
                        } else {
                            task.assignees.join(", ")
                        },
                    );
                    info.field(
                        "task.due",
                        "Due",
                        None,
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
                let (title, rows) = crate::craft::facts(live, galaxy, now.seconds);
                info.title = title;
                for (label, value) in rows {
                    info.fact(&label, value);
                }
                info.note = "Player-made · simulated live".into();
            }
        }
    }
    if info.title.is_empty() {
        info.title = "Object unavailable".into();
    }
    info.fields.sort_by_key(|field| field.section);
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

/// Read-only developer view of the very same facts and available UI actions.
/// Unreachable actions remain omitted; task links are URLs, not app commands.
pub fn readout(
    object: &ObjectRef,
    nav: &Navigation,
    galaxy: &Galaxy,
    field: &Starfield,
    cache: &mut Cache,
    now: Now,
    loading: &PlanetLoading,
) -> serde_json::Value {
    use serde_json::json;
    let position = position_ly(object, nav, galaxy, field);
    let info = cache.get(
        object,
        galaxy,
        position,
        now,
        Some(loading),
        nav.system(galaxy),
    );
    let export = |action: Action, group: &str| {
        json!({
            "label": action.label, "current": action.current, "available": true,
            "group": group, "kind": "command",
            "commands": [action.command],
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
        actions(object, nav, galaxy, field, now)
            .into_iter()
            .map(|action| export(action, "navigation")),
    );
    if let ObjectRef::List { id } = object
        && let Some((systems, _)) = list_pages(id, nav, galaxy)
    {
        buttons.extend(systems.into_iter().map(|action| export(action, "systems")));
    }
    json!({
        "object": object,
        "title": info.title,
        "facts": info.rows().map(|(label, value)| json!({"label": label, "value": value})).collect::<Vec<_>>(),
        "fields": info.fields,
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
    object: &ObjectRef,
    nav: &Navigation,
    galaxy: &Galaxy,
    field: &Starfield,
    cache: &mut Cache,
    (now, loading): (Now, &PlanetLoading),
    out: &mut Vec<UiCommand>,
) {
    let position = position_ly(object, nav, galaxy, field);
    cache.get(
        object,
        galaxy,
        position,
        now,
        Some(loading),
        nav.system(galaxy),
    );
    let width = ui.available_width().min(480.0);
    halo_wrapped_text(ui, &cache.info.title, width, CURRENT);
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing.y = 4.0;
        for section in Section::ALL {
            let fields: Vec<_> = cache
                .info
                .fields
                .iter()
                .filter(|f| f.section == section)
                .collect();
            if fields.is_empty() {
                continue;
            }
            if matches!(object, ObjectRef::Planet { .. }) {
                ui.add_space(HUD_LINE_GAP);
                if section == Section::ModelDetails {
                    let label = if cache.model_details_open {
                        "▾ Model details"
                    } else {
                        "▸ Model details"
                    };
                    if halo_text(ui, label, egui::Sense::click(), |response| {
                        if response.hovered() { HOVER } else { LINK }
                    }).clicked() {
                        cache.model_details_open = !cache.model_details_open;
                    }
                    if !cache.model_details_open {
                        continue;
                    }
                } else {
                halo_wrapped_text(ui, section.label(), width, LINK);
                }
                let explanation = match section {
                    Section::Composition => Some("Whole planet · % by mass. Materials may be in the interior or atmosphere; phases are not resolved."),
                    Section::Atmosphere => Some("Atmospheric mixture · % by molecule, not % of planet mass."),
                    _ => None,
                };
                if let Some(text) = explanation {
                    halo_wrapped_text(ui, text, width, LINK);
                }
            }
            for field in fields {
                ui.horizontal(|ui| {
                    let label_width = (width * 0.4).min(150.0);
                    ui.allocate_ui_with_layout(
                        egui::vec2(label_width, 0.0),
                        egui::Layout::top_down(egui::Align::Min),
                        |ui| {
                            ui.set_min_width(label_width);
                            halo_wrapped_text(ui, &field.label, label_width, LINK);
                        },
                    );
                    halo_wrapped_text(ui, field.value.text(), width - label_width - 16.0, CURRENT);
                });
            }
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
    if !actions.is_empty() {
        ui.add_space(HUD_LINE_GAP);
        ui.horizontal_wrapped(|ui| {
            ui.set_max_width(width);
            for (index, action) in actions.into_iter().enumerate() {
                if index > 0 {
                    halo_text(ui, "·", egui::Sense::hover(), |_| LINK);
                }
                if action_text(ui, &action) {
                    out.push(action.command);
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
                    out.push(action.command);
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paused_loading() -> PlanetLoading {
        let mut loading = PlanetLoading::default();
        loading.enabled = false;
        loading
    }

    #[test]
    fn typed_planet_fields_share_ids_sections_units_values_and_unknowns() {
        let mut galaxy = space_simulation::demo_galaxy();
        let planet = &galaxy.systems[0].planets[0];
        let object = ObjectRef::Planet {
            id: planet.id.clone(),
        };
        let info = describe(&object, &galaxy, None, Now::none());
        let mut ids = std::collections::HashSet::new();
        for field in &info.fields {
            assert!(ids.insert(&field.id), "duplicate field {}", field.id);
            assert!(!field.value.text().is_empty());
        }
        // Candidate diagnostics appear only while background refinement is on.
        for section in Section::ALL {
            let shown = info.fields.iter().any(|f| f.section == section);
            assert_eq!(shown, section != Section::Layered, "{section:?}");
        }
        let density = info
            .fields
            .iter()
            .find(|f| f.id == "composition.density")
            .unwrap();
        assert_eq!(density.unit, Some("kg/m³"));
        assert!(matches!(density.value, Value::Known(_)));
        let exported = readout(
            &object,
            &Navigation::default(),
            &galaxy,
            &Starfield::default(),
            &mut Cache::default(),
            Now::none(),
            &paused_loading(),
        );
        assert_eq!(
            exported["fields"],
            serde_json::to_value(&info.fields).unwrap()
        );
        let mut relabelled = info.fields.clone();
        relabelled[0].label = "Translated display label".into();
        assert_eq!(info.fields[0].id, relabelled[0].id);
        galaxy.systems[0].planets[0].physical = None;
        let unknown = describe(&object, &galaxy, None, Now::none());
        assert!(
            unknown
                .fields
                .iter()
                .any(|f| f.id == "composition.density" && matches!(f.value, Value::Unknown(_)))
        );
        assert!(!unknown.rows().any(|(_, value)| value == "0.0 kg/m³"));
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
            let exported = readout(
                &object,
                &nav,
                &galaxy,
                &field,
                &mut cache,
                now,
                &paused_loading(),
            );
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
                assert!(info.rows().any(|(label, _)| label == "Galactic velocity"));
            }
            assert_eq!(
                exported["facts"].as_array().unwrap().len(),
                info.fields.len()
            );
            for (fact, (label, value)) in exported["facts"]
                .as_array()
                .unwrap()
                .iter()
                .zip(info.rows())
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
    fn list_buttons_emit_only_the_star_action_and_cached_facts_invalidate() {
        let mut galaxy = space_simulation::demo_galaxy();
        let nav = Navigation::default();
        let field = Starfield::default();
        let fleet = crate::craft::Fleet::default();
        let now = Now {
            fleet: &fleet,
            seconds: 0.0,
        };
        let inspection = ObjectRef::List {
            id: galaxy.systems[0].id.clone(),
        };
        let mut cache = Cache::default();
        let exported = readout(
            &inspection,
            &nav,
            &galaxy,
            &field,
            &mut cache,
            now,
            &PlanetLoading::default(),
        );
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
            assert_eq!(commands.len(), 1);
        }
        galaxy.systems[0].name = "Updated list".into();
        assert_eq!(
            readout(
                &inspection,
                &nav,
                &galaxy,
                &field,
                &mut cache,
                now,
                &PlanetLoading::default()
            )["title"],
            exported["title"]
        );
        cache.invalidate();
        assert_eq!(
            readout(
                &inspection,
                &nav,
                &galaxy,
                &field,
                &mut cache,
                now,
                &PlanetLoading::default()
            )["title"],
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
        let inspection = ObjectRef::Planet {
            id: planet.id.clone(),
        };
        let mut cache = Cache::default();
        let exported = readout(
            &inspection,
            &nav,
            &galaxy,
            &field,
            &mut cache,
            now,
            &PlanetLoading::default(),
        );
        let link = &exported["actions"][0];
        assert_eq!(link["kind"], "link");
        assert_eq!(link["url"], "https://example.com/task");
        assert_eq!(link["commands"], serde_json::json!([]));
        let unreachable = inspection_for_unreachable();
        let exported = readout(
            &unreachable,
            &nav,
            &galaxy,
            &field,
            &mut cache,
            now,
            &PlanetLoading::default(),
        );
        assert_eq!(exported["actions"], serde_json::json!([]));
    }

    #[test]
    fn layered_section_shows_only_when_enabled_and_labels_unapplied_candidates() {
        use crate::planet_loading::PlanetState;
        use space_simulation::system::{
            generated::{Mode, generated_system_with_mode},
            physical,
            structure::{coupled, fallback, prototype},
        };
        let galaxy = space_simulation::demo_galaxy();
        let system = "g13b6ffffcfeba.2.1";
        let prepared =
            generated_system_with_mode(system, [0.0; 3], Mode::PrepareRefinement).unwrap();
        let (job, solved) = prepared
            .jobs
            .iter()
            .find_map(|j| {
                Some((
                    j,
                    prototype::solve(&j.budget, &j.environment, Default::default()).ok()?,
                ))
            })
            .unwrap();
        let object = ObjectRef::Planet { id: job.id.clone() };
        let layered = |loading: &PlanetLoading| {
            describe_with(&object, &galaxy, None, Now::none(), Some(loading), None)
                .fields
                .into_iter()
                .filter(|f| f.section == Section::Layered)
                .map(|f| (f.id, f.value.text().to_owned()))
                .collect::<Vec<_>>()
        };
        let mut loading = PlanetLoading::default();
        loading.enabled = false;
        assert!(layered(&loading).is_empty());
        let mut cache = Cache::default();
        let off = cache
            .get(&object, &galaxy, None, Now::none(), Some(&loading), None)
            .fields
            .len();
        loading.enabled = true;
        // The cached facts follow the planet's status.
        let waiting = cache.get(&object, &galaxy, None, Now::none(), Some(&loading), None);
        assert_eq!(waiting.fields.len(), off + 1);
        assert_eq!(layered(&loading)[0].1, "Waiting · active result unchanged");
        let planets = &mut loading.cache.entry(system.into()).or_default().planets;
        let fallback = physical::fallback(&fallback::solve(
            &job.budget,
            &job.environment,
            coupled::Error::DenseGasUnsupported,
        ));
        planets.insert(job.id.clone(), PlanetState::Fallback(Box::new(fallback)));
        let facts = layered(&loading);
        assert!(facts[0].1.starts_with("Fallback (layered unsupported: "));
        assert!(facts.iter().any(|(f, _)| f == "layered.radius"));
        let planets = &mut loading.cache.get_mut(system).unwrap().planets;
        planets.insert(
            job.id.clone(),
            PlanetState::Solved(Box::new(physical::layered(&solved))),
        );
        let facts = layered(&loading);
        assert_eq!(facts[0].1, "Solved · candidate, not applied");
        for id in [
            "layered.radius",
            "layered.temperature",
            "layered.pressure",
            "layered.models",
            "layered.fidelity",
            "layered.limitations",
        ] {
            assert!(facts.iter().any(|(f, _)| f == id), "{id}");
        }
        let layers = facts
            .iter()
            .filter(|(f, _)| f.starts_with("layered.layer."))
            .count();
        assert_eq!(layers, physical::layered(&solved).layers.len());
        assert_eq!(
            facts
                .iter()
                .filter(|(f, _)| f.starts_with("layered.assessment."))
                .count(),
            layers
        );
        assert!(
            facts
                .iter()
                .any(|(f, text)| f == "layered.fidelity" && text.contains("Approximate"))
        );
        // The plain readout and the browser never include candidate diagnostics.
        let plain = describe(&object, &galaxy, None, Now::none());
        assert!(!plain.fields.iter().any(|f| f.section == Section::Layered));
    }

    #[test]
    fn inspection_uses_the_loaded_revision_and_refreshes_without_a_worker_status_change() {
        let galaxy = space_simulation::demo_galaxy();
        let mut active = galaxy.systems[0].clone();
        let id = active.planets[0].id.clone();
        let object = ObjectRef::Planet { id };
        active.planets[0].designation = "Loaded revision".into();
        let mut cache = Cache::default();
        let first = cache.get(&object, &galaxy, None, Now::none(), None, Some(&active));
        assert_eq!(first.title, "Loaded revision");
        assert!(first.fields.iter().any(|f| f.id == "result.status"));
        active.planets[0].designation = "Next revision".into();
        active.planets[0]
            .initial_result
            .as_mut()
            .unwrap()
            .revision
            .sequence += 1;
        let next = cache.get(&object, &galaxy, None, Now::none(), None, Some(&active));
        assert_eq!(next.title, "Next revision");
        assert!(
            next.fields
                .iter()
                .any(|f| f.id == "result.sequence" && f.value.text().contains("sequence 1"))
        );
    }

    fn inspection_for_unreachable() -> ObjectRef {
        ObjectRef::Star {
            id: "g100.0.0".into(),
        }
    }
}
