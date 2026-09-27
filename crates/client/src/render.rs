//! Realistic rendering: an HDR camera with bloom, exposure worked out from the
//! photometry, and plain shaded spheres under the wireframes.
//!
//! - Physical exposure is one linear exposure for the whole frame, as a camera
//!   would take it, so planets all but vanish beside their star.
//! - Assisted exposure, the default, compresses brightness around the subject,
//!   exposes the star field on its own, keeps faint points above a floor and
//!   shows star colours more saturated.
//! - Bodies smaller than a point on screen are drawn as point sprites: a sharp
//!   core and a faint halo carrying the body's whole light, so distant stars
//!   keep their true relative brightness.
//! - Close up, a star is a disc with limb darkening, granulation and a corona.
//!   Assisted exposure eases large discs down so their surface stays readable.
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
use space_shared::{Star, StarSystem};
use space_simulation::{
    AU_METRES,
    light::{self, Surface},
};
use std::{collections::HashMap, f32::consts::PI, sync::Mutex};

const BODY_SHADER: Handle<Shader> = uuid_handle!("6f1f3c47-2a8e-4d61-9b53-4f8e0d7a91c2");
const POINT_SHADER: Handle<Shader> = uuid_handle!("0c9b5d2e-7a41-4e8f-a3d6-5b2f81c4e970");

/// Display value of the subject's key: a lit planet at the reference distance.
const KEY: f64 = 1.0;
/// Display value of the brightest star in the sky when it is exposed alone,
/// spread over a point-sized disc.
const SKY_KEY: f64 = 1.5;
/// Keeps HDR values well inside half-float range.
const MAX_VALUE: f64 = 20_000.0;
/// Albedo of the reference surface the subject exposure is metered on.
const REFERENCE_ALBEDO: f64 = 0.3;
/// Assisted exposure raises brightness ratios to this power: 0.5 halves stops.
const ASSISTED_CONTRAST: f64 = 0.5;
const ASSISTED_SATURATION: f64 = 2.5;
const ASSISTED_PLANET_FLOOR: f64 = 0.08;
const ASSISTED_STAR_FLOOR: f64 = 0.05;
const ASSISTED_AMBIENT: f32 = 0.03;
/// Least light of an assisted point, in display value × square pixels, so faint
/// stars stay findable and faint planets visible.
const ASSISTED_STAR_POINT_FLOOR: f64 = 1.0;
const ASSISTED_PLANET_POINT_FLOOR: f64 = 1.5;
/// Bodies below this on-screen radius, in logical pixels, are drawn as points.
pub const POINT_PIXELS: f32 = 1.5;
/// Point spread in logical pixels: a sharp core and a faint halo. Bloom adds the
/// wide glow of bright points.
const CORE_SIGMA: f32 = 0.8;
const HALO_SIGMA: f32 = 3.0;
const HALO_SHARE: f32 = 0.12;
/// Assisted exposure eases star discs between these on-screen radii down to
/// `DISC_KEY`, so their limb and granulation are not lost to white. Physical
/// exposure adapts the whole frame over the same range instead, as a camera
/// metering on the disc would.
const DISC_NEAR: f32 = 12.0;
const DISC_FAR: f32 = 160.0;
const DISC_KEY: f64 = 0.8;
/// Corona brightness just outside the limb, relative to the disc: the real
/// inner corona is about a millionth, so a physical camera shows only bloom.
/// Assisted, it is relative to the disc eased to `DISC_KEY`, however far away,
/// or a glaring disc would blow it out to its whole sprite.
const CORONA_SHARE: f64 = 0.2;
const PHYSICAL_CORONA_SHARE: f64 = 1e-6;
/// Corona sprite half side and falloff, in disc radii.
const CORONA_EXTENT: f32 = 4.0;
const CORONA_FALLOFF: f32 = 3.0;

pub struct RenderPlugin;

