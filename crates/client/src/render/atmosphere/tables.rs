//! The atmosphere's transmittance table (`atmosphere.wgsl`): optical depth
//! from any height, in any direction, to the top of the air, so the view
//! march reads the starlight reaching each step instead of marching towards
//! each star. Built on the CPU in the background when the air changes and
//! cached by air, in units of the planet's radius, so identical air on two
//! planets shares a table. The multiple scattering's tables
//! (`multiple.rs`) take longer, so are built after it.
use super::Air;
use bevy::{
    asset::RenderAssetUsages,
    image::ImageSampler,
    math::DVec3,
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
    tasks::{AsyncComputeTaskPool, Task, futures::check_ready},
};
use std::collections::{HashMap, HashSet};

/// Texels across (the cosine of the zenith angle) and down (height).
/// Matched in `atmosphere.wgsl`.
pub const SIZE: [usize; 2] = [256, 64];
/// Steps along each texel's ray.
const STEPS: usize = 512;
/// Tables kept for air no longer drawn.
const CACHED: usize = 8;
/// Hard resident table budget, including currently requested air (~9 MiB).
const MAX_RESIDENT: usize = 32;

/// Extinction at `height` above the ground, as `medium` in `atmosphere.wgsl`.
pub(super) fn extinction(air: &Air, height: f64) -> DVec3 {
    let above = height.max(0.0);
    let rayleigh = (-above / f64::from(air.rayleigh.w)).exp();
    let mie = (-above / f64::from(air.shape.x)).exp();
    let layer = (1.0 - (height - f64::from(air.shape.z)).abs() / f64::from(air.shape.w)).max(0.0);
    (air.rayleigh.truncate() + air.gas_absorption).as_dvec3() * rayleigh
        + (air.mie.truncate().as_dvec3() + f64::from(air.mie.w)) * mie
        + air.absorption.as_dvec3() * layer
}

fn distance_to_top(top: f64, r: f64, mu: f64) -> f64 {
    (-r * mu + (r * r * (mu * mu - 1.0) + top * top).max(0.0).sqrt()).max(0.0)
}

/// Bruneton's coordinates, 0 to 1, of the ray from radius `r` at cosine `mu`
/// from the zenith, as `transmittance_uv` in `atmosphere.wgsl` before its
/// texel-centre mapping. Rays towards the ground read the horizon's.
fn unit(top: f64, r: f64, mu: f64) -> (f64, f64) {
    let horizon = (top * top - 1.0).sqrt();
    let rho = (r * r - 1.0).max(0.0).sqrt();
    let (near, far) = (top - r, rho + horizon);
    let along = (distance_to_top(top, r, mu) - near) / (far - near).max(1e-12);
    (along.clamp(0.0, 1.0), (rho / horizon).clamp(0.0, 1.0))
}

/// The table's optical depth from radius `r` at cosine `mu`, bilinear, as
/// the GPU samples it.
pub(super) fn sample(table: &[DVec3], top: f64, r: f64, mu: f64) -> DVec3 {
    let [w, h] = SIZE;
    let (along, height) = unit(top, r, mu);
    let (x, y) = (along * (w - 1) as f64, height * (h - 1) as f64);
    let (i, j) = ((x as usize).min(w - 2), (y as usize).min(h - 2));
    let (fx, fy) = (x - i as f64, y - j as f64);
    let at = |i: usize, j: usize| table[j * w + i];
    (at(i, j) * (1.0 - fx) + at(i + 1, j) * fx) * (1.0 - fy)
        + (at(i, j + 1) * (1.0 - fx) + at(i + 1, j + 1) * fx) * fy
}

/// The ray at Bruneton's coordinates: its radius and cosine from the zenith.
fn ray(top: f64, along: f64, height: f64) -> (f64, f64) {
    let horizon = (top * top - 1.0).sqrt();
    let rho = horizon * height;
    let r = (rho * rho + 1.0).sqrt();
    let (near, far) = (top - r, rho + horizon);
    let d = near + along * (far - near);
    let mu = if d <= 0.0 {
        1.0
    } else {
        ((horizon * horizon - rho * rho - d * d) / (2.0 * r * d)).clamp(-1.0, 1.0)
    };
    (r, mu)
}

fn optical_depth(air: &Air, r: f64, mu: f64) -> DVec3 {
    let ds = distance_to_top(f64::from(air.top), r, mu) / STEPS as f64;
    (0..STEPS)
        .map(|i| {
            let t = (i as f64 + 0.5) * ds;
            let height = (r * r + t * t + 2.0 * r * mu * t).sqrt() - 1.0;
            extinction(air, height) * ds
        })
        .sum()
}

