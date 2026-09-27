//! Realistic rendering: an HDR camera with bloom, exposure worked out from the
//! photometry, and plain shaded spheres under the wireframes.
//!
//! - Physical exposure is one linear exposure for the whole frame, as a camera
//!   would take it, so planets all but vanish beside their star.
//! - Assisted exposure, the default, compresses brightness around the subject,
//!   exposes the star field on its own, keeps planets above a floor and shows
//!   star colours more saturated.
//! - Stars smaller than a point on screen are drawn at the point size with their
//!   flux conserved, so distant stars keep their true relative brightness.
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
use std::{collections::HashMap, sync::Mutex};

const SHADER: Handle<Shader> = uuid_handle!("6f1f3c47-2a8e-4d61-9b53-4f8e0d7a91c2");

/// Display value of the subject's key: a lit planet at the reference distance.
const KEY: f64 = 1.0;
/// Display value of the brightest point star when the sky is exposed alone.
const SKY_KEY: f64 = 3.0;
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
/// Smallest drawn angular radius of a star, in radians, as for the distant-star
/// wireframes.
pub const STAR_POINT: f64 = 0.004;
/// Smallest drawn angular radius of a planet, in radians.
pub const PLANET_POINT: f64 = 0.0015;

pub struct RenderPlugin;

