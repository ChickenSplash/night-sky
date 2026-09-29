//! How bright the streamed sky is in each direction, on an octahedral map in
//! galactic axes. It meters the exposure of galactic light, as an eye adapts
//! to a bright sky.
use bevy::{math::DVec3, prelude::*};
use space_simulation::population::flux_per_luminosity;

/// Bins per side of the map, about 3° each.
pub const SIZE: usize = 64;
/// Display value × square pixels of a magnitude-0 star's point.
pub const POINT_LIGHT: f64 = 74.6;
/// Galactic light is stopped down until the metered sky shows at this display
/// value per pixel.
const SKY_BACKGROUND: f64 = 0.15;
/// Stars brighter than this apparent magnitude count as this bright in the
/// map, so one bright star is a point, never a bright or crowded sky.
const CAP_MAGNITUDE: f64 = -1.0;
/// Time constant of the exposure's adaptation, in seconds.
const ADAPT_SECONDS: f64 = 0.5;

/// One batch's share of the map: capped apparent flux (luminosity per squared
/// parsec) and star count, per bin.
pub type Share = Vec<(u16, [f32; 2])>;

#[derive(Resource)]
pub struct Sky {
    totals: Vec<[f64; 2]>,
    solid: Vec<f64>,
    centres: Vec<DVec3>,
    /// Multiplier on galactic light, at most one.
    pub exposure: f32,
}

impl Default for Sky {
    fn default() -> Self {
        let bins = SIZE * SIZE;
        Self {
            totals: vec![[0.0; 2]; bins],
            solid: (0..bins).map(solid_angle).collect(),
            centres: (0..bins).map(|i| decode(bin_uv(i, [0.5, 0.5]))).collect(),
            exposure: 1.0,
        }
    }
}

impl Sky {
    /// Adds a batch's share, or removes it with `sign` -1.
    pub fn add(&mut self, share: &Share, sign: f64) {
        for &(bin, [flux, count]) in share {
            let total = &mut self.totals[usize::from(bin)];
            total[0] = (total[0] + sign * f64::from(flux)).max(0.0);
            total[1] = (total[1] + sign * f64::from(count)).max(0.0);
        }
    }

    /// Exposure that shows the sky in view, averaged over its solid angle, at
    /// `SKY_BACKGROUND`. `gain` is display light per unit of flux, and
    /// `pixel_sr` a pixel's solid angle.
    fn meter(&self, forward: DVec3, tan_half: f64, aspect: f64, gain: f64, pixel_sr: f64) -> f64 {
        let cos_reach = tan_half
            .mul_add(tan_half * (1.0 + aspect * aspect), 1.0)
            .sqrt()
            .recip();
        let (mut flux, mut solid) = (0.0, 0.0);
        for (i, centre) in self.centres.iter().enumerate() {
            if centre.dot(forward) >= cos_reach {
                flux += self.totals[i][0];
                solid += self.solid[i];
            }
        }
        if solid <= 0.0 {
            return 1.0;
        }
        let background = gain * flux / solid * pixel_sr;
        (SKY_BACKGROUND / background).min(1.0)
    }

    /// Eases the exposure towards the metered one.
    pub fn update(
        &mut self,
        seconds: f64,
        forward: DVec3,
        tan_half: f64,
        aspect: f64,
        gain: f64,
        pixel_sr: f64,
    ) {
        let target = self.meter(forward, tan_half, aspect, gain, pixel_sr);
        let ease = 1.0 - (-seconds / ADAPT_SECONDS).exp();
        let current = f64::from(self.exposure);
        self.exposure = (current.ln() + (target.ln() - current.ln()) * ease).exp() as f32;
    }
}

/// Flux counted in the map for a star, at most `CAP_MAGNITUDE`.
fn capped(ratio: f64) -> f64 {
    ratio.min(10f64.powf(-0.4 * CAP_MAGNITUDE) / flux_per_luminosity())
}

/// Gathers stars, as direction from the eye and flux, into a share.
pub fn share(stars: impl Iterator<Item = (DVec3, f64)>) -> Share {
    let mut bins: Vec<(u16, [f32; 2])> = stars
        .filter(|(direction, _)| direction.length_squared() > 0.0)
        .map(|(direction, ratio)| (bin(direction) as u16, [capped(ratio) as f32, 1.0]))
        .collect();
    bins.sort_unstable_by_key(|(bin, _)| *bin);
    bins.dedup_by(|(bin, [flux, count]), (kept, [kept_flux, kept_count])| {
        let same = bin == kept;
        if same {
            *kept_flux += *flux;
            *kept_count += *count;
        }
        same
    });
    bins
}

fn sign(x: f64) -> f64 {
    if x >= 0.0 { 1.0 } else { -1.0 }
}

/// Map coordinates in [0, 1]² of a direction. `starfield.wgsl` repeats this.
fn encode(direction: DVec3) -> [f64; 2] {
    let n = direction / (direction.x.abs() + direction.y.abs() + direction.z.abs());
    let (mut u, mut v) = (n.x, n.y);
    if n.z < 0.0 {
        (u, v) = ((1.0 - n.y.abs()) * sign(n.x), (1.0 - n.x.abs()) * sign(n.y));
    }
    [u * 0.5 + 0.5, v * 0.5 + 0.5]
}

fn decode([u, v]: [f64; 2]) -> DVec3 {
    let (mut x, mut y) = (u * 2.0 - 1.0, v * 2.0 - 1.0);
    let z = 1.0 - x.abs() - y.abs();
    if z < 0.0 {
        (x, y) = ((1.0 - y.abs()) * sign(x), (1.0 - x.abs()) * sign(y));
    }
    DVec3::new(x, y, z).normalize()
}

