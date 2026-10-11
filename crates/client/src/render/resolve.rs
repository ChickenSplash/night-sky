//! Turns a frame's `Lights` into exposed, balanced bodies and sprites ready
//! for the GPU (`resolve`).
use super::{
    POINT_PIXELS, PointMaterial,
    lights::{Lights, Photometry, granulation_contrast, phase},
};
use bevy::{math::DVec3, prelude::*};
use space_simulation::light::{self};

/// Arithmetic safety for f32 uniforms only, not an HDR cap: the shaders store
/// light above white as glare (`hdr.wgsl`).
pub(super) const MAX_VALUE: f64 = 1e30;

/// Display values a body's key shows at when exposed for itself: a planet's
/// sub-stellar point just under white, so its shading shows, and a star's
/// surface a little over, so its disc reads as white with darkened limbs.
pub(super) const PLANET_KEY: f64 = 0.8;
pub(super) const STAR_KEY: f64 = 2.0;
/// Corona scale relative to the disc: `point.wgsl` gives Baumbach's profile
/// in millionths, about 4 just outside the limb.
pub(super) const CORONA_SHARE: f64 = 1e-6;
/// Bounds of `point.wgsl`'s corona structure: the outer corona's streamers
/// and rays, the inner corona's plumes and rays, and the rays alone.
pub(super) const CORONA_OUTER_MAX: f32 = 5.5;
pub(super) const CORONA_INNER_MAX: f32 = 2.2;
pub(super) const CORONA_RAYS_MAX: f32 = 1.25;
/// A star's smallest rim sprite half side, in disc radii.
pub(super) const CORONA_EXTENT: f32 = 4.0;
/// Limb brightness relative to the disc's mean, per channel (`body.wgsl`
/// `LIMB` at the very edge).
pub(super) const LIMB_EDGE: DVec3 = DVec3::new(0.6, 0.5, 1.0 / 3.0);
/// Sprites reach until their light is fainter than this display value, so
/// their edge fade is never visible.
pub(super) const SPRITE_FLOOR: f32 = 1.0 / 1024.0;
/// Largest sprite half side, in pixels. Only extreme exposures reach it.
pub(super) const SPRITE_MAX_PIXELS: f32 = 16_384.0;
/// Eye-like streaks start this many stops over white, growing this many
/// pixels per stop up to a limit, and reach full strength (relative to white)
/// over `STREAK_RAMP` stops. They mark light too bright for the camera.
pub(super) const STREAK_START: f32 = 3.0;
pub(super) const STREAK_PX_PER_STOP: f32 = 3.5;
pub(super) const STREAK_MAX_PX: f32 = 160.0;
pub(super) const STREAK_GAIN: f32 = 0.4;
pub(super) const STREAK_RAMP: f32 = 6.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Exposure {
    pub gain: f64,
}

impl Exposure {
    /// Display value of a surface.
    pub fn value(&self, radiance: f64) -> f64 {
        (radiance.max(0.0) * self.gain).min(MAX_VALUE)
    }

    /// Light of a point, in display value × square pixels, exposed as if spread
    /// over a point-sized disc. It matches the disc it replaces at the switch,
    /// and stays linear until the display curve.
    pub fn point(&self, radiance: f64, pixels: f32) -> f64 {
        let switch = disc_area(POINT_PIXELS);
        self.value(spread(radiance, pixels)) * switch
    }
}

/// Gain that moves a body from the shared exposure towards its own, where its
/// `key` (display value at the shared exposure) shows as `target`. `amount` is
/// the share of the stops between them removed: zero is one physical exposure,
/// one exposes every body for itself. Bodies are only ever dimmed, never
/// brightened beyond physical.
pub(super) fn balance(key: f64, target: f64, amount: f64) -> f64 {
    if key <= target {
        return 1.0;
    }
    (target / key).powf(amount.clamp(0.0, 1.0))
}

/// Exposed colour for f32 uniforms, preserving hue at the safety limit.
pub(super) fn hdr_colour(colour: DVec3, value: f64) -> Vec3 {
    (colour * value.min(MAX_VALUE / colour.max_element().max(1.0))).as_vec3()
}

pub(super) fn disc_area(pixels: f32) -> f64 {
    std::f64::consts::PI * f64::from(pixels).powi(2)
}

/// Radiance of a point's light spread over a point-sized disc.
pub(super) fn spread(radiance: f64, pixels: f32) -> f64 {
    radiance * disc_area(pixels) / disc_area(POINT_PIXELS)
}

/// A shaded sphere ready for the GPU.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shaded {
    pub centre: Vec3,
    pub radius: f32,
    pub colour: Vec3,
    /// Direction to the star and night-side ambient; negative ambient for stars.
    pub light: Vec4,
    pub rotation: Quat,
    /// Granules per radius, on-screen radius and contrast; zero for planets.
    pub surface: Vec4,
}

