//! Independent location and selection trails. Crumbs inspect, never navigate.
use crate::Navigation;
use crate::actions::{GalaxyReference, UiCommand};
use crate::objects::{self, ObjectRef};
use bevy_egui::egui;
use space_shared::Galaxy;

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
    Planet,
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
    let label = if list.is_some() {
        let pages = system.pages();
        let number = pages.iter().position(|p| *p == nav.page).unwrap_or(0) + 1;
        format!("System {number}")
    } else {
        system.name.clone()
    };
    trail.push(Crumb {
        level: Level::Star,
        label,
        object: ObjectRef::Star {
            id: objects::star_id(system, nav.page),
        },
    });
    if let Some(planet) = nav
        .location_planet
        .as_ref()
        .and_then(|id| system.planets.iter().find(|planet| planet.id == *id))
        .filter(|planet| planet.page() == nav.page)
    {
        trail.push(Crumb {
            level: Level::Planet,
            label: planet.designation.clone(),
            object: ObjectRef::Planet {
                id: planet.id.clone(),
            },
        });
    }
    trail
}

/// Keyboard navigation stays separate from breadcrumb inspection.
pub fn up(nav: &Navigation, galaxy: &Galaxy) -> Option<UiCommand> {
    if nav.location_planet.is_some() {
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

pub fn selection_trail(nav: &Navigation, galaxy: &Galaxy) -> Vec<Crumb> {
    let Some(selection) = &nav.selection else {
        return Vec::new();
    };
    let object = selection.object();
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
    let mapped = objects::known_system(galaxy, star);
    if let Some((system, page)) = &mapped {
        if let Some(list) = galaxy.systems.iter().find(|list| list.id == system.id) {
            trail.push(Crumb {
                level: Level::List,
                label: list.name.clone(),
                object: ObjectRef::List {
                    id: list.id.clone(),
                },
            });
        }
        let label = if galaxy.systems.iter().any(|list| list.id == system.id) {
            format!(
                "System {}",
                system.pages().iter().position(|p| p == page).unwrap_or(0) + 1
            )
        } else {
            system.name.clone()
        };
        trail.push(Crumb {
            level: Level::Star,
            label,
            object: ObjectRef::Star { id: star.into() },
        });
        if let ObjectRef::Planet { id } = &object
            && let Some(planet) = system.planets.iter().find(|planet| planet.id == *id)
        {
            trail.push(Crumb {
                level: Level::Planet,
                label: planet.designation.clone(),
                object,
            });
        }
    } else {
        trail.push(Crumb {
            level: Level::Star,
            label: crate::galaxy_nav::label(galaxy, star),
            object: ObjectRef::Star { id: star.into() },
        });
        if let ObjectRef::Planet { id } = &object
            && let Some((_, index)) = id.rsplit_once('/')
            && let Ok(index) = index.parse::<usize>()
        {
            trail.push(Crumb {
                level: Level::Planet,
                label: space_simulation::cells::planet_designation(star, index),
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

pub(crate) const LINK: egui::Color32 = egui::Color32::from_rgb(150, 174, 202);
pub(crate) const HOVER: egui::Color32 = egui::Color32::from_rgb(99, 214, 240);
pub(crate) const CURRENT: egui::Color32 = egui::Color32::from_rgb(238, 244, 252);
const SIZE: f32 = 13.0;
/// Shared inset for text HUDs around the 3D viewport.
pub(crate) const HUD_PADDING: f32 = 20.0;
pub(crate) const HUD_LINE_GAP: f32 = 8.0;
/// The soft halo behind the text, so it reads over bright stars and the
/// galaxy band without a box: a dark ring at the letter edges (`HALO_EDGE`
/// opacity, 1 px out), fading through a fainter one (`HALO_FADE`, 2 px out).
/// Overlapping copies make it densest right at the letters.
const HALO_EDGE: u8 = 130;
const HALO_FADE: u8 = 45;

/// Draws the trail as free-floating text at the top left of the 3D view,
/// under any window.
pub fn show(
    ctx: &egui::Context,
    rect: egui::Rect,
    nav: &Navigation,
    galaxy: &Galaxy,
    inspector: &mut crate::inspector::Cache,
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
            let selected = selection_trail(nav, galaxy);
            let active = active_inspection(nav, &location, &selected);
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
            crate::inspector::show(ui, &active.object, galaxy, inspector);
        });
    // Background areas are excluded by egui's normal pointer-area test.
    if let Some(area) = ctx.memory(|memory| memory.area_rect(egui::Id::new("breadcrumb"))) {
        ctx.data_mut(|data| {
            data.insert_temp(egui::Id::new("breadcrumb-pointer-capture"), (area, false))
        });
    }
}

pub fn active_inspection(nav: &Navigation, location: &[Crumb], selected: &[Crumb]) -> Inspection {
    if let Some(active) = &nav.inspection {
        let trail = if active.source == TrailSource::Location {
            location
        } else {
            selected
        };
        if trail.iter().any(|crumb| crumb.object == active.object) {
            return active.clone();
        }
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

/// Text with a soft dark halo, coloured once its response (hover) is known.
pub(crate) fn halo_text(
    ui: &mut egui::Ui,
    text: &str,
    sense: egui::Sense,
    colour: impl FnOnce(&egui::Response) -> egui::Color32,
) -> egui::Response {
    let galley = ui.painter().layout_no_wrap(
        text.to_owned(),
        egui::FontId::proportional(SIZE),
        egui::Color32::PLACEHOLDER,
    );
    paint_halo(ui, galley, sense, colour)
}

pub(crate) fn halo_wrapped_text(
    ui: &mut egui::Ui,
    text: &str,
    width: f32,
    colour: egui::Color32,
) -> egui::Response {
    let galley = ui.painter().layout(
        text.to_owned(),
        egui::FontId::proportional(SIZE),
        egui::Color32::PLACEHOLDER,
        width.max(1.0),
    );
    paint_halo(ui, galley, egui::Sense::hover(), |_| colour)
}

fn paint_halo(
    ui: &mut egui::Ui,
    galley: std::sync::Arc<egui::Galley>,
    sense: egui::Sense,
    colour: impl FnOnce(&egui::Response) -> egui::Color32,
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(galley.size(), sense);
    let painter = ui.painter();
    for (radius, alpha, steps) in [(2.0, HALO_FADE, 16), (1.0, HALO_EDGE, 8)] {
        for step in 0..steps {
            let angle = std::f32::consts::TAU * step as f32 / steps as f32;
            let offset = egui::vec2(angle.cos(), angle.sin()) * radius;
            painter.galley(
                rect.min + offset,
                galley.clone(),
                egui::Color32::from_black_alpha(alpha),
            );
        }
    }
    painter.galley(rect.min, galley, colour(&response));
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Selection, Visit, presentation::PresentationMode};

    #[test]
    fn selection_trail_is_remote_and_inspector_falls_back_when_a_location_disappears() {
        let galaxy = space_simulation::demo_galaxy();
        let mut nav = at_list(&galaxy, 0);
        let planet = &galaxy.systems[1].planets[0];
        nav.select(Some(Selection::Planet(planet.id.clone())));
        let location = trail(&nav, &galaxy);
        let selected = selection_trail(&nav, &galaxy);
        assert_eq!(location.last().unwrap().level, Level::Star);
        assert_eq!(selected.last().unwrap().object, planet_ref(planet));
        assert_eq!(
            active_inspection(&nav, &location, &selected).object,
            planet_ref(planet)
        );
        nav.inspection = Some(Inspection {
            source: TrailSource::Location,
            object: location.last().unwrap().object.clone(),
        });
        nav.arrived = false;
        assert_eq!(
            active_inspection(&nav, &trail(&nav, &galaxy), &selected).object,
            planet_ref(planet)
        );
    }

    fn planet_ref(planet: &space_shared::Planet) -> ObjectRef {
        ObjectRef::Planet {
            id: planet.id.clone(),
        }
    }

    #[test]
    fn a_local_planet_selection_survives_reordering_by_identity() {
        let mut galaxy = space_simulation::demo_galaxy();
        let mut nav = at_list(&galaxy, 0);
        let id = galaxy.systems[0].planets[0].id.clone();
        nav.select(Some(Selection::Planet(id.clone())));
        galaxy.systems[0].planets.reverse();
        let index = nav.selected_planet(&galaxy).unwrap();
        assert_eq!(galaxy.systems[0].planets[index].id, id);
    }

    fn labels(nav: &Navigation, galaxy: &Galaxy) -> Vec<String> {
        trail(nav, galaxy).into_iter().map(|c| c.label).collect()
    }

    fn at_list(galaxy: &Galaxy, page_index: usize) -> Navigation {
        let page = galaxy.systems[0].pages()[page_index];
        Navigation {
            visit: Some(Visit::List(0)),
            page,
            arrived: true,
            ..Navigation::default()
        }
    }

    #[test]
    fn open_space_is_only_the_galaxy() {
        let galaxy = space_simulation::demo_galaxy();
        let nav = Navigation::default();
        assert_eq!(labels(&nav, &galaxy), ["Milky Way"]);
        assert_eq!(up(&nav, &galaxy), None);
    }

    #[test]
    fn a_constellation_is_not_a_physical_location() {
        let galaxy = space_simulation::demo_galaxy();
        let nav = Navigation {
            presentation_mode: PresentationMode::Constellation,
            arrived: false,
            ..at_list(&galaxy, 0)
        };
        assert_eq!(labels(&nav, &galaxy), ["Milky Way"]);
        assert_eq!(
            up(&nav, &galaxy),
            Some(UiCommand::GalaxyReference {
                view: GalaxyReference::FaceOn
            })
        );
    }

    #[test]
    fn a_list_star_slots_its_constellation_in_and_numbers_its_system() {
        let galaxy = space_simulation::demo_galaxy();
        let nav = at_list(&galaxy, 1);
        let trail = trail(&nav, &galaxy);
        assert_eq!(
            trail.iter().map(|c| c.level).collect::<Vec<_>>(),
            [Level::Galaxy, Level::List, Level::Star]
        );
        assert_eq!(trail[2].label, "System 2");
        assert_eq!(
            up(&nav, &galaxy),
            Some(UiCommand::Jump {
                list: galaxy.systems[0].id.clone()
            })
        );
    }

    #[test]
    fn physical_planet_location_is_independent_of_selection() {
        let galaxy = space_simulation::demo_galaxy();
        let mut nav = at_list(&galaxy, 0);
        let (_, planet) = galaxy.systems[0]
            .planets
            .iter()
            .enumerate()
            .find(|(_, p)| p.page() == nav.page)
            .unwrap();
        nav.select(Some(Selection::Planet(planet.id.clone())));
        assert_eq!(trail(&nav, &galaxy).last().unwrap().level, Level::Star);
        assert_eq!(
            selection_trail(&nav, &galaxy).last().unwrap().level,
            Level::Planet
        );
        nav.location_planet = Some(planet.id.clone());
        nav.select(None);
        let trail = trail(&nav, &galaxy);
        assert_eq!(trail.last().unwrap().label, planet.designation);
        assert_eq!(
            trail.last().unwrap().object,
            ObjectRef::Planet {
                id: planet.id.clone()
            }
        );
        assert_eq!(
            up(&nav, &galaxy),
            Some(UiCommand::ShowPage { page: nav.page })
        );
    }

    #[test]
    fn a_generated_star_follows_the_galaxy_and_a_star_elsewhere_is_left_out() {
        use space_simulation::{
            cells::{Cell, Sky},
            galaxy::GalaxyModel,
        };
        let galaxy = space_simulation::demo_galaxy();
        let home = GalaxyModel::milky_way().home_pc;
        let cell = Cell::containing(10, home).unwrap();
        let id = Sky::default().cell_star_at(cell, 0).unwrap().id;
        let system = crate::galaxy_nav::generated(&id, bevy::math::DVec3::ZERO).unwrap();
        let name = system.name.clone();
        let nav = Navigation {
            visit: Some(Visit::Generated(Box::new(system))),
            arrived: true,
            selection: crate::galaxy_nav::page_pick(
                &galaxy.systems[1],
                galaxy.systems[1].first_page(),
            )
            .map(Selection::Star),
            ..Navigation::default()
        };
        assert_eq!(labels(&nav, &galaxy), ["Milky Way".to_owned(), name]);
        assert_eq!(text(&trail(&nav, &galaxy)).matches(" › ").count(), 1);
    }

    #[test]
    fn not_yet_arrived_shows_no_star() {
        let galaxy = space_simulation::demo_galaxy();
        let nav = Navigation {
            arrived: false,
            ..at_list(&galaxy, 0)
        };
        assert_eq!(trail(&nav, &galaxy).len(), 1);
    }
}
