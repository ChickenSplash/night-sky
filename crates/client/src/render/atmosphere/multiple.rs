//! The atmosphere's multiple scattering table (`atmosphere.wgsl`): the mean
//! radiance of light that has scattered at least once, by height and the
//! star's angle, per unit of starlight. Light past its first scattering is
//! taken as arriving evenly from every direction (Hillaire 2020), and the
//! haze's forward peak as no scattering at all (the transport
//! approximation: scattering `(1 - g)` of the haze's). Rather than Hillaire's
//! geometric sum of one more order, every order is found at once by solving
//! the grid's linear system, so light diffusing down through thick cloud
//! (Venus, Titan) is kept. The ground is part of the system: it reflects
//! the starlight and sky light reaching it, evenly, by its albedo.
use super::Air;
use super::tables::{depths, extinction, image, sample};
use bevy::{math::DVec3, prelude::*};
use std::f64::consts::PI;

/// Texels across (the star's cosine from the zenith, -1 to 1) and down
/// (height, ground to top), with the ground's light in one more row below
/// the sky's top. Matched in `atmosphere.wgsl`.
pub const SIZE: usize = 32;
/// Directions gathered at each texel: bands of equal solid angle from the
/// zenith, and azimuths over half the sphere, mirrored across the star's
/// plane.
const POLAR: usize = 24;
const AZIMUTH: usize = 8;
/// Steps along each direction, bunched up near the texel.
const STEPS: usize = 64;

pub(super) struct Multiple {
    /// Mean radiance of light scattered at least once, against starlight
    /// of 1, row by row from the ground up.
    pub sky: Vec<DVec3>,
    /// The ground's radiance, by the star's cosine.
    pub ground: Vec<DVec3>,
    /// The net upward flow of that light, per texel of `sky`.
    pub up: Vec<DVec3>,
}

fn rayleigh_phase(mu: f64) -> f64 {
    3.0 / (16.0 * PI) * (1.0 + mu * mu)
}

#[cfg(test)]
fn mie_phase(mu: f64, g: f64) -> f64 {
    let g2 = g * g;
    let denominator = (2.0 + g2) * (1.0 + g2 - 2.0 * g * mu).max(1e-6).powf(1.5);
    3.0 / (8.0 * PI) * (1.0 - g2) * (1.0 + mu * mu) / denominator
}

/// Of the light entering a step of extinction `x` per unit length and
/// length `ds`, the share of its length it travels: `(1 - e^-x ds) / x`.
fn within(x: DVec3, ds: f64) -> DVec3 {
    (x * ds)
        .to_array()
        .map(|a| {
            if a < 1e-6 {
                ds * (1.0 - 0.5 * a)
            } else {
                (1.0 - (-a).exp()) * ds / a
            }
        })
        .into()
}

/// Starlight reaching `p` from `sun`, against 1 above the air.
pub(super) fn starlight(depths: &[DVec3], top: f64, p: DVec3, sun: DVec3) -> DVec3 {
    let b = p.dot(sun);
    let r2 = p.length_squared();
    if b < 0.0 && r2 - b * b < 1.0 {
        return DVec3::ZERO;
    }
    let r = r2.sqrt();
    if r >= top {
        return DVec3::ONE;
    }
    (-sample(depths, top, r.max(1.0), b / r)).exp()
}

/// The grid's coordinate, 0 to `SIZE - 1`, of a star's cosine and of a
/// radius.
fn across(mu: f64) -> f64 {
    (0.5 + 0.5 * mu).clamp(0.0, 1.0) * (SIZE - 1) as f64
}

/// Rows are spaced evenly in `1 - e^(-h / 2H)`, `H` the air's tallest
/// scale height: bunched up in the thick low air where light diffuses down
/// through cloud, with enough left high up for where a thick cloud's top
/// reflects starlight.
fn down(air: &Air, r: f64) -> f64 {
    let scale = 2.0 * f64::from(air.rayleigh.w.max(air.shape.x));
    let whole = 1.0 - (-(f64::from(air.top) - 1.0) / scale).exp();
    let x = (1.0 - (-(r - 1.0).max(0.0) / scale).exp()) / whole;
    x.clamp(0.0, 1.0) * (SIZE - 1) as f64
}

/// The radius of row `j`, as `down` inverts.
fn row_radius(air: &Air, j: usize) -> f64 {
    let scale = 2.0 * f64::from(air.rayleigh.w.max(air.shape.x));
    let whole = 1.0 - (-(f64::from(air.top) - 1.0) / scale).exp();
    let x = j as f64 / (SIZE - 1) as f64;
    (1.0 - scale * (1.0 - x * whole).ln()).min(f64::from(air.top))
}

