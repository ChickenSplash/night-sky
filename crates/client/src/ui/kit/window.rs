//! A kit window: floating (movable by its whole padded header, resizable,
//! opening at the top right below the HUD icons) or docked into the sidebar.
//! Carrying the header to the screen's right edge docks it and pulling its
//! tab out floats it (see `dock`); double-clicking the header switches too.
//! Each window remembers its choice. Switching glides it between the two,
//! and a docked window coming to the front slides in; a window carried out
//! of the sidebar follows the pointer instead.
use super::interact::ANIMATION_SECONDS;
use super::motion::Glide;
use super::{SPACING, Theme, dock, layout, widgets};
use bevy_egui::egui::{self, Color32, Id, Rect, Sense, Ui, UiBuilder, Vec2, epaint, pos2, vec2};

/// Space between a window and the screen's edges when it first opens.
const GAP: f32 = 12.0;
/// How far a docked window coming to the front slides in from.
pub(super) const REVEAL: f32 = 24.0;

const RESIZE_HANDLES: [&str; 8] = [
    "left",
    "right",
    "top",
    "bottom",
    "left_top",
    "right_top",
    "left_bottom",
    "right_bottom",
];

/// A default length along one axis: a share of the screen, then clamped.
#[derive(Clone, Copy)]
pub struct Extent {
    pub share: f32,
    pub min: f32,
    pub max: f32,
}

impl Extent {
    pub const fn new(share: f32, min: f32, max: f32) -> Self {
        Self { share, min, max }
    }

    fn of(self, length: f32) -> f32 {
        (length * self.share).clamp(self.min, self.max)
    }
}

pub struct Window<'a> {
    /// Stable identity: state survives title changes and docking.
    key: &'a str,
    title: &'a str,
    /// Small text above a framed title; text-mode windows have a prompt.
    eyebrow: &'a str,
    width: Extent,
    height: Extent,
    min_size: egui::Vec2,
}

/// The egui ID of the floating window registered under `key`.
pub fn window_id(key: &str) -> Id {
    Id::new(("window", key))
}

/// Opens or closes a window from its shortcut or icon. A window docked
/// behind another tab comes to the front instead of closing.
pub fn toggle_window(ctx: &egui::Context, key: &str, open: &mut bool) {
    let window = window_id(key);
    if *open && dock::is_background(ctx, window) {
        dock::activate(ctx, window);
    } else {
        *open = !*open;
    }
}

/// Opens a window, bringing it to the front if it is docked behind a tab.
pub fn reveal_window(ctx: &egui::Context, key: &str, open: &mut bool) {
    *open = true;
    dock::activate(ctx, window_id(key));
}

/// Where and how a window last drew, to notice it switching mode or
/// coming to the front.
#[derive(Clone, Copy)]
struct Drawn {
    docked: bool,
    /// Carried by the pointer, which may take it past the scene's edges.
    carried: bool,
    rect: Rect,
    frame: u64,
}

/// A window on its way to where it now belongs.
#[derive(Clone, Copy)]
enum Arrival {
    /// From floating into the sidebar, or back out (`docked: false`).
    Glide { glide: Glide, docked: bool },
    /// A floating window let go partly outside the scene, returning inside.
    Settle { glide: Glide },
    /// A docked window coming to the front, sliding in and fading up.
    Front { started: f64 },
}

/// How a window drawn at a fixed place looks this frame.
struct Look {
    fill: Color32,
    opacity: f32,
    /// Docked, or floating again but still gliding out of the sidebar.
    docked: bool,
    /// Mid-glide: only the close button responds.
    gliding: bool,
}

#[derive(Default)]
struct HeaderActions {
    close: bool,
    /// Double-clicked: dock a floating window, or float a docked one.
    dock: bool,
    /// Dragged far enough to start carrying the window.
    grabbed: bool,
}

impl<'a> Window<'a> {
    pub fn new(key: &'a str, title: &'a str) -> Self {
        Self {
            key,
            title,
            eyebrow: "",
            width: Extent::new(0.4, 420.0, 760.0),
            height: Extent::new(0.6, 320.0, 680.0),
            min_size: vec2(320.0, 200.0),
        }
    }

