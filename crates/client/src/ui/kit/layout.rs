//! Where the 3D scene and the HUD icons sit this frame, so panels drawn later
//! in the frame (including the debug console's separate system) agree on it.
use bevy_egui::egui::{self, Rect};

#[derive(Clone, Copy)]
struct Layout {
    scene: Rect,
    icons: Rect,
}

fn id() -> egui::Id {
    egui::Id::new("kit-layout")
}

fn load(ctx: &egui::Context) -> Option<Layout> {
    ctx.data(|data| data.get_temp::<Layout>(id()))
}

/// The part of the screen the 3D view uses: everything beside the dock.
/// Contexts that never laid out a dock, such as tests, use the whole screen.
pub fn scene(ctx: &egui::Context) -> Rect {
    load(ctx).map_or_else(|| ctx.content_rect(), |layout| layout.scene)
}

/// The HUD icons' bounds, which windows open below. Nothing if not drawn.
pub(super) fn icons(ctx: &egui::Context) -> Rect {
    load(ctx).map_or(Rect::NOTHING, |layout| layout.icons)
}

pub(super) fn set_scene(ctx: &egui::Context, scene: Rect) {
    let icons = icons(ctx);
    ctx.data_mut(|data| data.insert_temp(id(), Layout { scene, icons }));
}

pub fn set_icons(ctx: &egui::Context, icons: Rect) {
    let scene = scene(ctx);
    ctx.data_mut(|data| data.insert_temp(id(), Layout { scene, icons }));
}
