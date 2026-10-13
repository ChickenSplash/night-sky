//! The object browser: a window that builds a `Query` from a search box,
//! filter chips and a scope, sends it as `UiCommand::Search` whenever it
//! changes, and lists the shown request's results (`search::Searches`).
//! Opened by the third halo icon, Ctrl+K, or any `search` command, such as
//! `ns search --show`, which also fills in its filters.
mod filters;
mod picker;
mod results;

use crate::actions::UiCommand;
use crate::search::Searches;
use crate::ui::kit::{self, Theme};
use crate::{Navigation, Universe};
use bevy::prelude::*;
use bevy_egui::egui;
use filters::Chip;
use space_simulation::objects::ObjectRef;
use space_simulation::search::{Query, Scope};

const THEME: Theme = Theme::GAME;
/// Radius choices, in light years.
const RADII: [f64; 5] = [10.0, 50.0, 100.0, 500.0, 1000.0];
const LIMITS: [u32; 3] = [50, 250, 1000];

#[derive(Resource, Default)]
pub struct Browser {
    pub open: bool,
    query: Query,
    /// The search box: a designation, or `ns search` text.
    text: String,
    /// Why the search box's text cannot be read.
    text_error: Option<String>,
    /// Why the query cannot be searched for, such as clashing filters.
    problem: Option<String>,
    /// The text of the query last sent, so any change sends it again.
    sent: Option<String>,
    picker: Option<picker::Picker>,
    selected: Option<ObjectRef>,
    inspector: crate::ui::inspector::Cache,
}

pub struct BrowserPlugin;

impl Plugin for BrowserPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Browser>().add_systems(Update, follow);
    }
}

/// A `search` command from anywhere opens the browser on its query; the
/// browser's own come back here too, unchanged.
fn follow(mut commands: MessageReader<UiCommand>, mut browser: ResMut<Browser>) {
    for command in commands.read() {
        match command {
            UiCommand::Search { query } => {
                let Ok(query) = Query::parse(query) else {
                    continue;
                };
                let text = query.to_text();
                if browser.sent.as_deref() != Some(&text) {
                    browser.selected = None;
                }
                browser.text = query.text.clone();
                browser.query = query;
                browser.sent = Some(text);
                browser.open = true;
            }
            UiCommand::ClearSearch => {
                browser.query = Query::default();
                browser.text.clear();
                browser.selected = None;
                // Nothing is searched for until the query changes.
                browser.sent = Some(browser.query.to_text());
            }
            _ => {}
        }
    }
}

/// What the window reads.
pub struct View<'a> {
    pub searches: &'a Searches,
    pub nav: &'a Navigation,
    pub universe: &'a Universe,
    pub field: &'a crate::stars::stream::Starfield,
}

pub fn window(ctx: &egui::Context, browser: &mut Browser, view: View, out: &mut Vec<UiCommand>) {
    if ctx.input_mut(|input| input.consume_key(egui::Modifiers::COMMAND, egui::Key::K)) {
        browser.open = !browser.open;
    }
    let mut open = browser.open;
    kit::Window {
        title: "Object browser",
        eyebrow: "Explore the galaxy",
        width: (0.45, 560.0, 920.0),
        height: (0.78, 440.0, 900.0),
        min_size: egui::vec2(480.0, 380.0),
    }
    .show(ctx, &THEME, &mut open, |ui| browser.body(ui, &view, out));
    browser.open = open;
    if browser.open {
        picker::show(ctx, &THEME, &mut browser.picker, &mut browser.query);
        if view.searches.available() {
            browser.send(out);
        }
    }
}

impl Browser {
    /// Sends the query if it changed since it was last sent.
    fn send(&mut self, out: &mut Vec<UiCommand>) {
        let text = self.query.to_text();
        if self.sent.as_deref() == Some(&text) {
            return;
        }
        match self.query.check() {
            Ok(()) => {
                out.push(UiCommand::Search {
                    query: text.clone(),
                });
                self.problem = None;
                self.selected = None;
            }
            Err(reason) => self.problem = Some(reason),
        }
        self.sent = Some(text);
    }

    /// Reads the search box: a designation, or whole `ns search` text, which
    /// replaces the filters.
    fn read_text(&mut self) {
        let text = self.text.trim();
        if text.is_empty() {
            self.query.text.clear();
            self.text_error = None;
            return;
        }
        match Query::parse(text) {
            Ok(parsed) => {
                let designation_only = Query {
                    text: parsed.text.clone(),
                    ..Query::default()
                } == parsed;
                if designation_only {
                    self.query.text = parsed.text;
                } else {
                    self.text = parsed.text.clone();
                    self.query = parsed;
                }
                self.text_error = None;
            }
            Err(reason) => self.text_error = Some(reason),
        }
    }

