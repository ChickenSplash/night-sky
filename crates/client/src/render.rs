//! Realistic bodies: physical photometry, one manual linear exposure and
//! per-body balance (`resolve`). The display pass lives in `exposure`.
//! - Every body shares the camera's point spread with the galactic stars
//!   (`PSF_SIGMA_PX`). Bodies smaller than a point on screen are point sprites
//!   carrying the body's whole light; a disc's limb is blurred by the same
//!   spread in a sprite around it, so nothing jumps at the switch.
//! - Close up, a star is a disc with limb darkening, granulation and a
//!   structured corona.
//! - Far over white, every body shows eye-like streaks, and light above white
//!   is stored as glare (`hdr.wgsl`), so bloom grows with stops, not light.
//! - Colours reach the GPU already exposed, so half-float HDR targets never see
//!   raw radiance.

use crate::{Navigation, scene::SpaceCamera};
use bevy::{
    asset::{load_internal_asset, uuid_handle},
    camera::visibility::{NoFrustumCulling, VisibilitySystems},
    core_pipeline::tonemapping::Tonemapping,
    math::DVec3,
    pbr::{Material, MaterialPlugin, MeshMaterial3d},
    post_process::bloom::Bloom,
    prelude::*,
    render::{render_resource::AsBindGroup, view::Hdr},
    shader::ShaderRef,
};
use big_space::prelude::*;
use space_shared::Star;
use space_simulation::light::{self, Surface};
use std::{collections::HashMap, f32::consts::PI, sync::Mutex};

const BODY_SHADER: Handle<Shader> = uuid_handle!("6f1f3c47-2a8e-4d61-9b53-4f8e0d7a91c2");
const POINT_SHADER: Handle<Shader> = uuid_handle!("0c9b5d2e-7a41-4e8f-a3d6-5b2f81c4e970");

/// Deep navy behind the wireframes; realistic space is black.
pub const SPACE_BLUE: Color = Color::srgb(0.015, 0.035, 0.09);

/// Arithmetic safety for f32 uniforms only, not an HDR cap: the shaders store
/// light above white as glare (`hdr.wgsl`).
const MAX_VALUE: f64 = 1e30;
/// Bodies below this on-screen radius, in logical pixels, are drawn as points.
pub const POINT_PIXELS: f32 = 1.5;
/// The camera's point spread: a Gaussian of this sigma in physical pixels, as
/// for galactic stars (`starfield.wgsl` `SIGMA_PX`). Bloom adds the wide glow.
pub const PSF_SIGMA_PX: f32 = 0.55;
/// Display values a body's key shows at when exposed for itself: a planet's
/// sub-stellar point just under white, so its shading shows, and a star's
/// surface a little over, so its disc reads as white with darkened limbs.
const PLANET_KEY: f64 = 0.8;
const STAR_KEY: f64 = 2.0;
/// Corona scale relative to the disc: `point.wgsl` gives Baumbach's profile
/// in millionths, about 4 just outside the limb.
const CORONA_SHARE: f64 = 1e-6;
/// Bounds of `point.wgsl`'s corona structure: the outer corona's streamers
/// and rays, the inner corona's plumes and rays, and the rays alone.
const CORONA_OUTER_MAX: f32 = 5.5;
const CORONA_INNER_MAX: f32 = 2.2;
const CORONA_RAYS_MAX: f32 = 1.25;
/// A star's smallest rim sprite half side, in disc radii.
const CORONA_EXTENT: f32 = 4.0;
/// Limb brightness relative to the disc's mean, per channel (`body.wgsl`
/// `LIMB` at the very edge).
const LIMB_EDGE: DVec3 = DVec3::new(0.6, 0.5, 1.0 / 3.0);
/// Sprites reach until their light is fainter than this display value, so
/// their edge fade is never visible.
const SPRITE_FLOOR: f32 = 1.0 / 1024.0;
/// Largest sprite half side, in pixels. Only extreme exposures reach it.
const SPRITE_MAX_PIXELS: f32 = 16_384.0;
/// Eye-like streaks start this many stops over white, growing this many
/// pixels per stop up to a limit, and reach full strength (relative to white)
/// over `STREAK_RAMP` stops. They mark light too bright for the camera.
const STREAK_START: f32 = 3.0;
const STREAK_PX_PER_STOP: f32 = 3.5;
const STREAK_MAX_PX: f32 = 160.0;
const STREAK_GAIN: f32 = 0.4;
const STREAK_RAMP: f32 = 6.0;

