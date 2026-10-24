//! Camera flights along van Wijk and Nuij's optimal zoom-and-pan path
//! ("Smooth and efficient zooming and panning", 2003). Long trips zoom out first,
//! so the destination stays in view from the interstellar scale down to a planet.
use bevy::math::{DVec3, Quat};

const RHO: f64 = std::f64::consts::SQRT_2;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pose {
    /// Metres relative to the current star, in galactic axes.
    pub target: DVec3,
    pub distance: f64,
    /// Camera orientation in galactic axes, including the system's tilt.
    pub rotation: Quat,
    /// 0 for orbital positions, 1 for the task table layout.
    pub layout: f64,
}

pub struct Flight {
    from: Pose,
    to: Pose,
    r0: f64,
    length: f64,
    travel: f64,
    duration: f64,
    elapsed: f64,
}

fn ease(t: f64) -> f64 {
    // Quintic smoothstep is C2-continuous and has a smaller midpoint velocity
    // than the old piecewise cubic. Long interstellar jumps no longer get a
    // sharp-looking kick right where the camera zoom is widest.
    let t = t.clamp(0.0, 1.0);
    t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
}

/// Duration of every flight from one star to another, whether it is a page in
/// the same list or a different list, so both have the same rhythm.
pub const STAR_JUMP_SECONDS: f64 = 0.65;

impl Flight {
    /// `None` when the camera would not visibly move.
    pub fn new(from: Pose, to: Pose) -> Option<Self> {
        let (w0, w1) = (from.distance, to.distance);
        let travel = (to.target - from.target).length();
        let still = travel < w0.min(w1) * 1e-6
            && (w1 / w0).ln().abs() < 1e-6
            && from.rotation.angle_between(to.rotation) < 1e-4
            && (to.layout - from.layout).abs() < 1e-6;
        if still {
            return None;
        }
        let (r0, length) = if travel < w0.min(w1) * 1e-6 {
            (0.0, (w1 / w0).ln().abs() / RHO)
        } else {
            let rho2 = RHO * RHO;
            let b0 =
                (w1 * w1 - w0 * w0 + rho2 * rho2 * travel * travel) / (2.0 * w0 * rho2 * travel);
            let b1 =
                (w1 * w1 - w0 * w0 - rho2 * rho2 * travel * travel) / (2.0 * w1 * rho2 * travel);
            // ln(-b + sqrt(b² + 1)), without cancellation at interstellar ratios.
            let (r0, r1) = (-b0.asinh(), -b1.asinh());
            (r0, (r1 - r0) / RHO)
        };
        Some(Self {
            from,
            to,
            r0,
            length,
            travel,
            // Snappy: a quarter second for small moves, under 0.75 s between stars.
            duration: (0.25 + 0.03 * length).min(0.75),
            elapsed: 0.0,
        })
    }

    pub fn advance(&mut self, seconds: f64) {
        self.elapsed += seconds;
    }

    pub fn progress(&self) -> f64 {
        (self.elapsed / self.duration).clamp(0.0, 1.0)
    }

    pub fn duration_seconds(&self) -> f64 {
        self.duration
    }

    pub fn set_duration(&mut self, seconds: f64) {
        self.duration = seconds;
    }

    pub fn finished(&self) -> bool {
        self.elapsed >= self.duration
    }

    /// Moves the stored start into a new star's frame.
    pub fn shift(&mut self, offset: DVec3) {
        self.from.target += offset;
        self.to.target += offset;
    }

    /// Travelled fraction and view width after path length `s`.
    fn path(&self, s: f64) -> (f64, f64) {
        let w0 = self.from.distance;
        if self.travel < w0.min(self.to.distance) * 1e-6 {
            let e = if self.length > 0.0 {
                s / self.length
            } else {
                1.0
            };
            return (e, (w0.ln() + (self.to.distance / w0).ln() * e).exp());
        }
        let r0 = self.r0;
        let u = w0 / (RHO * RHO) * (r0.cosh() * (RHO * s + r0).tanh() - r0.sinh());
        (u / self.travel, w0 * r0.cosh() / (RHO * s + r0).cosh())
    }

