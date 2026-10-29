//! The right-hand sidebar that kit windows dock into. `begin` reserves its
//! width before anything else is laid out, so the 3D view and HUD shrink
//! beside it rather than sitting underneath. Several docked windows share it
//! as tabs. Each docked window checks in every frame it is open; one that
//! stops checking in, such as after a shortcut closed it, leaves the strip.
use super::interact::pointer_edge;
use super::{SPACING, Theme, layout, widgets};
use bevy_egui::egui::{self, Id, Rect, Sense, UiBuilder, pos2, vec2};

/// The sidebar's own background and tab strip.
const THEME: Theme = Theme::GAME;
/// The widest the sidebar may grow, as a share of the screen.
const MAX_SHARE: f32 = 0.5;
/// How far either side of the sidebar's edge it can be grabbed.
const GRIP: f32 = 4.0;

#[derive(Clone)]
struct Tab {
    window: Id,
    title: String,
    /// The window's narrowest outer width, which the sidebar keeps.
    min_width: f32,
    seen: u64,
}

#[derive(Clone, Default)]
struct Dock {
    tabs: Vec<Tab>,
    active: Option<Id>,
    width: f32,
    /// Where the active window goes, once `begin` has laid out this frame.
    content: Option<(u64, Rect)>,
}

impl Dock {
    fn width_bounds(&self, screen: Rect) -> (f32, f32) {
        let min = self
            .tabs
            .iter()
            .map(|tab| tab.min_width)
            .fold(0.0, f32::max);
        (min, (screen.width() * MAX_SHARE).max(min))
    }

    fn remove(&mut self, window: Id) {
        let Some(index) = self.tabs.iter().position(|tab| tab.window == window) else {
            return;
        };
        self.tabs.remove(index);
        if self.active == Some(window) {
            self.active = self
                .tabs
                .get(index)
                .or(self.tabs.last())
                .map(|tab| tab.window);
        }
    }

    /// Adds or refreshes a window's tab. A new tab becomes the active one.
    fn enter(&mut self, frame: u64, window: Id, title: &str, min_width: f32) {
        match self.tabs.iter_mut().find(|tab| tab.window == window) {
            Some(tab) => {
                tab.seen = frame;
                tab.min_width = min_width;
                if tab.title != title {
                    title.clone_into(&mut tab.title);
                }
            }
            None => {
                self.tabs.push(Tab {
                    window,
                    title: title.to_owned(),
                    min_width,
                    seen: frame,
                });
                self.active = Some(window);
            }
        }
    }
}

fn id() -> Id {
    Id::new("kit-dock")
}

fn resize_id() -> Id {
    id().with("resize")
}

fn with<R>(ctx: &egui::Context, change: impl FnOnce(&mut Dock) -> R) -> R {
    ctx.data_mut(|data| change(data.get_temp_mut_or_default::<Dock>(id())))
}

/// Lays out the sidebar and returns the rest of the screen, for the scene.
/// Call once per frame, before anything is placed relative to the scene.
pub fn begin(ctx: &egui::Context) -> Rect {
    let screen = ctx.content_rect();
    let frame = ctx.cumulative_frame_nr();
    let resizing = ctx
        .read_response(resize_id())
        .is_some_and(|response| response.dragged());
    let drag = ctx.input(|input| input.pointer.total_drag_delta());
    let start_id = resize_id().with("start");
    let start = ctx.data(|data| data.get_temp::<f32>(start_id));
    let dock = with(ctx, |dock| {
        let gone: Vec<_> = dock
            .tabs
            .iter()
            .filter(|tab| tab.seen + 1 < frame)
            .map(|tab| tab.window)
            .collect();
        for window in gone {
            dock.remove(window);
        }
        if resizing && let (Some(start), Some(drag)) = (start, drag) {
            dock.width = start - drag.x;
        }
        let (min, max) = dock.width_bounds(screen);
        // Whole points: pointer deltas are fractional, and egui's debug
        // builds outline every widget whose edges are off its rounding grid.
        dock.width = dock.width.clamp(min, max).round();
        dock.content = None;
        dock.clone()
    });
    // Until a drag is under way, keep its starting width current. A drag
    // then measures from the width it was grabbed at, every time.
    if !resizing {
        ctx.data_mut(|data| data.insert_temp(start_id, dock.width));
    }
    if dock.tabs.is_empty() {
        layout::set_scene(ctx, screen);
        return screen;
    }
    let rect = Rect::from_min_max(pos2(screen.right() - dock.width, screen.top()), screen.max);
    let scene = Rect::from_min_max(screen.min, pos2(rect.left(), screen.bottom()));
    layout::set_scene(ctx, scene);
    let (content, chosen) = show(ctx, rect, &dock);
    with(ctx, |dock| {
        dock.content = Some((frame, content));
        if chosen.is_some() {
            dock.active = chosen;
        }
    });
    scene
}

