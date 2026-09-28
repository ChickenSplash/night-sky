use crate::flight::{Flight, Pose, STAR_JUMP_SECONDS};
use crate::render::{self, Lights, Photometry, PlanetLight, StarLight};
use crate::views::{TableLayout, View};
use crate::{Clock, Navigation, Universe};
use bevy::{
    input::mouse::{AccumulatedMouseMotion, MouseWheel},
    math::{DQuat, DVec3},
    prelude::*,
};
use bevy_egui::EguiContexts;
use big_space::prelude::*;
use space_shared::{Orbit, Spin, StarSystem};
use space_simulation::{
    LY_METRES, SUN_RADIUS_METRES, plane_tilt, position, position_at_eccentric_anomaly, spin_angle,
};
use std::f64::consts::TAU;

/// Beyond this camera range stars draw at their true separation. Planet
/// visibility also depends on the actual size of the current page's system.
const WIDE_METRES: f64 = 0.02 * LY_METRES;

#[derive(Component)]
pub struct SpaceCamera;

/// The displayed camera, which may still be flying towards `Navigation`'s target.
#[derive(Resource, Default)]
pub struct CameraPose {
    /// Metres relative to the current star, retained in double precision.
    pub eye: DVec3,
    pub distance: f64,
    /// 0 for orbital positions, 1 for the task table layout.
    pub layout: f64,
    /// Page anchoring the camera (the list's first page in constellation view).
    pub page: u32,
}

impl CameraPose {
    /// Metres per render unit. Keeps the camera about 1,000 units from its target, so
    /// GPU clip-space values stay small at every zoom level (perspective is
    /// scale-invariant, so the image does not change).
    pub fn render_scale(&self) -> f64 {
        self.distance / 1000.0
    }
}

#[derive(Resource, Default)]
pub struct CameraRig {
    shown: Option<Pose>,
    /// Page star position the shown pose is relative to.
    frame_ly: DVec3,
    flight: Option<Flight>,
    selected: Option<usize>,
    warned_stall: bool,
}

impl CameraRig {
    pub fn flying(&self) -> bool {
        self.flight.is_some()
    }

    /// The displayed camera in galactic axes, in light years from home.
    pub fn galactic_ly(&self, pose: &CameraPose) -> DVec3 {
        self.frame_ly + pose.eye / LY_METRES
    }