pub struct RenderPlugin;

impl Plugin for RenderPlugin {
    fn build(&self, app: &mut App) {
        load_internal_asset!(app, BODY_SHADER, "body.wgsl", Shader::from_wgsl);
        load_internal_asset!(app, POINT_SHADER, "point.wgsl", Shader::from_wgsl);
        app.add_plugins((
            MaterialPlugin::<BodyMaterial>::default(),
            MaterialPlugin::<PointMaterial>::default(),
            crate::exposure::ExposurePlugin,
        ))
        .init_resource::<Lights>()
        .init_resource::<Photometry>()
        .init_resource::<Frame>()
        .init_resource::<Pool<BodyMaterial>>()
        .init_resource::<Pool<PointMaterial>>()
        .add_systems(Startup, setup)
        .add_systems(Update, switch_camera)
        .add_systems(
            PostUpdate,
            (shade, sync_bodies, sync_points)
                .chain()
                .after(crate::scene::draw)
                .before(VisibilitySystems::VisibilityPropagate),
        );
    }
}

#[derive(Asset, TypePath, AsBindGroup, Clone, Default)]
pub struct BodyMaterial {
    #[uniform(0)]
    colour: Vec4,
    #[uniform(0)]
    light: Vec4,
    /// Rotation of the body's surface, as a quaternion.
    #[uniform(0)]
    spin: Vec4,
    /// Granules per disc radius, on-screen radius in pixels and granulation
    /// contrast; zero for planets.
    #[uniform(0)]
    surface: Vec4,
}

impl Material for BodyMaterial {
    fn fragment_shader() -> ShaderRef {
        BODY_SHADER.into()
    }
}

/// Fields as `Point` (`point.wgsl`), with `axis` projected onto the sprite.
#[derive(Asset, TypePath, AsBindGroup, Clone, Default, PartialEq)]
pub struct PointMaterial {
    #[uniform(0)]
    colour: Vec4,
    #[uniform(0)]
    shape: Vec4,
    #[uniform(0)]
    edge: Vec4,
    #[uniform(0)]
    streaks: Vec4,
    #[uniform(0)]
    axis: Vec4,
}

impl Material for PointMaterial {
    fn fragment_shader() -> ShaderRef {
        POINT_SHADER.into()
    }

    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Add
    }
}

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
    /// Orientation of the surface, in world axes.
    pub spin: Quat,
    /// Granules per disc radius (`granules`).
    pub granules: f64,
}

/// How a planet is lit, before exposure.
pub struct PlanetLight {
    pub centre: Vec3,
    pub radius: f32,
    pub pixels: f32,
    pub per_pixel: f32,
    /// Direction to the star, in world axes.
    pub towards_star: Vec3,
    pub flux: f64,
    pub surface: Surface,
}

/// One frame's realistic bodies, gathered while drawing the wireframes.
#[derive(Resource, Default)]
pub struct Lights {
    pub stars: Vec<StarLight>,
    pub planets: Vec<PlanetLight>,
    /// Render position of the camera.
    pub eye: Vec3,
    /// Temperature of the star lighting the planets.
    pub kelvin: f64,
    /// Streak visibility, from zero at the edge of the body range to one well
    /// inside it, so streaks never appear suddenly as bodies take over.
    pub streaks: f32,
}

impl Lights {
    pub fn clear(&mut self) {
        self.stars.clear();
        self.planets.clear();
        self.streaks = 0.0;
    }
}

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
fn balance(key: f64, target: f64, amount: f64) -> f64 {
    if key <= target {
        return 1.0;
    }
    (target / key).powf(amount.clamp(0.0, 1.0))
}

