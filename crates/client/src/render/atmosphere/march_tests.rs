//! f64 mirror of the shader's view march (single scattering, starlight from
//! the CPU transmittance table), checked against a fine even march across
//! the limb. It compares where the steps go, not the lighting model.
use super::{Air, AirSettings, Preset, generated, tables};
use bevy::math::DVec3;
use std::f64::consts::PI;

#[derive(Clone, Copy, Debug)]
enum Steps {
    /// The fine reference: evenly spaced.
    Even(usize),
    /// Steps growing with the square of their index at the ground, even from
    /// the top of the air: the march before focusing.
    Power(usize),
    /// Steps bunched around the densest visible point, as `focus` in
    /// `atmosphere.wgsl`.
    Focused(usize),
}

fn exp(v: DVec3) -> DVec3 {
    DVec3::new((-v.x).exp(), (-v.y).exp(), (-v.z).exp())
}

/// As `focus` in `atmosphere.wgsl`.
fn focus(air: &Air, d2: f64, start: f64, span: f64, seen: Option<f64>) -> (f64, f64) {
    let mut centre = (-start).clamp(0.0, span);
    if let Some(seen) = seen {
        centre = centre.min(seen);
    }
    let along = start + centre;
    let r = (d2 + along * along).sqrt();
    let rate = along.abs() / r;
    let mut scale = f64::from(air.rayleigh.w);
    if air.spherical {
        scale *= r * r;
    }
    if air.mie.max_element() > 0.0 {
        scale = scale.min(f64::from(air.shape.x));
    }
    let mut width = 2.0 * scale / (rate + (rate * rate + 2.0 * scale / r).sqrt());
    if let Some(seen) = seen {
        width = width.min(seen.max(1e-9));
    }
    (centre, width)
}

fn pixel(air: &Air, table: &[DVec3], eye: DVec3, dir: DVec3, sun: DVec3, steps: Steps) -> f64 {
    let top = f64::from(air.top);
    let closest = -eye.dot(dir);
    let nearest = eye + dir * closest;
    let d2 = nearest.length_squared();
    if d2 >= top * top {
        return 0.0;
    }
    let half = (top * top - d2).sqrt();
    let start = (-half).max(-closest);
    let mut end = half;
    let ground = -(1.0 - d2).max(0.0).sqrt();
    if d2 < 1.0 && ground > start {
        end = end.min(ground);
    }
    if end <= start {
        return 0.0;
    }
    // Where the mean depth reaches one and the least reaches ten, finely.
    let mut span = end - start;
    let fine = 50_000;
    let ds = span / f64::from(fine);
    let mut depth = DVec3::ZERO;
    let mut seen = None;
    for i in 0..fine {
        let at = start + (f64::from(i) + 0.5) * ds;
        depth += tables::extinction(air, (nearest + dir * at).length() - 1.0) * ds;
        if seen.is_none() && depth.element_sum() >= 3.0 {
            seen = Some((f64::from(i) + 1.0) * ds);
        }
        if depth.min_element() >= 10.0 {
            span = (f64::from(i) + 1.0) * ds;
            break;
        }
    }
    let place: Box<dyn Fn(f64) -> f64> = match steps {
        Steps::Even(_) => Box::new(move |x| span * x),
        Steps::Power(_) => {
            let height = ((eye.length() - 1.0) / (top - 1.0)).clamp(0.0, 1.0);
            let power = 2.0 - height;
            Box::new(move |x: f64| span * x.powf(power))
        }
        Steps::Focused(_) => {
            let (centre, width) = focus(air, d2, start, span, seen);
            let low = (-centre / width).asinh();
            let high = ((span - centre) / width).asinh();
            Box::new(move |x| centre + width * (low + (high - low) * x).sinh())
        }
    };
    let (Steps::Even(count) | Steps::Power(count) | Steps::Focused(count)) = steps;
    let mu = dir.dot(sun);
    let g = f64::from(air.shape.y);
    let rayleigh_phase = 3.0 / (16.0 * PI) * (1.0 + mu * mu);
    let mie_phase = 3.0 / (8.0 * PI) * (1.0 - g * g) * (1.0 + mu * mu)
        / ((2.0 + g * g) * (1.0 + g * g - 2.0 * g * mu).max(1e-6).powf(1.5));
    let (mut depth, mut light) = (DVec3::ZERO, DVec3::ZERO);
    for i in 0..count {
        let ds = place((i + 1) as f64 / count as f64) - place(i as f64 / count as f64);
        let p = nearest + dir * (start + place((i as f64 + 0.5) / count as f64));
        let r = p.length();
        let h = (r - 1.0).max(0.0);
        let x = tables::extinction(air, h) * ds;
        let within = DVec3::from_array(x.to_array().map(|x| {
            if x < 1e-4 {
                1.0 - 0.5 * x
            } else {
                (1.0 - (-x).exp()) / x
            }
        }));
        let b = p.dot(sun);
        if b >= 0.0 || r * r - b * b >= 1.0 {
            let scattering = air.rayleigh.truncate().as_dvec3()
                * tables::gas_density(air, h)
                * rayleigh_phase
                + air.mie.truncate().as_dvec3() * (-h / f64::from(air.shape.x)).exp() * mie_phase;
            let path = tables::sample(table, top, r, b / r);
            light += exp(path) * exp(depth) * within * ds * scattering;
        }
        depth += x;
    }
    light.y
}