    pub fn rotation(&self) -> Quat {
        self.shown.map_or(Quat::IDENTITY, |pose| pose.rotation)
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn progress(&self) -> Option<f64> {
        self.flight.as_ref().map(Flight::progress)
    }
}

/// A planet's label and hit target. Stars have none: they are picked from the
/// streamed galaxy like any other star.
#[derive(Clone)]
pub struct Marker {
    pub point: Vec2,
    pub radius: f32,
    pub label: String,
    pub colour: [u8; 3],
    /// Index into the visited system's planets.
    pub planet: usize,
}

#[derive(Resource, Default)]
pub struct Markers(pub Vec<Marker>);

pub fn setup(mut commands: Commands, mut gizmo_store: ResMut<GizmoConfigStore>) {
    // UI owns the full window; the 3D camera owns only the unobscured centre.
    commands.spawn((
        Camera2d,
        Camera {
            order: 1,
            clear_color: ClearColorConfig::None,
            ..default()
        },
        bevy_egui::PrimaryEguiContext,
        bevy::camera::visibility::RenderLayers::layer(31),
    ));
    let (config, _) = gizmo_store.config_mut::<DefaultGizmoConfigGroup>();
    config.line.width = 1.25;
    commands.init_resource::<CameraPose>();
    commands.init_resource::<CameraRig>();
    commands.init_resource::<Markers>();
    commands.spawn_big_space_default(|root| {
        root.spawn_spatial((
            SpaceCamera,
            FloatingOrigin,
            Camera3d::default(),
            Projection::Perspective(PerspectiveProjection {
                near: 1000.0,
                far: 1e22,
                fov: 50.0_f32.to_radians(),
                ..default()
            }),
            Msaa::Sample4,
            // GPU-driven culling drops every mesh with this far plane. WebGL 2
            // has no indirect drawing, so native now matches the browser.
            bevy::render::view::NoIndirectDrawing,
        ));
    });
}

#[allow(clippy::too_many_arguments)]
pub fn controls(
    time: Res<Time>,
    input: (Res<ButtonInput<KeyCode>>, Res<ButtonInput<MouseButton>>),
    motion: Res<AccumulatedMouseMotion>,
    mut wheel: MessageReader<MouseWheel>,
    mut contexts: EguiContexts,
    rig: Res<CameraRig>,
    pose: Res<CameraPose>,
    universe: Res<Universe>,
    field: Res<crate::stream::Starfield>,
    mut nav: ResMut<Navigation>,
    mut clock: ResMut<Clock>,
) {
    let (keys, buttons) = input;
    let scroll = wheel
        .read()
        .map(|e| match e.unit {
            bevy::input::mouse::MouseScrollUnit::Line => e.y,
            bevy::input::mouse::MouseScrollUnit::Pixel => e.y / 40.0,
        })
        .sum::<f32>();
    let (pointer, keyboard) = contexts
        .ctx_mut()
        .map(|ctx| (ctx.is_pointer_over_area(), ctx.wants_keyboard_input()))
        .unwrap_or_default();
    // Flights are short; camera input resumes when they land. Constellations
    // rotate around their auto-framed centre, but cannot zoom or translate.
    let free = nav.view == View::Orbit && !rig.flying();
    let rotating = matches!(nav.view, View::Orbit | View::Constellation) && !rig.flying();
    if !pointer && rotating {
        if buttons.pressed(MouseButton::Right) {
            nav.yaw -= motion.delta.x * 0.005;
            nav.pitch = (nav.pitch + motion.delta.y * 0.005).clamp(-1.48, 1.48);
        }
        if free {
            nav.distance = (nav.distance * (-f64::from(scroll) * 0.12).exp())
                .clamp(20_000_000.0, 40_000.0 * LY_METRES);
        }
    }
    if !keyboard {
        if keys.just_pressed(KeyCode::KeyF) {
            nav.picked = crate::galaxy_nav::pick_centre(&field, &rig, &pose, rig.rotation());
        }
        if keys.just_pressed(KeyCode::Enter) {
            crate::galaxy_nav::visit_picked(&mut nav, &universe.0);
        }
        if keys.just_pressed(KeyCode::Space) {
            clock.0.paused = !clock.0.paused;
        }
        if keys.just_pressed(KeyCode::Escape) {
            nav.picked = None;
            nav.selected = None;
            nav.following = false;
        }
        let axis = |positive, negative| {
            f64::from(keys.pressed(positive) as u8) - f64::from(keys.pressed(negative) as u8)
        };
        let movement = DVec3::new(
            axis(KeyCode::KeyD, KeyCode::KeyA),
            axis(KeyCode::KeyE, KeyCode::KeyQ),
            axis(KeyCode::KeyS, KeyCode::KeyW),
        );
        if movement.length_squared() > 0.0 && free {
            nav.following = false;
            let rotation = Quat::from_rotation_y(nav.yaw) * Quat::from_rotation_x(-nav.pitch);
            let boost = if keys.pressed(KeyCode::ShiftLeft) {
                4.0
            } else {
                1.0
            };
            let step = (rotation * movement.normalize().as_vec3()).as_dvec3()
                * nav.distance
                * time.delta_secs_f64()
                * 0.35
                * boost;
            nav.target += step;
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub fn place_camera(
    time: Res<Time>,
    universe: Res<Universe>,
    clock: Res<Clock>,
    mut nav: ResMut<Navigation>,
    mut rig: ResMut<CameraRig>,
    mut pose: ResMut<CameraPose>,
    grids: Query<&Grid>,
    mut camera: Query<
        (
            &mut Transform,
            &mut CellCoord,
            &ChildOf,
            &mut Projection,
            &mut Camera,
        ),
        With<SpaceCamera>,
    >,
) {
    nav.fit_preset(&universe.0);
    if nav.following
        && let Some(followed) = nav.system(&universe.0).and_then(|system| {
            let planet = system.planets.get(nav.selected?)?;
            Some(DVec3::from_array(position(
                &planet.orbit,
                system.mass(planet.page()),
                clock.0.seconds,
            )))
        })
    {
        nav.target = followed;
    }
    let fly = std::mem::take(&mut nav.fly);
    let system = nav.system(&universe.0);
    // The constellation uses one page's frame for the whole list; page selection
    // does not move the camera. Other views use the selected page's plane. In
    // open space the camera is relative to home, in galactic axes.
    let frame_page = system.map_or(0, |system| nav.frame_page(system));
    let frame_tilt = system.map_or(DQuat::IDENTITY, |system| page_frame(system, frame_page));
    let desired = Pose {
        target: frame_tilt * nav.target,
        distance: nav.distance,
        rotation: frame_tilt.as_quat()
            * Quat::from_rotation_y(nav.yaw)
            * Quat::from_rotation_x(-nav.pitch),
        layout: f64::from(u8::from(nav.view == View::Table)),
    };
    let frame = system.map_or(DVec3::ZERO, |system| page_origin_ly(system, frame_page));
    let mut shown = rig.shown.unwrap_or(desired);
    let frame_shift_ly = (frame - rig.frame_ly).length();
    if rig.shown.is_some() && frame != rig.frame_ly {
        // Re-express the camera relative to the new star, so jumps start in place.
        let offset = (rig.frame_ly - frame) * LY_METRES;
        shown.target += offset;
        if let Some(flight) = &mut rig.flight {
            flight.shift(offset);
        }
    }
    let retarget = nav.following && nav.selected != rig.selected;
    let star_jump = rig.shown.is_some() && frame != rig.frame_ly;
    rig.frame_ly = frame;
    rig.selected = nav.selected;
    // Page jumps, list jumps, task focus and preset changes all use this flight.
    if fly || retarget {
        rig.flight = Flight::new(shown, desired);
        // Every jump to another star, in this list or another, keeps one rhythm
        // whatever the distance travelled.
        if star_jump && let Some(flight) = &mut rig.flight {
            flight.set_duration(STAR_JUMP_SECONDS);
        }
        rig.warned_stall = false;
        if let Some(flight) = &rig.flight {
            info!(
                "Flight started: list={} page={} view={:?} star travel={:.3} ly duration={:.3} s",
                system.map_or("galaxy", |s| s.id.as_str()),
                nav.page + 1,
                nav.view,
                frame_shift_ly,
                flight.duration_seconds(),
            );
        }
    }
    if rig.flight.is_some() && time.delta_secs_f64() > 0.12 && !rig.warned_stall {
        warn!(
            "Flight frame delayed by {:.0} ms (list={}, page={})",
            time.delta_secs_f64() * 1000.0,
            system.map_or("galaxy", |s| s.id.as_str()),
            nav.page + 1,
        );
        rig.warned_stall = true;
    }
    shown = match &mut rig.flight {
        Some(flight) => {
            flight.advance(time.delta_secs_f64());
            if flight.finished() {
                desired
            } else {
                flight.sample(desired)
            }
        }
        None => desired,
    };
    if rig.flight.as_ref().is_some_and(Flight::finished) {
        info!(
            "Flight landed: list={} page={} view={:?}",
            system.map_or("galaxy", |s| s.id.as_str()),
            nav.page + 1,
            nav.view
        );
        rig.flight = None;
    }
    rig.shown = Some(shown);
    pose.page = frame_page;

    let rotation = shown.rotation;
    pose.eye = shown.target + (rotation * Vec3::Z).as_dvec3() * shown.distance;
    pose.distance = shown.distance;
    pose.layout = shown.layout;
    let Ok((mut transform, mut cell, parent, mut projection, mut camera)) = camera.single_mut()
    else {
        return;
    };
    camera.viewport = nav.viewport.clone();
    let Ok(grid) = grids.get(parent.parent()) else {
        return;
    };
    let absolute = frame * LY_METRES + pose.eye;
    let (new_cell, offset) = grid.translation_to_grid(absolute);
    *cell = new_cell;
    transform.translation = offset;
    transform.rotation = rotation;
    if let Projection::Perspective(p) = &mut *projection {
        // Render units, see `CameraPose::render_scale`.
        p.near = 0.01;
    }
}

fn rotation([inclination, node, twist]: [f64; 3]) -> DQuat {
    DQuat::from_rotation_y(node)
        * DQuat::from_rotation_x(inclination)
        * DQuat::from_rotation_y(twist)
}

/// A page star's orbital-plane tilt: inclination, node and twist, in radians.
pub fn page_tilt(system: &StarSystem, page: u32) -> [f64; 3] {
    system
        .page_star(page)
        .map_or([0.0; 3], |star| plane_tilt(&star.id))
}

/// Rotation from a page's orbital-plane axes into galactic axes. Every page is its
/// own star system, so each has its own plane.
pub fn page_frame(system: &StarSystem, page: u32) -> DQuat {
    rotation(page_tilt(system, page))
}

/// A body's wireframe orientation in its system's plane: its axis tilted, then
/// turned to its current rotation angle.
fn spin_basis(frame: DQuat, spin: &Spin, seconds: f64) -> DQuat {
    frame
        * DQuat::from_rotation_y(spin.azimuth_rad)
        * DQuat::from_rotation_x(spin.tilt_rad)
        // Orbit positions progress from +X towards +Z; positive Y rotation
        // turns +X towards -Z. Negate the spin to match orbital direction.
        * DQuat::from_rotation_y(-spin_angle(spin, seconds))
}

/// A page star's position: light years from home, in galactic axes.
pub fn page_origin_ly(system: &StarSystem, page: u32) -> DVec3 {
    DVec3::from_array(
        system
            .page_star(page)
            .map_or(system.position_ly, |star| star.position_ly),
    )
}

/// Every page star of a system relative to one of them, in galactic metres.
pub fn page_offsets(system: &StarSystem, from: u32) -> Vec<DVec3> {
    let here = page_origin_ly(system, from);
    system
        .pages()
        .iter()
        .map(|&page| (page_origin_ly(system, page) - here) * LY_METRES)
        .collect()
}

fn colour(rgb: [u8; 3]) -> Color {
    Color::srgb_u8(rgb[0], rgb[1], rgb[2])
}

fn orbit_point(orbit: &Orbit, step: usize, steps: usize) -> DVec3 {
    DVec3::from_array(position_at_eccentric_anomaly(
        orbit,
        step as f64 / steps as f64 * TAU,
    ))
}

const ORBIT: Color = Color::srgb(0.075, 0.17, 0.25);
/// Bodies below this on-screen radius, in logical pixels, draw as a single ring.
const RING_PIXELS: f32 = 6.0;
const LINK: Color = Color::srgb(0.12, 0.3, 0.4);

/// Keep even unusually wide systems visible at their initial fitted range and
/// while following an outer planet. Small systems retain the original cutoff.
fn planet_visibility_range(system: &StarSystem, page: u32) -> f64 {
    let outer_apoapsis = system
        .on_page(page)
        .map(|p| p.orbit.semi_major_au * (1.0 + p.orbit.eccentricity))
        .fold(0.0, f64::max);
    WIDE_METRES.max(4.0 * outer_apoapsis * space_simulation::AU_METRES)
}

/// Within this range of the visited star, in metres, its planets and body are
/// drawn. Beyond it the star is a galactic point like any other, bound by the
/// same magnitude limit, and the planets are hidden.
pub fn body_range(system: &StarSystem, pose: &CameraPose) -> Option<f64> {
    let range = planet_visibility_range(system, pose.page);
    (pose.distance <= range && pose.eye.length() <= range).then_some(range)
}

/// One frame's projection from f64 metres relative to the current star into render
/// units around the floating-origin camera.
struct Canvas<'a> {
    camera: &'a Camera,
    view: &'a GlobalTransform,
    origin: Vec3,
    scale: f64,
    pose: &'a CameraPose,
    nav: &'a Navigation,
    system: &'a StarSystem,
    /// Plane of the page on display.
    frame: DQuat,
    seconds: f64,
    photometry: &'a Photometry,
    /// The current page's planets are on screen.
    subject: bool,
}

impl Canvas<'_> {
    /// Subtracts in f64 before conversion, so small local positions are never added
    /// to an interstellar f32 coordinate, including while following a moving planet.
    fn world(&self, galactic: DVec3) -> Vec3 {
        self.origin + ((galactic - self.pose.eye) / self.scale).as_vec3()
    }

    fn units(&self, metres: f64) -> f32 {
        (metres / self.scale) as f32
    }

    fn screen(&self, point: Vec3) -> Option<Vec2> {
        self.camera.world_to_viewport(self.view, point).ok()
    }

    /// On-screen radius in logical pixels, or `None` when the centre is off-screen.
    /// Measured as if the body sat at the screen centre: perspective stretches
    /// bodies towards the edges, which would switch their line density there.
    fn pixels(&self, centre: Vec3, radius: f32) -> Option<f32> {
        self.screen(centre)?;
        let eye = self.view.translation();
        let ahead = eye + self.view.forward() * centre.distance(eye);
        let point = self.screen(ahead)?;
        let edge = self.screen(ahead + self.view.right() * radius)?;
        Some(edge.distance(point))
    }

    /// Render units per logical pixel at a point's depth, on or off screen.
    fn per_pixel(&self, centre: Vec3) -> Option<f32> {
        let eye = self.view.translation();
        let probe = centre.distance(eye) * 0.01;
        let ahead = eye + self.view.forward() * centre.distance(eye);
        let pixels = self
            .screen(ahead)?
            .distance(self.screen(ahead + self.view.right() * probe)?);
        (pixels > 0.0).then(|| probe / pixels)
    }

    /// Draws a wireframe body and returns its on-screen radius.
    /// `basis` orients the latitude rings, so bodies share their system's plane.
    /// `flat` projects the sphere onto its equatorial plane, as seen from directly
    /// above. Used for a star beside a close top-down camera, where its upper rings
    /// would otherwise reach past the camera.
    fn sphere(
        &self,
        gizmos: &mut Gizmos,
        centre: Vec3,
        radius: f32,
        tint: Color,
        basis: DQuat,
        flat: bool,
    ) -> Option<f32> {
        use std::f32::consts::{PI, TAU};
        let pixels = self.pixels(centre, radius);
        let basis = basis.as_quat();
        // Line density follows on-screen size, so small bodies stay legible instead of
        // collapsing into a solid blob of overlapping rings.
        let (latitudes, longitudes, steps) = match pixels.unwrap_or(0.0) {
            p if p < RING_PIXELS => (0, 0, 24),
            p if p < 24.0 => (1, 3, 24),
            p if p < 90.0 => (2, 6, 40),
            p => (4, 12, (p as usize / 4).clamp(96, 1024)),
        };
        let point = |v: Vec3| centre + basis * if flat { v.with_y(0.0) } else { v } * radius;
        if latitudes == 0 {
            let (right, up) = if flat {
                (basis * Vec3::X, basis * Vec3::Z)
            } else {
                (self.view.right().as_vec3(), self.view.up().as_vec3())
            };
            gizmos.linestrip(
                (0..=steps).map(|i| {
                    let a = i as f32 / steps as f32 * TAU;
                    centre + (right * a.cos() + up * a.sin()) * radius
                }),
                tint,
            );
            return pixels;
        }
        let ring =
            |phi: f32, a: f32| Vec3::new(phi.cos() * a.cos(), phi.sin(), phi.cos() * a.sin());
        let band = PI / (2 * latitudes + 2) as f32;
        for latitude in -latitudes..=latitudes {
            let phi = latitude as f32 * band;
            gizmos.linestrip(
                (0..=steps).map(|i| point(ring(phi, i as f32 / steps as f32 * TAU))),
                tint,
            );
        }
        for longitude in 0..longitudes {
            let a = longitude as f32 * PI / longitudes as f32;
            gizmos.linestrip(
                (0..=steps).map(|i| point(ring(i as f32 / steps as f32 * TAU, a))),
                tint,
            );
        }
        pixels
    }

    /// A dot about two pixels across: a ring too small to have a hole.
    fn dot(&self, gizmos: &mut Gizmos, centre: Vec3, tint: Color) {
        use std::f32::consts::TAU;
        let Some(per_pixel) = self.per_pixel(centre) else {
            return;
        };
        let (right, up) = (self.view.right().as_vec3(), self.view.up().as_vec3());
        gizmos.linestrip(
            (0..=8).map(|i| {
                let a = i as f32 / 8.0 * TAU;
                centre + (right * a.cos() + up * a.sin()) * 0.5 * per_pixel
            }),
            tint,
        );
    }

    /// A body's wireframe, unless realistic bodies are drawn without them, and its
    /// on-screen radius. Over a realistic body the wireframe sits just outside its
    /// surface, so the body hides the far side.
    #[allow(clippy::too_many_arguments)]
    fn outline(
        &self,
        gizmos: &mut Gizmos,
        centre: Vec3,
        radius: f32,
        tint: Color,
        basis: DQuat,
        flat: bool,
    ) -> Option<f32> {
        match (self.nav.realistic, self.nav.wireframe) {
            // Without realistic lighting a small body is a dot: its marker is the ring.
            (false, _) => match self.pixels(centre, radius) {
                Some(p) if p < RING_PIXELS => {
                    self.dot(gizmos, centre, tint);
                    Some(p)
                }
                _ => self.sphere(gizmos, centre, radius, tint, basis, flat),
            },
            // A small realistic body is its own point: a ring would only hide it.
            (true, true) => match self.pixels(centre, radius) {
                Some(p) if p < RING_PIXELS => Some(p),
                _ => self.sphere(gizmos, centre, radius * 1.01, tint, basis, flat),
            },
            (true, false) => self.pixels(centre, radius),
        }
    }

    /// Joins the current list's page stars where they already are, like a star
    /// chart: each star links to its nearest neighbour in a minimum spanning tree.
    fn constellation(&self, gizmos: &mut Gizmos) {
        let stars = page_offsets(self.system, self.pose.page);
        let Some(&first) = stars.first() else {
            return;
        };
        let mut joined = vec![false; stars.len()];
        let mut nearest: Vec<_> = stars.iter().map(|s| ((*s - first).length(), 0)).collect();
        joined[0] = true;
        for _ in 1..stars.len() {
            let Some(next) = (0..stars.len())
                .filter(|&i| !joined[i])
                .min_by(|&a, &b| nearest[a].0.total_cmp(&nearest[b].0))
            else {
                break;
            };
            joined[next] = true;
            gizmos.line(
                self.world(stars[nearest[next].1]),
                self.world(stars[next]),
                LINK,
            );
            for i in 0..stars.len() {
                let distance = (stars[i] - stars[next]).length();
                if !joined[i] && distance < nearest[i].0 {
                    nearest[i] = (distance, next);
                }
            }
        }
    }

    /// The visited page's star, within `body_range`. Every other star, including
    /// the rest of its list, is a streamed galactic point.
    fn star(&self, gizmos: &mut Gizmos, lights: &mut Lights) {
        let (nav, pose, system) = (self.nav, self.pose, self.system);
        let wide = pose.distance > WIDE_METRES;
        let table = pose.layout > 0.5;
        let relative = -pose.eye;
        let centre = self.world(DVec3::ZERO);
        let body = system.star(pose.page);
        let physical = body.map_or(SUN_RADIUS_METRES, |b| b.radius_solar * SUN_RADIUS_METRES);
        let tint = body.map_or([255, 192, 108], |b| b.colour);
        let basis = match body {
            Some(b) => spin_basis(self.frame, &b.spin, self.seconds),
            None => self.frame,
        };
        let radius = if wide && !table {
            relative.length() * 0.004
        } else if nav.exaggerate && !table {
            (pose.distance * 0.008).max(physical)
        } else {
            physical
        };
        if nav.realistic
            && let Some(body) = body
            && let Some(per_pixel) = self.per_pixel(centre)
        {
            let surface = self.photometry.surface_radiance(body);
            // An enlarged star dims to keep its light. From afar it keeps its
            // true angular size.
            let (disc, radiance) = if wide && !table {
                (physical, surface)
            } else {
                (radius, surface * (physical / radius).powi(2))
            };
            let disc = self.units(disc);
            lights.stars.push(StarLight {
                centre,
                radius: disc,
                pixels: disc / per_pixel,
                per_pixel,
                radiance,
                kelvin: body.temperature_k,
                subject: self.subject,
                spin: basis.as_quat(),
                granules: render::granules(body),
            });
        }
        self.outline(
            gizmos,
            centre,
            self.units(radius),
            colour(tint),
            basis,
            table,
        );
    }

    /// The destination page's planets, blending between running orbits and the
    /// table row while the camera flies. Orbital radii are never presentation-scaled.
    fn planets(
        &self,
        gizmos: &mut Gizmos,
        seconds: f64,
        markers: &mut Vec<Marker>,
        lights: &mut Lights,
    ) {
        let (nav, pose, system) = (self.nav, self.pose, self.system);
        let layout = TableLayout::new(system, pose.page);
        let place = |local: DVec3| self.frame * local;
        if nav.orbits && nav.view == View::Orbit && pose.layout <= 0.5 {
            for planet in system.on_page(pose.page) {
                gizmos.linestrip(
                    (0..=192).map(|step| self.world(place(orbit_point(&planet.orbit, step, 192)))),
                    ORBIT,
                );
            }
        }
        for (index, planet) in system.planets.iter().enumerate() {
            if planet.page() != pose.page {
                continue;
            }
            let orbital =
                DVec3::from_array(position(&planet.orbit, system.mass(pose.page), seconds));
            let galactic = place(orbital.lerp(layout.position(planet), pose.layout));
            let centre = self.world(galactic);
            let physical = planet.radius_km * 1000.0;
            // Exaggerated sizes keep the cube root of the real ratio, so giants still
            // dwarf rocky planets without hiding their neighbours.
            let orbital_radius = if nav.exaggerate {
                let size = (planet.radius_km / space_simulation::EARTH_RADIUS_KM).cbrt();
                physical.max((galactic - pose.eye).length() * 0.012 * size)
            } else {
                physical
            };
            let radius = orbital_radius + (physical - orbital_radius) * pose.layout;
            let tint = colour(planet.colour());
            let basis = spin_basis(self.frame, &planet.spin, seconds);
            if nav.realistic
                && let Some(per_pixel) = self.per_pixel(centre)
            {
                let luminosity = system
                    .star(pose.page)
                    .map_or(1.0, |s| self.photometry.visual_luminosity(s));
                let disc = self.units(radius);
                lights.planets.push(PlanetLight {
                    centre,
                    radius: disc,
                    pixels: disc / per_pixel,
                    per_pixel,
                    // Lit from the star even in the table, which is only a layout.
                    towards_star: (-galactic).normalize_or_zero().as_vec3(),
                    flux: space_simulation::light::flux(luminosity, orbital.length()),
                    surface: space_simulation::light::surface(&planet.class),
                });
            }
            let pixels = self.outline(gizmos, centre, self.units(radius), tint, basis, false);
            if let Some(point) = self.screen(centre) {
                markers.push(Marker {
                    point,
                    radius: pixels.unwrap_or(6.0).max(5.0),
                    label: planet.label().to_owned(),
                    colour: planet.colour(),
                    planet: index,
                });
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub fn draw(
    mut gizmos: Gizmos,
    universe: Res<Universe>,
    nav: Res<Navigation>,
    clock: Res<Clock>,
    pose: Res<CameraPose>,
    camera: Query<(&Camera, &GlobalTransform), With<SpaceCamera>>,
    mut markers: ResMut<Markers>,
    light: (ResMut<Lights>, Res<Photometry>),
) {
    let (mut lights, photometry) = light;
    markers.0.clear();
    lights.clear();
    let Ok((camera, view)) = camera.single() else {
        return;
    };
    lights.eye = view.translation();
    let Some(system) = nav.system(&universe.0) else {
        return;
    };
    // During a page-to-page flight the camera may still be near the departure
    // star. Do not draw the destination's AU-scale orbits from interstellar range.
    let planets_shown = body_range(system, &pose).is_some();
    let canvas = Canvas {
        camera,
        view,
        origin: view.translation(),
        scale: pose.render_scale(),
        pose: &pose,
        nav: &nav,
        system,
        frame: page_frame(system, pose.page),
        seconds: clock.0.seconds,
        photometry: &photometry,
        subject: planets_shown,
    };
    let markers = &mut markers.0;
    if nav.view == View::Constellation {
        canvas.constellation(&mut gizmos);
    }
    if planets_shown {
        canvas.star(&mut gizmos, &mut lights);
        canvas.planets(&mut gizmos, clock.0.seconds, markers, &mut lights);
        let luminosity = system
            .star(pose.page)
            .map_or(1.0, |s| photometry.visual_luminosity(s));
        let followed = nav
            .selected
            .filter(|_| nav.following)
            .and_then(|i| system.planets.get(i))
            .filter(|p| p.page() == pose.page)
            .map(|p| {
                DVec3::from_array(position(&p.orbit, system.mass(pose.page), clock.0.seconds))
                    .length()
            });
        let reference = render::reference_distance(system, pose.page, followed, luminosity);
        lights.subject_key = Some(render::subject_key(luminosity, reference));
        lights.metered = Some((reference, followed.is_some()));
        lights.kelvin = system.star(pose.page).map_or(5772.0, |s| s.temperature_k);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use space_simulation::AU_METRES;

    #[test]
    fn synchronous_spin_keeps_one_face_towards_star() {
        let orbit = Orbit {
            semi_major_au: 0.1,
            eccentricity: 0.0,
            inclination_rad: 0.0,
            ascending_node_rad: 0.0,
            phase_rad: 0.0,
        };
        let period = space_simulation::period_seconds(&orbit, 1.0);
        let spin = Spin {
            period_hours: period / 3_600.0,
            tilt_rad: 0.0,
            azimuth_rad: 0.0,
            phase_rad: 0.0,
            locked: true,
        };
        let facing = -DVec3::X;
        for fraction in [0.0, 0.125, 0.25, 0.5, 0.75] {
            let seconds = fraction * period;
            let towards_star = -DVec3::from_array(position(&orbit, 1.0, seconds)).normalize();
            let surface = spin_basis(DQuat::IDENTITY, &spin, seconds) * facing;
            assert!(surface.dot(towards_star) > 1.0 - 1e-12);
        }
    }

    #[test]
    fn wide_orbits_remain_visible_at_their_fitted_camera_range() {
        let mut system = space_simulation::demo_galaxy().systems.remove(0);
        let page = system.first_page();
        let normal = planet_visibility_range(&system, page);
        system.planets[0].orbit.semi_major_au = 5_183.534;
        system.planets[0].orbit.eccentricity = 0.057;
        let apoapsis = 5_183.534 * 1.057 * AU_METRES;
        let wide = planet_visibility_range(&system, page);
        assert!(wide > 0.02 * LY_METRES);
        assert!(wide >= 3.1 * apoapsis);
        assert!(wide > normal);
    }

    #[test]
    fn constellation_pages_have_independent_planes_and_local_coordinates() {
        let system = &space_simulation::demo_galaxy().systems[0];
        let poles: Vec<_> = (0..5)
            .map(|page| page_frame(system, page) * DVec3::Y)
            .collect();
        for pole in &poles {
            assert!((pole.length() - 1.0).abs() < 1e-12);
        }
        assert!(poles.windows(2).all(|pair| pair[0].dot(pair[1]) < 0.99));
        let orbit = &system.planets[0].orbit;
        for page in 0..5 {
            let frame = page_frame(system, page);
            let point = orbit_point(orbit, 7, 48);
            // Drawing and navigation use the same f64 transform. Rotation changes
            // the plane, never the orbit's radius or the table's local ordering.
            assert!(((frame * point).length() - point.length()).abs() < 0.001);
        }
    }
}