/// The background, tab strip and resizable edge. Returns where the active
/// window goes and any tab clicked.
fn show(ctx: &egui::Context, rect: Rect, dock: &Dock) -> (Rect, Option<Id>) {
    let mut content = rect;
    let mut chosen = None;
    egui::Area::new(id())
        .order(egui::Order::Middle)
        .fixed_pos(rect.min)
        .constrain(false)
        .show(ctx, |ui| {
            ui.set_clip_rect(rect.expand2(vec2(GRIP, 0.0)));
            ui.expand_to_include_rect(rect);
            ui.painter().rect_filled(rect, 0, THEME.fill.to_opaque());
            if dock.tabs.len() > 1 {
                let padding = f32::from(THEME.padding);
                let strip = ui
                    .scope_builder(UiBuilder::new().max_rect(rect.shrink(padding)), |ui| {
                        ui.spacing_mut().item_spacing = vec2(SPACING * 0.5, SPACING * 0.5);
                        ui.horizontal_wrapped(|ui| {
                            for tab in &dock.tabs {
                                let active = dock.active == Some(tab.window);
                                if widgets::choice(ui, &THEME, &tab.title, active) {
                                    chosen = Some(tab.window);
                                }
                            }
                        })
                    })
                    .response
                    .rect;
                content.min.y = strip.bottom() + padding;
            }
            resize_edge(ui, rect);
        });
    (content, chosen)
}

/// The sidebar's left edge: dragged to resize, lit like a window edge.
/// `begin` applies the drag before laying out the frame.
fn resize_edge(ui: &mut egui::Ui, rect: Rect) {
    let handle = Rect::from_min_max(
        pos2(rect.left() - GRIP, rect.top()),
        pos2(rect.left() + GRIP, rect.bottom()),
    );
    let response = ui
        .interact(handle, resize_id(), Sense::drag())
        .on_hover_cursor(egui::CursorIcon::ResizeHorizontal);
    if response.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
    }
    let widgets = &ui.style().visuals.widgets;
    let stroke = if response.dragged() {
        widgets.active.bg_stroke.width
    } else {
        widgets.hovered.bg_stroke.width
    };
    let x = rect.left() + THEME.border_width * 0.5;
    pointer_edge(
        ui.ctx(),
        ui.painter(),
        resize_id().with("edge"),
        [pos2(x, rect.top()), pos2(x, rect.bottom())],
        response.hovered() || response.dragged(),
        (stroke, THEME.accent).into(),
    );
}

/// Docks a floating window as the active tab. A sidebar that has never been
/// sized takes the window's floating `width`; after that it keeps its own.
pub(super) fn enter(ctx: &egui::Context, window: Id, title: &str, min_width: f32, width: f32) {
    let frame = ctx.cumulative_frame_nr();
    with(ctx, |dock| {
        if dock.width <= 0.0 {
            dock.width = width;
        }
        dock.enter(frame, window, title, min_width);
        dock.active = Some(window);
    });
}

/// Keeps an open docked window's tab. Returns where to draw it this frame,
/// or nothing while another tab is in front or the sidebar is not laid out.
/// A sidebar that has never been sized opens at the window's `width`.
pub(super) fn check_in(
    ctx: &egui::Context,
    window: Id,
    title: &str,
    min_width: f32,
    width: f32,
) -> Option<Rect> {
    let frame = ctx.cumulative_frame_nr();
    let (front, content) = with(ctx, |dock| {
        if dock.width <= 0.0 {
            dock.width = width;
        }
        dock.enter(frame, window, title, min_width);
        let content = dock.content.filter(|(laid_out, _)| *laid_out == frame);
        (dock.active == Some(window), content.map(|(_, rect)| rect))
    });
    if content.is_none() {
        ctx.request_repaint();
    }
    content.filter(|_| front)
}

pub(super) fn leave(ctx: &egui::Context, window: Id) {
    with(ctx, |dock| dock.remove(window));
}

pub(super) fn activate(ctx: &egui::Context, window: Id) {
    with(ctx, |dock| {
        if dock.tabs.iter().any(|tab| tab.window == window) {
            dock.active = Some(window);
        }
    });
}

/// Whether the window is docked behind another tab.
pub(super) fn is_background(ctx: &egui::Context, window: Id) -> bool {
    with(ctx, |dock| {
        dock.active != Some(window) && dock.tabs.iter().any(|tab| tab.window == window)
    })
}

#[cfg(test)]
mod tests;