/// A deep, puffy envelope like `g1724b6d.2.20326/5`'s: hydrogen at 788 K
/// under 0.497 m/s², continued below one bar.
pub(super) fn puffy() -> Air {
    use space_simulation::{
        system::atmosphere::{Composition, Gas, GasAtmosphere, ReferenceLevel},
        units::{EARTH_SOLAR, SUN_GM},
    };
    let radius = 8.7188e7;
    let gm = 0.497 * radius * radius;
    let gas = GasAtmosphere::new(
        Composition::new(&[(Gas::Hydrogen, 0.86), (Gas::Helium, 0.14)]).unwrap(),
        ReferenceLevel::Surface,
        1e5,
        788.0,
        None,
    )
    .unwrap();
    let planet = space_shared::Planet {
        mass_earth: gm / (EARTH_SOLAR * SUN_GM),
        radius_km: radius / 1e3,
        environment: Some(space_shared::PlanetEnvironment {
            irradiation_w_m2: 1e5,
            assumed_bond_albedo: 0.2,
            equilibrium_temperature_k: 788.0,
            age_gyr: 4.5,
            atmosphere: Some(gas.to_data(gm, radius).unwrap()),
        }),
        ..Default::default()
    };
    generated::descending(generated::atmosphere(&planet).unwrap().scaled(radius))
}

/// A row of pixels from the centre of the disc to past the top of the air,
/// with the planet flattened along `pole` and made round as the shader does:
/// the largest error against the fine reference, and the largest jump
/// between neighbours not in the reference, as fractions of the brightest.
fn row(air: &Air, eye: DVec3, pole: DVec3, flattening: f64, steps: Steps) -> (f64, f64) {
    let table = tables::depths(air);
    let sun = DVec3::new(-0.8, 0.3, 0.5).normalize();
    let rounded = |v: DVec3| v + pole * (v.dot(pole) * (1.0 / (1.0 - flattening) - 1.0));
    let edge = (f64::from(air.top) / eye.length()).min(1.0).asin();
    let mut reference = Vec::new();
    let mut march = Vec::new();
    for i in 0..120 {
        let theta = edge * (f64::from(i) + 0.5) / 120.0;
        let view = -eye.normalize() * theta.cos() + DVec3::X * theta.sin();
        let (eye, dir) = (rounded(eye), rounded(view).normalize());
        reference.push(pixel(
            air,
            &table,
            eye,
            dir,
            rounded(sun).normalize(),
            Steps::Even(8000),
        ));
        march.push(pixel(
            air,
            &table,
            eye,
            dir,
            rounded(sun).normalize(),
            steps,
        ));
    }
    let brightest = reference.iter().copied().fold(1e-30, f64::max);
    let error = reference
        .iter()
        .zip(&march)
        .map(|(r, m)| (m - r).abs())
        .fold(0.0, f64::max);
    let seam = (1..reference.len())
        .map(|i| ((march[i] - march[i - 1]) - (reference[i] - reference[i - 1])).abs())
        .fold(0.0, f64::max);
    (error / brightest, seam / brightest)
}

#[test]
fn focused_steps_meet_the_limb_of_puffy_air_without_a_seam() {
    let air = puffy();
    for (eye, pole, flattening) in [
        (DVec3::Z * 5.2, DVec3::Y, 0.0),
        (DVec3::Z * (air.top as f64 + 0.02), DVec3::Y, 0.0),
        // The row crosses the polar limb of a planet at the flattening cap.
        (DVec3::Z * 5.2, DVec3::X, 0.2),
    ] {
        let (_, power_seam) = row(&air, eye, pole, flattening, Steps::Power(16));
        let (error, seam) = row(&air, eye, pole, flattening, Steps::Focused(16));
        assert!(
            power_seam > 0.05,
            "the old march no longer seams: {power_seam}"
        );
        assert!(
            error < 0.015 && seam < 0.01,
            "eye {eye}: error {error}, seam {seam}"
        );
    }
}

#[test]
fn focused_steps_hold_for_presets_from_space_and_inside_the_air() {
    for preset in [Preset::Earth, Preset::Venus, Preset::Mars, Preset::Titan] {
        let air = AirSettings::preset(preset).atmosphere().scaled(6.371e6);
        let h = f64::from(air.rayleigh.w);
        for height in [3.2, f64::from(air.top) - 1.0 + 0.02, 0.3 * h, 3.0 * h] {
            let eye = DVec3::Z * (1.0 + height);
            let (error, seam) = row(&air, eye, DVec3::Y, 0.0, Steps::Focused(16));
            assert!(
                error < 0.02 && seam < 0.02,
                "{preset:?} at {height}: error {error}, seam {seam}"
            );
        }
    }
}
