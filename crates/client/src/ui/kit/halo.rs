//! Free-floating scene text with a soft dark halo, so it reads over bright
//! stars and the galaxy band without a box: the breadcrumb, inspector, HUD
//! controls, icons and canvas labels.
use bevy_egui::egui;

pub const LINK: egui::Color32 = egui::Color32::from_rgb(150, 174, 202);
pub const HOVER: egui::Color32 = egui::Color32::from_rgb(99, 214, 240);
pub const CURRENT: egui::Color32 = egui::Color32::from_rgb(238, 244, 252);
pub const SIZE: f32 = 13.0;
/// Shared inset for text HUDs around the 3D viewport.
pub const HUD_PADDING: f32 = 20.0;
pub const HUD_LINE_GAP: f32 = 8.0;
/// The soft halo behind the text, so it reads over bright stars and the
/// galaxy band without a box: a dark ring at the letter edges (`HALO_EDGE`
/// opacity, 1 px out), fading through a fainter one (`HALO_FADE`, 2 px out).
/// Overlapping copies make it densest right at the letters.
const HALO_EDGE: u8 = 130;
const HALO_FADE: u8 = 45;

/// Text with a soft dark halo, coloured once its response (hover) is known.
pub fn halo_text(
    ui: &mut egui::Ui,
    text: &str,
    sense: egui::Sense,
    colour: impl FnOnce(&egui::Response) -> egui::Color32,
) -> egui::Response {
    halo_glyph(ui, text, SIZE, sense, colour)
}

/// `halo_text` at another size, such as the HUD icons.
pub fn halo_glyph(
    ui: &mut egui::Ui,
    text: &str,
    size: f32,
    sense: egui::Sense,
    colour: impl FnOnce(&egui::Response) -> egui::Color32,
) -> egui::Response {
    let galley = ui.painter().layout_no_wrap(
        text.to_owned(),
        egui::FontId::proportional(size),
        egui::Color32::PLACEHOLDER,
    );
    paint_halo(ui, galley, sense, colour)
}

pub fn halo_wrapped_text(
    ui: &mut egui::Ui,
    text: &str,
    width: f32,
    colour: egui::Color32,
) -> egui::Response {
    let galley = ui.painter().layout(
        text.to_owned(),
        egui::FontId::proportional(SIZE),
        egui::Color32::PLACEHOLDER,
        width.max(1.0),
    );
    paint_halo(ui, galley, egui::Sense::hover(), |_| colour)
}

fn paint_halo(
    ui: &mut egui::Ui,
    galley: std::sync::Arc<egui::Galley>,
    sense: egui::Sense,
    colour: impl FnOnce(&egui::Response) -> egui::Color32,
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(galley.size(), sense);
    halo_galley(ui.painter(), rect.min, galley, colour(&response));
    response
}

/// Paints a galley laid out in `Color32::PLACEHOLDER` over its halo, for
/// scene text placed by hand rather than by a `Ui`.
pub fn halo_galley(
    painter: &egui::Painter,
    at: egui::Pos2,
    galley: std::sync::Arc<egui::Galley>,
    colour: egui::Color32,
) {
    for (radius, alpha, steps) in [(2.0, HALO_FADE, 16), (1.0, HALO_EDGE, 8)] {
        for step in 0..steps {
            let angle = std::f32::consts::TAU * step as f32 / steps as f32;
            let offset = egui::vec2(angle.cos(), angle.sin()) * radius;
            painter.galley(
                at + offset,
                galley.clone(),
                egui::Color32::from_black_alpha(alpha),
            );
        }
    }
    painter.galley(at, galley, colour);
}
