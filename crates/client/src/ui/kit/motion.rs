//! Retargetable layout motion using the same timing as control highlights.
use super::interact::{ANIMATION_SECONDS, ease_out};
use bevy_egui::egui::{Id, Rect, Ui};

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
