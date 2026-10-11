//! Physical photometry and manual exposure, kept unclipped until fragment lighting.
//! The shared highlight shoulder runs before half-float storage.
//! - Bodies smaller than a point on screen are drawn as point sprites: a sharp
//!   core and a faint halo carrying the body's whole light, so distant stars
//!   keep their true relative brightness.
//! - Close up, a star is a disc with limb darkening, granulation and a corona.
//! - Exposed radiance is carried in f32 uniforms, not written directly to HDR.

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

/// Uniform arithmetic safety, far above any generated exposed radiance. Not an
/// HDR cap: fragment lighting and compression happen before half-float storage.
const MAX_VALUE: f64 = 1e30;
/// Bodies below this on-screen radius, in logical pixels, are drawn as points.
pub const POINT_PIXELS: f32 = 1.5;
/// Point spread in logical pixels: a sharp core and a faint halo. Bloom adds the
/// wide glow of bright points.
const CORE_SIGMA: f32 = 0.8;
const HALO_SIGMA: f32 = 3.0;
const HALO_SHARE: f32 = 0.12;
/// Corona brightness just outside the limb, relative to the disc: the real
/// inner corona is about a millionth.
const CORONA_SHARE: f64 = 1e-6;
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
    /// contrast and shared compression; the first three are zero for planets.
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
    /// Disc radius and falloff for a corona (zero for a point), unused, compression.
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
}

impl Lights {
    pub fn clear(&mut self) {
        self.stars.clear();
        self.planets.clear();
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
    /// and stays linear until the fragment's highlight shoulder.
    pub fn point(&self, radiance: f64, pixels: f32) -> f64 {
        let switch = disc_area(POINT_PIXELS);
        self.value(spread(radiance, pixels)) * switch
    }
}

/// Exposed colour for f32 uniforms, preserving values far beyond half-float range.
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
    /// Granules per radius, on-screen radius, contrast and compression.
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
    fn new(
        centre: Vec3,
        pixels: f32,
        per_pixel: f32,
        colour: DVec3,
        light: f64,
        compression: f32,
    ) -> Self {
        // A point about to become a disc spreads as wide as the disc.
        let core = CORE_SIGMA.max(0.7 * pixels);
        let half = 4.0 * core.max(HALO_SIGMA);
        Self {
            centre,
            half: half * per_pixel,
            colour: hdr_colour(colour, light),
            shape: Vec4::new(half, core, HALO_SIGMA, HALO_SHARE),
            corona: Vec4::new(0.0, 0.0, 0.0, compression),
        }
    }

    /// A glow around a star's disc, `colour` its brightness just outside the limb.
    fn corona(centre: Vec3, pixels: f32, per_pixel: f32, colour: DVec3, compression: f32) -> Self {
        let half = CORONA_EXTENT * pixels;
        Self {
            centre,
            half: half * per_pixel,
            colour: hdr_colour(colour, 1.0),
            shape: Vec4::new(half, 0.0, 0.0, 0.0),
            corona: Vec4::new(pixels, CORONA_FALLOFF, 0.0, compression),
        }
    }
}

#[derive(Resource, Default)]
pub struct Frame {
    pub bodies: Vec<Shaded>,
    pub points: Vec<Point>,
    pub coronas: Vec<Point>,
}

