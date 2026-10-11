//! Manual exposure and a shared highlight shoulder. Apply the shoulder after
//! fragment lighting, before half-float storage can discard bright detail.
//! Faint light remains linear. Bloom/additive overlaps combine mapped light,
//! an intentional approximation, not a fully scene-linear HDR compositor.
use crate::Navigation;
use bevy::{
    asset::{load_internal_asset, uuid_handle},
    prelude::*,
};

pub const STOPS: std::ops::RangeInclusive<f64> = -64.0..=16.0;
const SHADER: Handle<Shader> = uuid_handle!("37845990-7da2-4e71-85f7-61115fe77e33");

pub struct ExposurePlugin;
impl Plugin for ExposurePlugin {
    fn build(&self, app: &mut App) {
        load_internal_asset!(app, SHADER, "exposure.wgsl", Shader::from_wgsl);
    }
}

/// Zero stops retains the fixed dark-space calibration, never a view meter.
pub fn gain(stops: f64) -> f64 {
    stops.exp2() / crate::sky::reference_radiance()
}

pub fn facts(nav: &Navigation) -> Vec<(&'static str, String, Option<&'static str>)> {
    vec![
        (
            "exposure",
            format!("{:+.2} stops", nav.exposure),
            Some(
                "Manual sensitivity relative to a fixed dark-space reference. +1 stop doubles incoming scene light, -1 halves it. Set this for the galaxy/background first, then use compression to fit bright surfaces. No automatic adjustment on moving, selecting or entering a system.",
            ),
        ),
        (
            "compression",
            format!("{:.0}%", nav.compression * 100.0),
            Some(
                "A shared highlight-only shoulder, not a shadow lift. Faint light stays unchanged; stronger compression starts the shoulder lower and fits a wider highlight range below white. Applied after surface lighting, before HDR storage, so bright planets retain their shading instead of being capped on the CPU. Bloom and additive overlaps combine already-mapped light, an approximation. Extreme highlights still have reduced contrast; compression cannot preserve every real brightness difference.",
            ),
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// CPU mirror of the shared shoulder, including final display clipping at zero.
    fn curve(light: f64, amount: f64) -> f64 {
        if amount <= 0.0 {
            return light.clamp(0.0, 1.0);
        }
        let knee = 1.0 - 0.82 * amount;
        if light <= knee {
            return light.max(0.0);
        }
        let room = 1.0 - knee;
        let range = (5.0 * amount).exp2();
        knee + room * -(-((light - knee) * range / room).ln_1p() / range).exp_m1()
    }

    #[test]
    fn stops_double_or_halve_all_light() {
        assert_eq!(gain(1.0) / gain(0.0), 2.0);
        assert_eq!(gain(-1.0) / gain(0.0), 0.5);
        for stops in [*STOPS.start(), 0.0, *STOPS.end()] {
            assert!(gain(stops).is_finite());
        }
    }

    #[test]
    fn shoulder_preserves_faint_light_and_orders_extreme_highlights() {
        for amount in [0.0, 0.25, 0.5, 1.0] {
            for light in [0.0, 1e-8, 0.001, 0.01, 0.1, 0.18] {
                assert!((curve(light, amount) - light).abs() < 1e-12);
            }
            let mut previous = -1.0;
            for i in -80..=100 {
                let value = curve((f64::from(i) / 2.0).exp2(), amount);
                assert!((0.0..=1.0).contains(&value));
                assert!(value >= previous);
                previous = value;
            }
        }
        // The old 20,000 safety cap made these identical before display mapping.
        assert!(curve(2e5, 1.0) < curve(2e6, 1.0));
        assert!(curve(2e6, 1.0) < curve(2e7, 1.0));
        assert!(curve(1e20, 1.0) < 1.0);
        assert!(curve(2e6, 1.0) - curve(2e5, 1.0) > 0.02);
        assert!(curve(100.0, 1.0) < curve(100.0, 0.5));
    }

    #[test]
    fn shoulder_joins_linear_light_without_a_slope_jump() {
        for amount in [0.01, 0.25, 0.5, 1.0] {
            let knee = 1.0 - 0.82 * amount;
            let delta = 1e-8;
            assert!(((curve(knee + delta, amount) - knee) / delta - 1.0).abs() < 1e-4);
        }
    }

    #[test]
    fn stronger_compression_never_lifts_shadows_or_highlights() {
        for light in [0.0, 0.01, 0.18, 0.5, 1.0, 100.0, 2e7] {
            let mut previous = curve(light, 0.0);
            for step in 1..=100 {
                let mapped = curve(light, f64::from(step) / 100.0);
                assert!(mapped <= previous + 1e-12);
                previous = mapped;
            }
        }
    }

    #[test]
    fn manual_controls_reject_non_finite_values_and_clamp() {
        let mut nav = Navigation::default();
        nav.set_exposure(-100.0);
        assert_eq!(nav.exposure, -64.0);
        nav.set_exposure(100.0);
        assert_eq!(nav.exposure, 16.0);
        nav.set_exposure(f64::NAN);
        assert_eq!(nav.exposure, 16.0);
        nav.set_compression(-1.0);
        assert_eq!(nav.compression, 0.0);
        nav.set_compression(2.0);
        assert_eq!(nav.compression, 1.0);
        nav.set_compression(f64::INFINITY);
        assert_eq!(nav.compression, 1.0);
        let rows = facts(&nav);
        assert_eq!(rows.len(), 2);
        assert!(rows.iter().all(|row| row.2.is_some()));
    }
}
