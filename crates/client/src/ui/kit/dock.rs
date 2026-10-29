//! The right-hand sidebar that kit windows dock into. `begin` reserves its
//! width before anything else is laid out, so the 3D view and HUD shrink
//! beside it rather than sitting underneath. Docked windows share it as tabs.
//! Each docked window checks in every frame it is open; one that stops
//! checking in, such as after a shortcut closed it, leaves the strip.
//!
//! Windows move between floating and docked by being carried: a floating
//! window's header dropped on the screen's right edge (or on the open sidebar)
//! docks it, and a tab or docked header pulled out of the sidebar floats it
//! under the pointer. The carry is the kit's own state rather than an egui
//! drag, because the widget that started it stops drawing when the window
//! changes mode mid-drag.
use super::interact::{ANIMATION_SECONDS, ease_out, pointer_edge};
use super::motion::Eased;
use super::{SPACING, Theme, layout, widgets};
use bevy_egui::egui::{self, Id, Pos2, Rect, Sense, UiBuilder, Vec2, pos2, vec2};

/// The sidebar's own background and tab strip.
const THEME: Theme = Theme::GAME;
/// The widest the sidebar may grow, as a share of the screen.
const MAX_SHARE: f32 = 0.5;
/// How far either side of the sidebar's edge it can be grabbed.
const GRIP: f32 = 4.0;
/// How close to the screen's right edge a carried window docks.
const EDGE_ZONE: f32 = 48.0;
/// How far past the sidebar's edge a tab must be pulled to float.
const TEAR: f32 = 8.0;
/// The drop preview's size, the same on every screen: about the middle
/// tenth of a typical one.
const PREVIEW_SIZE: Vec2 = vec2(64.0, 128.0);
/// How much shorter the preview's outer edge is than its inner one, at each
/// end, as a share of its height.
const PREVIEW_TAPER: f32 = 0.18;

#[derive(Clone)]
struct Tab {
    window: Id,
    title: String,
    /// The window's narrowest outer width, which the sidebar keeps.
    min_width: f32,
    seen: u64,
}

/// A window being dragged between floating and docked.
#[derive(Clone)]
struct Carry {
    window: Id,
    title: String,
    min_width: f32,
    /// Outer floating width, which a never-sized sidebar opens at.
    width: f32,
    /// The pointer's offset from the floating window's top centre.
    grab: Vec2,
    /// Still a tab: it reorders along the strip until pulled out.
    docked: bool,
    /// The press this carry belongs to.
    origin: Option<Pos2>,
}

#[derive(Clone, Default)]
struct Dock {
    tabs: Vec<Tab>,
    active: Option<Id>,
    /// The chosen width, kept while the sidebar is closed.
    width: f32,
    /// The width on screen: easing open, closed or to the chosen width,
    /// except while the user drags the edge.
    shown: Eased,
    /// Where the active window goes, once `begin` has laid out this frame.
    content: Option<(u64, Rect)>,
    /// Last frame's tab buttons and the strip holding them.
    tab_rects: Vec<(Id, Rect)>,
    strip: Option<Rect>,
    carry: Option<Carry>,
    /// A tab double-clicked last frame, floated before this frame's layout
    /// so its window floats beside the full-width scene.
    float: Option<Id>,
    /// The drop preview last shown, kept while it fades out.
    preview: Option<Rect>,
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

