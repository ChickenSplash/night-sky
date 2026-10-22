//! Independent location and selection trails. Crumbs inspect, never navigate.
use crate::Navigation;
use crate::actions::{GalaxyReference, UiCommand};
use crate::objects::{self, ObjectRef};
use crate::ui::kit::halo::{CURRENT, HOVER, HUD_LINE_GAP, HUD_PADDING, LINK, halo_text};
use bevy_egui::egui;
use space_shared::{Galaxy, StarSystem};

/// Crumbs from below a system's outermost barycentre down to a planet's host:
/// none around a single star, its star in a binary, and the close pair too in a
/// triple or quadruple. `name` is the system's.
fn members(system: &StarSystem, page: u32, host: &str, name: &str) -> Vec<Crumb> {
    system
        .lineage(page, host)
        .into_iter()
        .skip(1)
        .map(|id| match system.barycentre(&id) {
            Some(pair) => Crumb {
                level: Level::Barycentre,
                label: objects::member_label(name, &pair.component),
                object: ObjectRef::Barycentre { id },
            },
            None => Crumb {
                level: Level::Star,
                label: objects::member_label(name, space_simulation::multiple::member_letters(&id)),
                object: ObjectRef::Star { id },
            },
        })
        .collect()
}

/// The letters of a page's whole system, such as "AB"; empty for one star.
fn root_letters(system: &StarSystem, page: u32) -> String {
    system
        .barycentres_on(page)
        .last()
        .map_or_else(String::new, |pair| pair.component.clone())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrailSource {
    Location,
    Selection,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Inspection {
    pub source: TrailSource,
    pub object: ObjectRef,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    Galaxy,
    /// A list's constellation.
    List,
    Star,
    /// A pair within a multiple system.
    Barycentre,
    Planet,
    Craft,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Crumb {
    pub level: Level,
    pub label: String,
    pub object: ObjectRef,
}

/// Where the camera physically is. Selection is deliberately not consulted.
pub fn trail(nav: &Navigation, galaxy: &Galaxy) -> Vec<Crumb> {
    let mut trail = vec![Crumb {
        level: Level::Galaxy,
        label: "Milky Way".into(),
        object: ObjectRef::Galaxy,
    }];
    let list = nav
        .location(galaxy)
        .and_then(|_| nav.list())
        .and_then(|index| galaxy.systems.get(index));
    if let Some(list) = list {
        trail.push(Crumb {
            level: Level::List,
            label: list.name.clone(),
            object: ObjectRef::List {
                id: list.id.clone(),
            },
        });
    }
    let Some(system) = nav.location(galaxy) else {
        return trail;
    };
    // A list's stars are its systems, numbered as the page controls number them.
    let name = if list.is_some() {
        let pages = system.pages();
        let number = pages.iter().position(|p| *p == nav.page).unwrap_or(0) + 1;
        format!("System {number}")
    } else {
        system.name.clone()
    };
    // A multiple system is its outermost barycentre, named for all its stars.
    trail.push(Crumb {
        level: Level::Star,
        label: objects::member_label(&name, &root_letters(system, nav.page)),
        object: ObjectRef::Star {
            id: objects::star_id(system, nav.page),
        },
    });
    let Some(id) = &nav.location_body else {
        return trail;
    };
    match system.planets.iter().find(|planet| planet.id == *id) {
        Some(planet) if planet.page() == nav.page => {
            trail.extend(members(system, nav.page, &planet.host, &name));
            trail.push(Crumb {
                level: Level::Planet,
                label: planet.designation.clone(),
                object: ObjectRef::Planet {
                    id: planet.id.clone(),
                },
            });
        }
        Some(_) => {}
        // A star or pair of a multiple system, below the whole system.
        None => trail.extend(members(system, nav.page, id, &name)),
    }
    trail
}

/// Keyboard navigation stays separate from breadcrumb inspection.
pub fn up(nav: &Navigation, galaxy: &Galaxy) -> Option<UiCommand> {
    if nav.location_body.is_some() {
        return Some(UiCommand::ShowPage { page: nav.page });
    }
    if nav.location(galaxy).is_some()
        && let Some(index) = nav.list()
    {
        return Some(UiCommand::Jump {
            list: galaxy.systems[index].id.clone(),
        });
    }
    if nav.visit.is_some() {
        return Some(UiCommand::GalaxyReference {
            view: GalaxyReference::FaceOn,
        });
    }
    None
}

/// The selected object's hierarchy. A craft hangs below the planet it
/// orbits, else below its system's star.
pub fn selection_trail(nav: &Navigation, galaxy: &Galaxy, now: crate::craft::Now) -> Vec<Crumb> {
    let Some(selection) = &nav.selection else {
        return Vec::new();
    };
    let object = selection.object();
    let ObjectRef::Craft { id } = &object else {
        return object_trail(nav, galaxy, object);
    };
    let Some(live) = now.get(id) else {
        return Vec::new();
    };
    let parent = match live.surroundings(now.seconds).1 {
        Some(body) if body.kind == space_simulation::craft::ReferenceKind::Planet => {
            ObjectRef::Planet { id: body.id }
        }
        _ => ObjectRef::Star {
            id: live.star().to_owned(),
        },
    };
    let mut trail = if live.star().is_empty() {
        Vec::new()
    } else {
        object_trail(nav, galaxy, parent)
    };
    trail.push(Crumb {
        level: Level::Craft,
        label: live.craft.name.clone(),
        object,
    });
    trail
}

fn object_trail(nav: &Navigation, galaxy: &Galaxy, object: ObjectRef) -> Vec<Crumb> {
    let star = match &object {
        ObjectRef::Star { id } => id.as_str(),
        ObjectRef::Planet { id } => match objects::parent_star(id) {
            Some(id) => id,
            None => return Vec::new(),
        },
        _ => return Vec::new(),
    };
    let mut trail = vec![Crumb {
        level: Level::Galaxy,
        label: "Milky Way".into(),
        object: ObjectRef::Galaxy,
    }];
    // A list page, or the generated system being visited.
    let local = objects::known_system(galaxy, star).or_else(|| {
        let system = nav.system(galaxy)?;
        let page = system
            .pages()
            .into_iter()
            .find(|page| objects::star_id(system, *page) == star)?;
        Some((system, page))
    });
    if let Some((system, page)) = local {
        let list = galaxy.systems.iter().find(|list| list.id == system.id);
        if let Some(list) = list {
            trail.push(Crumb {
                level: Level::List,
                label: list.name.clone(),
                object: ObjectRef::List {
                    id: list.id.clone(),
                },
            });
        }
        let name = if list.is_some() {
            format!(
                "System {}",
                system.pages().iter().position(|p| *p == page).unwrap_or(0) + 1
            )
        } else {
            system.name.clone()
        };
        trail.push(Crumb {
            level: Level::Star,
            label: objects::member_label(&name, &root_letters(system, page)),
            object: ObjectRef::Star { id: star.into() },
        });
        if let ObjectRef::Planet { id } = &object
            && let Some(planet) = system.planets.iter().find(|planet| planet.id == *id)
        {
            trail.extend(members(system, page, &planet.host, &name));
            trail.push(Crumb {
                level: Level::Planet,
                label: planet.designation.clone(),
                object,
            });
        }
    } else {
        // A remote system, never generated whole here: its host is in the
        // planet's ID, though not a triple's pair above it.
        let name = crate::galaxy_nav::label(galaxy, star);
        trail.push(Crumb {
            level: Level::Star,
            label: objects::member_label(&name, &objects::system_letters(star)),
            object: ObjectRef::Star { id: star.into() },
        });
        if let ObjectRef::Planet { id } = &object
            && let Some(host) = objects::planet_host(id)
            && let Some((_, index)) = id.rsplit_once('/')
            && let Ok(index) = index.parse::<usize>()
        {
            let letters = space_simulation::multiple::member_letters(host);
            if !letters.is_empty() && letters != objects::system_letters(star) {
                trail.push(Crumb {
                    level: if letters.len() == 1 {
                        Level::Star
                    } else {
                        Level::Barycentre
                    },
                    label: objects::member_label(&name, letters),
                    object: if letters.len() == 1 {
                        ObjectRef::Star { id: host.into() }
                    } else {
                        ObjectRef::Barycentre { id: host.into() }
                    },
                });
            }
            trail.push(Crumb {
                level: Level::Planet,
                label: space_simulation::cells::planet_designation(host, index),
                object,
            });
        }
    }
    trail
}

/// The trail as plain text, for the debug window and browser tests.
pub fn text(trail: &[Crumb]) -> String {
    trail
        .iter()
        .map(|crumb| crumb.label.as_str())
        .collect::<Vec<_>>()
        .join(" › ")
}

/// Draws the trail as free-floating text at the top left of the 3D view,
/// under any window.
#[allow(clippy::too_many_arguments)]
pub fn show(
    ctx: &egui::Context,
    rect: egui::Rect,
    nav: &Navigation,
    galaxy: &Galaxy,
    field: &crate::stars::stream::Starfield,
    inspector: &mut crate::ui::inspector::Cache,
    now: crate::craft::Now,
    out: &mut Vec<crate::actions::UiCommand>,
) {
    egui::Area::new(egui::Id::new("breadcrumb"))
        .order(egui::Order::Background)
        .constrain(false)
        .fixed_pos(rect.left_top() + egui::Vec2::splat(HUD_PADDING))
        .show(ctx, |ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            // Use the text's height rather than a default button-height row,
            // so the shared inset is measured from the text itself.
            ui.spacing_mut().interact_size.y = 0.0;
            ui.set_max_width((rect.width() - HUD_PADDING * 2.0).max(1.0));
            let location = trail(nav, galaxy);
            let selected = selection_trail(nav, galaxy, now);
            let active = active_inspection(nav, galaxy, &location, &selected);
            for (source, title, trail) in [
                (TrailSource::Location, "Location", &location),
                (TrailSource::Selection, "Selected", &selected),
            ] {
                if trail.is_empty() {
                    continue;
                }
                ui.horizontal_wrapped(|ui| {
                    halo_text(ui, title, egui::Sense::hover(), |_| LINK);
                    for (index, crumb) in trail.iter().enumerate() {
                        if index > 0 {
                            halo_text(ui, "›", egui::Sense::hover(), |_| LINK);
                        }
                        let current = active.source == source && active.object == crumb.object;
                        let clicked =
                            halo_text(ui, &crumb.label, egui::Sense::click(), |response| {
                                if response.hovered() {
                                    HOVER
                                } else if current {
                                    CURRENT
                                } else {
                                    LINK
                                }
                            })
                            .on_hover_cursor(egui::CursorIcon::PointingHand)
                            .clicked();
                        if clicked {
                            out.push(UiCommand::InspectObject {
                                inspection: Inspection {
                                    source,
                                    object: crumb.object.clone(),
                                },
                            });
                        }
                    }
                });
            }
            ui.add_space(HUD_LINE_GAP);
            // Natural content height: no scrolling, scrollbar or reserved
            // bottom strip. Long information can use the whole left side.
            crate::ui::inspector::show(ui, &active, nav, galaxy, field, inspector, now, out);
        });
    // Background areas are excluded by egui's normal pointer-area test.
    if let Some(area) = ctx.memory(|memory| memory.area_rect(egui::Id::new("breadcrumb"))) {
        ctx.data_mut(|data| {
            data.insert_temp(egui::Id::new("breadcrumb-pointer-capture"), (area, false))
        });
    }
}

/// An inspection stays valid while its crumb is in its trail. A list's tab
/// also stays while that list is open, so stepping through its systems (mid
/// flight, or selecting their stars) does not move the inspector away.
pub fn holds(
    nav: &Navigation,
    galaxy: &Galaxy,
    inspection: &Inspection,
    location: &[Crumb],
    selected: &[Crumb],
) -> bool {
    let trail = if inspection.source == TrailSource::Location {
        location
    } else {
        selected
    };
    trail.iter().any(|crumb| crumb.object == inspection.object)
        || matches!(&inspection.object, ObjectRef::List { id }
            if nav.list().is_some_and(|index| galaxy.systems[index].id == *id))
}

pub fn active_inspection(
    nav: &Navigation,
    galaxy: &Galaxy,
    location: &[Crumb],
    selected: &[Crumb],
) -> Inspection {
    if let Some(active) = &nav.inspection
        && holds(nav, galaxy, active, location, selected)
    {
        return active.clone();
    }
    let (source, trail) = if selected.is_empty() {
        (TrailSource::Location, location)
    } else {
        (TrailSource::Selection, selected)
    };
    Inspection {
        source,
        object: trail
            .last()
            .map_or(ObjectRef::Galaxy, |crumb| crumb.object.clone()),
    }
}

#[cfg(test)]
mod tests;
