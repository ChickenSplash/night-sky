//! f32 mirror of the shader's foreground march, checked against a fine f64
//! integration. These tests intentionally do not use transmittance-table depths.
use super::{Air, AirSettings, Preset, generated, tables};
use bevy::{math::DVec3, prelude::*};

struct Path {
    end: f32,
    depth: Vec3,
    opaque: bool,
    complete: bool,
}

fn march(air: &Air, impact: f32, start: f32, end: f32) -> Path {
    let mut scale = air.rayleigh.w;
    if air.mie.max_element() > 0.0 {
        scale = scale.min(air.shape.x);
    }
    if air.absorption.max_element() > 0.0 {
        scale = scale.min(air.shape.w);
    }
    let medium = |t: f32| {
        let h = impact.hypot(t) - 1.0;
        let above = h.max(0.0);
        let potential = if air.spherical {
            above / (1.0 + above)
        } else {
            above
        };
        let gas = (-potential / air.rayleigh.w).exp();
        let haze = (-h.max(0.0) / air.shape.x).exp();
        let layer = (1.0 - (h - air.shape.z).abs() / air.shape.w).max(0.0);
        (air.rayleigh.truncate() + air.gas_absorption) * gas
            + (air.mie.truncate() + air.mie.w) * haze
            + air.absorption * layer
    };
    let span = end - start;
    let mut t = 0.0;
    let mut depth = Vec3::ZERO;
    for _ in 0..1024 {
        if t >= span {
            break;
        }
        let along = start + t;
        let r = impact.hypot(along);
        let rate = (along.abs() / r).max((scale / r).sqrt());
        let ds = (span - t)
            .min(0.25 * scale / rate)
            .min(1.0 / medium(along).min_element().max(1e-30));
        let x = medium(along + 0.5 * ds) * ds;
        if (depth + x).min_element() >= 10.0 {
            let (mut low, mut high) = (0.0, ds);
            for _ in 0..12 {
                let mid = 0.5 * (low + high);
                if (depth + medium(along + 0.5 * mid) * mid).min_element() < 10.0 {
                    low = mid;
                } else {
                    high = mid;
                }
            }
            return Path {
                end: t + high,
                depth: depth + medium(along + 0.5 * high) * high,
                opaque: true,
                complete: true,
            };
        }
        depth += x;
        t += ds;
    }
    Path {
        end: span,
        depth,
        opaque: false,
        complete: t >= span,
    }
}

fn exact(air: &Air, impact: f32, start: f32, span: f32) -> DVec3 {
    let ds = f64::from(span) / 20_000.0;
    (0..20_000)
        .map(|i| {
            let t = f64::from(start) + (f64::from(i) + 0.5) * ds;
            tables::extinction(air, f64::from(impact).hypot(t) - 1.0) * ds
        })
        .sum()
}

fn selected_air() -> Air {
    let system = space_simulation::system::generated_system("g1e0024924b.2.113", [0.0; 3]).unwrap();
    let planet = system
        .planets
        .iter()
        .find(|p| p.id == "g1e0024924b.2.113#A/1")
        .unwrap();
    generated::atmosphere(planet)
        .unwrap()
        .scaled(planet.radius_km * 1e3)
}

fn check(air: &Air, impact: f32, start: f32, end: f32) -> Path {
    let path = march(air, impact, start, end);
    assert!(path.complete, "foreground budget exhausted: {air:?}");
    let measured = exact(air, impact, start, path.end);
    if path.opaque {
        assert!(
            (9.85..10.15).contains(&measured.min_element()),
            "impact {impact}, start {start}: actual {measured}, reported {}",
            path.depth
        );
    } else {
        let error = ((-measured).exp() - (-path.depth.as_dvec3()).exp()).abs();
        assert!(error.max_element() < 0.005, "transmission error {error}");
    }
    path
}

#[test]
fn extreme_generated_limb_stops_at_actual_opacity_without_a_ground_seam() {
    let air = selected_air();
    let mut stops = Vec::new();
    for impact in [
        0.99_f32, 0.999, 0.99999, 1.00001, 1.001, 1.005, 1.01, 1.02, 1.05, 1.1,
    ] {
        let half = (air.top * air.top - impact * impact).sqrt();
        let end = if impact < 1.0 {
            -(1.0 - impact * impact).sqrt()
        } else {
            half
        };
        let path = check(&air, impact, -half, end);
        assert!(path.opaque);
        if (impact - 1.0).abs() < 0.0001 {
            stops.push(path.end);
        }
    }
    assert!((stops[0] - stops[1]).abs() < 0.001);
}

#[test]
fn foreground_handles_presets_giant_descent_and_inside_cameras() {
    let mut atmospheres: Vec<_> = [Preset::Earth, Preset::Venus, Preset::Mars, Preset::Titan]
        .into_iter()
        .map(|p| AirSettings::preset(p).atmosphere().scaled(6.371e6))
        .collect();
    atmospheres.push(generated::descending(selected_air()));
    for air in atmospheres {
        for impact in [0.0_f32, 0.99, 0.99999, 1.00001, 1.001] {
            if impact >= air.top {
                continue;
            }
            let half = (air.top * air.top - impact * impact).sqrt();
            let end = if impact < 1.0 {
                -(1.0 - impact * impact).sqrt()
            } else {
                half
            };
            check(&air, impact, -half, end);
            check(&air, impact, -half + 0.9 * (end + half), end);
            // Upward rays from just above the lower boundary.
            let start = if impact < 1.0 { -end + 1e-5 } else { 0.0 };
            if start < half {
                check(&air, impact, start, half);
            }
        }
    }
}

#[test]
fn logarithmic_depth_texture_preserves_extreme_depths_and_transmission() {
    let air = selected_air();
    let depths = tables::depths(&air);
    assert!(depths.iter().any(|d| d.max_element() > 65504.0));
    let image = tables::depth_image(&depths);
    let data = image.data.as_ref().unwrap();
    for (original, pixel) in depths.iter().zip(data.chunks_exact(8)) {
        for c in 0..3 {
            let stored = half::f16::from_le_bytes([pixel[c * 2], pixel[c * 2 + 1]]);
            let decoded = stored.to_f64().exp2() - 1.0;
            assert!(decoded.is_finite());
            assert!((decoded - original[c]).abs() < 0.015 * original[c] + 1e-6);
            assert!(((-decoded).exp() - (-original[c]).exp()).abs() < 0.001);
        }
    }
}