    #[cfg_attr(not(test), expect(dead_code, reason = "no player window has one yet"))]
    pub fn eyebrow(mut self, eyebrow: &'a str) -> Self {
        self.eyebrow = eyebrow;
        self
    }

    /// Default content size when first opened, from the screen's size.
    pub fn size(mut self, width: Extent, height: Extent) -> Self {
        self.width = width;
        self.height = height;
        self
    }

    pub fn min_size(mut self, min_size: egui::Vec2) -> Self {
        self.min_size = min_size;
        self
    }

    /// Draws the window while `open`; its close button clears `open`.
    pub fn show(
        self,
        ctx: &egui::Context,
        theme: &Theme,
        open: &mut bool,
        add: impl FnOnce(&mut Ui),
    ) {
        if !*open {
            return;
        }
        let id = window_id(self.key);
        let frame = 2.0 * (f32::from(theme.padding) + theme.border_width);
        let min_width = self.min_size.x + frame;
        let docked = dock::is_docked(ctx, id);
        let arrival = arrival(ctx, id, docked);
        let actions = if docked {
            // Carried into the sidebar: floating again returns it to where
            // it was picked up from, not the screen's edge.
            let home = self.position_id().with("home");
            if let Some(home) = ctx.data_mut(|data| data.remove_temp::<egui::Pos2>(home)) {
                ctx.data_mut(|data| data.insert_temp(self.position_id(), home));
            }
            let width = self.width.of(ctx.content_rect().width()) + frame;
            let actions = match dock::check_in(ctx, id, self.title, min_width, width) {
                Some(target) => {
                    let (rect, look) = docked_look(ctx, theme, target, arrival);
                    self.show_fixed(ctx, theme, id, rect, &look, add)
                }
                None => HeaderActions::default(),
            };
            if actions.grabbed {
                dock::pick_up(ctx, id, self.title, min_width, width, Vec2::ZERO, true);
            }
            if actions.dock {
                dock::leave(ctx, id);
            }
            actions
        } else if let Some(Arrival::Glide { glide, .. } | Arrival::Settle { glide }) = arrival {
            // Drawn like a docked window until it lands, so it can change
            // size and opacity on the way.
            let now = ctx.input(|input| input.time);
            let (rect, amount) = glide.sample(self.floating_target(ctx, theme, id), now);
            let from = match arrival {
                Some(Arrival::Settle { .. }) => theme.fill,
                _ => theme.fill.to_opaque(),
            };
            let look = Look {
                fill: from.lerp_to_gamma(theme.fill, amount),
                opacity: 1.0,
                docked: false,
                gliding: true,
            };
            self.show_fixed(ctx, theme, id, rect, &look, add)
        } else {
            let (actions, rect) = self.show_floating(ctx, theme, id, min_width, add);
            if let Some(rect) = rect {
                let carried = dock::carried(ctx, id).is_some();
                drawn(ctx, id, false, carried, rect);
            }
            if actions.dock
                && let Some(rect) = rect
            {
                dock::enter(ctx, id, self.title, min_width, rect.width());
            }
            actions
        };
        if actions.close {
            dock::close(ctx, id);
            *open = false;
        }
    }

    /// Where the floating window's centre was last left.
    fn position_id(&self) -> Id {
        Id::new(("window-center", self.key))
    }

    /// Where a window first opens: at the top right, below the HUD icons.
    fn default_position(&self, ctx: &egui::Context, scene: Rect, outer: egui::Vec2) -> egui::Pos2 {
        pos2(
            scene.right() - GAP - outer.x * 0.5,
            below_icons(ctx, scene.top() + GAP) + outer.y * 0.5,
        )
    }