/// The three rows nearest row coordinate `v` and their weights, a parabola
/// through them: unlike a straight line between two rows, it keeps how far
/// light gets before scattering again when that is much less than a row,
/// so light doesn't diffuse through thick cloud faster than it should.
fn rows_around(v: f64) -> [(usize, f64); 3] {
    let j = (v.round() as usize).clamp(1, SIZE - 2);
    let z = v - j as f64;
    [
        (j - 1, 0.5 * z * (z - 1.0)),
        (j, 1.0 - z * z),
        (j + 1, 0.5 * z * (z + 1.0)),
    ]
}

/// The two grid points either side of `x`, and the second's weight.
fn split(x: f64) -> (usize, f64) {
    let i = (x as usize).min(SIZE - 2);
    (i, x - i as f64)
}

/// The images for `air`, built in the background: the `thinned` air's
/// transmittance, the sky's light with the ground's in one more row, and
/// its upward flow.
pub(super) fn images(air: Air) -> [Image; 3] {
    let started = std::time::Instant::now();
    let thin = depths(&thinned(&air));
    let table = multiple(&air, &thin);
    info!(
        "Atmosphere multiple scattering table built in {:.1} s",
        started.elapsed().as_secs_f64()
    );
    let mut sky = table.sky;
    sky.extend(table.ground);
    [
        super::tables::depth_image(&thin),
        image([SIZE, SIZE + 1], &sky),
        image([SIZE, SIZE], &table.up),
    ]
}

/// The air with the haze's forward peak taken as no scattering at all.
/// Its transmittance table dims the starlight wherever the table's light is
/// used: starlight scattered slightly forwards still counts as starlight.
pub(super) fn thinned(air: &Air) -> Air {
    Air {
        mie: (air.mie.truncate() * (1.0 - air.shape.y)).extend(air.mie.w),
        ..*air
    }
}

