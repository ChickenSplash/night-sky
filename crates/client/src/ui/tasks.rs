//! Lists and tasks as their own window, apart from the space simulator. A
//! future task integration belongs here; the rest of the client reaches lists
//! only through `UiCommand`s and the snapshot.
use crate::Navigation;
use crate::actions::UiCommand;
use crate::format::due_short;
use crate::ui::kit::TooltipExt;
use crate::ui::kit::{self, Theme};
use bevy_egui::egui;
use space_shared::{Galaxy, StarSystem, Task};
use std::collections::HashSet;

const ROW: f32 = 30.0;
pub const WINDOW: &str = "tasks";

fn tint(colour: [u8; 3]) -> egui::Color32 {
    egui::Color32::from_rgb(colour[0], colour[1], colour[2])
}

/// Panel-only choices: filters, sorting and folded status groups.
pub struct Tasks {
    list_filter: String,
    /// Searches every list's task planets while not empty.
    task_filter: String,
    list_sort: ListSort,
    task_sort: TaskSort,
    group: bool,
    /// Folded status groups, by lower-case status.
    collapsed: HashSet<String>,
}

impl Default for Tasks {
    fn default() -> Self {
        Self {
            list_filter: String::new(),
            task_filter: String::new(),
            list_sort: ListSort::Default,
            task_sort: TaskSort::Default,
            group: true,
            collapsed: HashSet::new(),
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum ListSort {
    Default,
    Name,
    Tasks,
}

impl ListSort {
    const ALL: [Self; 3] = [Self::Default, Self::Name, Self::Tasks];
    fn label(self) -> &'static str {
        match self {
            Self::Default => "List order",
            Self::Name => "Name A–Z",
            Self::Tasks => "Most tasks",
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum TaskSort {
    Default,
    Name,
    Due,
}

impl TaskSort {
    const ALL: [Self; 3] = [Self::Default, Self::Name, Self::Due];
    fn label(self) -> &'static str {
        match self {
            Self::Default => "List order",
            Self::Name => "Name A–Z",
            Self::Due => "Due date",
        }
    }
}

/// A row of the task index: a status group's heading, or a task.
enum Row<'a> {
    Group {
        key: String,
        name: &'a str,
        colour: [u8; 3],
        count: usize,
    },
    Task(&'a StarSystem, usize, &'a Task),
}

/// Uses the same header movement, bounded resizing and foreground stacking
/// as the object browser and debug console.
pub fn window(
    ctx: &egui::Context,
    open: &mut bool,
    galaxy: &Galaxy,
    nav: &Navigation,
    state: &mut Tasks,
    out: &mut Vec<UiCommand>,
) {
    let mut panel = Panel {
        galaxy,
        nav,
        state,
        out,
    };
    kit::Window::new(WINDOW, "Tasks").show(ctx, &Theme::GAME, open, |ui| {
        kit::ScrollArea::vertical()
            .id_salt("tasks")
            .scroll_source(kit::SCROLL_SOURCE)
            .auto_shrink([false, false])
            .min_scrolled_height(0.0)
            .max_height(ui.available_height().max(0.0))
            .show(ui, |ui| panel.sections(ui));
    });
}

/// It only reads state; every change is a `UiCommand`.
struct Panel<'a> {
    galaxy: &'a Galaxy,
    nav: &'a Navigation,
    state: &'a mut Tasks,
    out: &'a mut Vec<UiCommand>,
}

impl Panel<'_> {
    /// Every section in one column, each collapsible.
    fn sections(&mut self, ui: &mut egui::Ui) {
        if kit::section(ui, &Theme::GAME, "01 / List constellations") {
            self.lists(ui);
        }
        ui.add_space(kit::SPACING);
        if kit::section(ui, &Theme::GAME, "02 / Task index") {
            self.tasks(ui);
        }
    }

    fn lists(&mut self, ui: &mut egui::Ui) {
        let (galaxy, nav, state) = (self.galaxy, self.nav, &mut *self.state);
        kit::note(
            ui,
            &Theme::GAME,
            &format!(
                "{} lists in your local universe\n{}\n{}",
                galaxy.systems.len(),
                galaxy.source,
                nav.notice
            ),
        );
        kit::field(
            ui,
            &Theme::GAME,
            "task-list-filter",
            "Filter lists…",
            &mut state.list_filter,
            f32::INFINITY,
        );
        ui.horizontal_wrapped(|ui| {
            for sort in ListSort::ALL {
                if kit::choice(ui, &Theme::GAME, sort.label(), state.list_sort == sort) {
                    state.list_sort = sort;
                }
            }
        });
        let filter = state.list_filter.trim().to_lowercase();
        let mut lists: Vec<_> = galaxy
            .systems
            .iter()
            .enumerate()
            .filter(|(_, s)| filter.is_empty() || s.name.to_lowercase().contains(&filter))
            .collect();
        match state.list_sort {
            ListSort::Default => {}
            ListSort::Name => lists.sort_by_key(|(_, s)| s.name.to_lowercase()),
            ListSort::Tasks => lists.sort_by_key(|(_, s)| std::cmp::Reverse(s.tasks().count())),
        }
        kit::ScrollArea::vertical()
            .id_salt("systems")
            .scroll_source(kit::SCROLL_SOURCE)
            .min_scrolled_height(0.0)
            .max_height(170.0)
            .show(ui, |ui| {
                for (i, system) in lists {
                    let active = nav.list() == Some(i);
                    let text = format!("{}\n{} task planets", system.name, system.tasks().count());
                    if task_row(ui, &text, Theme::GAME.text, 48.0, active).clicked() {
                        self.out.push(UiCommand::Jump {
                            list: system.id.clone(),
                        });
                    }
                }
            });
    }

    fn tasks(&mut self, ui: &mut egui::Ui) {
        let (galaxy, nav, state) = (self.galaxy, self.nav, &mut *self.state);
        let search = kit::field(
            ui,
            &Theme::GAME,
            "task-filter",
            "Search all task planets…",
            &mut state.task_filter,
            f32::INFINITY,
        );
        ui.horizontal_wrapped(|ui| {
            for sort in TaskSort::ALL {
                if kit::choice(ui, &Theme::GAME, sort.label(), state.task_sort == sort) {
                    state.task_sort = sort;
                }
            }
        });
        if kit::toggle(ui, &Theme::GAME, state.group, "Group by status") {
            state.group = !state.group;
        }
        let query = state.task_filter.trim().to_lowercase();
        // The current list's tasks, or every list's while searching.
        let mut matches: Vec<_> = galaxy
            .systems
            .iter()
            .enumerate()
            .filter(|(si, _)| !query.is_empty() || nav.list() == Some(*si))
            .flat_map(|(_, system)| {
                system
                    .tasks()
                    .filter(|(_, task)| {
                        query.is_empty() || task.name.to_lowercase().contains(&query)
                    })
                    .map(move |(pi, task)| (system, pi, task))
            })
            .collect();
        match state.task_sort {
            TaskSort::Default => {}
            TaskSort::Name => matches.sort_by_key(|(_, _, t)| t.name.to_lowercase()),
            TaskSort::Due => matches.sort_by_key(|(_, _, t)| t.due_date_ms.unwrap_or(i64::MAX)),
        }
        let total = matches.len();
        let rows = if state.group {
            group_rows(matches, &state.collapsed)
        } else {
            matches
                .into_iter()
                .map(|(s, pi, t)| Row::Task(s, pi, t))
                .collect()
        };
        let shown = rows.iter().filter(|r| matches!(r, Row::Task(..))).count();
        // Enter finds the first task in view.
        if search.lost_focus()
            && ui.input(|i| i.key_pressed(egui::Key::Enter))
            && let Some(Row::Task(system, _, task)) =
                rows.iter().find(|r| matches!(r, Row::Task(..)))
        {
            self.out.push(UiCommand::FindTask {
                list: system.id.clone(),
                task: task.id.clone(),
            });
        }
        kit::note(
            ui,
            &Theme::GAME,
            &format!(
                "{total} task{}{}{}",
                if total == 1 { "" } else { "s" },
                if query.is_empty() { "" } else { " matching" },
                if total > shown {
                    format!(" · {} folded", total - shown)
                } else {
                    String::new()
                }
            ),
        );
        let selected_list = nav.list();
        let selected = nav.selected_planet(galaxy);
        // Searching still scans names, but egui only builds widgets for the
        // visible rows. A list can hold over a thousand tasks.
        kit::ScrollArea::vertical()
            .id_salt("task-index")
            .scroll_source(kit::SCROLL_SOURCE)
            .min_scrolled_height(0.0)
            .auto_shrink([false, true])
            .max_height(360.0)
            .show_rows(ui, ROW, rows.len(), |ui, range| {
                for row in &rows[range] {
                    match row {
                        Row::Group {
                            key,
                            name,
                            colour,
                            count,
                        } => {
                            let folded = state.collapsed.contains(key);
                            let text = format!(
                                "{} {}  ({count})",
                                if folded { "⏵" } else { "⏷" },
                                name.to_uppercase()
                            );
                            if task_row(ui, &text, tint(*colour), ROW, false).clicked()
                                && !state.collapsed.remove(key)
                            {
                                state.collapsed.insert(key.clone());
                            }
                        }
                        Row::Task(system, pi, task) => {
                            let active = selected_list
                                .is_some_and(|i| galaxy.systems[i].id == system.id)
                                && selected == Some(*pi);
                            let due = task
                                .due_date_ms
                                .map(|ms| format!("   {}", due_short(ms)))
                                .unwrap_or_default();
                            let text = format!("· {}{due}", task.name);
                            if task_row(ui, &text, tint(task.colour), ROW, active)
                                .on_hover_text_themed(&Theme::GAME, &task.name)
                                .clicked()
                            {
                                self.out.push(UiCommand::FindTask {
                                    list: system.id.clone(),
                                    task: task.id.clone(),
                                });
                            }
                        }
                    }
                }
            });
    }
}

/// Shared animated rows, retaining task/status colours and multiline list
/// captions. Truncate each line rather than growing the window for long names.
fn task_row(
    ui: &mut egui::Ui,
    text: &str,
    colour: egui::Color32,
    height: f32,
    selected: bool,
) -> egui::Response {
    let theme = &Theme::GAME;
    let (rect, response, _) = kit::list_row(ui, theme, height, selected);
    let lines: Vec<_> = text.lines().collect();
    let line_height = ui.fonts_mut(|fonts| fonts.row_height(&theme.font.id()));
    let top = rect.center().y - line_height * lines.len() as f32 * 0.5;
    for (index, line) in lines.iter().enumerate() {
        let mut job =
            egui::text::LayoutJob::simple_singleline((*line).to_owned(), theme.font.id(), colour);
        job.wrap = egui::text::TextWrapping::truncate_at_width(
            (rect.width() - 2.0 * theme.inset).max(0.0),
        );
        let galley = ui.painter().layout_job(job);
        ui.painter().galley(
            egui::pos2(rect.left() + theme.inset, top + index as f32 * line_height),
            galley,
            colour,
        );
    }
    response
}

/// Status groups follow the board: open, then custom, then done and closed.
fn group_rows<'a>(
    tasks: Vec<(&'a StarSystem, usize, &'a Task)>,
    collapsed: &HashSet<String>,
) -> Vec<Row<'a>> {
    struct Group<'a> {
        key: String,
        task: &'a Task,
        rank: u32,
        tasks: Vec<(&'a StarSystem, usize, &'a Task)>,
    }
    let mut groups: Vec<Group> = Vec::new();
    for entry in tasks {
        let key = entry.2.status.to_lowercase();
        match groups.iter_mut().find(|g| g.key == key) {
            Some(group) => group.tasks.push(entry),
            None => groups.push(Group {
                key,
                task: entry.2,
                rank: status_rank(entry.2),
                tasks: vec![entry],
            }),
        }
    }
    groups.sort_by(|a, b| {
        (a.rank, a.task.status_order, &a.task.status).cmp(&(
            b.rank,
            b.task.status_order,
            &b.task.status,
        ))
    });
    let mut rows = Vec::new();
    for group in groups {
        let folded = collapsed.contains(&group.key);
        rows.push(Row::Group {
            name: &group.task.status,
            colour: group.task.colour,
            count: group.tasks.len(),
            key: group.key,
        });
        if !folded {
            rows.extend(
                group
                    .tasks
                    .into_iter()
                    .map(|(s, pi, t)| Row::Task(s, pi, t)),
            );
        }
    }
    rows
}

fn status_rank(task: &Task) -> u32 {
    match task.status_type.as_str() {
        "open" => 0,
        "custom" => 1,
        "done" => 2,
        "closed" => 3,
        _ => {
            let status = task.status.to_lowercase();
            if ["complete", "done", "closed"]
                .iter()
                .any(|word| status.contains(word))
            {
                2
            } else {
                1
            }
        }
    }
}

#[cfg(test)]
mod tests;
