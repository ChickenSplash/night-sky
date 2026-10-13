//! A floating window: movable by its whole padded header, resizable, opening
//! at the top right below the HUD icons.
use super::Theme;
use bevy_egui::egui::{self, Color32, Rect, Sense, Ui, epaint, pos2, vec2};

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

pub struct Window<'a> {
    pub title: &'a str,
    /// Small text above a framed title; text-mode windows have a prompt.
    pub eyebrow: &'a str,
    /// Default width and height: a share of the screen, then clamped.
    pub width: (f32, f32, f32),
    pub height: (f32, f32, f32),
    pub min_size: egui::Vec2,
}

impl<'a> Window<'a> {
    pub fn new(title: &'a str) -> Self {
        Self {
            title,
            eyebrow: "",
            width: (0.4, 420.0, 760.0),
            height: (0.6, 320.0, 680.0),
            min_size: vec2(320.0, 200.0),
        }
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
        let screen = ctx.content_rect();
        let size = vec2(
            (screen.width() * self.width.0).clamp(self.width.1, self.width.2),
            (screen.height() * self.height.0).clamp(self.height.1, self.height.2),
        );
        let padding = f32::from(theme.padding);
        let window_id = egui::Id::new(("window", self.title));
        clamp_resize(
            ctx,
            window_id,
            self.min_size + egui::Vec2::splat(2.0 * (padding + theme.border_width)),
            screen,
        );
        let mut close = false;
        let position_id = egui::Id::new(("window-center", self.title));
        let position = ctx
            .data(|data| data.get_temp::<egui::Pos2>(position_id))
            .unwrap_or_else(|| {
                pos2(
                    screen.right() - GAP - size.x * 0.5 - padding - theme.border_width,
                    below_icons(ctx, screen.top() + GAP)
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
            .anchor(egui::Align2::CENTER_CENTER, position - screen.center())
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
            theme.style_scrollbars(ui.style_mut());
            ui.style_mut().spacing.item_spacing = egui::Vec2::splat(super::SPACING);
            let mut close_rect = Rect::NOTHING;
            let header = ui.horizontal(|ui| {
                ui.set_min_height(28.0);
                let title = header(ui, theme, &self);
                let response = super::widgets::aligned_close_button(ui, theme, title);
                close = response.clicked();
                close_rect = response.rect;
            });
            // horizontal() already advances by item_spacing.y. Complete
            // the same padding below the header as the frame gives above it.
            ui.add_space((padding - ui.spacing().item_spacing.y).max(0.0));
            // The whole header, including its surrounding frame padding, is
            // a handle. Cut out only the close button, not its whole column.
            let handle = Rect::from_min_max(
                header.response.rect.min - vec2(padding, padding),
                pos2(ui.max_rect().right() + padding, ui.cursor().top()),
            );
            let content_clip = ui.clip_rect();
            ui.set_clip_rect(content_clip.union(handle));
            for (index, rect) in [
                Rect::from_min_max(handle.min, pos2(close_rect.left(), handle.bottom())),
                Rect::from_min_max(pos2(close_rect.right(), handle.top()), handle.max),
                Rect::from_min_max(
                    pos2(close_rect.left(), handle.top()),
                    close_rect.right_top(),
                ),
                Rect::from_min_max(
                    close_rect.left_bottom(),
                    pos2(close_rect.right(), handle.bottom()),
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
            ui.add_space(super::SPACING);
            add(ui);
        });
        ctx.set_style(style.clone());
        if let Some(shown) = shown {
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
        }
        if close {
            *open = false;
        }
    }
}

#[derive(Clone, Copy)]
struct ResizeDrag {
    bounds: Rect,
    edges: [bool; 4],
}

/// Clamp the dragged edge before egui applies resize sizing or viewport
/// translation. Its native path otherwise moves the whole rectangle first
/// and only afterwards enforces the minimum content size.
fn clamp_resize(ctx: &egui::Context, window_id: egui::Id, minimum: egui::Vec2, screen: Rect) {
    let layer = egui::LayerId::new(egui::Order::Foreground, window_id);
    let handles_id = egui::Id::new(layer).with("edge_drag");
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
    use super::interact::{ANIMATION_SECONDS, ease_out};
    // These response IDs belong to the pinned egui 0.33 Window implementation.
    let id = egui::Id::new(layer).with("edge_drag");
    let responses = RESIZE_HANDLES.map(|side| ctx.read_response(id.with(side)));
    let dragging = responses.iter().flatten().any(egui::Response::dragged);
    let frame = ctx.cumulative_frame_nr();
    let painter = ctx.layer_painter(layer);
    let border = rect.shrink(theme.border_width * 0.5);
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
        let animation_id = id.with(("kit-resize-edge", side));
        let origin_id = animation_id.with("origin");
        let vertical = side < 2;
        let pointer = ctx.input(|input| input.pointer.hover_pos());
        let origin = ctx.data_mut(|data| {
            let (was_active, mut origin, previous) = data
                .get_temp::<(bool, f32, u64)>(origin_id)
                .unwrap_or((false, 0.5, frame));
            if active
                && (!was_active || frame > previous + 1)
                && let Some(pointer) = pointer
            {
                origin = if vertical {
                    (pointer.y - border.top()) / border.height().max(1.0)
                } else {
                    (pointer.x - border.left()) / border.width().max(1.0)
                }
                .clamp(0.0, 1.0);
            }
            data.insert_temp(origin_id, (active, origin, frame));
            origin
        });
        // Initialise idle edges before their first hover, matching inputs.
        let amount = ctx.animate_bool_with_time_and_easing(
            animation_id,
            active,
            ANIMATION_SECONDS,
            ease_out,
        );
        if amount <= 0.0 {
            continue;
        }
        let start = origin * (1.0 - amount);
        let end = origin + (1.0 - origin) * amount;
        let width = if dragging {
            style.visuals.widgets.active.bg_stroke.width
        } else {
            style.visuals.widgets.hovered.bg_stroke.width
        };
        let points = if vertical {
            let x = if side == 0 {
                border.left()
            } else {
                border.right()
            };
            [
                pos2(x, border.top() + border.height() * start),
                pos2(x, border.top() + border.height() * end),
            ]
        } else {
            let y = if side == 2 {
                border.top()
            } else {
                border.bottom()
            };
            [
                pos2(border.left() + border.width() * start, y),
                pos2(border.left() + border.width() * end, y),
            ]
        };
        painter.line_segment(points, (width, theme.accent));
    }
}

/// A prompt and the title in text mode; else the eyebrow over a large title.
fn header(ui: &mut Ui, theme: &Theme, window: &Window) -> Rect {
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
                super::widgets::eyebrow(ui, theme, window.eyebrow);
            }
            let response = ui.label(title);
            bounds.translate(response.rect.min.to_vec2())
        })
        .inner
    }
}

/// Clears the HUD icons at the top right, so a window never covers them.
fn below_icons(ctx: &egui::Context, top: f32) -> f32 {
    ctx.data(|data| data.get_temp::<(Rect, bool)>(egui::Id::new("icons-pointer-capture")))
        .map_or(top, |(icons, _)| top.max(icons.bottom() + GAP))
}