    /// Where a window floating again lands: its last floating place and
    /// size, kept within the scene.
    fn floating_target(&self, ctx: &egui::Context, theme: &Theme, window_id: Id) -> Rect {
        let scene = layout::scene(ctx);
        let frame = egui::Vec2::splat(2.0 * (f32::from(theme.padding) + theme.border_width));
        let size = ctx
            .memory(|memory| memory.area_rect(window_id))
            .map_or_else(
                || vec2(self.width.of(scene.width()), self.height.of(scene.height())) + frame,
                |rect| rect.size(),
            );
        let centre = ctx
            .data(|data| data.get_temp::<egui::Pos2>(self.position_id()))
            .unwrap_or_else(|| self.default_position(ctx, scene, size));
        let rect = Rect::from_center_size(centre, size);
        let shift =
            |low: f32, high: f32, min: f32, max: f32| (min - low).max(0.0) + (max - high).min(0.0);
        rect.translate(vec2(
            shift(rect.left(), rect.right(), scene.left(), scene.right()),
            shift(rect.top(), rect.bottom(), scene.top(), scene.bottom()),
        ))
    }

    fn show_floating(
        &self,
        ctx: &egui::Context,
        theme: &Theme,
        window_id: Id,
        min_width: f32,
        add: impl FnOnce(&mut Ui),
    ) -> (HeaderActions, Option<Rect>) {
        let scene = layout::scene(ctx);
        let size = vec2(self.width.of(scene.width()), self.height.of(scene.height()));
        let padding = f32::from(theme.padding);
        let pointer = ctx.input(|input| input.pointer.latest_pos());
        let carried = dock::carried(ctx, window_id).zip(pointer);
        // A carried window follows the pointer anywhere, over the sidebar
        // and past the screen's edges; let go, it glides back beside them.
        let bounds = if carried.is_some() {
            ctx.content_rect()
        } else {
            scene
        };
        clamp_resize(
            ctx,
            window_id,
            self.min_size + egui::Vec2::splat(2.0 * (padding + theme.border_width)),
            scene,
        );
        let mut actions = HeaderActions::default();
        let position_id = self.position_id();
        let outer = size + egui::Vec2::splat(2.0 * (padding + theme.border_width));
        let height = ctx
            .memory(|memory| memory.area_rect(window_id))
            .map_or(outer.y, |rect| rect.height());
        let position = carried
            .map(|(grab, pointer)| pointer - grab + vec2(0.0, height * 0.5))
            .or_else(|| ctx.data(|data| data.get_temp::<egui::Pos2>(position_id)))
            .unwrap_or_else(|| self.default_position(ctx, scene, outer));
        // egui paints resize feedback from the area's context style, outside
        // the content closure. Suppress only that native stroke, then restore
        // the original style for controls and after this window is drawn.
        let style = ctx.style();
        let mut frame_style = (*style).clone();
        frame_style.visuals.widgets.hovered.bg_stroke.color = Color32::TRANSPARENT;
        frame_style.visuals.widgets.active.bg_stroke.color = Color32::TRANSPARENT;
        frame_style.visuals.widgets.hovered.fg_stroke.color = theme.accent;
        frame_style.visuals.widgets.active.fg_stroke.color = theme.accent;
        ctx.set_style(frame_style);
        // egui fades in a window that was not drawn last frame. Only fade
        // one opening: one landing from a glide was on screen all along.
        let frame_nr = ctx.cumulative_frame_nr();
        let opening = ctx
            .data(|data| data.get_temp::<Drawn>(drawn_id(window_id)))
            .is_none_or(|drawn| drawn.frame + 1 < frame_nr);
        let window = egui::Window::new(self.title)
            .id(window_id)
            .fade_in(opening)
            // egui 0.33 checks the anchor (not just the pivot) to decide
            // which edges an immovable window may resize from. Track the
            // centre anchor ourselves to allow every edge and header moves.
            .pivot(egui::Align2::CENTER_CENTER)
            .constrain_to(bounds)
            .constrain(carried.is_none())
            .anchor(egui::Align2::CENTER_CENTER, position - bounds.center())
            .title_bar(false)
            .frame(
                egui::Frame::new()
                    .fill(theme.fill)
                    .stroke((theme.border_width, theme.border))
                    .corner_radius(theme.radius)
                    .inner_margin(theme.padding)
                    .shadow(epaint::Shadow {
                        offset: [0, 6],
                        blur: 24,
                        spread: 0,
                        color: Color32::from_black_alpha(140),
                    }),
            )
            // `size` is the content's; the frame's padding and border go
            // outside it.
            .default_size(size)
            .min_size(self.min_size)
            .movable(false)
            .resizable(true)
            .order(egui::Order::Foreground);
        let shown = window.show(ctx, |ui| {
            ui.set_style(style.clone());
            // egui sizes the frame to the content, while the header aligns to
            // the size egui keeps. Short content, such as a single note, would
            // otherwise shrink the frame and leave the header buttons outside.
            ui.set_min_size(ui.max_rect().size());
            actions = self.header(ui, theme, egui::CursorIcon::AllScroll);
            content(ui, window_id, add);
        });
        ctx.set_style(style.clone());
        let rect = shown.map(|shown| {
            crate::render::window_blur::register(ctx, window_id, shown.response.rect);
            resize_edges(
                ctx,
                theme,
                shown.response.rect,
                shown.response.layer_id,
                &style,
            );
            let rect = shown.response.rect;
            if actions.grabbed
                && let Some(origin) = ctx.input(|input| input.pointer.press_origin())
            {
                let grab = origin - pos2(rect.center().x, rect.top());
                if dock::pick_up(
                    ctx,
                    window_id,
                    self.title,
                    min_width,
                    rect.width(),
                    grab,
                    false,
                ) {
                    ctx.data_mut(|data| data.insert_temp(position_id.with("home"), rect.center()));
                }
            }
            let position = dock::carried(ctx, window_id)
                .zip(pointer)
                .map_or(rect.center(), |(grab, pointer)| {
                    pointer - grab + vec2(0.0, rect.height() * 0.5)
                });
            ctx.data_mut(|data| data.insert_temp(position_id, position));
            rect
        });
        (actions, rect)
    }

