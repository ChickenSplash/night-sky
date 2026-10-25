//! The shown request's progress and results, and the selected result's
//! facts and actions (the inspector's, so they act as they do there).
use super::{Browser, THEME, View};
use crate::actions::UiCommand;
use crate::search::Entry;
use crate::ui::kit;
use crate::ui::kit::TooltipExt;
use bevy_egui::egui::{self, Align2, pos2, vec2};
use space_simulation::search::registry::{self, KINDS};
use space_simulation::search::{Scope, SearchResult, State};

const ROW: f32 = 46.0;
/// The detail pane's width beside the results.
const DETAIL: f32 = 300.0;

pub(super) fn show(
    ui: &mut egui::Ui,
    browser: &mut Browser,
    view: &View,
    out: &mut Vec<UiCommand>,
) {
    // Only the request for the query as it stands, not an earlier one.
    let entry =
        view.searches.shown_entry.as_ref().filter(|e| {
            browser.sent.as_deref() == Some(e.query.as_str()) && browser.problem.is_none()
        });
    status(ui, browser, entry);
    let results: &[SearchResult] = entry.and_then(|e| e.results.as_deref()).unwrap_or(&[]);
    let selected = view
        .nav
        .selection
        .as_ref()
        .and_then(|selection| results.iter().find(|r| r.object == selection.object()));
    let size = ui.available_size().max(egui::Vec2::ZERO);
    let full = egui::Rect::from_min_size(egui::Pos2::ZERO, size);
    let wide = size.x >= 720.0;
    let gap = 2.0 * kit::SPACING;
    let (table_target, detail_target, hidden) = if wide {
        let left = (size.x - DETAIL - gap).max(0.0);
        (
            egui::Rect::from_min_size(egui::Pos2::ZERO, vec2(left, size.y)),
            egui::Rect::from_min_size(pos2(left + gap, 0.0), vec2(DETAIL, size.y)),
            egui::Rect::from_min_size(pos2(size.x + gap, 0.0), vec2(DETAIL, size.y)),
        )
    } else {
        let height = ((size.y - gap) * 0.5).max(0.0);
        (
            egui::Rect::from_min_size(egui::Pos2::ZERO, vec2(size.x, height)),
            egui::Rect::from_min_size(pos2(0.0, height + gap), vec2(size.x, height)),
            egui::Rect::from_min_size(pos2(0.0, size.y + gap), vec2(size.x, height)),
        )
    };
    let table_rect = kit::animated_rect(
        ui,
        ui.id().with("results-table-motion"),
        full,
        if selected.is_some() {
            table_target
        } else {
            full
        },
    );
    let detail_rect = kit::animated_rect(
        ui,
        ui.id().with("results-detail-motion"),
        hidden,
        if selected.is_some() {
            detail_target
        } else {
            hidden
        },
    );
    // Retain only the outgoing readout for its exit animation. Highlight and
    // actions still follow the authoritative application selection.
    let readout_id = ui.id().with("results-outgoing-detail");
    let readout = ui.data_mut(|data| {
        if let Some(result) = selected {
            data.insert_temp(readout_id, result.clone());
        }
        let result = data.get_temp::<SearchResult>(readout_id);
        if selected.is_none() && detail_rect == hidden {
            data.remove::<SearchResult>(readout_id);
            None
        } else {
            result
        }
    });
    // Reserve the viewport once. Animated children must never feed their
    // off-screen positions or natural content sizes back into window sizing.
    let (viewport, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    let clip = ui.clip_rect().intersect(viewport);
    let table_rect = table_rect.translate(viewport.min.to_vec2());
    let mut table_ui = ui.new_child(
        egui::UiBuilder::new()
            .id_salt("results-table-pane")
            .max_rect(table_rect)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    table_ui.set_clip_rect(clip.intersect(table_rect));
    list(&mut table_ui, results, table_rect.height(), out, view);
    if let Some(result) = readout {
        let rect = detail_rect.translate(viewport.min.to_vec2());
        let mut detail_ui = ui.new_child(
            egui::UiBuilder::new()
                .id_salt("results-detail-pane")
                .max_rect(rect)
                .layout(egui::Layout::top_down(egui::Align::Min)),
        );
        detail_ui.set_clip_rect(clip.intersect(rect));
        if selected.is_none() {
            detail_ui.disable();
        }
        detail(&mut detail_ui, browser, &result, view, out);
    }
}

fn radius_text(browser: &Browser) -> String {
    match &browser.query.scope {
        Scope::Within { radius_ly } => {
            format!("within {} ly", super::filters::number_text(*radius_ly))
        }
        Scope::System { .. } => "in this system".into(),
    }
}

/// Status text, inside the progress bar while a search runs.
fn status(ui: &mut egui::Ui, browser: &mut Browser, entry: Option<&Entry>) {
    let theme = &THEME;
    let count = |n: usize| match n {
        1 => "1 result".to_owned(),
        n => format!("{} results", crate::format::grouped(n)),
    };
    let (text, bar) = if let Some(problem) = &browser.problem {
        (format!("Can't search for this: {problem}"), None)
    } else if let Some(entry) = entry {
        let p = &entry.progress;
        let n = entry.result_count;
        let scope = radius_text(browser);
        match (&entry.error, p.state) {
            (Some(error), _) => (format!("Not found: {error}"), None),
            (None, State::Running) => {
                let radius = match browser.query.scope {
                    Scope::Within { radius_ly } => radius_ly,
                    Scope::System { .. } => 0.0,
                };
                let share = (radius > 0.0).then(|| (p.complete_to_ly / radius) as f32);
                (
                    format!(
                        "Searching {scope} · out to {:.1} ly · {} so far",
                        p.complete_to_ly,
                        count(n)
                    ),
                    Some(share),
                )
            }
            (None, State::Done) if n == 0 => (format!("Nothing matches {scope}"), None),
            (None, State::Done) => (String::new(), None),
            (None, State::Capped) => (
                format!(
                    "{} · stopped at its work limit, complete out to {:.1} ly",
                    count(n),
                    p.complete_to_ly
                ),
                None,
            ),
            (None, State::Cancelled) => ("Stopped".into(), None),
        }
    } else if browser.sent.is_some() {
        ("Searching…".into(), Some(None))
    } else {
        ("".into(), None)
    };
    if let Some(done) = bar {
        kit::progress(ui, theme, done, &text);
    } else if !text.is_empty() {
        ui.add(
            egui::Label::new(
                egui::RichText::new(text)
                    .font(theme.small.id())
                    .color(theme.dim),
            )
            .truncate(),
        );
    }
}

fn distance_text(ly: f64) -> String {
    match ly {
        ly if ly < 10.0 => format!("{ly:.2} ly"),
        ly if ly < 100.0 => format!("{ly:.1} ly"),
        ly => format!("{} ly", crate::format::grouped(ly.round() as usize)),
    }
}

fn kind_label(id: &str) -> &str {
    registry::kind_index(id).map_or(id, |i| KINDS[i].label)
}

/// One line of text, cut short with `…` to `width`.
fn fitted(
    ui: &egui::Ui,
    text: &str,
    font: egui::FontId,
    colour: egui::Color32,
    width: f32,
) -> std::sync::Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::simple_singleline(text.to_owned(), font, colour);
    job.wrap = egui::text::TextWrapping::truncate_at_width(width.max(10.0));
    ui.painter().layout_job(job)
}

pub(super) fn list(
    ui: &mut egui::Ui,
    results: &[SearchResult],
    height: f32,
    out: &mut Vec<UiCommand>,
    view: &View,
) {
    ui.scope(|ui| {
        list_content(ui, results, height, out, view);
    });
}

fn list_content(
    ui: &mut egui::Ui,
    results: &[SearchResult],
    height: f32,
    out: &mut Vec<UiCommand>,
    view: &View,
) {
    let theme = &THEME;
    let selection = view.nav.selection.as_ref().map(crate::Selection::object);
    let width = ui.available_width() - ui.spacing().scroll.allocated_width();
    let class_x = (width * 0.55).max(200.0);
    let (rect, _) = ui.allocate_exact_size(vec2(width, 20.0), egui::Sense::hover());
    for (x, align, text) in [
        (
            rect.left() + theme.inset,
            Align2::LEFT_TOP,
            "OBJECT · SYSTEM",
        ),
        (rect.left() + class_x, Align2::LEFT_TOP, "CLASS"),
        (rect.right() - theme.inset, Align2::RIGHT_TOP, "DISTANCE"),
    ] {
        let galley = egui::WidgetText::from(kit::eyebrow_text(theme, text)).into_galley(
            ui,
            Some(egui::TextWrapMode::Extend),
            f32::INFINITY,
            theme.font.id(),
        );
        let label_rect = align.anchor_size(pos2(x, rect.top()), galley.size());
        ui.place(
            label_rect,
            egui::Label::new(kit::eyebrow_text(theme, text)).extend(),
        );
    }
    // Move inter-row spacing inside the hit/highlight rectangle, keeping
    // the same text spacing but no dead strip between neighbouring rows.
    let padding = ui.spacing().item_spacing.y;
    let row_height = ROW + padding;
    let viewport_height = (height - 20.0 - padding).max(0.0);
    ui.spacing_mut().item_spacing.y = 0.0;
    kit::ScrollArea::vertical()
        .id_salt("browser-results")
        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::VisibleWhenNeeded)
        .scroll_source(kit::SCROLL_SOURCE)
        .auto_shrink([false, false])
        .min_scrolled_height(0.0)
        .max_height(viewport_height)
        .show_rows(ui, row_height, results.len(), |ui, rows| {
            // Keep rows aligned with their headings even without a scrollbar.
            ui.set_max_width(width);
            for result in &results[rows] {
                let selected = selection.as_ref() == Some(&result.object);
                let (rect, response, hover) = kit::list_row(ui, theme, row_height, selected);
                let name_width = class_x - theme.inset * 2.0;
                let top = rect.top() + 7.0 + padding * 0.5;
                let colour = theme.text.lerp_to_gamma(theme.accent, hover * 0.5);
                let painter = ui.painter();
                painter.galley(
                    pos2(rect.left() + theme.inset + 4.0, top),
                    fitted(ui, &result.name, theme.font.id(), colour, name_width),
                    colour,
                );
                let secondary = format!(
                    "{} · {}",
                    result.system,
                    kind_label(&result.kind).to_lowercase()
                );
                painter.galley(
                    pos2(rect.left() + theme.inset + 4.0, top + 19.0),
                    fitted(ui, &secondary, theme.small.id(), theme.dim, name_width),
                    theme.dim,
                );
                let distance = distance_text(result.distance_ly);
                let distance_width = 80.0;
                painter.galley(
                    pos2(rect.left() + class_x, top + 7.0),
                    fitted(
                        ui,
                        &result.class,
                        theme.small.id(),
                        theme.text,
                        rect.width() - class_x - distance_width - theme.inset,
                    ),
                    theme.text,
                );
                painter.text(
                    pos2(rect.right() - theme.inset, rect.center().y),
                    Align2::RIGHT_CENTER,
                    distance,
                    theme.mono.id(),
                    theme.text,
                );
                if response.clicked() {
                    match &result.object {
                        space_simulation::objects::ObjectRef::Star { id } => {
                            out.push(UiCommand::SelectStar { id: id.clone() });
                        }
                        space_simulation::objects::ObjectRef::Planet { id } => {
                            out.push(UiCommand::Inspect { planet: id.clone() });
                        }
                        space_simulation::objects::ObjectRef::Craft { id } => {
                            out.push(UiCommand::SelectCraft { id: id.clone() });
                        }
                        _ => {}
                    }
                }
                if response.double_clicked()
                    && let Some(action) = crate::ui::inspector::actions(
                        &result.object,
                        view.nav,
                        &view.universe.0,
                        view.field,
                        view.craft,
                    )
                    .into_iter()
                    .next()
                {
                    out.push(action.command);
                }
            }
        });
}

