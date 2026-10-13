//! The interface's widget kit: one set of controls drawn from a `Theme`, so
//! the debug console (`Theme::TERMINAL`, through `ui::terminal`) and
//! player-facing windows such as the object browser (`Theme::GAME`) share
//! their behaviour: one hover and held-press light, header-only window
//! dragging, and scroll areas that never steal a control's press. Scene text
//! uses `halo`.
mod facts;
pub mod halo;
mod interact;
mod motion;
mod number;
mod range;
mod theme;
mod widgets;
mod window;

pub use facts::{facts, facts_with_details};
pub use interact::SCROLL_SOURCE;
pub use motion::animated_rect;
pub use number::number_field;
#[cfg(test)]
pub use range::parse_range_end;
pub use range::{NumericRange, RangeField, RangeInput, RangeScale, RangeSlider, range_field};
pub use theme::{ACCENT, SPACING, Theme, plain_style};
pub use widgets::{
    button, check, choice, close_button, eyebrow, field, heading, input, item, link, list_row,
    note, pill, primary_button, progress, rule, section, tab, toggle,
};
pub use window::Window;

#[cfg(test)]
mod tests;
