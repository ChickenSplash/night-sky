//! The camera's adaptation to the light in view, metered as a light meter
//! would: the average radiance across the view. The galaxy is metered from its
//! smooth model, every star whether streamed or not, so the exposure does not
//! change as stars load. The visited system's stars add their own light.
use crate::{
    Navigation,
    render::{self, Lights},
    scene::{CameraPose, CameraRig, SpaceCamera},
};
use bevy::{math::DVec3, prelude::*};
use space_simulation::{
    galaxy::{GalaxyModel, PC_METRES},
    glow::{mean_visual_luminosity, patches},
    light::SUN_LUMINOSITY_W,
    population::flux_per_luminosity,
};
use std::f64::consts::{PI, SQRT_2, TAU};

/// Display value × square pixels of a magnitude-0 star's point at the most
/// sensitive exposure, on a `reference_pixel_sr` pixel.
pub const POINT_LIGHT: f64 = 74.6;
/// The average radiance in view shows at this display value.
const SKY_BACKGROUND: f64 = 0.15;
/// Adaptation time constants, in seconds: to a brighter view, as an eye
/// adapts to light, and more slowly to a darker one.
const BRIGHTER_SECONDS: f64 = 0.1;
const DARKER_SECONDS: f64 = 0.5;
/// Patch detail for metering: coarser than the glow's, plenty for an average.
const DETAIL: f64 = 0.5;

/// Visual irradiance, in W m⁻², per solar luminosity per square parsec.
pub fn irradiance_per_ratio() -> f64 {
    SUN_LUMINOSITY_W / (4.0 * PI * PC_METRES * PC_METRES)
}

/// Solid angle of a reference pixel: one of 1080 rows over a 50° view.
fn reference_pixel_sr() -> f64 {
    (2.0 * 25f64.to_radians().tan() / 1080.0).powi(2)
}

/// The least physical key, as a fully dark-adapted eye: it matches the most
/// sensitive assisted exposure on a reference pixel.
pub fn dark_key() -> f64 {
    irradiance_per_ratio() * render::KEY
        / (POINT_LIGHT * flux_per_luminosity() * reference_pixel_sr())
}

/// A patch of the galaxy model, for metering.
struct Metered {
    position: DVec3,
    /// Mean squared Gaussian width, in square parsecs.
    sigma_sq: f64,
    /// Visual solar luminosities.
    light: f64,
}

#[derive(Resource)]
pub struct Sky {
    patches: Vec<Metered>,
    /// Physical: the radiance shown at display `KEY`, eased like an eye.
    pub key: f64,
    /// The physical key the view's average radiance alone asks for.
    pub sky_key: f64,
    /// The physical key `key` eases towards (`render::physical_key`).
    pub target: f64,
    /// Share of the view's average radiance from the galaxy, not the visited
    /// system's stars.
    pub galaxy_share: f64,
    /// Assisted and wireframe: multiplier on galactic light, at most one, eased.
    pub exposure: f32,
    adapted: bool,
}

impl Default for Sky {
    fn default() -> Self {
        let mean = mean_visual_luminosity();
        let patches = patches(&GalaxyModel::milky_way(), DETAIL)
            .into_iter()
            .map(|p| Metered {
                position: DVec3::from_array(p.position_pc),
                sigma_sq: p.sigma_pc.iter().map(|s| s * s).sum::<f64>() / 3.0,
                light: p.stars * mean[p.component],
            })
            .collect();
        Self {
            patches,
            key: dark_key(),
            sky_key: dark_key(),
            target: dark_key(),
            galaxy_share: 1.0,
            exposure: 1.0,
            adapted: false,
        }
    }
}

impl Sky {
    /// The galaxy's light within a cone around `forward`, as visual solar
    /// luminosities per square parsec, seen from `eye` in galactic parsecs.
    fn galaxy_flux(&self, eye: DVec3, forward: DVec3, solid: f64) -> f64 {
        let cone = (1.0 - solid / TAU).clamp(-1.0, 1.0).acos();
        self.patches
            .iter()
            .map(|p| {
                let offset = p.position - eye;
                let squared = offset.length_squared();
                let distance = squared.sqrt();
                // Softened so the eye inside a patch sees its finite light,
                // exactly so at its centre.
                let flux = p.light / (squared + p.sigma_sq);
                let cosine = if distance > 0.0 {
                    offset.dot(forward) / distance
                } else {
                    1.0
                };
                flux * in_cone(cosine, p.sigma_sq.sqrt() / distance, cone, solid)
            })
            .sum()
    }
}