    /// Interpolates towards the live destination, so a moving planet is met exactly.
    pub fn sample(&self, live: Pose) -> Pose {
        let e = ease(self.progress());
        let (fraction, width) = self.path(e * self.length);
        // Normalised by the path's own end, so rounding cannot leave a final jump.
        let (end_fraction, end_width) = self.path(self.length);
        let fraction = fraction / end_fraction;
        let width = width * (self.to.distance / end_width).powf(e);
        Pose {
            target: self.from.target + (live.target - self.from.target) * fraction,
            // Follows live framing changes, such as a window resize mid-flight.
            distance: width * (live.distance / self.to.distance).powf(e),
            // Shortest turn, including rolling between two systems' orbital planes.
            rotation: self.from.rotation.slerp(live.rotation, e as f32),
            layout: self.from.layout + (live.layout - self.from.layout) * e,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use space_simulation::{AU_METRES, LY_METRES};

    fn pose(target: DVec3, distance: f64) -> Pose {
        Pose {
            target,
            distance,
            rotation: Quat::from_rotation_y(0.3) * Quat::from_rotation_x(-0.85),
            layout: 0.0,
        }
    }

    fn fly(from: Pose, to: Pose, steps: usize) -> Vec<Pose> {
        let mut flight = Flight::new(from, to).unwrap();
        let dt = flight.duration / steps as f64;
        (0..=steps)
            .map(|i| {
                if i > 0 {
                    flight.advance(dt);
                }
                flight.sample(to)
            })
            .collect()
    }

    #[test]
    fn interstellar_flight_is_quick_continuous_and_keeps_the_destination_in_view() {
        let from = pose(DVec3::ZERO, 20.0 * AU_METRES);
        let to = pose(DVec3::new(9.0, 1.0, -3.0) * LY_METRES, 18.0 * AU_METRES);
        assert!(Flight::new(from, to).unwrap().duration <= 0.75);
        let path = fly(from, to, 400);
        let (first, last) = (path[0], path[path.len() - 1]);
        assert!((first.target - from.target).length() < 1.0e3);
        assert!((first.distance / from.distance - 1.0).abs() < 1e-6);
        assert!((last.target - to.target).length() < 1.0e3);
        assert!((last.distance / to.distance - 1.0).abs() < 1e-6);
        // Zooms out to about the trip length, so both stars fit the 50° view.
        let peak = path.iter().map(|p| p.distance).fold(0.0, f64::max);
        assert!(peak > 0.9 * (to.target - from.target).length());
        for pair in path.windows(2) {
            assert!((pair[1].distance / pair[0].distance).ln().abs() < 0.2);
            // Each step moves the target by less than the current view distance.
            assert!((pair[1].target - pair[0].target).length() < pair[0].distance);
        }
    }

    #[test]
    fn page_and_list_jumps_share_one_rhythm() {
        let from = pose(DVec3::ZERO, 20.0 * AU_METRES);
        let profile = |light_years: f64| {
            let to = pose(DVec3::X * light_years * LY_METRES, from.distance);
            let mut flight = Flight::new(from, to).unwrap();
            flight.set_duration(STAR_JUMP_SECONDS);
            let dt = flight.duration / 100.0;
            let widths: Vec<f64> = (0..=100)
                .map(|i| {
                    if i > 0 {
                        flight.advance(dt);
                    }
                    flight.sample(to).distance.ln()
                })
                .collect();
            let peak = widths.iter().copied().fold(f64::MIN, f64::max);
            // Zoom height as a fraction of this jump's own peak.
            let base = from.distance.ln();
            widths
                .iter()
                .map(|w| (w - base) / (peak - base))
                .collect::<Vec<_>>()
        };
        let page = profile(0.16);
        let list = profile(4.3);
        for (p, l) in page.iter().zip(&list) {
            assert!((p - l).abs() < 0.05, "page {p:.3} vs list {l:.3}");
        }
        assert!((page[50] - 1.0).abs() < 1e-3 && (list[50] - 1.0).abs() < 1e-3);
    }

    #[test]
    fn zoom_only_rotation_and_no_change() {
        let from = pose(DVec3::ZERO, 20.0 * AU_METRES);
        let path = fly(from, pose(DVec3::ZERO, 60_000_000.0), 50);
        assert!(path.windows(2).all(|p| p[1].distance < p[0].distance));
        // A near full turn one way is a short turn the other way.
        let turned = Pose {
            rotation: Quat::from_rotation_z(std::f32::consts::TAU - 0.2) * from.rotation,
            ..from
        };
        let path = fly(from, turned, 50);
        assert!(
            path.iter()
                .all(|p| p.rotation.angle_between(from.rotation) <= 0.2 + 1e-4)
        );
        assert!(path[50].rotation.angle_between(turned.rotation) < 1e-4);
        assert!(Flight::new(from, from).is_none());
    }

    #[test]
    fn page_jumps_use_the_same_zoom_pan_and_rotation_as_star_jumps() {
        let from = pose(-DVec3::X * 0.16 * LY_METRES, 20.0 * AU_METRES);
        let to = Pose {
            target: DVec3::ZERO,
            rotation: Quat::from_rotation_z(2.0) * from.rotation,
            ..from
        };
        let path = fly(from, to, 200);
        assert!((path[0].target - from.target).length() < 1.0);
        assert!((path[200].target - to.target).length() < 1.0);
        assert!(path[100].distance > 10.0 * from.distance);
        assert!(path[100].rotation.angle_between(from.rotation) > 0.1);
        assert!(path[100].rotation.angle_between(to.rotation) > 0.1);
        assert!(path[200].rotation.angle_between(to.rotation) < 1e-4);
        // A second click departs from the pose actually on screen, not the first star.
        let next = pose(DVec3::Y * 0.16 * LY_METRES, from.distance);
        let retargeted = fly(path[100], next, 200);
        assert!((retargeted[0].target - path[100].target).length() < 1.0);
    }
}
