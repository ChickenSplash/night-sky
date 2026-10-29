//! Independent location and selection trails. Crumbs select, never navigate.
use crate::Navigation;
use crate::actions::UiCommand;
use crate::navigation::objects::{self, ObjectRef};
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
                label: objects::member_label(
                    name,
                    space_simulation::star::multiple::member_letters(&id),
                ),
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
    let name = objects::system_name(galaxy, system, nav.page);
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

/// Keyboard navigation stays separate from breadcrumb selection.
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
        return Some(UiCommand::ViewGalaxy);
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
    if object == ObjectRef::Galaxy {
        return vec![Crumb {
            level: Level::Galaxy,
            label: "Milky Way".into(),
            object,
        }];
    }
    if let ObjectRef::List { id } = &object {
        let Some(list) = galaxy.systems.iter().find(|list| list.id == *id) else {
            return Vec::new();
        };
        let mut trail = object_trail(nav, galaxy, ObjectRef::Galaxy);
        trail.push(Crumb {
            level: Level::List,
            label: list.name.clone(),
            object,
        });
        return trail;
    }
    let star = match &object {
        ObjectRef::Star { id } | ObjectRef::Barycentre { id } => {
            space_simulation::star::multiple::system_key(id)
        }
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
        let name = objects::system_name(galaxy, system, page);
        trail.push(Crumb {
            level: Level::Star,
            label: objects::member_label(&name, &root_letters(system, page)),
            object: ObjectRef::Star { id: star.into() },
        });
        // A star or pair below the whole system, as the location shows it.
        if let ObjectRef::Star { id } | ObjectRef::Barycentre { id } = &object
            && id != star
        {
            trail.extend(members(system, page, id, &name));
        }
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
        let name = crate::navigation::galaxy_nav::label(galaxy, star);
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
            let letters = space_simulation::star::multiple::member_letters(host);
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
                label: space_simulation::galaxy::cells::planet_designation(host, index),
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
    (now, loading): (crate::craft::Now, &crate::planet_loading::PlanetLoading),
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
            // Area's previous natural height must not constrain a newly longer tab.
            ui.set_max_height((rect.height() - HUD_PADDING * 2.0).max(1.0));
            let location = trail(nav, galaxy);
            let selected = selection_trail(nav, galaxy, now);
            let active = inspector_object(nav, &location);
            for (title, trail) in [("Location", &location), ("Selected", &selected)] {
                if trail.is_empty() {
                    continue;
                }
                ui.horizontal_wrapped(|ui| {
                    halo_text(ui, title, egui::Sense::hover(), |_| LINK);
                    for (index, crumb) in trail.iter().enumerate() {
                        if index > 0 {
                            halo_text(ui, "›", egui::Sense::hover(), |_| LINK);
                        }
                        let current = active == crumb.object;
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
                            out.push(objects::selection_command(&crumb.object));
                        }
                    }
                });
            }
            ui.add_space(HUD_LINE_GAP);
            // Keep long compositions inside the viewport, including small windows.
            let height = (rect.bottom() - ui.cursor().top() - HUD_PADDING * 3.0).max(1.0);
            ui.set_max_width(ui.available_width().min(480.0));
            crate::ui::kit::ScrollArea::vertical()
                .id_salt(("inspector", format!("{active:?}")))
                .max_height(height)
                .min_scrolled_height(1.0)
                .show(ui, |ui| {
                    crate::ui::inspector::show(
                        ui,
                        &active,
                        nav,
                        galaxy,
                        field,
                        inspector,
                        (now, loading),
                        out,
                    );
                });
        });
    // Background areas are excluded by egui's normal pointer-area test.
    if let Some(area) = ctx.memory(|memory| memory.area_rect(egui::Id::new("breadcrumb"))) {
        ctx.data_mut(|data| {
            data.insert_temp(egui::Id::new("breadcrumb-pointer-capture"), (area, false))
        });
    }
}

/// Selection drives the inspector, with physical location as the no-selection fallback.
pub fn inspector_object(nav: &Navigation, location: &[Crumb]) -> ObjectRef {
    nav.selection.as_ref().map_or_else(
        || {
            location
                .last()
                .map_or(ObjectRef::Galaxy, |crumb| crumb.object.clone())
        },
        crate::Selection::object,
    )
}

#[cfg(test)]
mod tests;