/// A camera-facing sprite ready for the GPU (`point.wgsl`): a point, or the
/// rim around a disc.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Point {
    pub centre: Vec3,
    /// Half the sprite's side, in render units.
    pub half: f32,
    /// A point's whole light, in display value × square pixels, or a corona's
    /// scale: the disc's brightness times `CORONA_SHARE`.
    pub colour: Vec3,
    /// Half side in pixels, a point's Gaussian sigma in pixels, the disc radius
    /// in pixels (zero for a point) and a corona's seed.
    pub shape: Vec4,
    /// The disc's brightness at its limb, and the point spread's sigma in pixels.
    pub edge: Vec4,
    /// Streak colour, and the longest streak's length in pixels.
    pub streaks: Vec4,
    /// A star's pole or a planet's direction to its star, in world axes.
    pub axis: Vec3,
    /// A star's whole limb shines; a planet's only on its lit side.
    pub star: bool,
}

/// Streaks for light peaking at `peak` (display value), tinted towards white
/// like the eye's glare.
pub(super) fn streaks(colour: DVec3, peak: f64, visible: f32) -> Vec4 {
    let stops = peak.max(f64::MIN_POSITIVE).log2() as f32 - STREAK_START;
    if stops <= 0.0 || visible <= 0.0 {
        return Vec4::ZERO;
    }
    let strength = STREAK_GAIN * (stops / STREAK_RAMP).min(1.0) * visible;
    let tint = (colour / colour.max_element().max(1e-12) + DVec3::ONE) * 0.5;
    let length = (stops * STREAK_PX_PER_STOP).min(STREAK_MAX_PX) * visible;
    (tint.as_vec3() * strength).extend(length)
}

/// Pixels beyond which a Gaussian of `sigma` peaking at `peak` is invisible.
pub(super) fn gaussian_reach(peak: f32, sigma: f32) -> f32 {
    sigma * (2.0 * (peak / SPRITE_FLOOR).max(1.0).ln()).sqrt()
}

/// Disc radii beyond which a corona of `scale` (`point.wgsl` `corona`) is
/// invisible.
pub(super) fn corona_reach(scale: f32) -> f32 {
    let bound = |x: f32| {
        scale
            * (0.0532 * CORONA_OUTER_MAX * x.powf(-2.5)
                + 1.425 * CORONA_INNER_MAX * x.powi(-7)
                + 2.565 * CORONA_RAYS_MAX * x.powi(-17))
    };
    if bound(1.0) <= SPRITE_FLOOR {
        return 1.0;
    }
    let (mut low, mut high) = (1.0f32, 1e7f32);
    for _ in 0..60 {
        let mid = (low * high).sqrt();
        if bound(mid) > SPRITE_FLOOR {
            low = mid;
        } else {
            high = mid;
        }
    }
    high
}

/// Sprite half side for light reaching `reach` pixels: the fade-out starts at
/// 0.6 of the half side.
pub(super) fn sprite_half(reach: f32, least: f32) -> f32 {
    (reach / 0.6).max(least).min(SPRITE_MAX_PIXELS.max(least))
}

impl Point {
    /// A body below `POINT_PIXELS`: its light spread by the point spread,
    /// widened by its disc (a uniform disc's variance is a quarter of its
    /// squared radius), so it matches the blurred disc it becomes.
    pub(super) fn new(
        centre: Vec3,
        pixels: f32,
        per_pixel: f32,
        colour: DVec3,
        light: f64,
        psf: f32,
        visible: f32,
    ) -> Self {
        let sigma = (psf * psf + pixels * pixels / 4.0).sqrt();
        let peak = light / (std::f64::consts::TAU * f64::from(sigma).powi(2));
        let streaks = streaks(colour, peak * colour.max_element(), visible);
        let colour = hdr_colour(colour, light);
        let peak = colour.max_element() / (std::f32::consts::TAU * sigma * sigma);
        let reach = gaussian_reach(peak, sigma).max(1.5 * sigma + streaks.w);
        let half = sprite_half(reach, 4.0 * sigma);
        Self {
            centre,
            half: half * per_pixel,
            colour,
            shape: Vec4::new(half, sigma, 0.0, 0.0),
            edge: Vec4::ZERO,
            streaks,
            axis: Vec3::ZERO,
            star: false,
        }
    }

    /// The rim of a disc `pixels` in radius: its limb (`limb`, display value)
    /// blurred by the point spread, and for a star its corona (`corona`, the
    /// scale).
    #[allow(clippy::too_many_arguments)]
    pub(super) fn rim(
        centre: Vec3,
        pixels: f32,
        per_pixel: f32,
        psf: f32,
        limb: DVec3,
        corona: DVec3,
        streaks: Vec4,
        axis: Vec3,
        star: bool,
        seed: f32,
    ) -> Self {
        let limb = hdr_colour(limb, 1.0);
        let corona = hdr_colour(corona, 1.0);
        // erfc(z) < exp(-z²), so the blurred limb is within a Gaussian's reach.
        let mut reach = pixels + gaussian_reach(limb.max_element(), psf);
        reach = reach.max(pixels + streaks.w);
        let mut least = 0.0;
        if star {
            reach = reach.max(corona_reach(corona.max_element()) * pixels);
            least = CORONA_EXTENT * pixels;
        }
        let half = sprite_half(reach, least);
        Self {
            centre,
            half: half * per_pixel,
            colour: corona,
            shape: Vec4::new(half, 0.0, pixels, seed),
            edge: limb.extend(psf),
            streaks,
            axis,
            star,
        }
    }