/// Preserve exposed radiance in uniforms. The same shoulder is evaluated after
/// surface lighting in every shader, before half-float storage.
pub fn resolve(lights: &Lights, photometry: &Photometry, stops: f64, compression: f64) -> Frame {
    let exposure = Exposure {
        gain: crate::exposure::gain(stops),
    };
    let mut frame = Frame::default();
    for star in &lights.stars {
        let colour = DVec3::from_array(photometry.colour(star.kelvin));
        if star.pixels < POINT_PIXELS {
            let light = exposure.point(star.radiance, star.pixels);
            frame.points.push(Point::new(
                star.centre,
                star.pixels,
                star.per_pixel,
                colour,
                light,
                compression as f32,
            ));
            continue;
        }
        let value = exposure.value(star.radiance);
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
                compression as f32,
            ),
        });
        // The corona grows in from a point, and is whiter than the surface.
        let fade = f64::from(((star.pixels / POINT_PIXELS - 1.0) / 3.0).clamp(0.0, 1.0));
        let pearl = (colour + DVec3::splat(colour.max_element())) * 0.5;
        frame.coronas.push(Point::corona(
            star.centre,
            star.pixels,
            star.per_pixel,
            pearl * exposure.value(star.radiance * CORONA_SHARE) * fade,
            compression as f32,
        ));
    }
    let tint = DVec3::from_array(photometry.colour(lights.kelvin));
    for planet in &lights.planets {
        let radiance = light::reflected_radiance(planet.surface.albedo, planet.flux);
        let colour = tint * DVec3::from_array(planet.surface.tint);
        if planet.pixels < POINT_PIXELS {
            let towards_eye = (lights.eye - planet.centre).normalize_or_zero();
            let angle = planet.towards_star.angle_between(towards_eye);
            let light = exposure.point(radiance * phase(angle), planet.pixels);
            frame.points.push(Point::new(
                planet.centre,
                planet.pixels,
                planet.per_pixel,
                colour,
                light,
                compression as f32,
            ));
            continue;
        }
        let value = exposure.value(radiance);
        frame.bodies.push(Shaded {
            centre: planet.centre,
            radius: planet.radius,
            colour: hdr_colour(colour, value),
            light: planet.towards_star.extend(0.0),
            rotation: Quat::IDENTITY,
            surface: Vec4::new(0.0, 0.0, 0.0, compression as f32),
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
) {
    *frame = if nav.realistic {
        resolve(&lights, &photometry, nav.exposure, nav.compression)
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
        let physical = resolve(&lights(), &photometry, -30.0, 0.5);
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
        let dim = resolve(&dim, &photometry, -30.0, 0.5);
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
        let close = resolve(&near, &photometry, -40.0, 0.5);
        let far = resolve(&lights(), &photometry, -40.0, 0.5);
        assert_eq!(close.bodies[0].colour, far.bodies[0].colour);
        assert_eq!(close.bodies[1].colour, far.bodies[1].colour);
        let corona = |f: &Frame| f.coronas[0].colour.max_element() / brightness(&f.bodies[0]);
        assert!((corona(&close) - CORONA_SHARE as f32).abs() < 1e-8);
        // Only discs have coronas, and they fade in from a point.
        assert_eq!(close.coronas.len(), 1);
        near.stars[0].pixels = POINT_PIXELS;
        let edge = resolve(&near, &photometry, -40.0, 0.5);
        assert!(edge.coronas[0].colour.max_element() < 1e-6);
    }

    #[test]
    fn one_stop_doubles_bodies_points_and_coronas() {
        let photometry = Photometry::default();
        let a = resolve(&lights(), &photometry, -40.0, 0.5);
        let b = resolve(&lights(), &photometry, -39.0, 0.5);
        for (a, b) in a.bodies.iter().zip(&b.bodies) {
            assert_eq!(b.colour, a.colour * 2.0);
        }
        for (a, b) in a
            .points
            .iter()
            .chain(&a.coronas)
            .zip(b.points.iter().chain(&b.coronas))
        {
            assert_eq!(b.colour, a.colour * 2.0);
        }
    }

    #[test]
    fn uniforms_preserve_bright_radiance_until_fragment_lighting() {
        let mut lights = lights();
        lights.stars[0].kelvin = 50000.0;
        let frame = resolve(&lights, &Photometry::default(), 16.0, 1.0);
        assert!(
            frame
                .bodies
                .iter()
                .all(|b| b.colour.is_finite() && b.colour.max_element() <= MAX_VALUE as f32)
        );
        assert!(frame.bodies[0].colour.max_element() > 20_000.0);
        assert!(frame.bodies[1].colour.max_element() > 20_000.0);
        assert_eq!(frame.bodies[1].surface.w, 1.0);
        let dimmer = resolve(&lights, &Photometry::default(), 15.0, 1.0);
        assert_eq!(frame.bodies[1].colour, dimmer.bodies[1].colour * 2.0);
        assert!(
            frame
                .points
                .iter()
                .chain(&frame.coronas)
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