/// Exposed colour for f32 uniforms, preserving hue at the safety limit.
fn hdr_colour(colour: DVec3, value: f64) -> Vec3 {
    (colour * value.min(MAX_VALUE / colour.max_element().max(1.0))).as_vec3()
}

fn disc_area(pixels: f32) -> f64 {
    std::f64::consts::PI * f64::from(pixels).powi(2)
}

/// Radiance of a point's light spread over a point-sized disc.
fn spread(radiance: f64, pixels: f32) -> f64 {
    radiance * disc_area(pixels) / disc_area(POINT_PIXELS)
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
fn granulation_contrast(kelvin: f64) -> f32 {
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
fn streaks(colour: DVec3, peak: f64, visible: f32) -> Vec4 {
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
fn gaussian_reach(peak: f32, sigma: f32) -> f32 {
    sigma * (2.0 * (peak / SPRITE_FLOOR).max(1.0).ln()).sqrt()
}

/// Disc radii beyond which a corona of `scale` (`point.wgsl` `corona`) is
/// invisible.
fn corona_reach(scale: f32) -> f32 {
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
fn sprite_half(reach: f32, least: f32) -> f32 {
    (reach / 0.6).max(least).min(SPRITE_MAX_PIXELS.max(least))
}

impl Point {
    /// A body below `POINT_PIXELS`: its light spread by the point spread,
    /// widened by its disc (a uniform disc's variance is a quarter of its
    /// squared radius), so it matches the blurred disc it becomes.
    fn new(
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
    fn rim(
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
    fn material(&self, facing: Quat) -> PointMaterial {
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
        gain: crate::exposure::gain(stops),
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

#[derive(Component, Clone, Copy)]
struct Body;

#[derive(Component, Clone, Copy)]
struct PointSprite;

#[derive(Resource)]
struct Meshes {
    sphere: Handle<Mesh>,
    square: Handle<Mesh>,
}

/// Reused entities, grown on demand and hidden when unused.
#[derive(Resource)]
struct Pool<M: Material>(Vec<(Entity, Handle<M>)>);

impl<M: Material> Default for Pool<M> {
    fn default() -> Self {
        Self(Vec::new())
    }
}

fn setup(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>) {
    commands.insert_resource(Meshes {
        sphere: meshes.add(Sphere::new(1.0).mesh().uv(48, 24)),
        square: meshes.add(Rectangle::new(2.0, 2.0)),
    });
}

/// HDR, bloom and a black sky only while realistic, so the wireframe look is
/// unchanged.
/// Cameras sharing a window must agree on HDR, or the overlay presents a stale
/// frame, so the egui camera switches too, without tone mapping.
fn switch_camera(
    mut commands: Commands,
    nav: Res<Navigation>,
    mut clear: ResMut<ClearColor>,
    debug: Res<crate::render_debug::RenderDebug>,
    space: Query<(Entity, Has<Hdr>, Has<Bloom>), With<SpaceCamera>>,
    overlay: Query<Entity, With<bevy_egui::PrimaryEguiContext>>,
) {
    let Ok((entity, hdr, bloom)) = space.single() else {
        return;
    };
    let wants_bloom = nav.realistic && debug.bloom;
    if bloom != wants_bloom {
        if wants_bloom {
            commands.entity(entity).insert(Bloom::NATURAL);
        } else {
            commands.entity(entity).remove::<Bloom>();
        }
    }
    if nav.realistic == hdr {
        return;
    }
    clear.0 = if nav.realistic {
        Color::BLACK
    } else {
        SPACE_BLUE
    };
    for overlay in &overlay {
        if nav.realistic {
            commands.entity(overlay).insert((Hdr, Tonemapping::None));
        } else {
            commands.entity(overlay).remove::<Hdr>();
        }
    }
    if nav.realistic {
        commands.entity(entity).insert((Hdr, Tonemapping::None));
    } else {
        commands.entity(entity).remove::<Hdr>();
    }
}

fn shade(
    nav: Res<Navigation>,
    lights: Res<Lights>,
    photometry: Res<Photometry>,
    mut frame: ResMut<Frame>,
    camera: Query<&Camera, With<SpaceCamera>>,
) {
    // Physical pixels per logical pixel: the point spread is set in physical ones.
    let scale = camera
        .single()
        .ok()
        .and_then(|c| Some(c.physical_viewport_size()?.x as f32 / c.logical_viewport_size()?.x))
        .filter(|s| s.is_finite() && *s > 0.0)
        .unwrap_or(1.0);
    *frame = if nav.realistic {
        resolve(
            &lights,
            &photometry,
            nav.exposure,
            nav.compression,
            PSF_SIGMA_PX / scale,
        )
    } else {
        Frame::default()
    };
}

type Placement = (
    &'static mut Transform,
    &'static mut GlobalTransform,
    &'static mut CellCoord,
    &'static mut Visibility,
);

type SpaceCameraCell = (
    &'static CellCoord,
    &'static ChildOf,
    &'static GlobalTransform,
);

/// Grows a pool to `count` hidden entities in the camera's grid.
fn grow<M: Material + Default>(
    commands: &mut Commands,
    pool: &mut Pool<M>,
    materials: &mut Assets<M>,
    mesh: &Handle<Mesh>,
    count: usize,
    (cell, parent): (&CellCoord, &ChildOf),
    marker: impl Component + Copy,
) {
    if pool.0.len() < count {
        // The grid has no visibility of its own, which would hide its children.
        commands
            .entity(parent.parent())
            .insert_if_new(Visibility::default());
    }
    while pool.0.len() < count {
        let material = materials.add(M::default());
        let entity = commands
            .spawn((
                marker,
                Mesh3d(mesh.clone()),
                MeshMaterial3d(material.clone()),
                Transform::default(),
                *cell,
                Visibility::Hidden,
                NoFrustumCulling,
                ChildOf(parent.parent()),
            ))
            .id();
        pool.0.push((entity, material));
    }
}

/// Shows an entity in the camera's cell: the local translation is the render
/// position. Sets the global transform too, because propagation has run.
fn place(
    (mut transform, mut global, mut cell, mut visibility): (
        Mut<Transform>,
        Mut<GlobalTransform>,
        Mut<CellCoord>,
        Mut<Visibility>,
    ),
    camera: CellCoord,
    placed: Transform,
) {
    *cell = camera;
    *transform = placed;
    *global = GlobalTransform::from(placed);
    *visibility = Visibility::Inherited;
}

fn hide(mut visibility: Mut<Visibility>) {
    if *visibility != Visibility::Hidden {
        *visibility = Visibility::Hidden;
    }
}

fn sync_bodies(
    mut commands: Commands,
    frame: Res<Frame>,
    meshes: Res<Meshes>,
    mut pool: ResMut<Pool<BodyMaterial>>,
    mut materials: ResMut<Assets<BodyMaterial>>,
    camera: Query<SpaceCameraCell, With<SpaceCamera>>,
    mut bodies: Query<Placement, (With<Body>, Without<SpaceCamera>)>,
) {
    let Ok((cell, parent, _)) = camera.single() else {
        return;
    };
    let count = frame.bodies.len();
    grow(
        &mut commands,
        &mut pool,
        &mut materials,
        &meshes.sphere,
        count,
        (cell, parent),
        Body,
    );
    for (index, (entity, material)) in pool.0.iter().enumerate() {
        let Ok((transform, global, body_cell, visibility)) = bodies.get_mut(*entity) else {
            continue;
        };
        let Some(body) = frame.bodies.get(index) else {
            hide(visibility);
            continue;
        };
        place(
            (transform, global, body_cell, visibility),
            *cell,
            Transform::from_translation(body.centre).with_scale(Vec3::splat(body.radius)),
        );
        // Only touch changed materials: every change re-uploads the bind group.
        let changed = BodyMaterial {
            colour: body.colour.extend(1.0),
            light: body.light,
            spin: Vec4::from(body.rotation),
            surface: body.surface,
        };
        if materials.get(material).is_some_and(|m| {
            (m.colour, m.light, m.spin, m.surface)
                != (changed.colour, changed.light, changed.spin, changed.surface)
        }) && let Some(m) = materials.get_mut(material)
        {
            *m = changed;
        }
    }
}

fn sync_points(
    mut commands: Commands,
    frame: Res<Frame>,
    meshes: Res<Meshes>,
    mut pool: ResMut<Pool<PointMaterial>>,
    mut materials: ResMut<Assets<PointMaterial>>,
    camera: Query<SpaceCameraCell, With<SpaceCamera>>,
    mut points: Query<Placement, (With<PointSprite>, Without<SpaceCamera>)>,
) {
    let Ok((cell, parent, view)) = camera.single() else {
        return;
    };
    let count = frame.rims.len() + frame.points.len();
    grow(
        &mut commands,
        &mut pool,
        &mut materials,
        &meshes.square,
        count,
        (cell, parent),
        PointSprite,
    );
    // Sprites face the camera: they share its rotation.
    let facing = view.rotation();
    for (index, (entity, material)) in pool.0.iter().enumerate() {
        let Ok((transform, global, point_cell, visibility)) = points.get_mut(*entity) else {
            continue;
        };
        // Rims first: points come and go as bodies cross `POINT_PIXELS`, and a
        // sprite moved to another pooled entity shows its old material for a frame.
        let Some(point) = frame.rims.iter().chain(&frame.points).nth(index) else {
            hide(visibility);
            continue;
        };
        place(
            (transform, global, point_cell, visibility),
            *cell,
            Transform::from_translation(point.centre)
                .with_rotation(facing)
                .with_scale(Vec3::splat(point.half)),
        );
        let changed = point.material(facing);
        if materials.get(material).is_some_and(|m| *m != changed)
            && let Some(m) = materials.get_mut(material)
        {
            *m = changed;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use space_simulation::AU_METRES;

    fn star(radiance: f64, pixels: f32) -> StarLight {
        StarLight {
            centre: Vec3::ZERO,
            radius: 1.0,
            pixels,
            per_pixel: 1.0,
            radiance,
            kelvin: 5772.0,
            spin: Quat::IDENTITY,
            granules: 700.0,
        }
    }

    fn planet(flux: f64, pixels: f32) -> PlanetLight {
        PlanetLight {
            centre: Vec3::X * 10.0,
            radius: 1.0,
            pixels,
            per_pixel: 1.0,
            towards_star: -Vec3::X,
            flux,
            surface: light::surface("Rocky planet"),
        }
    }

    /// The Sun as a disc, two sky stars as points, and planets at 1 and 10 AU
    /// as discs, then two a hundred times apart in light as points, seen full.
    fn lights() -> Lights {
        let sun = light::flux(1.0, AU_METRES);
        Lights {
            stars: vec![star(2.0e7, 20.0), star(1e7, 1e-6), star(1e5, 1e-6)],
            planets: vec![
                planet(sun, 10.0),
                planet(sun / 100.0, 10.0),
                planet(10.0 * sun, 1.4),
                planet(sun / 10.0, 1.4),
            ],
            eye: Vec3::ZERO,
            kelvin: 5772.0,
            streaks: 1.0,
        }
    }

    fn brightness(body: &Shaded) -> f32 {
        body.colour.max_element()
    }

    impl Point {
        /// Whole light of the point, before colour.
        fn light(&self) -> f32 {
            self.colour.max_element()
        }
    }

    #[test]
    fn manual_exposure_preserves_physical_brightness_ratios() {
        let photometry = Photometry::default();
        let physical = resolve(&lights(), &photometry, -30.0, 0.0, PSF_SIGMA_PX);
        // Earth at 1 AU against a planet at 10 AU: 100 times dimmer,
        // as discs and as points alike, before shared display compression.
        let discs = |f: &Frame| brightness(&f.bodies[1]) / brightness(&f.bodies[2]);
        let points = |f: &Frame| f.points[2].light() / f.points[3].light();
        assert!((discs(&physical) - 100.0).abs() < 0.1);
        assert!((points(&physical) - 100.0).abs() < 0.1);
        // The Sun is far brighter than its planets either way, and stays in range.
        assert!(brightness(&physical.bodies[0]) > 1_000.0 * brightness(&physical.bodies[1]));
        assert!(
            physical
                .bodies
                .iter()
                .all(|b| b.colour.max_element() <= MAX_VALUE as f32)
        );
    }

    #[test]
    fn manual_exposure_has_no_brightness_floors() {
        let photometry = Photometry::default();
        let mut dim = lights();
        dim.planets[2].flux = 0.0;
        dim.planets[0].flux = 0.0;
        let dim = resolve(&dim, &photometry, -30.0, 0.0, PSF_SIGMA_PX);
        assert_eq!(dim.points[2].light(), 0.0);
        assert_eq!(brightness(&dim.bodies[1]), 0.0);
        assert_eq!(dim.bodies[1].light.w, 0.0);
    }

    #[test]
    fn a_point_carries_the_light_of_the_disc_it_replaces() {
        let exposure = Exposure { gain: 1.0 / 3.0 };
        let disc = exposure.value(5.0) * disc_area(POINT_PIXELS);
        assert!((exposure.point(5.0, POINT_PIXELS) - disc).abs() < 1e-9);
        // Physical points stay linear in their area, down to a speck.
        let physical = Exposure { gain: 1.0 };
        let ratio = physical.point(1.0, 0.1) / physical.point(1.0, 0.01);
        assert!((ratio - 100.0).abs() < 1e-3);
    }

    #[test]
    fn close_star_discs_do_not_change_exposure() {
        let photometry = Photometry::default();
        let mut near = lights();
        near.stars[0].pixels = 400.0;
        let close = resolve(&near, &photometry, -40.0, 0.0, PSF_SIGMA_PX);
        let far = resolve(&lights(), &photometry, -40.0, 0.0, PSF_SIGMA_PX);
        assert_eq!(close.bodies[0].colour, far.bodies[0].colour);
        assert_eq!(close.bodies[1].colour, far.bodies[1].colour);
        let corona = |f: &Frame| f.rims[0].colour.max_element() / brightness(&f.bodies[0]);
        assert!((corona(&close) - CORONA_SHARE as f32).abs() < 1e-8);
        // Every disc has a blurred rim, but only stars have coronas, which
        // fade in from a point; the blurred limb is there from the start.
        assert_eq!(close.rims.len(), 3);
        assert!(close.rims.iter().skip(1).all(|r| r.colour == Vec3::ZERO));
        near.stars[0].pixels = POINT_PIXELS;
        let edge = resolve(&near, &photometry, -40.0, 0.0, PSF_SIGMA_PX);
        assert!(edge.rims[0].colour.max_element() < 1e-6);
        assert!(edge.rims[0].edge.x > 0.0);
    }

    #[test]
    fn one_stop_doubles_bodies_points_and_coronas() {
        let photometry = Photometry::default();
        let a = resolve(&lights(), &photometry, -40.0, 0.0, PSF_SIGMA_PX);
        let b = resolve(&lights(), &photometry, -39.0, 0.0, PSF_SIGMA_PX);
        for (a, b) in a.bodies.iter().zip(&b.bodies) {
            assert_eq!(b.colour, a.colour * 2.0);
        }
        for (a, b) in a
            .points
            .iter()
            .chain(&a.rims)
            .zip(b.points.iter().chain(&b.rims))
        {
            assert_eq!(b.colour, a.colour * 2.0);
        }
    }

    #[test]
    fn compression_moves_each_body_towards_its_own_exposure() {
        let photometry = Photometry::default();
        let ratio = |f: &Frame| brightness(&f.bodies[1]) / brightness(&f.bodies[2]);
        // At galaxy exposure both planets are far over white: half compression
        // keeps half their 100x (6.6 stops) difference, full keeps none.
        let half = resolve(&lights(), &photometry, 0.0, 0.5, PSF_SIGMA_PX);
        assert!((ratio(&half) - 10.0).abs() < 0.01);
        let full = resolve(&lights(), &photometry, 0.0, 1.0, PSF_SIGMA_PX);
        assert!((ratio(&full) - 1.0).abs() < 1e-4);
        // Each shows at its key: the Sun's surface and Earth's sub-stellar point.
        let sun = DVec3::from_array(photometry.colour(5772.0)).max_element();
        assert!((f64::from(brightness(&full.bodies[0])) / sun - STAR_KEY).abs() < 1e-4);
        let earth = lights().planets[0].surface.tint;
        let earth =
            (DVec3::from_array(photometry.colour(5772.0)) * DVec3::from_array(earth)).max_element();
        assert!((f64::from(brightness(&full.bodies[1])) / earth - PLANET_KEY).abs() < 1e-4);
        // Points balance alike, and the corona keeps its share of the disc.
        assert!((full.points[2].light() / full.points[3].light() - 1.0).abs() < 1e-4);
        let corona = full.rims[0].colour.max_element() / brightness(&full.bodies[0]);
        assert!((corona - CORONA_SHARE as f32).abs() < 1e-8);
    }

    #[test]
    fn compression_never_brightens_dim_bodies() {
        let photometry = Photometry::default();
        let dim = resolve(&lights(), &photometry, -60.0, 0.0, PSF_SIGMA_PX);
        let full = resolve(&lights(), &photometry, -60.0, 1.0, PSF_SIGMA_PX);
        assert_eq!(dim.bodies, full.bodies);
        assert_eq!(dim.points, full.points);
        assert_eq!(balance(0.0, PLANET_KEY, 1.0), 1.0);
    }

    /// Complementary error function, as `point.wgsl`.
    fn erfc(x: f32) -> f32 {
        let t = 1.0 / (1.0 + 0.327_591_1 * x);
        let poly = t
            * (0.254_829_6
                + t * (-0.284_496_7 + t * (1.421_413_7 + t * (-1.453_152 + t * 1.061_405_4))));
        poly * (-x * x).exp()
    }

    /// Radius in pixels out to which a sprite shows white, by bisection.
    fn white_radius(light: impl Fn(f32) -> f32) -> f32 {
        let (mut low, mut high) = (0.0f32, 1000.0f32);
        for _ in 0..60 {
            let mid = 0.5 * (low + high);
            if light(mid) >= 1.0 {
                low = mid;
            } else {
                high = mid;
            }
        }
        low
    }

    #[test]
    fn an_overexposed_point_matches_the_blurred_disc_it_becomes() {
        // A star 2^12 over white at the switch: before, its point spread white
        // to about 9 pixels, then the disc showed only its 1.5.
        let value = 2f64.powi(12);
        let colour = DVec3::ONE;
        let psf = PSF_SIGMA_PX;
        let pixels = POINT_PIXELS;
        let point = Point::new(
            Vec3::ZERO,
            pixels - 1e-3,
            1.0,
            colour,
            value * f64::from(PI * pixels * pixels),
            psf,
            0.0,
        );
        let sigma = point.shape.y;
        let as_point = white_radius(|r| {
            point.colour.x * (-r * r / (2.0 * sigma * sigma)).exp()
                / (std::f32::consts::TAU * sigma * sigma)
        });
        // The disc is white inside its limb; its blurred limb continues outside.
        let limb = (value * LIMB_EDGE.max_element()) as f32;
        let as_disc = pixels.max(white_radius(|r| {
            limb * 0.5 * erfc((r - pixels).max(0.0) / (psf * std::f32::consts::SQRT_2))
        }));
        assert!((as_point - as_disc).abs() < 1.0, "{as_point} vs {as_disc}");
    }

    #[test]
    fn sprites_reach_until_their_light_is_invisible() {
        let rim = |pixels: f32, value: f64, corona: f64| {
            Point::rim(
                Vec3::ZERO,
                pixels,
                1.0,
                PSF_SIGMA_PX,
                DVec3::splat(value),
                DVec3::splat(corona),
                Vec4::ZERO,
                Vec3::Y,
                true,
                0.0,
            )
        };
        // At extreme brightness the cap applies, its fade far off-screen.
        assert_eq!(rim(50.0, 1e12, 1e6).shape.x, SPRITE_MAX_PIXELS);
        for corona in [1e-3, 1.0, 1e2] {
            let sprite = rim(50.0, corona * 1e6, corona);
            let x = 0.6 * sprite.shape.x / 50.0;
            let bound = corona as f32
                * (0.0532 * CORONA_OUTER_MAX * x.powf(-2.5)
                    + 1.425 * CORONA_INNER_MAX * x.powi(-7)
                    + 2.565 * CORONA_RAYS_MAX * x.powi(-17));
            assert!(bound <= SPRITE_FLOOR * 1.01, "{corona}: {bound}");
            assert!(sprite.shape.x >= CORONA_EXTENT * 50.0);
        }
        for light in [1.0, 1e4, 1e9] {
            let point = Point::new(Vec3::ZERO, 1.0, 1.0, DVec3::ONE, light, PSF_SIGMA_PX, 0.0);
            let (r, sigma) = (0.6 * point.shape.x, point.shape.y);
            let edge = point.colour.x * (-r * r / (2.0 * sigma * sigma)).exp()
                / (std::f32::consts::TAU * sigma * sigma);
            assert!(edge <= SPRITE_FLOOR * 1.001, "{light}: {edge}");
        }
    }

    #[test]
    fn streaks_grow_with_stops_over_white_and_fade_with_visibility() {
        let colour = DVec3::new(1.0, 0.5, 0.2);
        assert_eq!(
            streaks(colour, 2f64.powf(f64::from(STREAK_START)), 1.0),
            Vec4::ZERO
        );
        let short = streaks(colour, 2f64.powi(8), 1.0);
        let long = streaks(colour, 2f64.powi(20), 1.0);
        assert!(short.w > 0.0 && long.w > short.w);
        assert!(long.w <= STREAK_MAX_PX);
        assert!(streaks(colour, 2f64.powi(200), 1.0).w <= STREAK_MAX_PX);
        // Whiter than the source, never brighter than full strength.
        assert!(long.z / long.x > 0.2 && long.x <= STREAK_GAIN);
        assert_eq!(streaks(colour, 2f64.powi(20), 0.0), Vec4::ZERO);
        // Sprites reach past their streaks.
        let point = Point::new(Vec3::ZERO, 0.5, 1.0, colour, 1e9, PSF_SIGMA_PX, 1.0);
        assert!(0.6 * point.shape.x >= point.streaks.w);
    }

    #[test]
    fn hdr_safety_cap_keeps_extreme_manual_values_finite() {
        let mut lights = lights();
        lights.stars[0].kelvin = 50000.0;
        let frame = resolve(&lights, &Photometry::default(), 16.0, 0.0, PSF_SIGMA_PX);
        assert!(
            frame
                .bodies
                .iter()
                .all(|b| b.colour.is_finite() && b.colour.max_element() <= MAX_VALUE as f32)
        );
        assert!(
            frame
                .points
                .iter()
                .chain(&frame.rims)
                .all(|p| p.colour.is_finite() && p.colour.max_element() <= MAX_VALUE as f32)
        );
    }

    #[test]
    fn granules_follow_the_pressure_scale_height() {
        let star = |mass_solar, radius_solar, temperature_k| Star {
            mass_solar,
            radius_solar,
            temperature_k,
            ..space_simulation::demo_galaxy().systems[0].stars[0].clone()
        };
        let sun = granules(&star(1.0, 1.0, 5772.0));
        assert!((sun - 700.0).abs() < 1.0);
        // A red giant shows a handful of huge cells, a red dwarf finer ones.
        assert!(granules(&star(1.0, 50.0, 4000.0)) < 30.0);
        assert!(granules(&star(0.13, 0.16, 2800.0)) > sun);
        assert!(granulation_contrast(5772.0) > 0.99);
        assert!(granulation_contrast(9000.0) < 1e-6);
    }

    #[test]
    fn crescents_are_dimmer_than_full_discs() {
        assert!((phase(0.0) - 2.0 / 3.0).abs() < 1e-9);
        assert!(phase(PI / 2.0) < phase(0.0) / 2.0);
        assert!(phase(PI).abs() < 1e-9);
    }
}
