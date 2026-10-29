//! Retargetable layout motion using the same timing as control highlights.
use super::interact::{ANIMATION_SECONDS, ease_out};
use bevy_egui::egui::{self, Id, Rect, Ui};

#[derive(Clone, Copy)]
struct Motion {
    from: Rect,
    to: Rect,
    started: f64,
    frame: u64,
}

impl Motion {
    fn sample(self, now: f64) -> Rect {
        let progress = ((now - self.started) / f64::from(ANIMATION_SECONDS)).clamp(0.0, 1.0);
        let amount = ease_out(progress as f32);
        Rect::from_min_max(
            self.from.min.lerp(self.to.min, amount),
            self.from.max.lerp(self.to.max, amount),
        )
    }
}

/// Move and resize a local layout rectangle, continuing from its displayed
/// position if the target changes mid-animation. Reopening starts fresh.
pub fn animated_rect(ui: &Ui, id: Id, initial: Rect, target: Rect) -> Rect {
    let now = ui.input(|input| input.time);
    let frame = ui.ctx().cumulative_frame_nr();
    let rect = ui.data_mut(|data| {
        let mut motion = data
            .get_temp::<Motion>(id)
            .filter(|motion| frame <= motion.frame + 1)
            .unwrap_or(Motion {
                from: initial,
                to: initial,
                started: now,
                frame,
            });
        let current = motion.sample(now);
        if motion.to != target {
            motion.from = current;
            motion.to = target;
            motion.started = now;
        }
        motion.frame = frame;
        let rect = motion.sample(now);
        data.insert_temp(id, motion);
        rect
    });
    if rect != target {
        ui.ctx().request_repaint();
    }
    rect
}

fn progress(started: f64, now: f64) -> f32 {
    ((now - started) / f64::from(ANIMATION_SECONDS)).clamp(0.0, 1.0) as f32
}

/// One length easing towards a target that may change, such as the
/// sidebar's width. Retargeting continues from the shown value.
#[derive(Clone, Copy, Default)]
pub(super) struct Eased {
    from: f32,
    to: f32,
    started: f64,
}

impl Eased {
    pub fn value(self, now: f64) -> f32 {
        egui::lerp(self.from..=self.to, ease_out(progress(self.started, now)))
    }

    pub fn moving(self, now: f64) -> bool {
        self.value(now) != self.to
    }

    pub fn retarget(&mut self, to: f32, now: f64) {
        if to != self.to {
            *self = Self {
                from: self.value(now),
                to,
                started: now,
            };
        }
    }

    /// Jumps straight to `to`, such as while the user drags it.
    pub fn snap(&mut self, to: f32) {
        *self = Self {
            from: to,
            to,
            started: 0.0,
        };
    }
}

/// A rectangle easing from where it was towards a target that may itself
/// move while it travels, such as a window gliding into a sliding sidebar.
#[derive(Clone, Copy)]
pub(super) struct Glide {
    pub from: Rect,
    pub started: f64,
}

impl Glide {
    /// The rectangle shown now and how far along the glide is, 0 to 1.
    pub fn sample(self, to: Rect, now: f64) -> (Rect, f32) {
        let amount = ease_out(progress(self.started, now));
        let rect = Rect::from_min_max(
            self.from.min.lerp(to.min, amount),
            self.from.max.lerp(to.max, amount),
        );
        (rect, amount)
    }
}
