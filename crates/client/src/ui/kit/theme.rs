//! The values a window's widgets are drawn with. Widgets take a `&Theme`
//! rather than naming colours, so one widget serves the debug console and
//! the player-facing windows.
use bevy_egui::egui::{self, Color32, FontFamily, FontId};

/// Shared horizontal and vertical gap between window controls.
pub const SPACING: f32 = 10.0;

/// A font as plain values, so themes can be constants.
#[derive(Clone, Copy, Debug)]
pub struct Font {
    pub size: f32,
    pub mono: bool,
}

impl Font {
    pub fn id(self) -> FontId {
        let family = if self.mono {
            FontFamily::Monospace
        } else {
            FontFamily::Proportional
        };
        FontId::new(self.size, family)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Theme {
    /// Text-mode controls (`[ button ]`, `[x]`, `# heading`) rather than
    /// framed ones.
    pub text_mode: bool,
    pub text: Color32,
    pub dim: Color32,
    /// Headings, hover light and focus.
    pub accent: Color32,
    /// A switch that is on.
    pub on: Color32,
    pub fill: Color32,
    pub border: Color32,
    pub border_width: f32,
    /// Window corners.
    pub radius: u8,
    /// Control and hover-light corners.
    pub control_radius: u8,
    /// Space inside the window's border.
    pub padding: i8,
    /// A full-width row's height.
    pub row: f32,
    /// Where text starts inside a row, clear of the hover light's edge.
    pub inset: f32,
    pub font: Font,
    pub small: Font,
    /// Figures and IDs.
    pub mono: Font,
    pub title: Font,
}

const fn font(size: f32, mono: bool) -> Font {
    Font { size, mono }
}

impl Theme {
    /// Shared feedback colour for close, remove and reset actions.
    pub const DESTRUCTIVE: Color32 = Color32::from_rgb(240, 100, 100);
    /// Faint rows, idle scrollbars and progress remainders share this strength.
    pub const SUBTLE_ACCENT_OPACITY: f32 = 0.12;
    /// The debug console: a translucent black window after the desktop's
    /// Hyprland windows, with the player-window border and text-mode controls.
    pub const TERMINAL: Self = Self {
        text_mode: true,
        text: Color32::from_rgb(214, 228, 244),
        dim: Color32::from_rgb(110, 125, 140),
        accent: Color32::from_rgb(0x33, 0xcc, 0xff),
        on: Color32::from_rgb(0x00, 0xff, 0x99),
        fill: Color32::from_black_alpha(205),
        border: Self::GAME.border,
        border_width: Self::GAME.border_width,
        radius: 0,
        control_radius: 4,
        padding: 12,
        row: 24.0,
        inset: 8.0,
        font: font(13.0, true),
        small: font(11.0, true),
        mono: font(13.0, true),
        title: font(13.0, true),
    };

    /// Player-facing windows, such as the object browser: deep blue glass,
    /// a hairline border, framed controls and a teal accent.
    pub const GAME: Self = Self {
        text_mode: false,
        text: Color32::from_rgb(0xe6, 0xed, 0xf7),
        dim: Color32::from_rgb(0x94, 0xa6, 0xba),
        accent: Color32::from_rgb(0x77, 0xe2, 0xdf),
        on: Color32::from_rgb(0x77, 0xe2, 0xdf),
        // #0f1a2a at 93%.
        fill: Color32::from_rgba_premultiplied(14, 24, 39, 238),
        // #c3e7ff at 15%.
        border: Color32::from_rgba_premultiplied(29, 34, 38, 38),
        border_width: 1.0,
        radius: 0,
        control_radius: 6,
        padding: 12,
        row: 28.0,
        inset: 10.0,
        font: font(14.0, false),
        small: font(12.0, false),
        mono: font(12.0, true),
        title: font(21.0, false),
    };

    /// Faint lines: rules and dividers.
    pub fn line(&self) -> Color32 {
        self.dim.gamma_multiply(0.4)
    }

    /// Shared background for text and labelled number inputs.
    pub fn input_fill(&self) -> Color32 {
        Color32::from_black_alpha(70)
    }

    pub fn input_padding(&self) -> egui::Margin {
        egui::Margin::symmetric(10, 8)
    }

    pub fn field_height(&self) -> f32 {
        self.row + 12.0
    }

    /// Tooltips use the owning window's colours and border, but never glass.
    pub fn tooltip_frame(&self) -> egui::Frame {
        egui::Frame::new()
            .fill(self.fill.to_opaque())
            .stroke((self.border_width, self.border))
            .corner_radius(self.radius)
            .inner_margin(self.padding)
    }

    /// Full-width square accent handles, subtle until hovered or dragged,
    /// with an invisible track and reserved gutter.
    pub fn style_scrollbars(&self, style: &mut egui::Style) {
        let scroll = &mut style.spacing.scroll;
        scroll.floating = true;
        scroll.floating_width = scroll.bar_width;
        scroll.floating_allocated_width =
            scroll.bar_width + scroll.bar_inner_margin + scroll.bar_outer_margin;
        scroll.foreground_color = true;
        scroll.dormant_background_opacity = 0.0;
        scroll.active_background_opacity = 0.0;
        scroll.interact_background_opacity = 0.0;
        scroll.dormant_handle_opacity = Self::SUBTLE_ACCENT_OPACITY;
        scroll.active_handle_opacity = Self::SUBTLE_ACCENT_OPACITY;
        scroll.interact_handle_opacity = 1.0;
        for visuals in [
            &mut style.visuals.widgets.inactive,
            &mut style.visuals.widgets.hovered,
            &mut style.visuals.widgets.active,
            &mut style.visuals.widgets.noninteractive,
        ] {
            visuals.fg_stroke.color = self.accent;
            visuals.corner_radius = egui::CornerRadius::ZERO;
        }
    }
}

/// Scene selection accent; plain egui styling remains for native/debug helpers.
pub const ACCENT: Color32 = Color32::from_rgb(99, 214, 240);
const PANEL: Color32 = Color32::from_rgb(7, 15, 31);

/// The style of plain egui widgets, set once.
pub fn plain_style(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();
    style.visuals = egui::Visuals::dark();
    style.visuals.window_corner_radius = egui::CornerRadius::ZERO;
    style.visuals.menu_corner_radius = egui::CornerRadius::ZERO;
    style.visuals.panel_fill = PANEL;
    // Fallback for egui-generated tooltips and any plain helper popups.
    style.visuals.window_fill = Theme::GAME.fill.to_opaque();
    style.visuals.window_stroke = egui::Stroke::new(Theme::GAME.border_width, Theme::GAME.border);
    style.visuals.override_text_color = Some(Color32::from_rgb(214, 228, 244));
    style.visuals.selection.bg_fill = Color32::from_rgb(24, 65, 85);
    style.visuals.widgets.inactive.bg_fill = Color32::from_rgb(15, 31, 49);
    style.spacing.item_spacing = egui::Vec2::splat(SPACING);
    style.spacing.button_padding = egui::vec2(12.0, 8.0);
    Theme::GAME.style_scrollbars(&mut style);
    ctx.set_style(style);
}
