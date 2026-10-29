//! The interface's widget kit: one set of controls drawn from a `Theme`, so
//! the debug console (`Theme::TERMINAL`, through `ui::terminal`) and
//! player-facing windows such as the object browser (`Theme::GAME`) share
//! their behaviour: one hover and held-press light, header-only window
//! dragging, and scroll areas that never steal a control's press. Scene text
//! uses `halo`.
mod dropdown;
mod expandable;
mod facts;
pub mod halo;
mod interact;
mod motion;
mod number;
mod pill;
mod range;
mod scroll;
mod slider;
mod theme;
mod tooltip;
mod widgets;
mod window;

pub use dropdown::dropdown;
pub use expandable::expandable;
pub use facts::{facts, facts_with_details};
pub use interact::SCROLL_SOURCE;
pub use motion::animated_rect;
pub use number::number_field;
pub use pill::Pills;
#[cfg(test)]
pub use range::parse_range_end;
pub use range::{NumericRange, RangeField, RangeInput, RangeScale, RangeSlider, range_field};
pub use scroll::ScrollArea;
pub use slider::Slider;
pub use theme::{ACCENT, SPACING, Theme, plain_style};
pub use tooltip::TooltipExt;
pub use widgets::{
    aligned_close_button, button, button_size, check, choice, destructive_button, eyebrow,
    eyebrow_text, field, heading, input, item, link, list_row, note, primary_button, progress,
    rule, section, section_with_default, tab, toggle,
};
pub use window::Window;

#[cfg(test)]
mod tests;