/// Share of a blob's light inside a cone of half-angle `cone` and solid angle
/// `solid`, its centre `cosine` off the axis and about `width` radians wide.
/// The narrower of two estimates: all of it within the cone's edge, or its
/// density at the centre over the cone. Good to tens of percent, plenty for a
/// meter, and exact for a blob all around the eye.
fn in_cone(cosine: f64, width: f64, cone: f64, solid: f64) -> f64 {
    let angle = cosine.clamp(-1.0, 1.0).acos();
    let edge = 0.5 * erfc((angle - cone) / (SQRT_2 * width));
    let kappa = (1.0 / (width * width)).clamp(1e-6, 1e8);
    let density = kappa / (TAU * (1.0 - (-2.0 * kappa).exp())) * (kappa * (cosine - 1.0)).exp();
    edge.min(solid * density)
}

/// Complementary error function, to about 1e-7.
fn erfc(x: f64) -> f64 {
    let t = 1.0 / (1.0 + 0.3275911 * x.abs());
    let tail = (((((1.061405429 * t - 1.453152027) * t) + 1.421413741) * t - 0.284496736) * t
        + 0.254829592)
        * t
        * (-x * x).exp();
    if x >= 0.0 { tail } else { 2.0 - tail }
}

/// Solid angle of a view with these half-angle tangents.
fn view_solid(tan_x: f64, tan_y: f64) -> f64 {
    4.0 * (tan_x * tan_y / (1.0 + tan_x * tan_x + tan_y * tan_y).sqrt()).atan()
}

/// Eases towards a target in log space, quicker towards a brighter view.
fn ease(current: f64, target: f64, seconds: f64, brighter: bool) -> f64 {
    let constant = if brighter {
        BRIGHTER_SECONDS
    } else {
        DARKER_SECONDS
    };
    let t = 1.0 - (-seconds / constant).exp();
    (current.ln() + (target.ln() - current.ln()) * t).exp()
}

/// Meters the view and adapts the physical key and assisted exposure to it.
pub fn expose(
    mut sky: ResMut<Sky>,
    nav: Res<Navigation>,
    lights: Res<Lights>,
    pose: Res<CameraPose>,
    rig: Res<CameraRig>,
    time: Res<Time>,
    camera: Query<(&GlobalTransform, &Camera), With<SpaceCamera>>,
) {
    let Ok((transform, camera)) = camera.single() else {
        return;
    };
    let size = camera
        .physical_viewport_size()
        .unwrap_or(UVec2::ONE)
        .max(UVec2::ONE);
    let tan_half = nav.tan_half_fov();
    let aspect = f64::from(size.x) / f64::from(size.y);
    let solid = view_solid(tan_half * aspect, tan_half);
    let cone = 1.0 - solid / TAU;
    let forward = transform.forward().as_dvec3();
    let eye = DVec3::from_array(GalaxyModel::milky_way().home_pc)
        + rig.galactic_ly(&pose) * (space_simulation::LY_METRES / PC_METRES);
    let galaxy = sky.galaxy_flux(eye, forward, solid) / solid;
    // The visited system's stars in view, as the irradiance of their discs.
    let stars: f64 = lights
        .stars
        .iter()
        .filter_map(|star| {
            let offset = (star.centre - lights.eye).as_dvec3();
            let distance = offset.length();
            let sine = f64::from(star.radius) / distance;
            let disc = if sine < 1.0 {
                TAU * (1.0 - (1.0 - sine * sine).sqrt())
            } else {
                TAU
            };
            (offset.dot(forward) >= cone * distance).then_some(star.radiance * disc)
        })
        .sum();
    let radiance = irradiance_per_ratio() * galaxy + stars / solid;
    sky.galaxy_share = if radiance > 0.0 {
        irradiance_per_ratio() * galaxy / radiance
    } else {
        1.0
    };
    sky.sky_key = (radiance * render::KEY / SKY_BACKGROUND).max(dark_key());
    let key = render::physical_key(&lights, sky.sky_key);
    sky.target = key;
    let pixel = 2.0 * tan_half / f64::from(size.y);
    let background = POINT_LIGHT * flux_per_luminosity() * galaxy * pixel * pixel;
    let exposure = (SKY_BACKGROUND / background).min(1.0);
    if sky.adapted {
        let seconds = time.delta_secs_f64();
        sky.key = ease(sky.key, key, seconds, key > sky.key);
        let current = f64::from(sky.exposure);
        sky.exposure = ease(current, exposure, seconds, exposure < current) as f32;
    } else {
        (sky.key, sky.exposure, sky.adapted) = (key, exposure as f32, true);
    }
}