impl Plugin for RenderPlugin {
    fn build(&self, app: &mut App) {
        load_internal_asset!(app, BODY_SHADER, "body.wgsl", Shader::from_wgsl);
        load_internal_asset!(app, POINT_SHADER, "point.wgsl", Shader::from_wgsl);
        app.add_plugins((
            MaterialPlugin::<BodyMaterial>::default(),
            MaterialPlugin::<PointMaterial>::default(),
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

#[derive(Asset, TypePath, AsBindGroup, Clone, Default)]
pub struct PointMaterial {
    #[uniform(0)]
    colour: Vec4,
    #[uniform(0)]
    shape: Vec4,
    /// Disc radius in pixels and falloff for a corona; zero for a point.
    #[uniform(0)]
    corona: Vec4,
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
    /// The current page's star while its planets are on screen.
    pub subject: bool,
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
    /// Radiance metered for the planets, when they are on screen.
    pub subject_key: Option<f64>,
    /// Distance from the star the planets are metered at, and whether it is the
    /// followed planet's.
    pub metered: Option<(f64, bool)>,
    /// Temperature of the star lighting the planets.
    pub kelvin: f64,
}

impl Lights {
    pub fn clear(&mut self) {
        self.stars.clear();
        self.planets.clear();
        self.subject_key = None;
        self.metered = None;
    }
}

/// The camera's exposure in words, for the caption under the 3D view.
pub fn exposure_caption(nav: &Navigation, lights: &Lights) -> String {
    if !nav.realistic {
        return "Exposure off / wireframe".into();
    }
    let mode = if nav.physical { "physical" } else { "assisted" };
    let metering = match lights.metered {
        Some((metres, true)) => format!("followed planet at {:.3} AU", metres / AU_METRES),
        Some((metres, false)) => format!("typical orbit at {:.3} AU", metres / AU_METRES),
        None => "brightest star".into(),
    };
    let adapted = nav.physical && lights.stars.iter().any(|s| closeness(s.pixels) > 0.0);
    let adapted = if adapted {
        " / adapting to star disc"
    } else {
        ""
    };
    format!("Exposure {mode} / metered on {metering}{adapted}")
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Exposure {
    pub key: f64,
    pub target: f64,
    pub physical: bool,
}

impl Exposure {
    /// Display value of a surface.
    pub fn value(&self, radiance: f64) -> f64 {
        let ratio = (radiance / self.key).max(0.0);
        let ratio = if self.physical {
            ratio
        } else {
            ratio.powf(ASSISTED_CONTRAST)
        };
        (self.target * ratio).min(MAX_VALUE)
    }

    /// Physical exposure stopped down as a star disc fills the view, until the
    /// disc shows at `DISC_KEY`. It stays one linear exposure for the frame.
    fn adapt(self, stars: &[StarLight]) -> Self {
        let key = stars
            .iter()
            .map(|s| {
                let close = closeness(s.pixels);
                let disc = s.radiance * self.target / DISC_KEY;
                (self.key.ln() * (1.0 - close) + disc.ln() * close).exp()
            })
            .fold(self.key, f64::max);
        Self { key, ..self }
    }

    /// Light of a point, in display value × square pixels, exposed as if spread
    /// over a point-sized disc. It matches the disc it replaces at the switch,
    /// and a physical exposure stays linear.
    pub fn point(&self, radiance: f64, pixels: f32) -> f64 {
        let switch = disc_area(POINT_PIXELS);
        self.value(spread(radiance, pixels)) * switch
    }
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

    pub fn colour(&self, kelvin: f64, physical: bool) -> [f64; 3] {
        let colour = self.lookup(kelvin).0;
        if physical {
            colour
        } else {
            light::saturate(colour, ASSISTED_SATURATION)
        }
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

/// Distance the planets are metered at: the followed planet, else the typical
/// orbit on the page, else where the star gives Earth's sunlight.
pub fn reference_distance(
    system: &StarSystem,
    page: u32,
    followed: Option<f64>,
    visual_luminosity: f64,
) -> f64 {
    if let Some(distance) = followed {
        return distance;
    }
    let orbits: Vec<f64> = system
        .on_page(page)
        .map(|p| p.orbit.semi_major_au.max(1e-3).ln())
        .collect();
    if orbits.is_empty() {
        visual_luminosity.sqrt() * AU_METRES
    } else {
        (orbits.iter().sum::<f64>() / orbits.len() as f64).exp() * AU_METRES
    }
}

pub fn subject_key(visual_luminosity: f64, reference_metres: f64) -> f64 {
    light::reflected_radiance(
        REFERENCE_ALBEDO,
        light::flux(visual_luminosity, reference_metres),
    )
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

/// A point sprite ready for the GPU.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Point {
    pub centre: Vec3,
    /// Half the sprite's side, in render units.
    pub half: f32,
    /// Colour times the point's whole light, in display value × square pixels.
    pub colour: Vec3,
    /// Half side in pixels, core and halo spread in pixels, and the halo's share.
    pub shape: Vec4,
    /// Disc radius in pixels and falloff for a corona; zero for a point.
    pub corona: Vec4,
}

impl Point {
    fn new(centre: Vec3, pixels: f32, per_pixel: f32, colour: DVec3, light: f64) -> Self {
        // A point about to become a disc spreads as wide as the disc.
        let core = CORE_SIGMA.max(0.7 * pixels);
        let half = 4.0 * core.max(HALO_SIGMA);
        Self {
            centre,
            half: half * per_pixel,
            colour: (colour * light.min(MAX_VALUE)).as_vec3(),
            shape: Vec4::new(half, core, HALO_SIGMA, HALO_SHARE),
            corona: Vec4::ZERO,
        }
    }

    /// A glow around a star's disc, `colour` its brightness just outside the limb.
    fn corona(centre: Vec3, pixels: f32, per_pixel: f32, colour: DVec3) -> Self {
        let half = CORONA_EXTENT * pixels;
        Self {
            centre,
            half: half * per_pixel,
            colour: colour.min(DVec3::splat(MAX_VALUE)).as_vec3(),
            shape: Vec4::new(half, 0.0, 0.0, 0.0),
            corona: Vec4::new(pixels, CORONA_FALLOFF, 0.0, 0.0),
        }
    }
}

/// How far a disc of this on-screen radius has grown from `DISC_NEAR` to
/// `DISC_FAR`, eased.
fn closeness(pixels: f32) -> f64 {
    let t = ((pixels - DISC_NEAR) / (DISC_FAR - DISC_NEAR)).clamp(0.0, 1.0);
    f64::from(t * t * (3.0 - 2.0 * t))
}

/// Assisted display value of a star disc, eased down as it grows on screen.
fn ease_disc(value: f64, pixels: f32) -> f64 {
    let close = closeness(pixels);
    if value > DISC_KEY {
        (value.ln() * (1.0 - close) + DISC_KEY.ln() * close).exp()
    } else {
        value
    }
}

#[derive(Resource, Default)]
pub struct Frame {
    pub bodies: Vec<Shaded>,
    pub points: Vec<Point>,
    pub coronas: Vec<Point>,
}

pub fn resolve(lights: &Lights, photometry: &Photometry, physical: bool) -> Frame {
    // A physical frame has one exposure; otherwise the sky gets its own, keyed
    // on its brightest point.
    let sky = if physical && lights.subject_key.is_some() {
        0.0
    } else {
        lights
            .stars
            .iter()
            .filter(|s| !s.subject)
            .map(|s| {
                if s.pixels < POINT_PIXELS {
                    spread(s.radiance, s.pixels)
                } else {
                    s.radiance
                }
            })
            .fold(0.0, f64::max)
    };
    let subject = lights.subject_key.map(|key| Exposure {
        key,
        target: KEY,
        physical,
    });
    let mut sky = Exposure {
        key: if sky > 0.0 { sky } else { 1.0 },
        target: SKY_KEY,
        physical,
    };
    let mut subject = subject;
    if physical {
        sky = sky.adapt(&lights.stars);
        subject = subject.map(|s| s.adapt(&lights.stars));
    }
    let mut frame = Frame::default();
    for star in &lights.stars {
        let exposure = match subject {
            Some(subject) if star.subject || physical => subject,
            _ => sky,
        };
        let colour = DVec3::from_array(photometry.colour(star.kelvin, physical));
        if star.pixels < POINT_PIXELS {
            let mut light = exposure.point(star.radiance, star.pixels);
            if !physical {
                light = light.max(ASSISTED_STAR_POINT_FLOOR);
            }
            frame.points.push(Point::new(
                star.centre,
                star.pixels,
                star.per_pixel,
                colour,
                light,
            ));
            continue;
        }
        let mut value = exposure.value(star.radiance);
        if !physical {
            value = ease_disc(value.max(ASSISTED_STAR_FLOOR), star.pixels);
        }
        frame.bodies.push(Shaded {
            centre: star.centre,
            radius: star.radius,
            colour: (colour * value).as_vec3(),
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
        let share = if physical {
            PHYSICAL_CORONA_SHARE
        } else {
            CORONA_SHARE
        };
        let fade = f64::from(((star.pixels / POINT_PIXELS - 1.0) / 3.0).clamp(0.0, 1.0));
        let pearl = (colour + DVec3::splat(colour.max_element())) * 0.5;
        frame.coronas.push(Point::corona(
            star.centre,
            star.pixels,
            star.per_pixel,
            pearl * if physical { value } else { value.min(DISC_KEY) } * share * fade,
        ));
    }
    let Some(subject) = subject else {
        return frame;
    };
    let tint = DVec3::from_array(photometry.colour(lights.kelvin, physical));
    for planet in &lights.planets {
        let radiance = light::reflected_radiance(planet.surface.albedo, planet.flux);
        let colour = tint * DVec3::from_array(planet.surface.tint);
        if planet.pixels < POINT_PIXELS {
            let towards_eye = (lights.eye - planet.centre).normalize_or_zero();
            let angle = planet.towards_star.angle_between(towards_eye);
            let mut light = subject.point(radiance * phase(angle), planet.pixels);
            if !physical {
                light = light.max(ASSISTED_PLANET_POINT_FLOOR);
            }
            frame.points.push(Point::new(
                planet.centre,
                planet.pixels,
                planet.per_pixel,
                colour,
                light,
            ));
            continue;
        }
        let mut value = subject.value(radiance);
        if !physical {
            value = value.max(ASSISTED_PLANET_FLOOR);
        }
        frame.bodies.push(Shaded {
            centre: planet.centre,
            radius: planet.radius,
            colour: (colour * value).as_vec3(),
            light: planet
                .towards_star
                .extend(if physical { 0.0 } else { ASSISTED_AMBIENT }),
            rotation: Quat::IDENTITY,
            surface: Vec4::ZERO,
        });
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

/// HDR and bloom only while realistic, so the wireframe look is unchanged.
/// Cameras sharing a window must agree on HDR, or the overlay presents a stale
/// frame, so the egui camera switches too, without tone mapping.
fn switch_camera(
    mut commands: Commands,
    nav: Res<Navigation>,
    space: Query<(Entity, Has<Hdr>), With<SpaceCamera>>,
    overlay: Query<Entity, With<bevy_egui::PrimaryEguiContext>>,
) {
    let Ok((entity, hdr)) = space.single() else {
        return;
    };
    if nav.realistic == hdr {
        return;
    }
    for overlay in &overlay {
        if nav.realistic {
            commands.entity(overlay).insert((Hdr, Tonemapping::None));
        } else {
            commands.entity(overlay).remove::<Hdr>();
        }
    }
    if nav.realistic {
        commands.entity(entity).insert((Hdr, Bloom::NATURAL));
    } else {
        commands.entity(entity).remove::<(Bloom, Hdr)>();
    }
}

fn shade(
    nav: Res<Navigation>,
    lights: Res<Lights>,
    photometry: Res<Photometry>,
    mut frame: ResMut<Frame>,
) {
    *frame = if nav.realistic {
        resolve(&lights, &photometry, nav.physical)
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
    let count = frame.coronas.len() + frame.points.len();
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
        // Coronas first: points come and go as bodies cross `POINT_PIXELS`, and a
        // sprite moved to another pooled entity shows its old material for a frame.
        let Some(point) = frame.coronas.iter().chain(&frame.points).nth(index) else {
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
        let colour = point.colour.extend(1.0);
        if materials
            .get(material)
            .is_some_and(|m| (m.colour, m.shape, m.corona) != (colour, point.shape, point.corona))
            && let Some(m) = materials.get_mut(material)
        {
            m.colour = colour;
            m.shape = point.shape;
            m.corona = point.corona;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn star(radiance: f64, pixels: f32, subject: bool) -> StarLight {
        StarLight {
            centre: Vec3::ZERO,
            radius: 1.0,
            pixels,
            per_pixel: 1.0,
            radiance,
            kelvin: 5772.0,
            subject,
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
            stars: vec![
                star(2.0e7, 20.0, true),
                star(1e7, 1e-6, false),
                star(1e5, 1e-6, false),
            ],
            planets: vec![
                planet(sun, 10.0),
                planet(sun / 100.0, 10.0),
                planet(10.0 * sun, 1.4),
                planet(sun / 10.0, 1.4),
            ],
            eye: Vec3::ZERO,
            subject_key: Some(subject_key(1.0, AU_METRES)),
            metered: Some((AU_METRES, false)),
            kelvin: 5772.0,
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
    fn physical_exposure_is_linear_and_assisted_compresses() {
        let photometry = Photometry::default();
        let physical = resolve(&lights(), &photometry, true);
        let assisted = resolve(&lights(), &photometry, false);
        // Earth at 1 AU against a planet at 10 AU: 100 times dimmer, or 10 assisted,
        // as discs and as points alike.
        let discs = |f: &Frame| brightness(&f.bodies[1]) / brightness(&f.bodies[2]);
        let points = |f: &Frame| f.points[2].light() / f.points[3].light();
        assert!((discs(&physical) - 100.0).abs() < 0.1);
        assert!((discs(&assisted) - 10.0).abs() < 0.01);
        assert!((points(&physical) - 100.0).abs() < 0.1);
        assert!((points(&assisted) - 10.0).abs() < 0.01);
        // The Sun is far brighter than its planets either way, and stays in range.
        assert!(brightness(&physical.bodies[0]) > 1_000.0 * brightness(&physical.bodies[1]));
        assert!(brightness(&assisted.bodies[0]) > 50.0 * brightness(&assisted.bodies[1]));
        assert!(
            physical
                .bodies
                .iter()
                .all(|b| b.colour.max_element() <= MAX_VALUE as f32)
        );
    }

    #[test]
    fn assisted_exposure_keeps_the_sky_and_faint_points_visible() {
        let photometry = Photometry::default();
        let physical = resolve(&lights(), &photometry, true);
        let assisted = resolve(&lights(), &photometry, false);
        // A physical camera exposed for sunlit planets cannot see other stars:
        // their light is far below one step of an 8-bit display.
        assert!(physical.points[0].light() < 1e-3);
        // Assisted exposes the sky alone: the brightest star sits at the sky key,
        // and one five magnitudes fainter is still visible.
        let key = (SKY_KEY * disc_area(POINT_PIXELS)) as f32;
        assert!((assisted.points[0].light() - key).abs() < key * 1e-3);
        assert!(assisted.points[1].light() > ASSISTED_STAR_POINT_FLOOR as f32);
        let mut dim = lights();
        dim.planets[2].flux *= 1e-9;
        let dim = resolve(&dim, &photometry, false);
        assert!(dim.points[2].light() >= ASSISTED_PLANET_POINT_FLOOR as f32 * 0.99);
        assert!(
            physical
                .points
                .iter()
                .all(|p| p.light() <= MAX_VALUE as f32)
        );
    }

    #[test]
    fn a_point_carries_the_light_of_the_disc_it_replaces() {
        for physical in [true, false] {
            let exposure = Exposure {
                key: 3.0,
                target: 1.0,
                physical,
            };
            let disc = exposure.value(5.0) * disc_area(POINT_PIXELS);
            assert!((exposure.point(5.0, POINT_PIXELS) - disc).abs() < 1e-9);
        }
        // Physical points stay linear in their area, down to a speck.
        let physical = Exposure {
            key: 1.0,
            target: 1.0,
            physical: true,
        };
        let ratio = physical.point(1.0, 0.1) / physical.point(1.0, 0.01);
        assert!((ratio - 100.0).abs() < 1e-3);
    }

    #[test]
    fn close_star_discs_stay_readable_with_a_corona() {
        let photometry = Photometry::default();
        let mut near = lights();
        near.stars[0].pixels = 400.0;
        let assisted = resolve(&near, &photometry, false);
        let physical = resolve(&near, &photometry, true);
        // Assisted eases a large disc down to the disc key; physical exposure
        // adapts to it instead.
        let disc = brightness(&assisted.bodies[0]);
        assert!(disc <= DISC_KEY as f32 * 1.01 && disc > 0.5);
        let disc = brightness(&physical.bodies[0]);
        assert!(disc <= DISC_KEY as f32 * 1.01 && disc > 0.5);
        // A small disc keeps its exposure.
        assert!((ease_disc(300.0, DISC_NEAR) - 300.0).abs() < 1e-9);
        // The corona is visible assisted, and all but invisible physically.
        let corona = |f: &Frame| f.coronas[0].colour.max_element() / brightness(&f.bodies[0]);
        assert!((corona(&assisted) - CORONA_SHARE as f32).abs() < 0.05);
        assert!(corona(&physical) < 1e-5);
        // A glaring distant disc does not blow the corona out.
        near.stars[0].pixels = 30.0;
        let mid = resolve(&near, &photometry, false);
        assert!(mid.coronas[0].colour.max_element() <= (DISC_KEY * CORONA_SHARE) as f32 * 1.01);
        // Only discs have coronas, and they fade in from a point.
        assert_eq!(assisted.coronas.len(), 1);
        near.stars[0].pixels = POINT_PIXELS;
        let edge = resolve(&near, &photometry, false);
        assert!(edge.coronas[0].colour.max_element() < 1e-6);
    }

    #[test]
    fn physical_exposure_adapts_to_a_close_star_disc() {
        let photometry = Photometry::default();
        let mut near = lights();
        let far = resolve(&near, &photometry, true);
        near.stars[0].pixels = DISC_FAR;
        let close = resolve(&near, &photometry, true);
        // The disc shows its surface, and the frame stays one linear exposure.
        let disc = brightness(&close.bodies[0]);
        assert!((disc - DISC_KEY as f32).abs() < 0.01);
        let ratio = |f: &Frame| brightness(&f.bodies[1]) / brightness(&f.bodies[2]);
        assert!((ratio(&close) / ratio(&far) - 1.0).abs() < 1e-3);
        assert!(brightness(&close.bodies[1]) < brightness(&far.bodies[1]) * 1e-3);
        // A dim disc is never brightened.
        near.stars[0].radiance = 1e-9;
        let dim = resolve(&near, &photometry, true);
        assert!(brightness(&dim.bodies[1]) >= brightness(&far.bodies[1]) * 0.999);
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

    #[test]
    fn metering_follows_the_page_or_the_followed_planet() {
        let system = &space_simulation::demo_galaxy().systems[0];
        let page = system.first_page();
        let typical = reference_distance(system, page, None, 1.0);
        let inner = system
            .on_page(page)
            .map(|p| p.orbit.semi_major_au)
            .fold(f64::MAX, f64::min);
        assert!(typical >= inner * AU_METRES);
        assert_eq!(reference_distance(system, page, Some(5.0), 1.0), 5.0);
        let far = subject_key(1.0, 2.0 * AU_METRES);
        assert!((subject_key(1.0, AU_METRES) / far - 4.0).abs() < 1e-9);
    }
}