/// The selected result's name, actions and facts.
fn detail(
    ui: &mut egui::Ui,
    browser: &mut Browser,
    result: &SearchResult,
    view: &View,
    out: &mut Vec<UiCommand>,
) {
    let theme = &THEME;
    let galaxy = &view.universe.0;
    let position = crate::ui::inspector::position_ly(&result.object, view.nav, galaxy, view.field);
    let info = browser
        .inspector
        .get(&result.object, galaxy, position, view.craft);
    kit::eyebrow(ui, theme, "Selected");
    let title = if info.title.is_empty() {
        &result.name
    } else {
        &info.title
    };
    ui.label(
        egui::RichText::new(title)
            .font(egui::FontId::proportional(18.0))
            .color(theme.text),
    );
    ui.label(
        egui::RichText::new(format!(
            "{} · {}",
            result.class,
            distance_text(result.distance_ly)
        ))
        .font(theme.small.id())
        .color(theme.dim),
    );
    let actions =
        crate::ui::inspector::actions(&result.object, view.nav, galaxy, view.field, view.craft);
    ui.horizontal_wrapped(|ui| {
        for (index, action) in actions.iter().enumerate() {
            let response = if index == 0 {
                kit::primary_button(ui, theme, &action.label, true)
            } else {
                kit::button(ui, theme, &action.label)
            };
            if response.clicked() {
                out.push(action.command.clone());
            }
        }
        let id = match &result.object {
            space_simulation::objects::ObjectRef::Star { id }
            | space_simulation::objects::ObjectRef::Barycentre { id }
            | space_simulation::objects::ObjectRef::Planet { id }
            | space_simulation::objects::ObjectRef::List { id }
            | space_simulation::objects::ObjectRef::Craft { id } => id.clone(),
            space_simulation::objects::ObjectRef::Galaxy => String::new(),
        };
        if kit::link(ui, theme, "Copy ID")
            .on_hover_text_themed(theme, &id)
            .clicked()
        {
            ui.ctx().copy_text(id);
        }
    });
    let info = browser
        .inspector
        .get(&result.object, galaxy, position, view.craft);
    let rows: Vec<(&str, String)> = info
        .rows()
        .map(|(label, value)| (label, value.to_owned()))
        .collect();
    let note = info.note.clone();
    kit::ScrollArea::vertical()
        .id_salt("browser-detail")
        .scroll_source(kit::SCROLL_SOURCE)
        .auto_shrink([false, true])
        .show(ui, |ui| {
            kit::facts(ui, theme, "browser-facts", &rows);
            if !note.is_empty() {
                kit::note(ui, theme, &note);
            }
        });
}