    fn body(&mut self, ui: &mut egui::Ui, view: &View, out: &mut Vec<UiCommand>) {
        let theme = &THEME;
        let search = kit::field(
            ui,
            theme,
            "browser-search",
            "Designation or ID, or ns search text…",
            &mut self.text,
            f32::INFINITY,
        );
        if search.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter)) {
            self.read_text();
        }
        if let Some(error) = &self.text_error {
            kit::note(ui, theme, error);
        }
        self.chips(ui);
        self.scope(ui, view);
        kit::rule(ui, theme);
        results::show(ui, self, view, out);
        self.footer(ui);
    }

    fn chips(&mut self, ui: &mut egui::Ui) {
        let theme = &THEME;
        let chips = filters::chips(&self.query);
        ui.horizontal_wrapped(|ui| {
            if kit::primary_button(ui, theme, "+ Add filter", true).clicked() {
                self.picker = Some(picker::Picker::new());
            }
            if chips.is_empty() {
                ui.label(
                    egui::RichText::new("All objects · add a filter to narrow them")
                        .font(theme.small.id())
                        .color(theme.dim),
                );
            }
            for (chip, label) in chips {
                let pill = kit::pill(ui, theme, &label);
                if pill.remove {
                    filters::remove(&mut self.query, &chip);
                } else if pill.open {
                    self.picker = Some(match chip {
                        Chip::Filter { slot, property } => {
                            picker::Picker::edit(&self.query, slot, property)
                        }
                        Chip::Kinds | Chip::RelatedKinds(_) => picker::Picker::new(),
                    });
                }
            }
        });
    }

    /// Radius or the visited system, and the limit.
    fn scope(&mut self, ui: &mut egui::Ui, view: &View) {
        let theme = &THEME;
        let here = view
            .nav
            .location(&view.universe.0)
            .map(|system| crate::objects::star_id(system, view.nav.page))
            .map(|id| space_simulation::multiple::system_key(&id).to_owned())
            .filter(|key| space_simulation::cells::parse_cell_star(key).is_some());
        let label = |text: &str| {
            egui::RichText::new(text)
                .font(theme.small.id())
                .color(theme.dim)
        };
        ui.horizontal_wrapped(|ui| {
            ui.label(label("Within"));
            let radius = match self.query.scope {
                Scope::Within { radius_ly } => Some(radius_ly),
                Scope::System { .. } => None,
            };
            let mut radii = RADII.to_vec();
            if let Some(r) = radius.filter(|r| !RADII.contains(r)) {
                radii.push(r);
                radii.sort_by(f64::total_cmp);
            }
            for r in radii {
                let text = format!("{} ly", filters::number_text(r));
                if kit::choice(ui, theme, &text, radius == Some(r)) {
                    self.query.scope = Scope::Within { radius_ly: r };
                }
            }
            if let Some(key) = &here {
                let chosen = matches!(&self.query.scope, Scope::System { id } if id == key);
                if kit::choice(ui, theme, "This system", chosen) {
                    self.query.scope = Scope::System { id: key.clone() };
                }
            }
            ui.add_space(12.0);
            ui.label(label("Show"));
            for limit in LIMITS {
                let text = crate::format::grouped(limit as usize);
                if kit::choice(ui, theme, &text, self.query.limit == limit) {
                    self.query.limit = limit;
                }
            }
        });
    }

    /// Sends the query again, from where the camera is now.
    fn refresh(&mut self, out: &mut Vec<UiCommand>) {
        if self.problem.is_some() {
            return;
        }
        let text = self.query.to_text();
        out.push(UiCommand::Search {
            query: text.clone(),
        });
        self.sent = Some(text);
        self.selected = None;
    }

    /// The query as an `ns search` command, and Reset.
    fn footer(&mut self, ui: &mut egui::Ui) {
        let theme = &THEME;
        kit::rule(ui, theme);
        let command = format!("./scripts/ns search {}", self.query.to_text());
        ui.horizontal(|ui| {
            if kit::button(ui, theme, "Copy as ns command")
                .on_hover_text(&command)
                .clicked()
            {
                ui.ctx().copy_text(command.clone());
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if kit::link(ui, theme, "Reset").clicked() {
                    self.query = Query::default();
                    self.text.clear();
                    self.text_error = None;
                }
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(&command)
                            .font(theme.mono.id())
                            .color(theme.dim),
                    )
                    .truncate(),
                );
            });
        });
    }
}

#[cfg(test)]
mod tests;
