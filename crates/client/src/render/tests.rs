use super::*;
use bevy::math::DVec3;
use space_shared::Star;
use space_simulation::AU_METRES;
use space_simulation::light;
use std::f32::consts::PI;

/// Where test bodies sit: straight ahead of the camera, far enough that a
/// disc's outline is its size in pixels, with 1,000 pixels per radian.
const AHEAD: Vec3 = Vec3::new(0.0, 0.0, -1e6);

fn star(radiance: f64, pixels: f32) -> StarLight {
    StarLight {
        centre: AHEAD,
        radius: pixels * AHEAD.length() / 1000.0,
        pixels,
        per_pixel: AHEAD.length() / 1000.0,
        radiance,
        kelvin: 5772.0,
        spin: Quat::IDENTITY,
        flattening: 0.0,
        granules: 700.0,
    }
}

fn planet(flux: f64, pixels: f32) -> PlanetLight {
    PlanetLight {
        centre: AHEAD + Vec3::X * 10.0,
        radius: pixels * AHEAD.length() / 1000.0,
        spin: Quat::IDENTITY,
        flattening: 0.0,
        pixels,
        per_pixel: AHEAD.length() / 1000.0,
        light: vec![Illumination {
            towards: -Vec3::X,
            flux,
            kelvin: 5772.0,
        }],
        surface: light::surface("Rocky planet"),
        air: None,
        ground: None,
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
        grounds: Vec::new(),
        eye: Vec3::ZERO,
        scale: 1.0,
        facing: Quat::IDENTITY,
        streaks: 1.0,
    }
}

fn brightness(body: &Shaded) -> f32 {
    body.colour.max_element()
}

#[test]
fn descending_envelopes_have_only_a_shell_not_a_false_surface_or_rim() {
    use atmosphere::{AirSettings, Preset, generated::descending};
    let mut lights = lights();
    lights.stars.clear();
    lights.planets.truncate(2);
    let air = AirSettings::preset(Preset::HydrogenGiant)
        .atmosphere()
        .scaled(7e7);
    lights.planets[0].air = Some(descending(air));
    let frame = resolve(&lights, &Photometry::default(), -10.0, 1.0, PSF_SIGMA_PX);
    assert_eq!(frame.bodies.len(), 1);
    assert_eq!(frame.bodies[0].centre, lights.planets[1].centre);
    assert_eq!(frame.rims.len(), 1);
    assert_eq!(frame.shells.len(), 1);
    assert_eq!(frame.shells[0].air.ground, Vec3::ZERO);
    assert_eq!(
        frame.shells[0].radius,
        lights.planets[0].radius * descending(air).radius_scale
    );

    // The A/B path retains the original sphere. Distant points also remain.
    lights.planets[0].air = Some(air);
    let old = resolve(&lights, &Photometry::default(), -10.0, 1.0, PSF_SIGMA_PX);
    assert_eq!(old.bodies.len(), 2);
    lights.planets[0].air = Some(descending(air));
    lights.planets[0].pixels = POINT_PIXELS * 0.5;
    let far = resolve(&lights, &Photometry::default(), -10.0, 1.0, PSF_SIGMA_PX);
    assert_eq!(far.points.len(), 1);
    assert!(far.shells.is_empty());
}

#[test]
fn multiple_planets_keep_distinct_shells_and_airless_bodies_have_no_shell_index() {
    use atmosphere::{AirSettings, Preset};
    let mut lights = lights();
    lights.stars.clear();
    lights.planets.truncate(2);
    lights.planets.push(planet(1361.0, 10.0));
    lights.planets[0].air = Some(
        AirSettings::preset(Preset::Earth)
            .atmosphere()
            .scaled(6.371e6),
    );
    lights.planets[2].air = Some(
        AirSettings::preset(Preset::Mars)
            .atmosphere()
            .scaled(3.3895e6),
    );
    let frame = resolve(&lights, &Photometry::default(), -10.0, 1.0, PSF_SIGMA_PX);
    assert_eq!(frame.shells.len(), 2);
    assert_eq!(
        frame
            .bodies
            .iter()
            .map(|b| b.atmosphere)
            .collect::<Vec<_>>(),
        vec![Some(0), None, Some(1)]
    );
    assert_ne!(frame.shells[0].air, frame.shells[1].air);
}