/// The table for `air`, `depths` the transmittance table of its `thinned`
/// air.
pub(super) fn multiple(air: &Air, depths: &[DVec3]) -> Multiple {
    let top = f64::from(air.top);
    let g = f64::from(air.shape.y);
    let albedo = air.ground.as_dvec3();
    let sky = SIZE * SIZE;
    let n = sky + SIZE;
    // Per channel: the light each unknown gives every other, and the light
    // each gets straight from the stars' first scattering; the same again
    // for the sky's net flow of light upwards.
    let mut gives = [vec![0.0; n * n], vec![0.0; n * n], vec![0.0; n * n]];
    let mut first = vec![DVec3::ZERO; n];
    let mut ups = [vec![0.0; n * n], vec![0.0; n * n], vec![0.0; n * n]];
    let mut first_up = vec![DVec3::ZERO; n];
    let add = |gives: &mut [Vec<f64>; 3], row: usize, column: usize, light: DVec3| {
        for (c, matrix) in gives.iter_mut().enumerate() {
            matrix[row * n + column] += light[c];
        }
    };
    let weight = 1.0 / (POLAR * AZIMUTH) as f64;
    let directions: Vec<DVec3> = (0..POLAR)
        .flat_map(|i| (0..AZIMUTH).map(move |k| (i, k)))
        .map(|(i, k)| {
            let cos = 1.0 - 2.0 * (i as f64 + 0.5) / POLAR as f64;
            let phi = PI * (k as f64 + 0.5) / AZIMUTH as f64;
            let sin = (1.0 - cos * cos).sqrt();
            DVec3::new(sin * phi.cos(), sin * phi.sin(), cos)
        })
        .collect();
    let rows = (0..n).map(|row| {
        let (i, j, ground) = if row < sky {
            (row % SIZE, row / SIZE, false)
        } else {
            (row - sky, 0, true)
        };
        let mu = -1.0 + 2.0 * i as f64 / (SIZE - 1) as f64;
        let r = row_radius(air, j);
        let sun = DVec3::new((1.0 - mu * mu).max(0.0).sqrt(), 0.0, mu);
        (row, DVec3::Z * r.max(1.0 + 1e-9), sun, ground)
    });
    for (row, x, sun, ground) in rows {
        if ground {
            // The ground's own starlight.
            first[row] += albedo / PI * sun.z.max(0.0) * starlight(depths, top, x, sun);
        }
        for &d in &directions {
            // In the sky the mean over directions; on the ground the light
            // falling on it, reflected.
            let share = if ground {
                if d.z <= 0.0 {
                    continue;
                }
                albedo * (4.0 * weight * d.z)
            } else {
                DVec3::splat(weight)
            };
            // Light from `d` travels along `-d`: its share of the flow up.
            let upward = if ground { 0.0 } else { -4.0 * PI * d.z };
            let b = x.dot(d);
            let disc = b * b - x.length_squared() + 1.0;
            let hits = b < 0.0 && disc >= 0.0;
            let length = if hits {
                -b - disc.sqrt()
            } else {
                -b + (b * b - x.length_squared() + top * top).max(0.0).sqrt()
            };
            let phase = rayleigh_phase(d.dot(sun));
            let (mut through, mut last) = (DVec3::ONE, 0.0);
            // Steps bunched up near the texel, and never more than 0.3 deep:
            // light deep in cloud scatters again within a fraction of a step,
            // and placing it at a long step's middle would carry it too far.
            while last < length * (1.0 - 1e-12) && through.max_element() > 1e-7 {
                let here = extinction(air, (x + d * last).length() - 1.0).max_element();
                let bunched =
                    length / STEPS as f64 * (2.0 * (last / length).sqrt()).max(1.0 / STEPS as f64);
                let ds = (0.3 / here).min(bunched).min(length - last);
                let p = x + d * (last + 0.5 * ds);
                last += ds;
                let height = p.length() - 1.0;
                let rayleigh = air.rayleigh.truncate().as_dvec3()
                    * super::tables::gas_density(air, height.max(0.0));
                let mie = air.mie.truncate().as_dvec3()
                    * (-height.max(0.0) / f64::from(air.shape.x)).exp();
                let reduced = extinction(air, height) - mie * g;
                let into = share * through * within(reduced, ds);
                let once = into
                    * (rayleigh * phase + mie * (1.0 - g) / (4.0 * PI))
                    * starlight(depths, top, p, sun);
                first[row] += once;
                first_up[row] += once * upward;
                let into = into * (rayleigh + mie * (1.0 - g));
                let r = p.length();
                let (u, fu) = split(across(p.dot(sun) / r));
                for (v, fv) in rows_around(down(air, r)) {
                    for (cell, part) in
                        [(v * SIZE + u, (1.0 - fu) * fv), (v * SIZE + u + 1, fu * fv)]
                    {
                        add(&mut gives, row, cell, into * part);
                        add(&mut ups, row, cell, into * part * upward);
                    }
                }
                through *= (-reduced * ds).exp();
            }
            if hits && !ground {
                let at = x + d * length;
                let (u, fu) = split(across(at.dot(sun) / at.length()));
                for (cell, part) in [(sky + u, 1.0 - fu), (sky + u + 1, fu)] {
                    add(&mut gives, row, cell, share * through * part);
                    add(&mut ups, row, cell, share * through * part * upward);
                }
            }
        }
    }
    let mut light = vec![DVec3::ZERO; n];
    for (c, matrix) in gives.iter_mut().enumerate() {
        let solved = solve(matrix, first.iter().map(|f| f[c]).collect(), n);
        for (l, s) in light.iter_mut().zip(solved) {
            l[c] = s;
        }
    }
    let up = (0..sky)
        .map(|row| {
            first_up[row]
                + DVec3::from_array(std::array::from_fn(|c| {
                    ups[c][row * n..(row + 1) * n]
                        .iter()
                        .zip(&light)
                        .map(|(k, l)| k * l[c])
                        .sum()
                }))
        })
        .collect();
    Multiple {
        ground: light.split_off(sky),
        sky: light,
        up,
    }
}

/// Solves `x = b + K x` in place of `K`, by elimination: `I - K` is
/// diagonally dominant, as no unknown gives away more light than it has.
fn solve(k: &mut [f64], mut b: Vec<f64>, n: usize) -> Vec<f64> {
    for v in k.iter_mut() {
        *v = -*v;
    }
    for i in 0..n {
        k[i * n + i] += 1.0;
    }
    for i in 0..n {
        let pivot = k[i * n + i];
        let (above, below) = k.split_at_mut((i + 1) * n);
        let row = &above[i * n..];
        for (j, other) in below.chunks_exact_mut(n).enumerate() {
            let factor = other[i] / pivot;
            if factor == 0.0 {
                continue;
            }
            for (o, r) in other[i..].iter_mut().zip(&row[i..]) {
                *o -= factor * r;
            }
            b[i + 1 + j] -= factor * b[i];
        }
    }
    for i in (0..n).rev() {
        let sum: f64 = (i + 1..n).map(|j| k[i * n + j] * b[j]).sum();
        b[i] = (b[i] - sum) / k[i * n + i];
    }
    b
}