    /// Draws the window at `rect`, without resize handles: docked, where the
    /// sidebar's own edge resizes it, or gliding between modes.
    fn show_fixed(
        &self,
        ctx: &egui::Context,
        theme: &Theme,
        window_id: Id,
        rect: Rect,
        look: &Look,
        add: impl FnOnce(&mut Ui),
    ) -> HeaderActions {
        let mut actions = HeaderActions::default();
        let inset = f32::from(theme.padding) + theme.border_width;
        egui::Area::new(window_id.with("docked-area"))
            .order(egui::Order::Foreground)
            .fixed_pos(rect.min)
            .constrain(false)
            .fade_in(false)
            .show(ctx, |ui| {
                ui.set_clip_rect(rect);
                ui.multiply_opacity(look.opacity);
                theme.style_scrollbars(ui.style_mut());
                ui.scope_builder(UiBuilder::new().max_rect(rect), |ui| {
                    egui::Frame::new()
                        .fill(look.fill)
                        .stroke((theme.border_width, theme.border))
                        .corner_radius(theme.radius)
                        .inner_margin(theme.padding)
                        .show(ui, |ui| {
                            let size = rect.shrink(inset).size();
                            ui.set_min_size(size);
                            ui.set_max_size(size);
                            let cursor = if look.docked {
                                egui::CursorIcon::Grab
                            } else {
                                egui::CursorIcon::AllScroll
                            };
                            actions = self.header(ui, theme, cursor);
                            content(ui, window_id, add);
                        });
                });
            });
        // Blur the scene behind the part still translucent over it.
        let visible = rect.intersect(layout::scene(ctx));
        if look.fill.a() < 255 && visible.is_positive() {
            crate::render::window_blur::register(ctx, window_id, visible);
        }
        drawn(ctx, window_id, look.docked, false, rect);
        if look.gliding {
            actions = HeaderActions {
                close: actions.close,
                ..HeaderActions::default()
            };
        }
        actions
    }