#[test]
fn atmosphere_and_surface_lighting_fade_continuously_from_the_point_disc_switch() {
    use atmosphere::{AirSettings, Preset};
    let mut lights = lights();
    lights.stars.clear();
    lights.planets.truncate(1);
    lights.planets[0].air = Some(
        AirSettings::preset(Preset::Earth)
            .atmosphere()
            .scaled(6.371e6),
    );
    for (pixels, expected) in [
        (POINT_PIXELS, 0.0),
        (2.5 * POINT_PIXELS, 0.5),
        (4.0 * POINT_PIXELS, 1.0),
    ] {
        lights.planets[0].pixels = pixels;
        let frame = resolve(&lights, &Photometry::default(), -10.0, 1.0, PSF_SIGMA_PX);
        assert!((frame.shells[0].fade - expected).abs() < 1e-6);
    }
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
    dim.planets[2].light[0].flux = 0.0;
    dim.planets[0].light[0].flux = 0.0;
    let dim = resolve(&dim, &photometry, -30.0, 0.0, PSF_SIGMA_PX);
    assert_eq!(dim.points[2].light(), 0.0);
    assert_eq!(brightness(&dim.bodies[1]), 0.0);
    assert_eq!(dim.bodies[1].light.w, 0.0);
}

#[test]
fn each_star_lights_its_own_side_in_its_own_colour() {
    let photometry = Photometry::default();
    let sun = light::flux(1.0, AU_METRES);
    let mut binary = lights();
    // A cool companion behind the planet, a thousand times fainter.
    binary.planets[0].light.push(Illumination {
        towards: Vec3::X,
        flux: sun / 1000.0,
        kelvin: 3500.0,
    });
    let single = resolve(&lights(), &photometry, -30.0, 0.0, PSF_SIGMA_PX);
    let lit = resolve(&binary, &photometry, -30.0, 0.0, PSF_SIGMA_PX);
    let (body, alone) = (&lit.bodies[1], &single.bodies[1]);
    // The brightest star sets the exposure: the day side is unchanged.
    assert_eq!(body.tints[0], alone.tints[0]);
    assert_eq!(body.colour, alone.colour);
    assert_eq!(body.lights[1].truncate(), Vec3::X);
    let ratio = body.tints[0].max_element() / body.tints[1].max_element();
    assert!((500.0..2000.0).contains(&ratio), "{ratio}");
    // The red dwarf's side is redder than the Sun's.
    let warmth = |t: Vec4| t.x / t.z;
    assert!(warmth(body.tints[1]) > 1.5 * warmth(body.tints[0]));
    assert_eq!(body.tints[2], Vec4::ZERO);
    // A limb on each lit side, streaks only from the brighter.
    assert_eq!(lit.rims.len(), single.rims.len() + 1);
    // A point adds both stars' light.
    binary.planets[2].light.push(Illumination {
        towards: -Vec3::X,
        flux: 10.0 * sun,
        kelvin: 5772.0,
    });
    let doubled = resolve(&binary, &photometry, -30.0, 0.0, PSF_SIGMA_PX);
    let ratio = doubled.points[2].light() / single.points[2].light();
    assert!((ratio - 2.0).abs() < 1e-3, "{ratio}");
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

#[test]
fn an_atmosphere_is_lit_on_the_surface_exposure_and_only_around_discs() {
    let photometry = Photometry::default();
    let earth = atmosphere::AirSettings::preset(atmosphere::Preset::Earth)
        .atmosphere()
        .scaled(6.371e6);
    // About 100 km of air on Earth's 6,371 km, its gas scale height 8 km.
    assert!((earth.top - 1.0157).abs() < 1e-3);
    assert!((earth.rayleigh.w * 6.371e6 - 8e3).abs() < 1.0);
    let mut air = lights();
    for planet in &mut air.planets {
        planet.air = Some(earth);
    }
    let plain = resolve(&lights(), &photometry, -30.0, 0.0, PSF_SIGMA_PX);
    let lit = resolve(&air, &photometry, -30.0, 0.0, PSF_SIGMA_PX);
    assert!(plain.shells.is_empty());
    // Only the two disc planets, not the points.
    assert_eq!(lit.shells.len(), 2);
    let (shell, body) = (&lit.shells[0], &lit.bodies[1]);
    assert_eq!(body.atmosphere, Some(0));
    assert_eq!(lit.bodies[0].atmosphere, None);
    assert!(plain.bodies.iter().all(|body| body.atmosphere.is_none()));
    for (index, shell) in lit.shells.iter().enumerate() {
        let body = lit
            .bodies
            .iter()
            .find(|body| body.atmosphere == Some(index))
            .unwrap();
        assert_eq!(body.centre, shell.centre);
        assert_eq!(body.radius, shell.radius);
    }
    assert_eq!(shell.lights, body.lights);
    // The flux against the surface's AF/π, both on one exposure.
    let surface = light::surface("Rocky planet");
    let ratio = f64::from(shell.suns[0].x / body.tints[0].x);
    let expected = std::f64::consts::PI / (surface.albedo * surface.tint[0]);
    assert!((ratio / expected - 1.0).abs() < 1e-4, "{ratio} {expected}");
    assert_eq!(shell.suns[1], Vec4::ZERO);
}

#[test]
fn atmosphere_presets_and_changes() {
    use atmosphere::{AirSettings, AtmosphereChange, AtmosphereTuning, Preset};
    // Earth's preset gives Hillaire's (2020) air: 100 km deep, blue 33.1e-6.
    let earth = AirSettings::preset(Preset::Earth).atmosphere();
    assert!((earth.top - 100e3).abs() < 1.0);
    assert!((earth.rayleigh.z / 33.1e-6 - 1.0).abs() < 1e-3);
    let mut tuning = AtmosphereTuning::default();
    tuning.apply(&AtmosphereChange {
        preset: Some(Preset::Mars),
        haze: Some(2.0),
        view_steps: Some(1000),
        ..default()
    });
    assert_eq!(tuning.preset, Preset::Mars);
    assert_eq!(tuning.air.pressure_bar, 0.006);
    assert_eq!(tuning.air.haze, 2.0);
    assert_eq!(tuning.view_steps, *atmosphere::VIEW_STEPS.end());
    assert!(AtmosphereChange::default().check().is_err());
    // Every preset stays within the sliders' ranges.
    for (preset, _) in Preset::ALL {
        let air = AirSettings::preset(preset);
        assert!(atmosphere::PRESSURE_BAR.contains(&air.pressure_bar));
        assert!(atmosphere::HEIGHT_KM.contains(&air.scale_height_km));
        assert!(atmosphere::HEIGHT_KM.contains(&air.haze_height_km));
        assert!(atmosphere::GAS.contains(&air.gas));
        assert!(atmosphere::HAZE.contains(&air.haze));
        assert!(atmosphere::HAZE_ABSORBED.contains(&air.haze_absorbed));
    }
}

/// Each point of `outline`, seen from the eye, as a line of sight in world
/// axes, for a sprite through `from_eye` facing `facing`.
fn outline_sights(outline: &resolve::Outline, from_eye: Vec3, facing: Quat) -> Vec<DVec3> {
    let (right, up) = (facing * Vec3::X, facing * Vec3::Y);
    let long = Vec2::new(-outline.across.y, outline.across.x);
    (0..16)
        .map(|i| {
            let a = i as f32 / 16.0 * 2.0 * PI;
            let p = outline.offset
                + long * outline.pixels * a.cos()
                + outline.across * outline.pixels * outline.ratio * a.sin();
            let w = Vec2::new(p.x, -p.y) * outline.per_pixel;
            (from_eye + right * w.x + up * w.y).as_dvec3()
        })
        .collect()
}

#[test]
fn a_rim_follows_the_outline_seen_from_any_angle() {
    let radius = 100.0;
    // Pixels straight ahead at a distance, for a 1,000-pixel-high 60 degree view.
    let per_pixel = |distance: f32| distance * 2.0 * (PI / 6.0).tan() / 1000.0;
    for (distance, flattening, pole, facing) in [
        (300.0, 0.0, Vec3::Y, Quat::IDENTITY),
        (300.0, 0.0, Vec3::Y, Quat::from_rotation_y(0.4)),
        (
            130.0,
            0.176,
            Vec3::new(0.2, 1.0, 0.5),
            Quat::from_rotation_x(0.3),
        ),
        (
            150.0,
            0.2,
            Vec3::new(1.0, 0.3, 0.0),
            Quat::from_euler(EulerRot::YXZ, -0.35, 0.2, 0.1),
        ),
        (5_000.0, 0.1, Vec3::Z, Quat::from_rotation_y(0.05)),
    ] {
        let from_eye = Vec3::NEG_Z * distance;
        let outline = resolve::Outline::of(
            from_eye,
            radius,
            (pole, flattening),
            per_pixel(distance),
            facing,
        )
        .unwrap();
        let pole = pole.normalize().as_dvec3();
        let (a, c) = (f64::from(radius), f64::from(radius * (1.0 - flattening)));
        let shape = |v: DVec3| {
            let along = pole * v.dot(pole);
            (v - along) / (a * a) + along / (c * c)
        };
        let centre = from_eye.as_dvec3();
        for sight in outline_sights(&outline, from_eye, facing) {
            // The line of sight meets the spheroid in one point: it grazes.
            let (dd, dc, cc) = (
                sight.dot(shape(sight)),
                sight.dot(shape(centre)),
                centre.dot(shape(centre)),
            );
            let grazing = dc * dc / (dd * (cc - 1.0));
            assert!(
                (grazing - 1.0).abs() < 1e-4,
                "{distance} {flattening}: {grazing}"
            );
        }
    }
    // Straight at a sphere: round, centred, and wider than the radius by
    // the grazing cone.
    let outline = resolve::Outline::of(
        Vec3::NEG_Z * 200.0,
        radius,
        (Vec3::Y, 0.0),
        0.5,
        Quat::IDENTITY,
    )
    .unwrap();
    assert!(outline.offset.length() < 1e-3);
    assert!((outline.ratio - 1.0).abs() < 1e-5);
    let expected = radius * 200.0 / (200.0f32.powi(2) - radius * radius).sqrt() / 0.5;
    assert!((outline.pixels - expected).abs() < 1e-3 * expected);
    // From inside, behind the camera, or so close it reaches beside it: none.
    let none = |from_eye: Vec3, facing: Quat| {
        resolve::Outline::of(from_eye, radius, (Vec3::Y, 0.1), 1.0, facing).is_none()
    };
    assert!(none(Vec3::NEG_Z * 50.0, Quat::IDENTITY));
    assert!(none(Vec3::Z * 300.0, Quat::IDENTITY));
    assert!(none(Vec3::NEG_Y * 101.0, Quat::IDENTITY));
}

#[test]
fn rims_keep_their_places_when_a_disc_turns_out_of_view() {
    // Sprites are pooled by index, rims before points: a rim dropped while
    // its body is behind the camera would hand every point after it another
    // sprite's material for a frame.
    let photometry = Photometry::default();
    let ahead = resolve(&lights(), &photometry, -30.0, 0.0, PSF_SIGMA_PX);
    let mut turned = lights();
    turned.facing = Quat::from_rotation_y(PI);
    let behind = resolve(&turned, &photometry, -30.0, 0.0, PSF_SIGMA_PX);
    assert_eq!(ahead.rims.len(), behind.rims.len());
    assert!(
        behind
            .rims
            .iter()
            .all(|r| r.colour == Vec3::ZERO && r.edge == Vec4::ZERO)
    );
    assert!(
        behind
            .rims
            .iter()
            .all(|r| r.streaks.truncate() == Vec3::ZERO)
    );
}