    /// The material, with `axis` projected onto a sprite facing `facing`
    /// (local +y is down on the sprite).
    pub(super) fn material(&self, facing: Quat) -> PointMaterial {
        let right = facing * Vec3::X;
        let up = facing * Vec3::Y;
        PointMaterial {
            colour: self.colour.extend(1.0),
            shape: self.shape,
            edge: self.edge,
            streaks: self.streaks,
            axis: Vec4::new(
                self.axis.dot(right),
                -self.axis.dot(up),
                if self.star { 1.0 } else { 0.0 },
                0.0,
            ),
        }
    }
}

#[derive(Resource, Default)]
pub struct Frame {
    pub bodies: Vec<Shaded>,
    pub points: Vec<Point>,
    /// Sprites around discs: blurred limbs, coronas and streaks.
    pub rims: Vec<Point>,
}

/// Exposes every body with the shared manual gain, then moves each towards its
/// own exposure by `compression` (see `balance`). Galactic points and glow
/// never balance: Exposure alone sets the galaxy. `psf` is the point spread's
/// sigma in logical pixels.
pub fn resolve(
    lights: &Lights,
    photometry: &Photometry,
    stops: f64,
    compression: f64,
    psf: f32,
) -> Frame {
    let exposure = Exposure {
        gain: crate::render::exposure::gain(stops),
    };
    let mut frame = Frame::default();
    for star in &lights.stars {
        let colour = DVec3::from_array(photometry.colour(star.kelvin));
        // A point is keyed by its light spread over a point-sized disc, which
        // matches the disc's surface at the switch.
        let key = exposure.value(spread(star.radiance, star.pixels.min(POINT_PIXELS)));
        let balance = balance(key, STAR_KEY, compression);
        if star.pixels < POINT_PIXELS {
            let light = exposure.point(star.radiance, star.pixels) * balance;
            frame.points.push(Point::new(
                star.centre,
                star.pixels,
                star.per_pixel,
                colour,
                light,
                psf,
                lights.streaks,
            ));
            continue;
        }
        let value = exposure.value(star.radiance) * balance;
        frame.bodies.push(Shaded {
            centre: star.centre,
            radius: star.radius,
            colour: hdr_colour(colour, value),
            light: Vec4::new(0.0, 0.0, 0.0, -1.0),
            rotation: star.spin,
            surface: Vec4::new(
                star.granules as f32,
                star.pixels,
                granulation_contrast(star.kelvin),
                0.0,
            ),
        });
        // The corona grows in from a point, and is whiter than the surface.
        let fade = f64::from(((star.pixels / POINT_PIXELS - 1.0) / 3.0).clamp(0.0, 1.0));
        let pearl = (colour + DVec3::splat(colour.max_element())) * 0.5;
        frame.rims.push(Point::rim(
            star.centre,
            star.pixels,
            star.per_pixel,
            psf,
            colour * LIMB_EDGE * value,
            pearl * value * CORONA_SHARE * fade,
            streaks(colour, colour.max_element() * value, lights.streaks),
            star.spin * Vec3::Y,
            true,
            (star.kelvin * 0.618_034).fract() as f32,
        ));
    }
    let tint = DVec3::from_array(photometry.colour(lights.kelvin));
    for planet in &lights.planets {
        let radiance = light::reflected_radiance(planet.surface.albedo, planet.flux);
        let colour = tint * DVec3::from_array(planet.surface.tint);
        // Keyed by the sub-stellar point, never by phase, so crescents stay dim.
        let key = exposure.value(spread(radiance, planet.pixels.min(POINT_PIXELS)));
        let balance = balance(key, PLANET_KEY, compression);
        if planet.pixels < POINT_PIXELS {
            let towards_eye = (lights.eye - planet.centre).normalize_or_zero();
            let angle = planet.towards_star.angle_between(towards_eye);
            let light = exposure.point(radiance * phase(angle), planet.pixels) * balance;
            frame.points.push(Point::new(
                planet.centre,
                planet.pixels,
                planet.per_pixel,
                colour,
                light,
                psf,
                lights.streaks,
            ));
            continue;
        }
        let value = exposure.value(radiance) * balance;
        frame.bodies.push(Shaded {
            centre: planet.centre,
            radius: planet.radius,
            colour: hdr_colour(colour, value),
            light: planet.towards_star.extend(0.0),
            rotation: Quat::IDENTITY,
            surface: Vec4::ZERO,
        });
        frame.rims.push(Point::rim(
            planet.centre,
            planet.pixels,
            planet.per_pixel,
            psf,
            colour * value,
            DVec3::ZERO,
            streaks(colour, colour.max_element() * value, lights.streaks),
            planet.towards_star,
            false,
            0.0,
        ));
    }
    frame
}