    /// The title and close button. The rest of the header, including the
    /// frame padding around it, is a handle showing `cursor`: dragged to
    /// carry the window, or double-clicked to dock or float it.
    fn header(&self, ui: &mut Ui, theme: &Theme, cursor: egui::CursorIcon) -> HeaderActions {
        theme.style_scrollbars(ui.style_mut());
        ui.style_mut().spacing.item_spacing = egui::Vec2::splat(SPACING);
        let mut actions = HeaderActions::default();
        let mut close = Rect::NOTHING;
        let header = ui.horizontal(|ui| {
            ui.set_min_height(28.0);
            let title = title(ui, theme, self);
            let response = widgets::aligned_close_button(ui, theme, title);
            actions.close = response.clicked();
            close = response.rect;
        });
        // horizontal() already advances by item_spacing.y. Complete
        // the same padding below the header as the frame gives above it.
        let padding = f32::from(theme.padding);
        ui.add_space((padding - ui.spacing().item_spacing.y).max(0.0));
        let handle = Rect::from_min_max(
            header.response.rect.min - vec2(padding, padding),
            pos2(ui.max_rect().right() + padding, ui.cursor().top()),
        );
        let content_clip = ui.clip_rect();
        ui.set_clip_rect(content_clip.union(handle));
        // Cut out only the button, not its whole column.
        for (index, rect) in [
            Rect::from_min_max(handle.min, pos2(close.left(), handle.bottom())),
            Rect::from_min_max(pos2(close.right(), handle.top()), handle.max),
            Rect::from_min_max(pos2(close.left(), handle.top()), close.right_top()),
            Rect::from_min_max(close.left_bottom(), pos2(close.right(), handle.bottom())),
        ]
        .into_iter()
        .enumerate()
        {
            // Drag only, so egui's resize edges keep priority where they
            // overlap; double-clicks are read from the pointer directly.
            let response = ui
                .interact(rect, ui.id().with(("move-header", index)), Sense::drag())
                .on_hover_cursor(cursor);
            if response.dragged() {
                ui.ctx().set_cursor_icon(cursor);
                actions.grabbed = true;
            }
            actions.dock |= response.hovered()
                && ui.input(|input| {
                    input
                        .pointer
                        .button_double_clicked(egui::PointerButton::Primary)
                });
        }
        ui.set_clip_rect(content_clip);
        actions
    }
}

fn drawn_id(window: Id) -> Id {
    window.with("drawn")
}

fn drawn(ctx: &egui::Context, window: Id, docked: bool, carried: bool, rect: Rect) {
    let frame = ctx.cumulative_frame_nr();
    ctx.data_mut(|data| {
        data.insert_temp(
            drawn_id(window),
            Drawn {
                docked,
                carried,
                rect,
                frame,
            },
        )
    });
}

/// Starts, continues or ends the window's arrival. A window that drew in
/// the other mode last frame glides from there, unless it is being carried;
/// a docked window that did not draw last frame comes to the front.
fn arrival(ctx: &egui::Context, window: Id, docked: bool) -> Option<Arrival> {
    let id = window.with("arrival");
    let (now, frame) = (ctx.input(|input| input.time), ctx.cumulative_frame_nr());
    if dock::carried(ctx, window).is_some() {
        ctx.data_mut(|data| data.remove::<Arrival>(id));
        return None;
    }
    let last = ctx.data(|data| data.get_temp::<Drawn>(drawn_id(window)));
    let recent = last.filter(|last| last.frame + 1 >= frame);
    let under_way = ctx
        .data(|data| data.get_temp::<Arrival>(id))
        .filter(|arrival| {
            let (started, to) = match *arrival {
                Arrival::Glide { glide, docked } => (glide.started, docked),
                Arrival::Settle { glide } => (glide.started, false),
                Arrival::Front { started } => (started, true),
            };
            to == docked && now - started < f64::from(ANIMATION_SECONDS)
        });
    let arrival = match recent {
        Some(last) if last.docked != docked => Some(Arrival::Glide {
            glide: Glide {
                from: last.rect,
                started: now,
            },
            docked,
        }),
        Some(last) if !docked && last.carried && !layout::scene(ctx).contains_rect(last.rect) => {
            Some(Arrival::Settle {
                glide: Glide {
                    from: last.rect,
                    started: now,
                },
            })
        }
        None if docked && under_way.is_none() => Some(Arrival::Front { started: now }),
        _ => under_way,
    };
    ctx.data_mut(|data| match arrival {
        Some(arrival) => data.insert_temp(id, arrival),
        None => data.remove::<Arrival>(id),
    });
    if arrival.is_some() {
        ctx.request_repaint();
    }
    arrival
}