/// Row-major bin of a direction.
pub fn bin(direction: DVec3) -> usize {
    let [u, v] = encode(direction);
    let index = |t: f64| ((t * SIZE as f64) as usize).min(SIZE - 1);
    index(v) * SIZE + index(u)
}

/// A point within a bin, from its fractional position.
fn bin_uv(bin: usize, [a, b]: [f64; 2]) -> [f64; 2] {
    let size = SIZE as f64;
    [
        ((bin % SIZE) as f64 + a) / size,
        ((bin / SIZE) as f64 + b) / size,
    ]
}

/// Solid angle of a bin, from spherical triangles over a fine grid within it.
fn solid_angle(bin: usize) -> f64 {
    const STEPS: usize = 4;
    let at = |i: usize, j: usize| {
        decode(bin_uv(
            bin,
            [i as f64 / STEPS as f64, j as f64 / STEPS as f64],
        ))
    };
    let triangle = |a: DVec3, b: DVec3, c: DVec3| {
        2.0 * a
            .dot(b.cross(c))
            .abs()
            .atan2(1.0 + a.dot(b) + b.dot(c) + c.dot(a))
    };
    let mut total = 0.0;
    for i in 0..STEPS {
        for j in 0..STEPS {
            let (a, b, c, d) = (at(i, j), at(i + 1, j), at(i + 1, j + 1), at(i, j + 1));
            total += triangle(a, b, c) + triangle(a, c, d);
        }
    }
    total
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bins_cover_the_whole_sky_once() {
        let total: f64 = Sky::default().solid.iter().sum();
        assert!((total / (4.0 * std::f64::consts::PI) - 1.0).abs() < 1e-2);
        for direction in [
            DVec3::X,
            -DVec3::X,
            DVec3::new(0.3, -0.8, -0.52),
            DVec3::new(-0.6, 0.1, 0.79),
            -DVec3::Z,
        ] {
            let direction = direction.normalize();
            let centre = decode(bin_uv(bin(direction), [0.5, 0.5]));
            // Within a bin's diagonal, about 4° at the most stretched bins.
            assert!(centre.angle_between(direction) < 5f64.to_radians());
        }
    }

    #[test]
    fn shares_merge_bins_and_cap_bright_stars() {
        let cap = capped(f64::INFINITY);
        let share = share(
            [
                (DVec3::X, 1.0),
                (DVec3::X * 3.0, 2.0),
                (-DVec3::Y, 1e9),
                (DVec3::ZERO, 5.0),
            ]
            .into_iter(),
        );
        assert_eq!(share.len(), 2);
        let east = share.iter().find(|(b, _)| usize::from(*b) == bin(DVec3::X));
        assert_eq!(east.unwrap().1, [3.0, 2.0]);
        let south = share
            .iter()
            .find(|(b, _)| usize::from(*b) == bin(-DVec3::Y));
        assert_eq!(south.unwrap().1, [cap as f32, 1.0]);
    }

    #[test]
    fn a_bright_sky_stops_the_exposure_down_and_a_dark_one_does_not() {
        let (gain, pixel_sr) = (POINT_LIGHT * flux_per_luminosity(), 7.6e-7);
        let tan_half = 25f64.to_radians().tan();
        let meter = |sky: &Sky| sky.meter(-DVec3::X, tan_half, 16.0 / 9.0, gain, pixel_sr);
        let mut sky = Sky::default();
        assert_eq!(meter(&sky), 1.0);
        // Home-like: a few thousand faint stars over the whole sky.
        let faint = (0..5000).map(|i| {
            let t = f64::from(i) * 2.399;
            let y = 1.0 - 2.0 * (f64::from(i) + 0.5) / 5000.0;
            let r = (1.0 - y * y).sqrt();
            (DVec3::new(r * t.cos(), y, r * t.sin()), 0.01)
        });
        sky.add(&share(faint), 1.0);
        assert_eq!(meter(&sky), 1.0);
        // One very bright star straight ahead is capped: still a point.
        sky.add(&share([(-DVec3::X, 1e6)].into_iter()), 1.0);
        assert_eq!(meter(&sky), 1.0);
        // A crowded bulge ahead: 1e5 stars at the magnitude limit per bin,
        // dozens per pixel.
        let crowd: Share = (0..SIZE * SIZE)
            .filter(|&i| sky.centres[i].dot(-DVec3::X) > 0.7)
            .map(|i| (i as u16, [1e5 * 2e-3, 1e5]))
            .collect();
        sky.add(&crowd, 1.0);
        let stopped = meter(&sky);
        assert!(stopped < 0.5, "{stopped}");
        sky.add(&crowd, -1.0);
        assert_eq!(meter(&sky), 1.0);
    }

    #[test]
    fn exposure_adapts_over_about_half_a_second() {
        let mut sky = Sky::default();
        let crowd: Share = (0..SIZE * SIZE)
            .map(|i| (i as u16, [1e4 * sky.solid[i] as f32, 1.0]))
            .collect();
        sky.add(&crowd, 1.0);
        let gain = POINT_LIGHT * flux_per_luminosity();
        let step = |sky: &mut Sky, seconds| {
            sky.update(seconds, DVec3::X, 0.47, 1.0, gain, 7.6e-7);
            sky.exposure
        };
        let first = step(&mut sky, 1.0 / 60.0);
        assert!(first < 1.0 && first > 0.9, "{first}");
        for _ in 0..180 {
            step(&mut sky, 1.0 / 60.0);
        }
        let settled = f64::from(sky.exposure);
        let target = sky.meter(DVec3::X, 0.47, 1.0, gain, 7.6e-7);
        assert!((settled / target - 1.0).abs() < 0.01);
    }
}
