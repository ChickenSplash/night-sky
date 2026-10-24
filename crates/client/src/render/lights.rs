//! Each realistic body's light before exposure, gathered by `scene::draw`,
//! and cached star photometry.
use bevy::{
    math::{DQuat, DVec3},
    prelude::*,
};
use space_shared::Star;
use space_simulation::light::{self, Surface};
use std::{collections::HashMap, f32::consts::PI, sync::Mutex};

/// How a star looks from here, before exposure.
pub struct StarLight {
    pub centre: Vec3,
    /// Drawn disc radius, in render units.
    pub radius: f32,
    /// On-screen radius of the disc, in logical pixels.
    pub pixels: f32,
    /// Render units per logical pixel at the star.
    pub per_pixel: f32,
    /// Surface radiance of the drawn disc.
    pub radiance: f64,
    pub kelvin: f64,
    /// Orientation of the surface, in world axes: its pole is local +Y.
    pub spin: Quat,
    /// How much shorter the polar radius is than `radius`, the equatorial.
    pub flattening: f32,
    /// Granules per disc radius (`granules`).
    pub granules: f64,
}

/// Stars lighting one planet, at most: a quadruple system's.
pub const MAX_LIGHTS: usize = 4;

/// Light reaching a planet from one star.
#[derive(Clone, Copy, Debug)]
pub struct Illumination {
    /// Direction to the star, in world axes.
    pub towards: Vec3,
    /// Visual flux, W m⁻².
    pub flux: f64,
    pub kelvin: f64,
}

/// How a planet is lit, before exposure.
pub struct PlanetLight {
    pub centre: Vec3,
    /// Equatorial radius, as for `StarLight`.
    pub radius: f32,
    pub spin: Quat,
    pub flattening: f32,
    pub pixels: f32,
    pub per_pixel: f32,
    /// Each star's light, brightest first, at most `MAX_LIGHTS`.
    pub light: Vec<Illumination>,
    pub surface: Surface,
    pub air: Option<super::atmosphere::Air>,
    /// Its ground's key in `Lights::grounds`, when drawn as chunks.
    pub ground: Option<u64>,
}

/// What a planet's ground chunks need, in f64 so they keep their precision
/// near the camera (`ground.rs`).
pub struct GroundSource {
    /// Stable for the planet and its shape.
    pub key: u64,
    pub planet: space_shared::Planet,
    pub shape: space_simulation::Spheroid,
    /// The planet's centre from the camera, in metres, in world axes.
    pub from_eye: DVec3,
    /// From the planet's axes (pole +Y) into world axes.
    pub basis: DQuat,
}

/// One frame's realistic bodies, gathered while drawing the wireframes.
#[derive(Resource, Default)]
pub struct Lights {
    pub stars: Vec<StarLight>,
    pub planets: Vec<PlanetLight>,
    pub grounds: Vec<GroundSource>,
    /// Render position of the camera.
    pub eye: Vec3,
    /// Metres per render unit.
    pub scale: f64,
    /// The camera's rotation, which sprites share.
    pub facing: Quat,
    /// Streak visibility, from zero at the edge of the body range to one well
    /// inside it, so streaks never appear suddenly as bodies take over.
    pub streaks: f32,
}

impl Lights {
    pub fn clear(&mut self) {
        self.stars.clear();
        self.planets.clear();
        self.grounds.clear();
        self.streaks = 0.0;
    }
}

/// Mean brightness of a Lambertian sphere's disc at a phase angle, relative to
/// its sub-stellar point: 2/3 when full, none when new.
pub fn phase(angle: f32) -> f64 {
    let angle = f64::from(angle.clamp(0.0, PI));
    2.0 / 3.0 * (angle.sin() + (std::f64::consts::PI - angle) * angle.cos()) / std::f64::consts::PI
}

/// Granules across one radius of a star. Granules scale with the pressure scale
/// height, which grows with temperature and falls with surface gravity: about
/// 700 per radius on the Sun, a handful on a giant.
pub fn granules(star: &Star) -> f64 {
    let gravity = star.mass_solar / star.radius_solar.powi(2);
    let height = (star.temperature_k / 5772.0) / gravity;
    (700.0 * star.radius_solar / height).clamp(3.0, 5000.0)
}

/// Granulation contrast: convection reaches the surface only of cool stars.
pub(super) fn granulation_contrast(kelvin: f64) -> f32 {
    let hot = ((kelvin - 6500.0) / 1500.0).clamp(0.0, 1.0);
    (1.0 - hot * hot * (3.0 - 2.0 * hot)) as f32
}

/// Star colour and visual efficiency by temperature, cached: both integrate a
/// spectrum.
#[derive(Resource, Default)]
pub struct Photometry(Mutex<HashMap<u64, ([f64; 3], f64)>>);

impl Photometry {
    fn lookup(&self, kelvin: f64) -> ([f64; 3], f64) {
        let mut cache = self.0.lock().unwrap_or_else(|e| e.into_inner());
        *cache.entry(kelvin.to_bits()).or_insert_with(|| {
            (
                light::blackbody_colour(kelvin),
                light::visual_efficiency(kelvin),
            )
        })
    }

    pub fn colour(&self, kelvin: f64) -> [f64; 3] {
        self.lookup(kelvin).0
    }

    /// Visual luminosity in solar units.
    pub fn visual_luminosity(&self, star: &Star) -> f64 {
        star.luminosity_solar * self.lookup(star.temperature_k).1
    }

    pub fn surface_radiance(&self, star: &Star) -> f64 {
        let radius = star.radius_solar * space_simulation::SUN_RADIUS_METRES;
        light::flux(self.visual_luminosity(star), radius) / std::f64::consts::PI
    }
}