    fn rect(&self, screen: Rect) -> Rect {
        Rect::from_min_max(pos2(screen.right() - self.width, screen.top()), screen.max)
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

    /// Where along the strip `pointer` falls, among the tabs other than
    /// `except`: the number of them before it, reading row by row.
    fn slot(&self, pointer: Pos2, except: Option<Id>) -> usize {
        self.tab_rects
            .iter()
            .filter(|(window, _)| Some(*window) != except)
            .filter(|(_, rect)| {
                rect.bottom() < pointer.y
                    || (rect.y_range().contains(pointer.y) && rect.center().x < pointer.x)
            })
            .count()
    }

    fn move_tab(&mut self, window: Id, to: usize) {
        let Some(from) = self.tabs.iter().position(|tab| tab.window == window) else {
            return;
        };
        let tab = self.tabs.remove(from);
        self.tabs.insert(to.min(self.tabs.len()), tab);
    }
}

fn id() -> Id {
    Id::new("kit-dock")
}

fn resize_id() -> Id {
    id().with("resize")
}

fn docked_id(window: Id) -> Id {
    window.with("docked")
}

fn with<R>(ctx: &egui::Context, change: impl FnOnce(&mut Dock) -> R) -> R {
    ctx.data_mut(|data| change(data.get_temp_mut_or_default::<Dock>(id())))
}

/// Whether the window lives in the sidebar, remembered while it is closed.
pub(super) fn is_docked(ctx: &egui::Context, window: Id) -> bool {
    ctx.data(|data| data.get_temp::<bool>(docked_id(window)))
        .unwrap_or(false)
}

fn set_docked(ctx: &egui::Context, window: Id, docked: bool) {
    ctx.data_mut(|data| data.insert_temp(docked_id(window), docked));
}

/// What happened to the carried window this frame.
enum Carried {
    None,
    Torn(Id),
    Dropped(Id),
}

/// Lays out the sidebar and returns the rest of the screen, for the scene.
/// Call once per frame, before anything is placed relative to the scene.
pub fn begin(ctx: &egui::Context) -> Rect {
    let screen = ctx.content_rect();
    let frame = ctx.cumulative_frame_nr();
    let now = ctx.input(|input| input.time);
    let resizing = ctx
        .read_response(resize_id())
        .is_some_and(|response| response.dragged());
    let (drag, pointer, held) = ctx.input(|input| {
        (
            input.pointer.total_drag_delta(),
            input.pointer.latest_pos(),
            input.pointer.primary_down(),
        )
    });
    let start_id = resize_id().with("start");
    let start = ctx.data(|data| data.get_temp::<f32>(start_id));
    let (dock, carried, zone, floated) = with(ctx, |dock| {
        let gone: Vec<_> = dock
            .tabs
            .iter()
            .filter(|tab| tab.seen + 1 < frame)
            .map(|tab| tab.window)
            .collect();
        for window in gone {
            dock.remove(window);
        }
        let floated = dock.float.take();
        if let Some(window) = floated {
            dock.remove(window);
        }
        let (carried, zone) = carry(dock, screen, frame, pointer, held);
        if resizing && let (Some(start), Some(drag)) = (start, drag) {
            dock.width = start - drag.x;
        }
        let (min, max) = dock.width_bounds(screen);
        // Whole points: pointer deltas are fractional, and egui's debug
        // builds outline every widget whose edges are off its rounding grid.
        dock.width = dock.width.clamp(min, max).round();
        let target = if dock.tabs.is_empty() {
            0.0
        } else {
            dock.width
        };
        if resizing {
            dock.shown.snap(target);
        } else {
            dock.shown.retarget(target, now);
        }
        dock.content = None;
        if let Some(zone) = zone {
            dock.preview = Some(zone);
        }
        (dock.clone(), carried, zone, floated)
    });
    if let Some(window) = floated {
        set_docked(ctx, window, false);
    }
    match carried {
        Carried::Torn(window) => set_docked(ctx, window, false),
        Carried::Dropped(window) => set_docked(ctx, window, true),
        Carried::None => {}
    }
    match &dock.carry {
        Some(carry) if carry.docked => ctx.set_cursor_icon(egui::CursorIcon::Grabbing),
        Some(_) => ctx.set_cursor_icon(egui::CursorIcon::AllScroll),
        None => {}
    }
    // Until a drag is under way, keep its starting width current. A drag
    // then measures from the width it was grabbed at, every time.
    if !resizing {
        ctx.data_mut(|data| data.insert_temp(start_id, dock.width));
    }
    preview(ctx, dock.preview, zone.is_some());
    if dock.shown.moving(now) {
        ctx.request_repaint();
    }
    // Opening or closing, the sidebar slides at its full width rather than
    // squeezing its contents; the scene keeps the rest of the screen.
    let open = dock.shown.value(now).round();
    if open <= 0.0 && dock.tabs.is_empty() {
        layout::set_scene(ctx, screen);
        return screen;
    }
    let left = screen.right() - open;
    let rect = Rect::from_min_max(
        pos2(left, screen.top()),
        pos2(left + dock.width, screen.bottom()),
    );
    let scene = Rect::from_min_max(screen.min, pos2(left, screen.bottom()));
    layout::set_scene(ctx, scene);
    let shown = show(ctx, rect, screen, &dock);
    with(ctx, |dock| {
        dock.content = Some((frame, shown.content));
        dock.tab_rects = shown.tabs;
        dock.strip = Some(shown.strip);
        if shown.chosen.is_some() {
            dock.active = shown.chosen;
        }
        dock.float = shown.float;
    });
    if let Some((window, title)) = shown.grabbed {
        pick_up(ctx, window, &title, 0.0, 0.0, Vec2::ZERO, true);
    }
    scene
}

/// Moves the carried window on with the pointer: reordering or tearing out a
/// tab, and docking or letting go on release. Returns any change of mode and
/// the drop zone under a floating carried window.
fn carry(
    dock: &mut Dock,
    screen: Rect,
    frame: u64,
    pointer: Option<Pos2>,
    held: bool,
) -> (Carried, Option<Rect>) {
    let Some(mut carry) = dock.carry.take() else {
        return (Carried::None, None);
    };
    let mut carried = Carried::None;
    if carry.docked
        && let Some(pointer) = pointer
    {
        if pointer.x < dock.rect(screen).left() - TEAR {
            dock.remove(carry.window);
            carry.docked = false;
            // Hold the floating window by the middle of its title.
            carry.grab = vec2(0.0, f32::from(THEME.padding) + 14.0);
            carried = Carried::Torn(carry.window);
        } else if over_strip(dock, pointer) {
            let slot = dock.slot(pointer, Some(carry.window));
            dock.move_tab(carry.window, slot);
        }
    }
    let zone = (!carry.docked)
        .then_some(pointer)
        .flatten()
        .and_then(|pointer| drop_zone(dock, screen, &carry, pointer));
    if held {
        dock.carry = Some(carry);
        return (carried, zone);
    }
    if zone.is_some()
        && let Some(pointer) = pointer
    {
        if dock.width <= 0.0 {
            dock.width = carry.width;
        }
        let slot = if over_strip(dock, pointer) {
            dock.slot(pointer, None)
        } else {
            dock.tabs.len()
        };
        dock.enter(frame, carry.window, &carry.title, carry.min_width);
        dock.move_tab(carry.window, slot);
        dock.active = Some(carry.window);
        carried = Carried::Dropped(carry.window);
    }
    (carried, None)
}

/// Whether `pointer` is over the tab strip, give or take the tabs' spacing.
fn over_strip(dock: &Dock, pointer: Pos2) -> bool {
    !dock.tabs.is_empty()
        && dock
            .strip
            .is_some_and(|strip| strip.expand(SPACING * 0.5).contains(pointer))
}

/// Where a floating window carried to `pointer` would go: the open sidebar,
/// or a new one at the screen's right edge.
fn drop_zone(dock: &Dock, screen: Rect, carry: &Carry, pointer: Pos2) -> Option<Rect> {
    if !dock.tabs.is_empty() {
        let rect = dock.rect(screen);
        return (pointer.x >= rect.left()).then_some(rect);
    }
    if pointer.x < screen.right() - EDGE_ZONE {
        return None;
    }
    let width = if dock.width > 0.0 {
        dock.width
    } else {
        carry.width
    };
    let max = (screen.width() * MAX_SHARE).max(carry.min_width);
    let width = width.clamp(carry.min_width, max).round();
    Some(Rect::from_min_max(
        pos2(screen.right() - width, screen.top()),
        screen.max,
    ))
}

/// A small floating trapezium in the middle of where a carried window would
/// dock, opening towards the screen's centre. It is framed like a window
/// whose edges light on hover, the accent growing from each edge's middle as
/// it slides in from the screen's edge, and retracting as it slides back out.
fn preview(ctx: &egui::Context, rect: Option<Rect>, active: bool) {
    let amount = ctx.animate_bool_with_time_and_easing(
        id().with("preview"),
        active,
        ANIMATION_SECONDS,
        ease_out,
    );
    let Some(rect) = rect.filter(|_| amount > 0.0) else {
        return;
    };
    let screen = ctx.content_rect();
    let shape = Rect::from_center_size(rect.center(), PREVIEW_SIZE);
    let shape = shape.translate(vec2((1.0 - amount) * (screen.right() - shape.left()), 0.0));
    let taper = shape.height() * PREVIEW_TAPER;
    let corners = [
        shape.left_top(),
        pos2(shape.right(), shape.top() + taper),
        pos2(shape.right(), shape.bottom() - taper),
        shape.left_bottom(),
    ];
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Tooltip,
        id().with("preview"),
    ));
    painter.add(egui::Shape::convex_polygon(
        corners.to_vec(),
        THEME.fill.gamma_multiply(amount),
        (THEME.border_width, THEME.border.gamma_multiply(amount)),
    ));
    let width = ctx.style().visuals.widgets.hovered.bg_stroke.width;
    for (index, from) in corners.into_iter().enumerate() {
        let to = corners[(index + 1) % corners.len()];
        let middle = from.lerp(to, 0.5);
        painter.line_segment(
            [middle.lerp(from, amount), middle.lerp(to, amount)],
            (width, THEME.accent.gamma_multiply(amount)),
        );
    }
}

