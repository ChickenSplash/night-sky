//! A kit window: floating (movable by its whole padded header, resizable,
//! opening at the top right below the HUD icons) or docked into the sidebar.
//! Header buttons switch between the two; each window remembers its choice.
use super::{SPACING, Theme, dock, layout, widgets};
use bevy_egui::egui::{self, Color32, Id, Rect, Sense, Ui, UiBuilder, epaint, pos2, vec2};

/// Space between a window and the screen's edges when it first opens.
const GAP: f32 = 12.0;

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

fn docked_id(window: Id) -> Id {
    window.with("docked")
}

pub(super) fn is_docked(ctx: &egui::Context, window: Id) -> bool {
    ctx.data(|data| data.get_temp::<bool>(docked_id(window)))
        .unwrap_or(false)
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

#[derive(Default)]
struct HeaderActions {
    close: bool,
    dock: bool,
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
        let docked = is_docked(ctx, id);
        let actions = if docked {
            let width = self.width.of(ctx.content_rect().width()) + frame;
            let actions = match dock::check_in(ctx, id, self.title, min_width, width) {
                Some(rect) => self.show_docked(ctx, theme, id, rect, add),
                None => HeaderActions::default(),
            };
            if actions.dock {
                dock::leave(ctx, id);
            }
            actions
        } else {
            let (actions, rect) = self.show_floating(ctx, theme, id, add);
            if actions.dock
                && let Some(rect) = rect
            {
                dock::enter(ctx, id, self.title, min_width, rect.width());
            }
            actions
        };
        if actions.dock {
            ctx.data_mut(|data| data.insert_temp(docked_id(id), !docked));
        }
        if actions.close {
            dock::leave(ctx, id);
            *open = false;
        }
    }

    fn show_floating(
        &self,
        ctx: &egui::Context,
        theme: &Theme,
        window_id: Id,
        add: impl FnOnce(&mut Ui),
    ) -> (HeaderActions, Option<Rect>) {
        let scene = layout::scene(ctx);
        let size = vec2(self.width.of(scene.width()), self.height.of(scene.height()));
        let padding = f32::from(theme.padding);
        clamp_resize(
            ctx,
            window_id,
            self.min_size + egui::Vec2::splat(2.0 * (padding + theme.border_width)),
            scene,
        );
        let mut actions = HeaderActions::default();
        let position_id = Id::new(("window-center", self.key));
        let position = ctx
            .data(|data| data.get_temp::<egui::Pos2>(position_id))
            .unwrap_or_else(|| {
                pos2(
                    scene.right() - GAP - size.x * 0.5 - padding - theme.border_width,
                    below_icons(ctx, scene.top() + GAP)
                        + size.y * 0.5
                        + padding
                        + theme.border_width,
                )
            });
        let mut movement = egui::Vec2::ZERO;
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
        let window = egui::Window::new(self.title)
            .id(window_id)
            // egui 0.33 checks the anchor (not just the pivot) to decide
            // which edges an immovable window may resize from. Track the
            // centre anchor ourselves to allow every edge and header moves.
            .pivot(egui::Align2::CENTER_CENTER)
            .constrain_to(scene)
            .anchor(egui::Align2::CENTER_CENTER, position - scene.center())
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
            let (header, buttons, clicked) = self.header(ui, theme, false);
            actions = clicked;
            // The whole header, including its surrounding frame padding, is
            // a handle. Cut out only the buttons, not their whole column.
            let handle = Rect::from_min_max(
                header.min - vec2(padding, padding),
                pos2(ui.max_rect().right() + padding, ui.cursor().top()),
            );
            let content_clip = ui.clip_rect();
            ui.set_clip_rect(content_clip.union(handle));
            for (index, rect) in [
                Rect::from_min_max(handle.min, pos2(buttons.left(), handle.bottom())),
                Rect::from_min_max(pos2(buttons.right(), handle.top()), handle.max),
                Rect::from_min_max(pos2(buttons.left(), handle.top()), buttons.right_top()),
                Rect::from_min_max(
                    buttons.left_bottom(),
                    pos2(buttons.right(), handle.bottom()),
                ),
            ]
            .into_iter()
            .enumerate()
            {
                let response = ui
                    .interact(rect, ui.id().with(("move-header", index)), Sense::drag())
                    .on_hover_cursor(egui::CursorIcon::AllScroll);
                if response.dragged() {
                    ctx.set_cursor_icon(egui::CursorIcon::AllScroll);
                    movement = response.drag_delta();
                }
            }
            ui.set_clip_rect(content_clip);
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
            ctx.data_mut(|data| {
                data.insert_temp(position_id, shown.response.rect.center() + movement)
            });
            shown.response.rect
        });
        (actions, rect)
    }

    /// Fills the sidebar's content `rect`, opaque and without move or resize
    /// handles: the sidebar's own edge resizes it.
    fn show_docked(
        &self,
        ctx: &egui::Context,
        theme: &Theme,
        window_id: Id,
        rect: Rect,
        add: impl FnOnce(&mut Ui),
    ) -> HeaderActions {
        let mut actions = HeaderActions::default();
        let inset = f32::from(theme.padding) + theme.border_width;
        egui::Area::new(window_id.with("docked-area"))
            .order(egui::Order::Foreground)
            .fixed_pos(rect.min)
            .constrain(false)
            .show(ctx, |ui| {
                ui.set_clip_rect(rect);
                theme.style_scrollbars(ui.style_mut());
                ui.scope_builder(UiBuilder::new().max_rect(rect), |ui| {
                    egui::Frame::new()
                        .fill(theme.fill.to_opaque())
                        .stroke((theme.border_width, theme.border))
                        .corner_radius(theme.radius)
                        .inner_margin(theme.padding)
                        .show(ui, |ui| {
                            let size = rect.shrink(inset).size();
                            ui.set_min_size(size);
                            ui.set_max_size(size);
                            actions = self.header(ui, theme, true).2;
                            content(ui, window_id, add);
                        });
                });
            });
        actions
    }

    /// The title, dock button and close button. Returns the header block,
    /// the buttons' bounds and which were clicked.
    fn header(&self, ui: &mut Ui, theme: &Theme, docked: bool) -> (Rect, Rect, HeaderActions) {
        theme.style_scrollbars(ui.style_mut());
        ui.style_mut().spacing.item_spacing = egui::Vec2::splat(SPACING);
        let mut actions = HeaderActions::default();
        let mut buttons = Rect::NOTHING;
        let header = ui.horizontal(|ui| {
            ui.set_min_height(28.0);
            let title = title(ui, theme, self);
            let close = widgets::aligned_close_button(ui, theme, title);
            let dock = ui.place(widgets::header_slot(ui, title, 1), |ui: &mut Ui| {
                widgets::dock_button(ui, theme, docked)
            });
            actions.close = close.clicked();
            actions.dock = dock.clicked();
            buttons = close.rect.union(dock.rect);
        });
        // horizontal() already advances by item_spacing.y. Complete
        // the same padding below the header as the frame gives above it.
        ui.add_space((f32::from(theme.padding) - ui.spacing().item_spacing.y).max(0.0));
        (header.response.rect, buttons, actions)
    }
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