/// A docked window's place and look: at `target`, or still arriving there.
fn docked_look(
    ctx: &egui::Context,
    theme: &Theme,
    target: Rect,
    arrival: Option<Arrival>,
) -> (Rect, Look) {
    let now = ctx.input(|input| input.time);
    let mut look = Look {
        fill: theme.fill.to_opaque(),
        opacity: 1.0,
        docked: true,
        gliding: false,
    };
    let rect = match arrival {
        Some(Arrival::Glide { glide, .. }) => {
            let (rect, amount) = glide.sample(target, now);
            look.fill = theme.fill.lerp_to_gamma(look.fill, amount);
            look.gliding = true;
            rect
        }
        Some(Arrival::Front { started }) => {
            let from = Glide {
                from: target.translate(vec2(REVEAL, 0.0)),
                started,
            };
            let (rect, amount) = from.sample(target, now);
            look.opacity = amount;
            rect
        }
        Some(Arrival::Settle { .. }) | None => target,
    };
    (rect, look)
}

/// The caller's content, under one ID whether floating or docked, so its
/// scroll positions, folds and text survive moving between the two.
fn content(ui: &mut Ui, window_id: Id, add: impl FnOnce(&mut Ui)) {
    ui.add_space(SPACING);
    ui.scope_builder(UiBuilder::new().id(window_id.with("content")), add);
}

#[derive(Clone, Copy)]
struct ResizeDrag {
    bounds: Rect,
    edges: [bool; 4],
}

/// Clamp the dragged edge before egui applies resize sizing or viewport
/// translation. Its native path otherwise moves the whole rectangle first
/// and only afterwards enforces the minimum content size.
fn clamp_resize(ctx: &egui::Context, window_id: Id, minimum: egui::Vec2, screen: Rect) {
    let layer = egui::LayerId::new(egui::Order::Foreground, window_id);
    let handles_id = Id::new(layer).with("edge_drag");
    let dragging = RESIZE_HANDLES.map(|name| {
        ctx.read_response(handles_id.with(name))
            .is_some_and(|response| response.dragged())
    });
    let session_id = window_id.with("kit-resize-drag");
    if !dragging.iter().any(|drag| *drag) {
        ctx.data_mut(|data| data.remove::<ResizeDrag>(session_id));
        return;
    }
    let Some(delta) = ctx.input(|input| input.pointer.total_drag_delta()) else {
        return;
    };
    let Some(bounds) = ctx.memory(|memory| memory.area_rect(window_id)) else {
        return;
    };
    let session = ctx.data_mut(|data| {
        *data.get_temp_mut_or(
            session_id,
            ResizeDrag {
                bounds,
                edges: [
                    dragging[0] || dragging[4] || dragging[6],
                    dragging[1] || dragging[5] || dragging[7],
                    dragging[2] || dragging[4] || dragging[5],
                    dragging[3] || dragging[6] || dragging[7],
                ],
            },
        )
    });
    let target = bounded_resize(session.bounds, session.edges, delta, minimum, screen);
    // egui adds the total drag delta to this reference rectangle. Supply a
    // compensated reference so its native interaction lands on our bounded
    // target while retaining capture, cursors, rounding and resize feedback.
    let mut reference = target;
    if session.edges[0] {
        reference.min.x -= delta.x;
    }
    if session.edges[1] {
        reference.max.x -= delta.x;
    }
    if session.edges[2] {
        reference.min.y -= delta.y;
    }
    if session.edges[3] {
        reference.max.y -= delta.y;
    }
    ctx.data_mut(|data| {
        data.insert_temp(
            window_id.with("resize").with("window_rect_at_drag_start"),
            reference,
        )
    });
}