/// Display value per unit of galactic radiance, in visual solar luminosities
/// per square parsec per steradian. Physical exposure shares the frame's key
/// with every body; otherwise galactic light has its own exposure, as before,
/// in display value × square pixels per luminosity per square parsec on a
/// `pixel_sr` pixel.
pub fn radiance_gain(nav: &Navigation, sky: &Sky, pixel_sr: f64) -> f64 {
    if nav.realistic && nav.physical {
        irradiance_per_ratio() * render::KEY / sky.key
    } else {
        POINT_LIGHT * flux_per_luminosity() * f64::from(sky.exposure) * pixel_sr
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sky() -> Sky {
        Sky::default()
    }

    #[test]
    fn a_blob_around_the_eye_fills_the_view_evenly() {
        let solid = view_solid(0.8, 0.47);
        let cone = (1.0 - solid / TAU).acos();
        let share = in_cone(1.0, 1e6, cone, solid);
        assert!((share / (solid / (4.0 * PI)) - 1.0).abs() < 1e-3);
        // A small blob is all inside when ahead, and none of it when behind.
        assert!((in_cone(1.0, 1e-3, cone, solid) - 1.0).abs() < 1e-6);
        assert!(in_cone(-1.0, 1e-3, cone, solid) < 1e-9);
    }

    #[test]
    fn the_galactic_centre_is_far_brighter_than_the_view_away_from_it() {
        let sky = sky();
        let home = DVec3::from_array(GalaxyModel::milky_way().home_pc);
        let solid = view_solid(0.8, 0.47);
        let towards = sky.galaxy_flux(home, -DVec3::X, solid);
        let away = sky.galaxy_flux(home, DVec3::Y, solid);
        assert!(towards > 10.0 * away, "{towards} {away}");
        // At the centre, the sky is many times brighter than from home.
        let centre = sky.galaxy_flux(DVec3::new(10.0, 0.0, 0.0), -DVec3::X, solid);
        assert!(centre > 100.0 * towards, "{centre} {towards}");
    }

    #[test]
    fn home_stays_dark_adapted_and_the_centre_stops_down() {
        let sky = sky();
        let solid = view_solid(0.8, 0.47);
        let key = |eye: DVec3, forward| {
            let galaxy = sky.galaxy_flux(eye, forward, solid) / solid;
            (irradiance_per_ratio() * galaxy * render::KEY / SKY_BACKGROUND).max(dark_key())
        };
        let home = DVec3::from_array(GalaxyModel::milky_way().home_pc);
        assert_eq!(key(home, -DVec3::X), dark_key());
        assert!(key(DVec3::new(10.0, 0.0, 0.0), -DVec3::X) > 100.0 * dark_key());
    }

    #[test]
    fn adaptation_is_quicker_to_light_than_to_dark() {
        let brighter = ease(1.0, 100.0, 0.1, true);
        let darker = ease(100.0, 1.0, 0.1, false);
        assert!(brighter.ln() > (100.0 / darker).ln());
        assert!((ease(1.0, 100.0, 10.0, true) / 100.0 - 1.0).abs() < 1e-9);
    }
}