impl Plugin for RenderPlugin {
    fn build(&self, app: &mut App) {
        load_internal_asset!(app, SHADER, "body.wgsl", Shader::from_wgsl);
        app.add_plugins(MaterialPlugin::<BodyMaterial>::default())
            .init_resource::<Lights>()
            .init_resource::<Photometry>()
            .init_resource::<Pool>()
            .add_systems(Startup, setup)
            .add_systems(Update, switch_camera)
            .add_systems(
                PostUpdate,
                sync.after(crate::scene::draw)
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
}

impl Material for BodyMaterial {
    fn fragment_shader() -> ShaderRef {
        SHADER.into()
    }
}

/// How a star looks from here, before exposure.
pub struct StarLight {
    pub centre: Vec3,
    /// Drawn radius, in render units.
    pub radius: f32,
    /// Drawn radiance, with flux conserved when enlarged to a point.
    pub radiance: f64,
    pub kelvin: f64,
    /// The current page's star while its planets are on screen.
    pub subject: bool,
}

/// How a planet is lit, before exposure.
pub struct PlanetLight {
    pub centre: Vec3,
    pub radius: f32,
    /// Direction to the star, in world axes.
    pub towards_star: Vec3,
    pub flux: f64,
    pub surface: Surface,
    /// Share of the shown disc's light left after enlarging it to a point.
    pub conserved: f64,
}

/// One frame's realistic bodies, gathered while drawing the wireframes.
#[derive(Resource, Default)]
pub struct Lights {
    pub stars: Vec<StarLight>,
    pub planets: Vec<PlanetLight>,
    /// Radiance metered for the planets, when they are on screen.
    pub subject_key: Option<f64>,
    /// Temperature of the star lighting the planets.
    pub kelvin: f64,
}

impl Lights {
    pub fn clear(&mut self) {
        self.stars.clear();
        self.planets.clear();
        self.subject_key = None;
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Exposure {
    pub key: f64,
    pub target: f64,
    pub physical: bool,
}

impl Exposure {
    pub fn value(&self, radiance: f64) -> f64 {
        let ratio = (radiance / self.key).max(0.0);
        let ratio = if self.physical {
            ratio
        } else {
            ratio.powf(ASSISTED_CONTRAST)
        };
        (self.target * ratio).min(MAX_VALUE)
    }
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

/// Drawn radius and the share of light kept when a body is enlarged to `point`
/// radians.
pub fn enlarge(shown: f64, distance: f64, point: f64) -> (f64, f64) {
    let drawn = shown.max(point * distance);
    (drawn, (shown / drawn).powi(2))
}

/// A shaded body ready for the GPU.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shaded {
    pub centre: Vec3,
    pub radius: f32,
    pub colour: Vec3,
    /// Direction to the star and night-side ambient; negative ambient for stars.
    pub light: Vec4,
}

pub fn resolve(lights: &Lights, photometry: &Photometry, physical: bool) -> Vec<Shaded> {
    // A physical frame has one exposure; otherwise the sky gets its own.
    let sky = if physical && lights.subject_key.is_some() {
        0.0
    } else {
        lights
            .stars
            .iter()
            .filter(|s| !s.subject)
            .map(|s| s.radiance)
            .fold(0.0, f64::max)
    };
    let subject = lights.subject_key.map(|key| Exposure {
        key,
        target: KEY,
        physical,
    });
    let sky = Exposure {
        key: if sky > 0.0 { sky } else { 1.0 },
        target: SKY_KEY,
        physical,
    };
    let mut out = Vec::with_capacity(lights.stars.len() + lights.planets.len());
    for star in &lights.stars {
        let exposure = match subject {
            Some(subject) if star.subject || physical => subject,
            _ => sky,
        };
        let mut value = exposure.value(star.radiance);
        if !physical {
            value = value.max(ASSISTED_STAR_FLOOR);
        }
        let colour = photometry.colour(star.kelvin, physical);
        out.push(Shaded {
            centre: star.centre,
            radius: star.radius,
            colour: (DVec3::from_array(colour) * value).as_vec3(),
            light: Vec4::new(0.0, 0.0, 0.0, -1.0),
        });
    }
    if let Some(subject) = subject {
        let tint = DVec3::from_array(photometry.colour(lights.kelvin, physical));
        for planet in &lights.planets {
            let radiance = light::reflected_radiance(planet.surface.albedo, planet.flux);
            let value = if physical {
                subject.value(radiance * planet.conserved)
            } else {
                subject.value(radiance).max(ASSISTED_PLANET_FLOOR)
            };
            let colour = tint * DVec3::from_array(planet.surface.tint) * value;
            out.push(Shaded {
                centre: planet.centre,
                radius: planet.radius,
                colour: colour.as_vec3(),
                light: planet
                    .towards_star
                    .extend(if physical { 0.0 } else { ASSISTED_AMBIENT }),
            });
        }
    }
    out
}

#[derive(Component)]
struct Body;

#[derive(Resource)]
struct SphereMesh(Handle<Mesh>);

/// Reused body entities, grown on demand and hidden when unused.
#[derive(Resource, Default)]
struct Pool(Vec<(Entity, Handle<BodyMaterial>)>);

fn setup(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>) {
    commands.insert_resource(SphereMesh(meshes.add(Sphere::new(1.0).mesh().uv(48, 24))));
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

type BodyPlacement = (
    &'static mut Transform,
    &'static mut GlobalTransform,
    &'static mut CellCoord,
    &'static mut Visibility,
);

#[allow(clippy::too_many_arguments)]
fn sync(
    mut commands: Commands,
    nav: Res<Navigation>,
    lights: Res<Lights>,
    photometry: Res<Photometry>,
    mesh: Res<SphereMesh>,
    mut pool: ResMut<Pool>,
    mut materials: ResMut<Assets<BodyMaterial>>,
    camera: Query<(&CellCoord, &ChildOf), With<SpaceCamera>>,
    mut bodies: Query<BodyPlacement, (With<Body>, Without<SpaceCamera>)>,
) {
    let shaded = if nav.realistic {
        resolve(&lights, &photometry, nav.physical)
    } else {
        Vec::new()
    };
    let Ok((cell, parent)) = camera.single() else {
        return;
    };
    if pool.0.len() < shaded.len() {
        // The grid has no visibility of its own, which would hide its children.
        commands
            .entity(parent.parent())
            .insert_if_new(Visibility::default());
    }
    while pool.0.len() < shaded.len() {
        let material = materials.add(BodyMaterial::default());
        let entity = commands
            .spawn((
                Body,
                Mesh3d(mesh.0.clone()),
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
    for (index, (entity, material)) in pool.0.iter().enumerate() {
        let Ok((mut transform, mut global, mut body_cell, mut visibility)) =
            bodies.get_mut(*entity)
        else {
            continue;
        };
        let Some(body) = shaded.get(index) else {
            if *visibility != Visibility::Hidden {
                *visibility = Visibility::Hidden;
            }
            continue;
        };
        // Same cell as the floating-origin camera, so the local translation is the
        // render position. Set the global transform too: propagation has run.
        *body_cell = *cell;
        *transform = Transform::from_translation(body.centre).with_scale(Vec3::splat(body.radius));
        *global = GlobalTransform::from(*transform);
        *visibility = Visibility::Inherited;
        // Only touch changed materials: every change re-uploads the bind group.
        let colour = body.colour.extend(1.0);
        if materials
            .get(material)
            .is_some_and(|m| m.colour != colour || m.light != body.light)
            && let Some(m) = materials.get_mut(material)
        {
            m.colour = colour;
            m.light = body.light;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn star(radiance: f64, subject: bool) -> StarLight {
        StarLight {
            centre: Vec3::ZERO,
            radius: 1.0,
            radiance,
            kelvin: 5772.0,
            subject,
        }
    }

    fn planet(flux: f64, conserved: f64) -> PlanetLight {
        PlanetLight {
            centre: Vec3::ZERO,
            radius: 1.0,
            towards_star: Vec3::X,
            flux,
            surface: light::surface("Rocky planet"),
            conserved,
        }
    }

    fn lights() -> Lights {
        let sun = light::flux(1.0, AU_METRES);
        Lights {
            stars: vec![star(2.0e7, true), star(1e-3, false), star(1e-5, false)],
            planets: vec![planet(sun, 1.0), planet(sun / 100.0, 1.0)],
            subject_key: Some(subject_key(1.0, AU_METRES)),
            kelvin: 5772.0,
        }
    }

    fn brightness(body: &Shaded) -> f32 {
        body.colour.max_element()
    }

    #[test]
    fn physical_exposure_is_linear_and_assisted_compresses() {
        let photometry = Photometry::default();
        let physical = resolve(&lights(), &photometry, true);
        let assisted = resolve(&lights(), &photometry, false);
        // Earth at 1 AU against a planet at 10 AU: 100 times dimmer, or 10 assisted.
        let ratio = |set: &[Shaded]| brightness(&set[3]) / brightness(&set[4]);
        assert!((ratio(&physical) - 100.0).abs() < 0.1);
        assert!((ratio(&assisted) - 10.0).abs() < 0.01);
        // The Sun is far brighter than its planets either way, and stays in range.
        assert!(brightness(&physical[0]) > 1_000.0 * brightness(&physical[3]));
        assert!(brightness(&assisted[0]) > 50.0 * brightness(&assisted[3]));
        assert!(
            physical
                .iter()
                .all(|b| b.colour.max_element() <= MAX_VALUE as f32)
        );
    }

    #[test]
    fn assisted_exposure_keeps_the_sky_and_faint_planets_visible() {
        let photometry = Photometry::default();
        let physical = resolve(&lights(), &photometry, true);
        let assisted = resolve(&lights(), &photometry, false);
        // A physical camera exposed for sunlit planets cannot see other stars:
        // far below one step of an 8-bit display.
        assert!(brightness(&physical[1]) < 1e-4);
        // Assisted exposes the sky alone: the brightest star sits at the sky key,
        // and one five magnitudes fainter is still visible.
        assert!((brightness(&assisted[1]) - SKY_KEY as f32).abs() < 0.01);
        assert!(brightness(&assisted[2]) > 0.1);
        let mut dim = lights();
        dim.planets[0].flux *= 1e-9;
        let dim = resolve(&dim, &photometry, false);
        assert!(brightness(&dim[3]) >= ASSISTED_PLANET_FLOOR as f32 * 0.99);
    }

    #[test]
    fn enlarging_to_a_point_conserves_flux() {
        let (drawn, kept) = enlarge(1.0, 10_000.0, 0.004);
        assert_eq!(drawn, 40.0);
        assert!((kept * drawn * drawn - 1.0).abs() < 1e-12);
        assert_eq!(enlarge(100.0, 10_000.0, 0.004), (100.0, 1.0));
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