/// What the sidebar's own controls did this frame.
struct Shown {
    /// Where the active window goes.
    content: Rect,
    tabs: Vec<(Id, Rect)>,
    strip: Rect,
    chosen: Option<Id>,
    /// A tab dragged far enough to start carrying it.
    grabbed: Option<(Id, String)>,
    /// A tab double-clicked to float its window, next frame.
    float: Option<Id>,
}

/// The background, tab strip and resizable edge, clipped to the `screen`
/// while sliding in or out.
fn show(ctx: &egui::Context, rect: Rect, screen: Rect, dock: &Dock) -> Shown {
    let mut shown = Shown {
        content: rect,
        tabs: Vec::with_capacity(dock.tabs.len()),
        strip: Rect::NOTHING,
        chosen: None,
        grabbed: None,
        float: None,
    };
    egui::Area::new(id())
        .order(egui::Order::Middle)
        .fixed_pos(rect.min)
        .constrain(false)
        .show(ctx, |ui| {
            ui.set_clip_rect(rect.expand2(vec2(GRIP, 0.0)).intersect(screen));
            ui.expand_to_include_rect(rect);
            ui.painter().rect_filled(rect, 0, THEME.fill.to_opaque());
            let padding = f32::from(THEME.padding);
            shown.strip = ui
                .scope_builder(UiBuilder::new().max_rect(rect.shrink(padding)), |ui| {
                    ui.spacing_mut().item_spacing = vec2(SPACING * 0.5, SPACING * 0.5);
                    ui.horizontal_wrapped(|ui| {
                        for tab in &dock.tabs {
                            let active = dock.active == Some(tab.window);
                            let response = widgets::choice_response(ui, &THEME, &tab.title, active)
                                .on_hover_cursor(egui::CursorIcon::Grab);
                            if response.clicked() {
                                shown.chosen = Some(tab.window);
                            }
                            if response.double_clicked() {
                                shown.float = Some(tab.window);
                            }
                            if response.dragged() {
                                shown.grabbed = Some((tab.window, tab.title.clone()));
                            }
                            shown.tabs.push((tab.window, response.rect));
                        }
                    })
                })
                .response
                .rect;
            shown.content.min.y = shown.strip.bottom() + padding;
            resize_edge(ui, rect);
        });
    shown
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

/// Starts carrying a window whose header or tab is being dragged, unless
/// this press already carries one. `grab` is the pointer's offset from the
/// floating window's top centre; `width` its outer width. Returns whether
/// this started the carry.
pub(super) fn pick_up(
    ctx: &egui::Context,
    window: Id,
    title: &str,
    min_width: f32,
    width: f32,
    grab: Vec2,
    docked: bool,
) -> bool {
    let origin = ctx.input(|input| input.pointer.press_origin());
    with(ctx, |dock| {
        if dock
            .carry
            .as_ref()
            .is_some_and(|carry| carry.origin == origin)
        {
            return false;
        }
        dock.carry = Some(Carry {
            window,
            title: title.to_owned(),
            min_width,
            width,
            grab,
            docked,
            origin,
        });
        true
    })
}

/// The pointer's offset from the top centre of `window`, while it is being
/// carried as a floating window.
/// A carry left over from an earlier press, such as where `begin` did not
/// run to end it, is ignored.
pub(super) fn carried(ctx: &egui::Context, window: Id) -> Option<Vec2> {
    let origin = ctx.input(|input| input.pointer.press_origin())?;
    with(ctx, |dock| {
        dock.carry
            .as_ref()
            .filter(|carry| carry.window == window && !carry.docked && carry.origin == Some(origin))
            .map(|carry| carry.grab)
    })
}

/// Whether a window is being carried, so the pointer belongs to the UI even
/// where no widget is under it.
pub fn carrying(ctx: &egui::Context) -> bool {
    let Some(origin) = ctx.input(|input| input.pointer.press_origin()) else {
        return false;
    };
    with(ctx, |dock| {
        dock.carry
            .as_ref()
            .is_some_and(|carry| carry.origin == Some(origin))
    })
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
    set_docked(ctx, window, true);
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

/// Floats a docked window, which keeps its last floating place.
pub(super) fn leave(ctx: &egui::Context, window: Id) {
    with(ctx, |dock| dock.remove(window));
    set_docked(ctx, window, false);
}

/// Takes a closed window's tab away, remembering that it was docked.
pub(super) fn close(ctx: &egui::Context, window: Id) {
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
