//! The trail from the galaxy down to where the camera is: `Milky Way › list ›
//! star › planet`. Levels that do not apply are left out, so a star outside
//! every list goes straight from the galaxy to the star. Each crumb carries the
//! `UiCommand` that goes to its level, so clicks, `Esc` and typed `up` share
//! one path. New levels (moons, surface sites) are one more push in `trail`.
use crate::Navigation;
use crate::actions::{GalaxyReference, UiCommand};
use bevy_egui::egui;
use space_shared::Galaxy;

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
    /// Goes to this level. On the current level it frames it again.
    pub go: UiCommand,
}

/// Where the camera is, from the galaxy down. A star selected elsewhere is not
/// part of it; a selected planet of the system the camera is at is.
pub fn trail(nav: &Navigation, galaxy: &Galaxy) -> Vec<Crumb> {
    let mut trail = vec![Crumb {
        level: Level::Galaxy,
        label: "Milky Way".into(),
        go: UiCommand::GalaxyReference {
            view: GalaxyReference::FaceOn,
        },
    }];
    let list = nav.list().and_then(|index| galaxy.systems.get(index));
    if let Some(list) = list {
        trail.push(Crumb {
            level: Level::List,
            label: list.name.clone(),
            go: UiCommand::Jump {
                list: list.id.clone(),
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
        go: UiCommand::ShowPage { page: nav.page },
    });
    if let Some(planet) = nav
        .selected_planet()
        .and_then(|index| system.planets.get(index))
        .filter(|planet| planet.page() == nav.page)
    {
        trail.push(Crumb {
            level: Level::Planet,
            label: planet.designation.clone(),
            go: UiCommand::Follow,
        });
    }
    trail
}

/// Goes one level up the trail, or `None` at the galaxy.
pub fn up(nav: &Navigation, galaxy: &Galaxy) -> Option<UiCommand> {
    let mut trail = trail(nav, galaxy);
    trail.pop();
    trail.pop().map(|crumb| crumb.go)
}

/// The trail as plain text, for the debug window and browser tests.
pub fn text(trail: &[Crumb]) -> String {
    trail
        .iter()
        .map(|crumb| crumb.label.as_str())
        .collect::<Vec<_>>()
        .join(" › ")
}

const LINK: egui::Color32 = egui::Color32::from_rgb(150, 174, 202);
const HOVER: egui::Color32 = egui::Color32::from_rgb(99, 214, 240);
const CURRENT: egui::Color32 = egui::Color32::from_rgb(238, 244, 252);
const SIZE: f32 = 13.0;
const MARGIN: egui::Vec2 = egui::vec2(20.0, 16.0);
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
    trail: &[Crumb],
    out: &mut Vec<crate::actions::UiCommand>,
) {
    egui::Area::new(egui::Id::new("breadcrumb"))
        .order(egui::Order::Background)
        .fixed_pos(rect.left_top() + MARGIN)
        .show(ctx, |ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            ui.horizontal(|ui| {
                for (index, crumb) in trail.iter().enumerate() {
                    if index > 0 {
                        halo_text(ui, "›", egui::Sense::hover(), |_| LINK);
                    }
                    let current = index + 1 == trail.len();
                    let clicked = halo_text(ui, &crumb.label, egui::Sense::click(), |response| {
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
                        out.push(crumb.go.clone());
                    }
                }
            });
        });
}

/// Text with a soft dark halo, coloured once its response (hover) is known.
fn halo_text(
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
    use crate::{Selection, Visit, views::View};

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
    fn a_constellation_shows_its_list_but_no_star() {
        let galaxy = space_simulation::demo_galaxy();
        let nav = Navigation {
            view: View::Constellation,
            arrived: false,
            ..at_list(&galaxy, 0)
        };
        let name = galaxy.systems[0].name.clone();
        assert_eq!(labels(&nav, &galaxy), ["Milky Way".to_owned(), name]);
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
    fn a_selected_planet_of_the_system_ends_the_trail() {
        let galaxy = space_simulation::demo_galaxy();
        let mut nav = at_list(&galaxy, 0);
        let (index, planet) = galaxy.systems[0]
            .planets
            .iter()
            .enumerate()
            .find(|(_, p)| p.page() == nav.page)
            .unwrap();
        nav.selection = Some(Selection::Planet(index));
        let trail = trail(&nav, &galaxy);
        assert_eq!(trail.last().unwrap().label, planet.designation);
        assert_eq!(trail.last().unwrap().go, UiCommand::Follow);
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
        assert_eq!(trail(&nav, &galaxy).len(), 2);
    }
}