impl Multiple {
    /// The sky's value at radius `r` under a star at cosine `mu`, bilinear,
    /// as the GPU samples it.
    #[cfg(test)]
    pub fn at(&self, air: &Air, r: f64, mu: f64) -> DVec3 {
        let (u, fu) = split(across(mu));
        let (v, fv) = split(down(air, r));
        let at = |u: usize, v: usize| self.sky[v * SIZE + u];
        (at(u, v) * (1.0 - fu) + at(u + 1, v) * fu) * (1.0 - fv)
            + (at(u, v + 1) * (1.0 - fu) + at(u + 1, v + 1) * fu) * fv
    }

    /// The net upward flow at radius `r` under a star at cosine `mu`.
    #[cfg(test)]
    pub fn up_at(&self, air: &Air, r: f64, mu: f64) -> DVec3 {
        let (u, fu) = split(across(mu));
        let (v, fv) = split(down(air, r));
        let at = |u: usize, v: usize| self.up[v * SIZE + u];
        (at(u, v) * (1.0 - fu) + at(u + 1, v) * fu) * (1.0 - fv)
            + (at(u, v + 1) * (1.0 - fu) + at(u + 1, v + 1) * fu) * fv
    }

    /// The ground's radiance under a star at cosine `mu`.
    #[cfg(test)]
    pub fn ground_at(&self, mu: f64) -> DVec3 {
        let (u, fu) = split(across(mu));
        self.ground[u] * (1.0 - fu) + self.ground[u + 1] * fu
    }
}

/// What the shader draws with the table: radiance seen from `p` along `d`,
/// first scattering marched finely, every later one read from the table,
/// and the ground lit by the table's light where the ray meets it. Its
/// starlight is dimmed by `depths`, the `thinned` air's table, as the
/// table's is.
#[cfg(test)]
pub(super) fn seen(
    air: &Air,
    depths: &[DVec3],
    table: &Multiple,
    p: DVec3,
    d: DVec3,
    sun: DVec3,
) -> DVec3 {
    const FINE: usize = 4000;
    let top = f64::from(air.top);
    let g = f64::from(air.shape.y);
    let mut p = p;
    if p.length() >= top {
        let b = p.dot(d);
        let disc = b * b - p.length_squared() + top * top;
        if b >= 0.0 || disc < 0.0 {
            return DVec3::ZERO;
        }
        p += d * (-b - disc.sqrt());
        p *= 1.0 - 1e-12;
    }
    let b = p.dot(d);
    let disc = b * b - p.length_squared() + 1.0;
    let hits = b < 0.0 && disc >= 0.0;
    let length = if hits {
        -b - disc.sqrt()
    } else {
        -b + (b * b - p.length_squared() + top * top).max(0.0).sqrt()
    };
    let mu = d.dot(sun);
    let (mut seen_light, mut through, mut last) = (DVec3::ZERO, DVec3::ONE, 0.0);
    for step in 1..=FINE {
        let t = length * (step as f64 / FINE as f64).powi(2);
        let ds = t - last;
        let x = p + d * (last + 0.5 * ds);
        last = t;
        let height = x.length() - 1.0;
        let rayleigh =
            air.rayleigh.truncate().as_dvec3() * super::tables::gas_density(air, height.max(0.0));
        let mie = air.mie.truncate().as_dvec3() * (-height.max(0.0) / f64::from(air.shape.x)).exp();
        let total = extinction(air, height);
        let once = (rayleigh * rayleigh_phase(mu) + mie * mie_phase(mu, g))
            * starlight(depths, top, x, sun);
        let (r, cos) = (x.length(), x.dot(sun) / x.length());
        // The haze scatters the light's net flow on forwards, towards the
        // eye along `-d` (to first order in direction).
        let more = (rayleigh + mie) * table.at(air, r, cos)
            + mie * (3.0 * g / (4.0 * PI)) * table.up_at(air, r, cos) * (-d.dot(x / r));
        seen_light += through * within(total, ds) * (once + more);
        through *= (-total * ds).exp();
    }
    if hits {
        let at = p + d * length;
        seen_light += through * table.ground_at(at.dot(sun) / at.length());
    }
    seen_light
}