pub(super) fn bounded_resize(
    start: Rect,
    edges: [bool; 4],
    delta: egui::Vec2,
    minimum: egui::Vec2,
    screen: Rect,
) -> Rect {
    let mut rect = start;
    if edges[0] {
        rect.min.x = (start.left() + delta.x).clamp(
            screen.left().min(start.left()),
            start.right() - minimum.x.min(start.width()),
        );
    } else if edges[1] {
        rect.max.x = (start.right() + delta.x).clamp(
            start.left() + minimum.x.min(start.width()),
            screen.right().max(start.right()),
        );
    }
    if edges[2] {
        rect.min.y = (start.top() + delta.y).clamp(
            screen.top().min(start.top()),
            start.bottom() - minimum.y.min(start.height()),
        );
    } else if edges[3] {
        rect.max.y = (start.bottom() + delta.y).clamp(
            start.top() + minimum.y.min(start.height()),
            screen.bottom().max(start.bottom()),
        );
    }
    rect
}

/// Reuse egui's actual edge/corner responses, so feedback follows the same
/// hit testing and captured resize drag rather than competing with it.
fn resize_edges(
    ctx: &egui::Context,
    theme: &Theme,
    rect: Rect,
    layer: egui::LayerId,
    style: &egui::Style,
) {
    // These response IDs belong to the pinned egui 0.33 Window implementation.
    let id = Id::new(layer).with("edge_drag");
    let responses = RESIZE_HANDLES.map(|side| ctx.read_response(id.with(side)));
    let dragging = responses.iter().flatten().any(egui::Response::dragged);
    let painter = ctx.layer_painter(layer);
    let border = rect.shrink(theme.border_width * 0.5);
    let width = if dragging {
        style.visuals.widgets.active.bg_stroke.width
    } else {
        style.visuals.widgets.hovered.bg_stroke.width
    };
    for (side, corners) in [(0, [4, 6]), (1, [5, 7]), (2, [4, 5]), (3, [6, 7])] {
        let active = [side, corners[0], corners[1]].into_iter().any(|index| {
            responses[index].as_ref().is_some_and(|response| {
                if dragging {
                    response.dragged()
                } else {
                    response.hovered()
                }
            })
        });
        let segment = match side {
            0 => [border.left_top(), border.left_bottom()],
            1 => [border.right_top(), border.right_bottom()],
            2 => [border.left_top(), border.right_top()],
            _ => [border.left_bottom(), border.right_bottom()],
        };
        super::interact::pointer_edge(
            ctx,
            &painter,
            id.with(("kit-resize-edge", side)),
            segment,
            active,
            (width, theme.accent).into(),
        );
    }
}

/// A prompt and the title in text mode; else the eyebrow over a large title.
fn title(ui: &mut Ui, theme: &Theme, window: &Window) -> Rect {
    let font = if theme.text_mode {
        theme.font.id()
    } else {
        theme.title.id()
    };
    let colour = if theme.text_mode {
        theme.accent
    } else {
        theme.text
    };
    let title = ui
        .painter()
        .layout_no_wrap(window.title.into(), font, colour);
    let bounds = title.mesh_bounds;
    if theme.text_mode {
        ui.label(
            egui::RichText::new("night-sky:~$ ")
                .font(theme.font.id())
                .color(theme.dim),
        );
        let response = ui.label(title);
        bounds.translate(response.rect.min.to_vec2())
    } else {
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            if !window.eyebrow.is_empty() {
                widgets::eyebrow(ui, theme, window.eyebrow);
            }
            let response = ui.label(title);
            bounds.translate(response.rect.min.to_vec2())
        })
        .inner
    }
}

/// Clears the HUD icons at the top right, so a window never covers them.
fn below_icons(ctx: &egui::Context, top: f32) -> f32 {
    let icons = layout::icons(ctx);
    if icons.is_positive() {
        top.max(icons.bottom() + GAP)
    } else {
        top
    }
}
