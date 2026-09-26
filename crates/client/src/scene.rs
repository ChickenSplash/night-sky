use crate::flight::{Flight, Pose, STAR_JUMP_SECONDS};
use crate::views::{TableLayout, View, page_position};
use crate::{Clock, Navigation, Universe};
use bevy::{
    input::mouse::{AccumulatedMouseMotion, MouseWheel},
    math::{DQuat, DVec3},
    prelude::*,
};
use bevy_egui::EguiContexts;
use big_space::prelude::*;
use space_shared::{Galaxy, Orbit, StarSystem};
use space_simulation::{
    LY_METRES, SUN_RADIUS_METRES, page_tilt, position, position_at_eccentric_anomaly,
};
use std::f64::consts::TAU;

/// Beyond this camera range stars draw at their true separation and planets are hidden.
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
    /// Page currently at the camera destination.
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

    #[cfg(not(target_arch = "wasm32"))]
    pub fn progress(&self) -> Option<f64> {
        self.flight.as_ref().map(Flight::progress)
    }
}

#[derive(Clone)]
pub struct Marker {
    pub point: Vec2,
    pub radius: f32,
    pub label: String,
    pub colour: [u8; 3],
    pub kind: MarkerKind,
}

#[derive(Clone, Copy)]
pub enum MarkerKind {
    Planet(usize),
    Star { system: usize, page: u32 },
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
    // Flights are short; camera input resumes when they land.
    let free = nav.view == View::Orbit && !rig.flying();
    if !pointer && free {
        if buttons.pressed(MouseButton::Right) {
            nav.yaw -= motion.delta.x * 0.005;
            nav.pitch = (nav.pitch + motion.delta.y * 0.005).clamp(-1.48, 1.48);
        }
        let min = if nav.atlas {
            0.1 * LY_METRES
        } else {
            20_000_000.0
        };
        nav.distance =
            (nav.distance * (-f64::from(scroll) * 0.12).exp()).clamp(min, 200.0 * LY_METRES);
    }
    if !keyboard {
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
    let Some(system) = universe.0.systems.get(nav.system) else {
        return;
    };
    nav.fit_preset(system);
    if nav.following
        && !nav.atlas
        && let Some(planet) = nav.selected.and_then(|i| system.tasks.get(i))
    {
        nav.target = DVec3::from_array(position(&planet.orbit, system.mass_solar, clock.0.seconds));
    }
    // Navigation, including the constellation, works in the shown page's own plane,
    // and the atlas in galactic axes. Changing frame flies the camera round.
    let frame_tilt = if nav.atlas {
        DQuat::IDENTITY
    } else {
        page_frame(system, nav.page)
    };
    let desired = Pose {
        target: if nav.atlas {
            // The atlas target is in galactic axes relative to the list anchor.
            nav.target - page_offset(system, nav.page)
        } else {
            frame_tilt * nav.target
        },
        distance: nav.distance,
        rotation: frame_tilt.as_quat()
            * Quat::from_rotation_y(nav.yaw)
            * Quat::from_rotation_x(-nav.pitch),
        layout: f64::from(u8::from(nav.view == View::Table)),
    };
    let frame = page_origin_ly(system, nav.page);
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
    if std::mem::take(&mut nav.fly) || retarget {
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
                system.id,
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
            system.id,
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
            system.id,
            nav.page + 1,
            nav.view
        );
        rig.flight = None;
    }
    rig.shown = Some(shown);
    pose.page = nav.page;

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

/// Rotation from a page's orbital-plane axes into galactic axes. Every page is its
/// own star system, so each has its own plane.
pub fn page_frame(system: &StarSystem, page: u32) -> DQuat {
    rotation(page_tilt(&system.id, page))
}

/// Galactic offset of a page star from its list's anchor, in metres.
pub fn page_offset(system: &StarSystem, page: u32) -> DVec3 {
    page_position(&system.id, page)
}

pub fn page_origin_ly(system: &StarSystem, page: u32) -> DVec3 {
    DVec3::from_array(system.position_ly) + page_offset(system, page) / LY_METRES
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

const STAR: Color = Color::srgb(1.0, 0.72, 0.36);
const ORBIT: Color = Color::srgb(0.075, 0.17, 0.25);
const LINK: Color = Color::srgb(0.12, 0.3, 0.4);

/// Close up, distant stars sit on a shell at four camera ranges, so they stay
/// visible. The shell grows with the zoom and a star joins its true position
/// once the shell reaches it. There is no zoom threshold, so no star jumps or
/// races across the screen, however far away its list is.
fn visible_star_distance(real: f64, camera_range: f64) -> f64 {
    real.min(camera_range * 4.0)
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
    flying: bool,
    system: &'a StarSystem,
    /// Plane of the page on display.
    frame: DQuat,
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
            p if p < 6.0 => (0, 0, 24),
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

    /// Joins the current list's page stars where they already are, like a star
    /// chart: each star links to its nearest neighbour in a minimum spanning tree.
    fn constellation(&self, gizmos: &mut Gizmos) {
        let system = self.system;
        let here = page_offset(system, self.nav.page);
        let stars: Vec<_> = system
            .pages()
            .iter()
            .map(|&page| page_offset(system, page) - here)
            .collect();
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

    /// Every list draws all its page stars, so no stars appear or vanish when a
    /// jump changes list. Every star can be picked.
    fn stars(&self, gizmos: &mut Gizmos, galaxy: &Galaxy, markers: &mut Vec<Marker>) {
        for (index, star) in galaxy.systems.iter().enumerate() {
            for page in star.pages() {
                self.star(gizmos, markers, star, index, page);
            }
        }
    }

    fn star(
        &self,
        gizmos: &mut Gizmos,
        markers: &mut Vec<Marker>,
        star: &StarSystem,
        index: usize,
        page: u32,
    ) {
        let (nav, pose) = (self.nav, self.pose);
        let wide = pose.distance > WIDE_METRES;
        let table = pose.layout > 0.5;
        let current = index == nav.system && page == nav.page;
        if nav.view == View::Table && !current && (!self.flying || index != nav.system) {
            return;
        }
        let offset =
            (page_origin_ly(star, page) - page_origin_ly(self.system, nav.page)) * LY_METRES;
        let relative = offset - pose.eye;
        // Both the position and the angular size transition continuously from
        // the close-up marker to the star's real interstellar position.
        let visible_distance = if current {
            relative.length()
        } else {
            visible_star_distance(relative.length(), pose.distance)
        };
        let centre =
            self.origin + (relative.normalize_or_zero() * visible_distance / self.scale).as_vec3();
        let radius = if !current {
            visible_distance * 0.004
        } else if wide {
            relative.length() * 0.004
        } else if nav.exaggerate && !table {
            (pose.distance * 0.008).max(SUN_RADIUS_METRES)
        } else {
            SUN_RADIUS_METRES
        };
        let limb = current && table && !wide;
        let pixels = self
            .sphere(
                gizmos,
                centre,
                self.units(radius),
                STAR,
                page_frame(star, page),
                limb,
            )
            .unwrap_or(0.0);
        let anchor = if limb {
            let unit = TableLayout::new(self.system, pose.page).unit;
            self.world(self.frame * DVec3::new(0.0, 0.0, SUN_RADIUS_METRES + unit))
        } else {
            centre
        };
        if let Some(point) = self.screen(anchor) {
            markers.push(Marker {
                point,
                radius: pixels.min(9.0),
                label: if index == nav.system && !current {
                    format!(
                        "System {} · {:.2} ly",
                        page + 1,
                        relative.length() / LY_METRES
                    )
                } else if wide || !current {
                    format!("{} · {:.2} ly", star.name, relative.length() / LY_METRES)
                } else {
                    star.name.clone()
                },
                colour: [255, 192, 108],
                kind: MarkerKind::Star {
                    system: index,
                    page,
                },
            });
        }
    }

    /// The destination page's planets, blending between running orbits and table
    /// rows while the camera flies. Orbital radii are never presentation-scaled.
    fn planets(&self, gizmos: &mut Gizmos, seconds: f64, markers: &mut Vec<Marker>) {
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
        for (index, planet) in system.tasks.iter().enumerate() {
            if planet.page() != pose.page {
                continue;
            }
            let orbital = DVec3::from_array(position(&planet.orbit, system.mass_solar, seconds));
            let galactic = place(orbital.lerp(layout.position(planet), pose.layout));
            let centre = self.world(galactic);
            let physical = planet.radius_km * 1000.0;
            let orbital_radius = if nav.exaggerate {
                physical.max((galactic - pose.eye).length() * 0.018)
            } else {
                physical
            };
            let radius = orbital_radius + (physical - orbital_radius) * pose.layout;
            let tint = colour(planet.colour);
            let pixels = self.sphere(gizmos, centre, self.units(radius), tint, self.frame, false);
            if let Some(point) = self.screen(centre) {
                markers.push(Marker {
                    point,
                    radius: pixels.unwrap_or(6.0).max(5.0),
                    label: planet.name.clone(),
                    colour: planet.colour,
                    kind: MarkerKind::Planet(index),
                });
            }
        }
    }
}

pub fn draw(
    mut gizmos: Gizmos,
    universe: Res<Universe>,
    navigation: (Res<Navigation>, Res<CameraRig>),
    clock: Res<Clock>,
    pose: Res<CameraPose>,
    camera: Query<(&Camera, &GlobalTransform), With<SpaceCamera>>,
    mut markers: ResMut<Markers>,
) {
    let (nav, rig) = navigation;
    markers.0.clear();
    let Ok((camera, view)) = camera.single() else {
        return;
    };
    let Some(system) = universe.0.systems.get(nav.system) else {
        return;
    };
    let canvas = Canvas {
        camera,
        view,
        origin: view.translation(),
        scale: pose.render_scale(),
        pose: &pose,
        nav: &nav,
        flying: rig.flying(),
        system,
        frame: page_frame(system, pose.page),
    };
    let markers = &mut markers.0;
    if nav.view == View::Constellation {
        canvas.constellation(&mut gizmos);
    }
    canvas.stars(&mut gizmos, &universe.0, markers);
    // During a page-to-page flight the camera may still be near the departure
    // star. Do not draw the destination's AU-scale orbits from interstellar range.
    if pose.distance <= WIDE_METRES && pose.eye.length() <= WIDE_METRES {
        canvas.planets(&mut gizmos, clock.0.seconds, markers);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use space_simulation::AU_METRES;

    #[test]
    fn constellation_pages_have_independent_planes_and_local_coordinates() {
        let system = &space_shared::demo_galaxy().systems[0];
        let poles: Vec<_> = (0..5)
            .map(|page| page_frame(system, page) * DVec3::Y)
            .collect();
        for pole in &poles {
            assert!((pole.length() - 1.0).abs() < 1e-12);
        }
        assert!(poles.windows(2).all(|pair| pair[0].dot(pair[1]) < 0.99));
        let orbit = &system.tasks[0].orbit;
        for page in 0..5 {
            let frame = page_frame(system, page);
            let point = orbit_point(orbit, 7, 48);
            // Drawing and navigation use the same f64 transform. Rotation changes
            // the plane, never the orbit's radius or the table's local ordering.
            assert!(((frame * point).length() - point.length()).abs() < 0.001);
        }
    }

    #[test]
    fn constellation_markers_and_camera_destinations_are_the_same_stars() {
        let system = &space_shared::demo_galaxy().systems[0];
        for page in 0..10 {
            let origin = (page_origin_ly(system, page) - page_origin_ly(system, 0)) * LY_METRES;
            let marker = page_offset(system, page) - page_offset(system, 0);
            // Adding and subtracting the list's interstellar position may lose a
            // few metres, but never enough to move a star on screen.
            assert!((origin - marker).length() < 128.0);
        }
    }

    #[test]
    fn distant_stars_move_with_the_zoom_whatever_their_distance() {
        // A page star in this list and a star in another list behave the same:
        // while zooming out, neither moves faster than the zoom itself.
        for real in [0.16 * LY_METRES, 4.3 * LY_METRES, 60.0 * LY_METRES] {
            let mut range = 20.0 * AU_METRES;
            let mut last = visible_star_distance(real, range);
            while range < real {
                range *= 1.01;
                let shown = visible_star_distance(real, range);
                assert!(shown / last <= 1.01 + 1e-9);
                last = shown;
            }
            assert_eq!(last, real);
        }
    }
}