/// Optical depth per texel, row by row from the ground up.
pub(super) fn depths(air: &Air) -> Vec<DVec3> {
    let top = f64::from(air.top);
    let [w, h] = SIZE;
    (0..h)
        .flat_map(|j| (0..w).map(move |i| (i, j)))
        .map(|(i, j)| {
            let (r, mu) = ray(top, i as f64 / (w - 1) as f64, j as f64 / (h - 1) as f64);
            optical_depth(air, r, mu)
        })
        .collect()
}

/// A table of `size` texels, row by row, filtered.
pub(super) fn image(size: [usize; 2], texels: &[DVec3]) -> Image {
    let data = texels
        .iter()
        .flat_map(|d| [d.x, d.y, d.z, 0.0])
        .flat_map(|c| half::f16::from_f64(c.clamp(-65_504.0, 65_504.0)).to_le_bytes())
        .collect();
    let mut image = Image::new(
        Extent3d {
            width: size[0] as u32,
            height: size[1] as u32,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba16Float,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.sampler = ImageSampler::linear();
    image
}

/// An air, bit for bit.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct Key([u32; 22]);

impl Key {
    fn new(air: &Air) -> Self {
        let mut bits = [0; 22];
        let values = [air.top]
            .into_iter()
            .chain(air.rayleigh.to_array())
            .chain(air.mie.to_array())
            .chain(air.shape.to_array())
            .chain(air.absorption.to_array())
            .chain(air.gas_absorption.to_array())
            .chain(air.ground.to_array());
        for (bit, value) in bits.iter_mut().zip(values) {
            *bit = value.to_bits();
        }
        Self(bits)
    }
}

/// An air's tables: its transmittance, and once built, its multiple
/// scattering's.
#[derive(Clone)]
pub struct Tables {
    pub transmittance: Handle<Image>,
    pub multiple: Option<MultipleTables>,
}

#[derive(Clone)]
pub struct MultipleTables {
    /// The transmittance of the air with the haze's forward peak taken as
    /// no scattering, for the starlight.
    pub thinned: Handle<Image>,
    /// The light scattered more than once, its net upward flow, and the
    /// ground's light.
    pub sky: Handle<Image>,
    pub flow: Handle<Image>,
}

/// Built tables by air, and the one build of each kind in flight. Air asked
/// for while another builds waits, so a slider drag rebuilds only as fast
/// as builds finish, and only for its latest value.
#[derive(Resource, Default)]
pub struct AtmosphereTables {
    ready: HashMap<Key, Tables>,
    building: Option<(Key, Task<Image>)>,
    building_multiple: Option<(Key, Task<[Image; 3]>)>,
    wanted: Vec<(Key, Air)>,
    used: HashSet<Key>,
    last_used: HashMap<Key, u64>,
    frame: u64,
}

impl AtmosphereTables {
    /// The tables for this air, or `None` while its transmittance is built.
    pub fn get(&mut self, air: &Air) -> Option<Tables> {
        let key = Key::new(air);
        self.used.insert(key);
        self.last_used.insert(key, self.frame);
        if !self.wanted.iter().any(|w| w.0 == key) {
            self.wanted.push((key, *air));
        }
        if let Some(table) = self.ready.get(&key) {
            return Some(table.clone());
        }
        None
    }

    /// Once a frame, after every `get`: takes finished builds, starts the
    /// next and drops tables unused this frame beyond `CACHED`.
    pub fn update(&mut self, images: &mut Assets<Image>) {
        if let Some((key, task)) = self.building.as_mut()
            && let Some(image) = check_ready(task)
        {
            let transmittance = images.add(image);
            self.ready.insert(
                *key,
                Tables {
                    transmittance,
                    multiple: None,
                },
            );
            self.building = None;
        }
        if let Some((key, task)) = self.building_multiple.as_mut()
            && let Some([thinned, sky, flow]) = check_ready(task)
        {
            if let Some(tables) = self.ready.get_mut(key) {
                tables.multiple = Some(MultipleTables {
                    thinned: images.add(thinned),
                    sky: images.add(sky),
                    flow: images.add(flow),
                });
            }
            self.building_multiple = None;
        }
        let pool = AsyncComputeTaskPool::get();
        if self.building.is_none()
            && let Some(&(key, air)) = self.wanted.iter().find(|w| !self.ready.contains_key(&w.0))
        {
            let task = pool.spawn(async move { image(SIZE, &depths(&air)) });
            self.building = Some((key, task));
        }
        if self.building_multiple.is_none()
            && let Some(&(key, air)) = self
                .wanted
                .iter()
                .find(|w| self.ready.get(&w.0).is_some_and(|t| t.multiple.is_none()))
        {
            let task = pool.spawn(async move { super::multiple::images(air) });
            self.building_multiple = Some((key, task));
        }
        self.wanted.clear();
        self.trim();
        self.used.clear();
        self.frame = self.frame.wrapping_add(1);
    }

    fn trim(&mut self) {
        let mut unused: Vec<Key> = self
            .ready
            .keys()
            .filter(|k| !self.used.contains(k))
            .copied()
            .collect();
        unused.sort_by_key(|key| std::cmp::Reverse(self.last_used.get(key).copied().unwrap_or(0)));
        for key in unused.into_iter().skip(CACHED) {
            self.ready.remove(&key);
            self.last_used.remove(&key);
        }
        while self.ready.len() > MAX_RESIDENT {
            let key = *self
                .ready
                .keys()
                .min_by_key(|key| self.last_used.get(key).copied().unwrap_or(0))
                .unwrap();
            self.ready.remove(&key);
            self.last_used.remove(&key);
        }
        // Forget requests that never became resident, except in-flight work.
        self.last_used.retain(|key, _| {
            self.ready.contains_key(key)
                || self.building.as_ref().is_some_and(|(k, _)| k == key)
                || self
                    .building_multiple
                    .as_ref()
                    .is_some_and(|(k, _)| k == key)
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::atmosphere::{AirSettings, Preset};
    use bevy::math::DVec2;

    const EARTH_M: f64 = 6.371e6;

    #[test]
    fn ready_transmittance_still_requests_its_multiple_scattering_build() {
        let air = air(Preset::Earth);
        let key = Key::new(&air);
        let mut cache = AtmosphereTables::default();
        let mut images = Assets::<Image>::default();
        cache.ready.insert(
            key,
            Tables {
                transmittance: images.add(Image::default()),
                multiple: None,
            },
        );
        assert!(cache.get(&air).is_some());
        assert_eq!(cache.wanted, vec![(key, air)]);
        cache.get(&air);
        assert_eq!(cache.wanted.len(), 1);
    }

    #[test]
    fn table_cache_is_bounded_and_drops_least_recently_used_air() {
        let mut cache = AtmosphereTables::default();
        let mut images = Assets::<Image>::default();
        let mut keys = Vec::new();
        for i in 0..MAX_RESIDENT + 4 {
            let mut air = air(Preset::Earth);
            air.top += i as f32 * 0.01;
            let key = Key::new(&air);
            cache.ready.insert(
                key,
                Tables {
                    transmittance: images.add(Image::default()),
                    multiple: None,
                },
            );
            cache.last_used.insert(key, i as u64);
            cache.used.insert(key);
            keys.push(key);
        }
        cache.trim();
        assert_eq!(cache.ready.len(), MAX_RESIDENT);
        assert!(!cache.ready.contains_key(&keys[0]));
        assert!(cache.ready.contains_key(keys.last().unwrap()));
        cache.used.clear();
        cache.trim();
        assert_eq!(cache.ready.len(), CACHED);
        assert!(cache.ready.contains_key(keys.last().unwrap()));
    }

    #[test]
    fn exponential_gas_absorption_is_in_depths_and_the_cache_key() {
        let mut air = air(Preset::Earth);
        let original = Key::new(&air);
        air.mie = Vec4::ZERO;
        air.absorption = Vec3::ZERO;
        air.gas_absorption = Vec3::new(0.1, 0.02, 0.0);
        assert_ne!(Key::new(&air), original);
        let got = optical_depth(&air, 1.0, 1.0);
        let expected = (air.rayleigh.truncate() + air.gas_absorption).as_dvec3()
            * f64::from(air.rayleigh.w)
            * (1.0 - (-f64::from(air.top - 1.0) / f64::from(air.rayleigh.w)).exp());
        assert!((got / expected - DVec3::ONE).abs().max_element() < 0.01);
    }

    fn air(preset: Preset) -> Air {
        AirSettings::preset(preset).atmosphere().scaled(EARTH_M)
    }

    #[test]
    fn the_parametrisation_round_trips() {
        let top = f64::from(air(Preset::Earth).top);
        for (along, height) in [(0.0, 0.0), (0.3, 0.1), (0.5, 0.5), (1.0, 0.9), (0.7, 1.0)] {
            let (r, mu) = ray(top, along, height);
            let (a, h) = unit(top, r, mu);
            assert!(
                (a - along).abs() < 1e-6 && (h - height).abs() < 1e-6,
                "{along} {height}: {a} {h}"
            );
        }
    }

    #[test]
    fn straight_up_matches_the_scale_heights() {
        let air = air(Preset::Earth);
        let s = AirSettings::preset(Preset::Earth).atmosphere();
        // Gas, haze and the ozone layer's triangle, up from the ground.
        let expected = s.rayleigh * s.rayleigh_height
            + (s.mie + DVec3::splat(s.mie_absorption)) * s.mie_height
            + s.absorption * s.layer_width;
        let got = sample(&depths(&air), f64::from(air.top), 1.0, 1.0);
        for c in 0..3 {
            assert!(
                (got[c] / expected[c] - 1.0).abs() < 0.02,
                "{got} against {expected}"
            );
        }
    }

    /// The starlight the table gives matches a fine march, between texels
    /// too, from the ground to the top and from overhead to the horizon,
    /// where it changes fastest.
    #[test]
    fn the_table_matches_a_fine_march() {
        for preset in [Preset::Earth, Preset::Venus, Preset::Mars, Preset::Titan] {
            let air = air(preset);
            let top = f64::from(air.top);
            let table = depths(&air);
            for height in [0.0, 0.003, 0.02, 0.17, 0.5, 0.95] {
                let r = 1.0 + height * (top - 1.0);
                let horizon = -(1.0 - 1.0 / (r * r)).sqrt();
                for mu in [1.0, 0.61, 0.3, 0.1, 0.03, 0.0, horizon * 0.5, horizon] {
                    let ds = distance_to_top(top, r, mu) / 20_000.0;
                    let exact: DVec3 = (0..20_000)
                        .map(|i| {
                            let t = (f64::from(i) + 0.5) * ds;
                            extinction(&air, (r * r + t * t + 2.0 * r * mu * t).sqrt() - 1.0) * ds
                        })
                        .sum();
                    let got = sample(&table, top, r, mu);
                    // Light that gets through matches closely; light all
                    // but lost may differ in its tiny remainder.
                    let (seen, expected) = ((-got).exp(), (-exact).exp());
                    for c in 0..3 {
                        assert!(
                            (seen[c] - expected[c]).abs() < 0.01,
                            "{preset:?} at {height} up, mu {mu}: {seen} against {expected}"
                        );
                    }
                }
            }
        }
    }

    /// The point where the air in front turns opaque, found as the shader
    /// does from differences of the table's depths, is where a fine march
    /// reaches that depth, on Venus from the ground up, looking down and
    /// along the horizon.
    #[test]
    fn the_table_finds_where_the_air_turns_opaque() {
        const OPAQUE: f64 = 10.0;
        let radius = 5.756e6;
        let air = AirSettings::preset(Preset::Venus)
            .atmosphere()
            .scaled(radius);
        let top = f64::from(air.top);
        let table = depths(&air);
        for km in [0.05, 0.5, 2.0, 10.0, 40.0] {
            let r = 1.0 + km * 1e3 / radius;
            for mu in [-1.0_f64, -0.5, -0.1, -0.01, 0.0, 0.1] {
                let along = |t: f64| DVec2::new(t * (1.0 - mu * mu).sqrt(), r + t * mu);
                let b = r * mu;
                let ground = -b - (b * b - r * r + 1.0).max(0.0).sqrt();
                let reaches = b * b - r * r + 1.0 > 0.0 && ground > 0.0;
                let end = if reaches {
                    ground
                } else {
                    distance_to_top(top, r, mu)
                };
                // Forwards to the top, or backwards for rays reaching the
                // ground, whose backward line never meets it.
                let way = if reaches { -1.0 } else { 1.0 };
                let to_top = |t: f64| {
                    let p = along(t);
                    let dir = DVec2::new((1.0 - mu * mu).sqrt(), mu) * way;
                    sample(&table, top, p.length(), p.dot(dir) / p.length())
                };
                let depth = |t: f64| (way * (to_top(0.0) - to_top(t))).min_element();
                let marched = |t: f64| -> f64 {
                    let ds = t / 20_000.0;
                    (0..20_000)
                        .map(|i| {
                            extinction(&air, along((f64::from(i) + 0.5) * ds).length() - 1.0) * ds
                        })
                        .sum::<DVec3>()
                        .min_element()
                };
                if depth(end) < OPAQUE {
                    assert!(marched(end) < OPAQUE * 1.3, "{km} km, mu {mu}: never stops");
                    continue;
                }
                let (mut low, mut high) = (0.0, end);
                for _ in 0..10 {
                    let mid = 0.5 * (low + high);
                    if depth(mid) < OPAQUE {
                        low = mid;
                    } else {
                        high = mid;
                    }
                }
                let got = marched(high);
                assert!(
                    (OPAQUE * 0.9..OPAQUE * 1.3).contains(&got),
                    "{km} km, mu {mu}: stops at optical depth {got}"
                );
            }
        }
    }
}
