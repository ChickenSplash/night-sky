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
    LY_METRES, SUN_RADIUS_METRES,
    galaxy::{GalaxyModel, PC_METRES},
    plane_tilt, position, position_at_eccentric_anomaly, spin_angle,
};
use std::f64::consts::TAU;

/// Beyond this camera range stars draw at their true separation. Planet
/// visibility also depends on the actual size of the current page's system.
const WIDE_METRES: f64 = 0.02 * LY_METRES;
const APPROACH_METRES: f64 = 0.02 * LY_METRES;
const LEAVE_METRES: f64 = 0.04 * LY_METRES;
/// Keep nearby streamed points well clear of the camera's near clip plane even
/// when the selected camera range is interstellar.
const PRECISE_SCALE_PC: f64 = 20.0 * LY_METRES / PC_METRES;
pub const MIN_SPEED: f64 = 1.0;
pub const MAX_SPEED: f64 = 1000.0 * LY_METRES;

fn scroll_speed(speed: f64, scroll: f32) -> f64 {
    (speed * (f64::from(scroll) * 0.35).exp()).clamp(MIN_SPEED, MAX_SPEED)
}

/// Use larger units only once the value reaches one whole unit.
pub fn speed_label(metres_per_second: f64) -> String {
    let (value, unit) = if metres_per_second >= LY_METRES {
        (metres_per_second / LY_METRES, "ly/s")
    } else if metres_per_second >= space_simulation::AU_METRES {
        (metres_per_second / space_simulation::AU_METRES, "AU/s")
    } else if metres_per_second >= 1000.0 {
        (metres_per_second / 1000.0, "km/s")
    } else {
        (metres_per_second, "m/s")
    };
    format!("{value:.2} {unit}")
}

/// Camera velocity in galactic axes, measured from displayed positions rather
/// than the requested movement, so flights and turns are accounted for.
#[derive(Resource, Default)]
pub struct CameraVelocity {
    pub metres_per_second: DVec3,
    pub marker_metres_per_second: DVec3,
    previous: Option<(DVec3, DVec3)>,
}

/// Right-drag moves the eye around a fixed target. That is real camera motion,
/// but should not steer the travel-direction marker. Keyboard travel can still
/// happen during the drag, so retain its precise displacement separately.
#[derive(Resource, Default)]
pub struct CameraMotion {
    orbiting: bool,
    travel_metres: DVec3,
}

pub fn measure_velocity(
    time: Res<Time>,
    rig: Res<CameraRig>,
    pose: Res<CameraPose>,
    motion: Res<CameraMotion>,
    mut velocity: ResMut<CameraVelocity>,
) {
    let current = (rig.frame_ly, pose.eye);
    let dt = time.delta_secs_f64();
    velocity.metres_per_second = if let Some((frame, eye)) = velocity.previous {
        if dt > 0.0 {
            if frame != rig.frame_ly {
                // Switching a star-relative frame subtracts interstellar-sized
                // numbers. Do not interpret the rounding residual as motion.
                velocity.previous = Some(current);
                velocity.metres_per_second = DVec3::ZERO;
                velocity.marker_metres_per_second = DVec3::ZERO;
                return;
            }
            // Subtract local eyes first: adding the distant galactic origin to
            // each position would lose metre-scale motion to f64 precision.
            (pose.eye - eye) / dt
        } else {
            DVec3::ZERO
        }
    } else {
        DVec3::ZERO
    };
    velocity.marker_metres_per_second = if motion.orbiting {
        if dt > 0.0 {
            motion.travel_metres / dt
        } else {
            DVec3::ZERO
        }
    } else {
        velocity.metres_per_second
    };
    velocity.previous = Some(current);
}

#[derive(Component)]
pub struct SpaceCamera;

/// The displayed camera, which may still be flying towards `Navigation`'s target.
#[derive(Resource, Default)]
pub struct CameraPose {
    /// Metres relative to the current star, retained in double precision.
    pub eye: DVec3,
    pub distance: f64,
    /// Local rendering scale may tighten near a body without changing zoom or speed.
    pub scale_distance: f64,
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
        self.scale_distance.max(20_000_000.0) / 1000.0
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

/// An implicit system visit, preserving the explicit selection for when the
/// camera leaves. It never starts a flight or re-frames the camera.
#[derive(Resource, Default)]
pub struct NearbySystem {
    id: Option<String>,
    prior: Option<(Option<crate::Visit>, u32)>,
}

fn rebase(
    nav: &mut Navigation,
    rig: &CameraRig,
    displayed_eye: DVec3,
    lists: &space_shared::Galaxy,
    next: Option<crate::Visit>,
    page: u32,
) {
    let old_tilt = nav
        .system(lists)
        .map_or(DQuat::IDENTITY, |s| page_frame(s, nav.frame_page(s)));
    let old_frame = rig.frame_ly;
    let new_system = match &next {
        Some(crate::Visit::List(index)) => lists.systems.get(*index),
        Some(crate::Visit::Generated(system)) => Some(system.as_ref()),
        None => None,
    };
    let new_tilt = new_system.map_or(DQuat::IDENTITY, |s| page_frame(s, page));
    let new_frame = new_system.map_or(DVec3::ZERO, |s| page_origin_ly(s, page));
    let old_eye = nav
        .eye
        .unwrap_or_else(|| old_tilt.inverse() * displayed_eye);
    let new_eye = new_tilt.inverse() * ((old_frame - new_frame) * LY_METRES + old_tilt * old_eye);
    nav.rotation = ((new_tilt.inverse() * old_tilt).as_quat() * nav.rotation).normalize();
    // Quat is f32: changing its basis can rotate the distance-sized camera
    // offset slightly. Reconstruct the target from the eye, not vice versa.
    nav.target = new_eye - (nav.rotation * Vec3::Z).as_dvec3() * nav.distance;
    nav.eye = Some(new_eye);
    nav.visit = next;
    nav.page = page;
    nav.selected = None;
    nav.following = false;
}

/// Use the same star body and planetary system when arriving by free flight as
/// when arriving by clicking Visit. Only cells already streamed in are eligible.
pub fn nearby_system(
    field: Res<crate::stream::Starfield>,
    lists: Res<Universe>,
    rig: Res<CameraRig>,
    pose: Res<CameraPose>,
    mut nav: ResMut<Navigation>,
    mut nearby: ResMut<NearbySystem>,
) {
    if nav.fly {
        // A deliberate visit supersedes the implicit one.
        nearby.id = None;
        nearby.prior = None;
    }
    if nav.view != View::Orbit || rig.flying() || nav.fly {
        return;
    }
    let eye_ly = rig.galactic_ly(&pose);
    if nearby.id.is_some() {
        let current = nav.system(&lists.0).map(|s| page_origin_ly(s, nav.page));
        if current.is_some_and(|star| (star - eye_ly).length() * LY_METRES < LEAVE_METRES) {
            return;
        }
        let (prior, page) = nearby.prior.take().unwrap_or((None, 0));
        rebase(&mut nav, &rig, pose.eye, &lists.0, prior, page);
        nearby.id = None;
        // Let place_camera switch the displayed frame before another handoff.
        return;
    }
    // If the explicitly visited star is close, it already owns the body handoff.
    if nav
        .system(&lists.0)
        .is_some_and(|s| (page_origin_ly(s, nav.page) - eye_ly).length() * LY_METRES < LEAVE_METRES)
    {
        return;
    }
    let eye_pc =
        DVec3::from_array(GalaxyModel::milky_way().home_pc) + eye_ly * (LY_METRES / PC_METRES);
    let reach_pc = APPROACH_METRES / PC_METRES;
    let mut closest = None;
    for (&cell, stars) in &field.cells {
        if space_simulation::stream::gap_pc(cell, eye_pc.to_array()) > reach_pc {
            continue;
        }
        for star in &stars.stars {
            let distance = (DVec3::from_array(star.position_pc) - eye_pc).length();
            if distance >= reach_pc {
                continue;
            }
            let id = star.id(cell);
            if closest.as_ref().is_none_or(|(best, old_id, _)| {
                distance < *best || (distance == *best && id < *old_id)
            }) {
                closest = Some((distance, id, star.position_pc));
            }
        }
    }
    let Some((_, id, position_pc)) = closest else {
        return;
    };
    let position_ly = (DVec3::from_array(position_pc)
        - DVec3::from_array(GalaxyModel::milky_way().home_pc))
        * (PC_METRES / LY_METRES);
    let (visit, page) = if let Some((list, page)) = crate::galaxy_nav::mapped(&lists.0, &id) {
        (crate::Visit::List(list), page)
    } else if let Some(system) = crate::galaxy_nav::generated(&id, position_ly) {
        (crate::Visit::Generated(Box::new(system)), 0)
    } else {
        return;
    };
    nearby.prior = Some((nav.visit.clone(), nav.page));
    rebase(&mut nav, &rig, pose.eye, &lists.0, Some(visit), page);
    nearby.id = Some(id);
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
    commands.init_resource::<NearbySystem>();
    commands.init_resource::<CameraVelocity>();
    commands.init_resource::<CameraMotion>();
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
    mut camera_motion: ResMut<CameraMotion>,
) {
    camera_motion.orbiting = false;
    camera_motion.travel_metres = DVec3::ZERO;
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
    // orbit their auto-framed centre, but cannot zoom or translate.
    let free = nav.view == View::Orbit && !rig.flying();
    let rotating = matches!(nav.view, View::Orbit | View::Constellation) && !rig.flying();
    if !pointer && rotating {
        if buttons.pressed(MouseButton::Left) && free {
            let tilt = nav
                .system(&universe.0)
                .map_or(DQuat::IDENTITY, |s| page_frame(s, nav.frame_page(s)));
            drag_look(&mut nav, motion.delta, Some(tilt.inverse() * pose.eye));
        } else if buttons.pressed(MouseButton::Right) {
            nav.eye = None;
            orbit(&mut nav, motion.delta);
            camera_motion.orbiting = free;
        }
        if free && keys.pressed(KeyCode::ShiftLeft) {
            nav.move_speed = scroll_speed(nav.move_speed, scroll);
        } else if free && scroll != 0.0 {
            let tilt = nav
                .system(&universe.0)
                .map_or(DQuat::IDENTITY, |s| page_frame(s, nav.frame_page(s)));
            let old_eye = nav.eye.unwrap_or_else(|| tilt.inverse() * pose.eye);
            let old_distance = nav.distance;
            nav.distance = (nav.distance * (-f64::from(scroll) * 0.12).exp())
                .clamp(20_000_000.0, 40_000.0 * LY_METRES);
            let eye = old_eye + (nav.rotation * Vec3::Z).as_dvec3() * (nav.distance - old_distance);
            nav.eye = Some(eye);
            nav.target = eye - (nav.rotation * Vec3::Z).as_dvec3() * nav.distance;
        }
    }
    if !keyboard {
        if keys.just_pressed(KeyCode::KeyC) {
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
            axis(KeyCode::KeyR, KeyCode::KeyF),
            axis(KeyCode::KeyS, KeyCode::KeyW),
        );
        let roll_input = axis(KeyCode::KeyQ, KeyCode::KeyE) as f32;
        if roll_input != 0.0 && free {
            roll(&mut nav, roll_input * time.delta_secs() * 1.5);
        }
        if movement.length_squared() > 0.0 && free {
            nav.following = false;
            let tilt = nav
                .system(&universe.0)
                .map_or(DQuat::IDENTITY, |s| page_frame(s, nav.frame_page(s)));
            let rotation = nav.rotation;
            let step = (rotation * movement.normalize().as_vec3()).as_dvec3()
                * nav.move_speed
                * time.delta_secs_f64();
            camera_motion.travel_metres += tilt * step;
            let eye = nav.eye.get_or_insert_with(|| tilt.inverse() * pose.eye);
            *eye += step;
            nav.target = *eye - (rotation * Vec3::Z).as_dvec3() * nav.distance;
        }
    }
}

/// Roll in camera-local space, not around the galaxy's vertical axis. Rolling
/// leaves the line of sight and the camera's position unchanged at any pitch.
fn roll(nav: &mut Navigation, angle: f32) {
    nav.rotation = (nav.rotation * Quat::from_rotation_z(angle)).normalize();
    nav.following = false;
}

fn drag_look(nav: &mut Navigation, delta: Vec2, displayed_eye: Option<DVec3>) {
    if delta == Vec2::ZERO {
        return;
    }
    let eye = *nav.eye.get_or_insert_with(|| {
        displayed_eye
            .unwrap_or_else(|| nav.target + (nav.rotation * Vec3::Z).as_dvec3() * nav.distance)
    });
    // Both axes are local to the *current camera*. At a pole or upside down,
    // screen right is not galactic right; world-axis yaw behaves like roll there.
    let turn = Quat::from_rotation_y(-delta.x * 0.005) * Quat::from_rotation_x(-delta.y * 0.005);
    nav.rotation = (nav.rotation * turn).normalize();
    nav.target = eye - (nav.rotation * Vec3::Z).as_dvec3() * nav.distance;
    nav.following = false;
}

/// Right-drag keeps its target fixed and turns around the system's vertical.
fn orbit(nav: &mut Navigation, delta: Vec2) {
    if delta == Vec2::ZERO {
        return;
    }
    nav.rotation = (Quat::from_rotation_y(-delta.x * 0.005)
        * nav.rotation
        * Quat::from_rotation_x(-delta.y * 0.005))
    .normalize();
}

#[allow(clippy::too_many_arguments)]
pub fn place_camera(
    time: Res<Time>,
    universe: Res<Universe>,
    field: Res<crate::stream::Starfield>,
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
    if nav.fly || nav.following || nav.view != View::Orbit {
        nav.eye = None;
    }
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
        rotation: frame_tilt.as_quat() * nav.rotation,
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
    pose.eye = if rig.flight.is_none() {
        nav.eye.map_or_else(
            || shown.target + (rotation * Vec3::Z).as_dvec3() * shown.distance,
            |eye| frame_tilt * eye,
        )
    } else {
        shown.target + (rotation * Vec3::Z).as_dvec3() * shown.distance
    };
    pose.distance = shown.distance;
    // The camera range is only a framing control. A streamed point can need
    // camera-relative precision well before its planets/body are activated.
    let eye_pc = DVec3::from_array(GalaxyModel::milky_way().home_pc)
        + frame * (LY_METRES / PC_METRES)
        + pose.eye / PC_METRES;
    pose.scale_distance = proximity_scale_distance(&field, eye_pc, shown.distance);
    if system.is_some_and(|s| pose.eye.length() <= planet_visibility_range(s, frame_page)) {
        pose.scale_distance = pose.scale_distance.min(pose.eye.length().max(20_000_000.0));
    }
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

fn proximity_scale_distance(
    field: &crate::stream::Starfield,
    eye_pc: DVec3,
    selected_range: f64,
) -> f64 {
    let mut scale = selected_range;
    for (&cell, stars) in &field.cells {
        if space_simulation::stream::gap_pc(cell, eye_pc.to_array()) > PRECISE_SCALE_PC {
            continue;
        }
        for star in &stars.stars {
            let distance = DVec3::from_array(star.position_pc).distance(eye_pc);
            if distance < PRECISE_SCALE_PC {
                scale = scale.min(distance * PC_METRES);
            }
        }
    }
    scale.max(20_000_000.0)
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
    (pose.eye.length() <= range).then_some(range)
}

/// Exaggeration is a screen-space aid, not an orbit-sized shell tied to zoom.
/// Once the camera is close, even a 40,000 ly range must not put the camera
/// inside a star inflated to hundreds of light years across.
fn star_display_radius(
    pose: &CameraPose,
    physical: f64,
    exaggerate: bool,
    table: bool,
    wide: bool,
) -> f64 {
    if table {
        physical
    } else if wide {
        (pose.eye.length() * 0.004).max(physical)
    } else if exaggerate {
        (pose.distance.min(pose.eye.length()) * 0.008).max(physical)
    } else {
        physical
    }
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
        let wide = pose.distance > WIDE_METRES && pose.eye.length() > WIDE_METRES;
        let table = pose.layout > 0.5;
        let centre = self.world(DVec3::ZERO);
        let body = system.star(pose.page);
        let physical = body.map_or(SUN_RADIUS_METRES, |b| b.radius_solar * SUN_RADIUS_METRES);
        let tint = body.map_or([255, 192, 108], |b| b.colour);
        let basis = match body {
            Some(b) => spin_basis(self.frame, &b.spin, self.seconds),
            None => self.frame,
        };
        let radius = star_display_radius(pose, physical, nav.exaggerate, table, wide);
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
    fn drifting_into_a_streamed_star_preserves_the_eye_and_does_not_fly() {
        use space_simulation::{
            cells::{Cell, Sky, parse_cell_star},
            stream::{StarCell, StarPoint},
        };
        let home = DVec3::from_array(GalaxyModel::milky_way().home_pc);
        let cell = Cell::containing(10, home.to_array()).unwrap();
        let mut sky = Sky::default();
        let id = sky.cell_star_at(cell, 0).unwrap().id;
        let star = sky.star(&id).unwrap();
        let position_ly = (DVec3::from_array(star.position_pc) - home) * (PC_METRES / LY_METRES);
        let mut field = crate::stream::Starfield::default();
        field.cells.insert(
            cell,
            StarCell {
                cell,
                stars: vec![StarPoint {
                    position_pc: star.position_pc,
                    luminosity_solar: star.star.luminosity_solar as f32,
                    temperature_k: star.star.temperature_k as f32,
                    radius_solar: star.star.radius_solar as f32,
                    component: parse_cell_star(&id).unwrap().1,
                    index: parse_cell_star(&id).unwrap().2 as u32,
                }],
            },
        );
        let mut nav = Navigation {
            rotation: Quat::IDENTITY,
            distance: 40_000.0 * LY_METRES,
            ..default()
        };
        let eye = position_ly * LY_METRES + DVec3::X * (APPROACH_METRES * 0.5);
        nav.target = eye - DVec3::Z * nav.distance;
        let pose = CameraPose {
            eye,
            distance: nav.distance,
            ..default()
        };
        let mut app = App::new();
        app.insert_resource(field)
            .insert_resource(Universe(space_simulation::demo_galaxy()))
            .insert_resource(CameraRig::default())
            .insert_resource(pose)
            .insert_resource(nav)
            .init_resource::<NearbySystem>()
            .add_systems(Update, nearby_system);
        app.update();
        let nav = app.world().resource::<Navigation>();
        assert_eq!(
            nav.system(&app.world().resource::<Universe>().0)
                .unwrap()
                .page_star(0)
                .unwrap()
                .id,
            id
        );
        assert!(!nav.fly);
        let rig = app.world().resource::<CameraRig>();
        let system = nav.system(&app.world().resource::<Universe>().0).unwrap();
        let actual_eye = page_origin_ly(system, 0) * LY_METRES
            + page_frame(system, 0)
                * (nav.target + (nav.rotation * Vec3::Z).as_dvec3() * nav.distance);
        assert!((actual_eye - eye).length() < 100_000.0);
        assert_eq!(rig.frame_ly, DVec3::ZERO);
        let tilt = page_frame(system, 0);

        // Leaving the system restores open space without moving the eye.
        let departing_eye = DVec3::X * (LEAVE_METRES * 1.1);
        let frame = position_ly;
        app.world_mut().resource_mut::<CameraRig>().frame_ly = frame;
        app.world_mut().resource_mut::<CameraPose>().eye = departing_eye;
        {
            let mut nav = app.world_mut().resource_mut::<Navigation>();
            nav.target =
                tilt.inverse() * departing_eye - (nav.rotation * Vec3::Z).as_dvec3() * nav.distance;
            nav.eye = Some(tilt.inverse() * departing_eye);
        }
        app.update();
        let nav = app.world().resource::<Navigation>();
        assert!(nav.visit.is_none());
        let restored_eye = nav.eye.unwrap();
        assert!((restored_eye - (frame * LY_METRES + departing_eye)).length() < 100_000.0);
    }

    #[test]
    fn close_body_is_visible_without_zooming_camera_range() {
        let galaxy = space_simulation::demo_galaxy();
        let system = &galaxy.systems[0];
        let pose = CameraPose {
            distance: 120.0 * LY_METRES,
            eye: DVec3::X * (APPROACH_METRES * 0.5),
            scale_distance: APPROACH_METRES * 0.5,
            ..default()
        };
        assert!(body_range(system, &pose).is_some());
        assert!(pose.render_scale() < pose.distance / 1000.0);
    }

    #[test]
    fn close_star_exaggeration_does_not_depend_on_maximum_camera_range() {
        let eye = DVec3::X * (APPROACH_METRES * 0.5);
        let close = CameraPose {
            eye,
            distance: 100.0 * LY_METRES,
            ..default()
        };
        let maximum = CameraPose {
            distance: 40_000.0 * LY_METRES,
            ..close
        };
        let physical = SUN_RADIUS_METRES;
        let radius = star_display_radius(&maximum, physical, true, false, false);
        assert_eq!(
            radius,
            star_display_radius(&close, physical, true, false, false)
        );
        assert!(radius < eye.length());
        assert_eq!(
            star_display_radius(&maximum, physical, false, false, false),
            physical
        );
    }

    #[test]
    fn streamed_star_tightens_render_scale_before_system_handoff_at_max_range() {
        use space_simulation::{
            cells::Cell,
            galaxy::Component,
            stream::{StarCell, StarPoint},
        };
        let star_pc = DVec3::from_array(GalaxyModel::milky_way().home_pc);
        let cell = Cell::containing(10, star_pc.to_array()).unwrap();
        let mut field = crate::stream::Starfield::default();
        field.cells.insert(
            cell,
            StarCell {
                cell,
                stars: vec![StarPoint {
                    position_pc: star_pc.to_array(),
                    luminosity_solar: 1.0,
                    temperature_k: 5772.0,
                    radius_solar: 1.0,
                    component: Component::ThinDisc,
                    index: 0,
                }],
            },
        );
        let maximum = 40_000.0 * LY_METRES;
        let eye = star_pc + DVec3::X * (6.0 * LY_METRES / PC_METRES);
        let scale = proximity_scale_distance(&field, eye, maximum);
        assert!((scale / LY_METRES - 6.0).abs() < 1e-9);
        assert_eq!(
            proximity_scale_distance(
                &field,
                eye + DVec3::X * 30.0 * LY_METRES / PC_METRES,
                maximum
            ),
            maximum
        );
    }

    #[test]
    fn speed_scroll_is_independent_of_range_and_bounded() {
        let mut nav = Navigation::default();
        let range = nav.distance;
        nav.move_speed = scroll_speed(nav.move_speed, 3.0);
        assert!(nav.move_speed > 42.0 * LY_METRES);
        assert_eq!(nav.distance, range);
        nav.move_speed = scroll_speed(nav.move_speed, -3.0);
        assert!((nav.move_speed / (42.0 * LY_METRES) - 1.0).abs() < 1e-6);
        assert_eq!(scroll_speed(MIN_SPEED, -1000.0), MIN_SPEED);
        assert_eq!(scroll_speed(MAX_SPEED, 1000.0), MAX_SPEED);
    }

    #[test]
    fn speed_units_follow_the_distance_ladder() {
        assert_eq!(speed_label(42.0), "42.00 m/s");
        assert_eq!(speed_label(42_000.0), "42.00 km/s");
        assert_eq!(speed_label(2.0 * AU_METRES), "2.00 AU/s");
        assert_eq!(speed_label(2.0 * LY_METRES), "2.00 ly/s");
    }

    #[test]
    fn free_look_holds_the_eye_still_and_turns_past_both_poles() {
        let mut nav = Navigation {
            target: DVec3::new(2.0, 3.0, -5.0) * AU_METRES,
            distance: 10.0 * AU_METRES,
            rotation: Quat::from_rotation_x(-1.4),
            ..Navigation::default()
        };
        let eye = nav.target + (nav.rotation * Vec3::Z).as_dvec3() * nav.distance;
        drag_look(&mut nav, Vec2::new(80.0, 400.0), None);
        let expected =
            Quat::from_rotation_x(-1.4) * Quat::from_rotation_y(-0.4) * Quat::from_rotation_x(-2.0);
        assert!(nav.rotation.angle_between(expected) < 1e-5);
        let after = nav.target + (nav.rotation * Vec3::Z).as_dvec3() * nav.distance;
        assert!(eye.distance(after) < nav.distance * 1e-7);
        drag_look(&mut nav, Vec2::new(-80.0, 200.0), None);
        let after = nav.target + (nav.rotation * Vec3::Z).as_dvec3() * nav.distance;
        assert!(eye.distance(after) < nav.distance * 1e-7);
    }

    #[test]
    fn distant_range_free_look_keeps_precise_eye_through_repeated_turns() {
        let eye = DVec3::new(13_000_000.0, -24_000_000.0, 51_000_000.0);
        let mut nav = Navigation {
            distance: 120.0 * LY_METRES,
            rotation: Quat::IDENTITY,
            target: eye - DVec3::Z * (120.0 * LY_METRES),
            ..default()
        };
        for _ in 0..50 {
            drag_look(&mut nav, Vec2::new(2.0, -1.0), Some(eye));
            assert_eq!(nav.eye, Some(eye));
        }
        // The target is only an orbit/flight compatibility representation;
        // subtracting the large offset from it loses precision near the star.
        assert_eq!(nav.eye.unwrap(), eye);
    }

    #[test]
    fn turning_in_place_has_no_measured_velocity() {
        let mut app = App::new();
        app.insert_resource(Time::<()>::default())
            .insert_resource(CameraRig::default())
            .insert_resource(CameraPose {
                eye: DVec3::new(13_000_000.0, -24_000_000.0, 51_000_000.0),
                distance: 120.0 * LY_METRES,
                ..default()
            })
            .init_resource::<CameraMotion>()
            .init_resource::<CameraVelocity>()
            .add_systems(Update, measure_velocity);
        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(std::time::Duration::from_millis(16));
        app.update();
        // A rotation updates the camera orientation but not its precise eye.
        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(std::time::Duration::from_millis(16));
        app.update();
        assert_eq!(
            app.world().resource::<CameraVelocity>().metres_per_second,
            DVec3::ZERO
        );
    }

    #[test]
    fn orbit_motion_does_not_steer_travel_marker() {
        let mut app = App::new();
        app.insert_resource(Time::<()>::default())
            .insert_resource(CameraRig::default())
            .insert_resource(CameraPose::default())
            .init_resource::<CameraMotion>()
            .init_resource::<CameraVelocity>()
            .add_systems(Update, measure_velocity);
        let frame = std::time::Duration::from_millis(16);
        app.world_mut().resource_mut::<Time>().advance_by(frame);
        app.update();

        // The eye really moves during right-drag, but the direction marker
        // should represent keyboard travel, not motion around the orbit pivot.
        app.world_mut().resource_mut::<CameraPose>().eye = DVec3::X * 1e9;
        let travel = DVec3::Y * 160.0;
        *app.world_mut().resource_mut::<CameraMotion>() = CameraMotion {
            orbiting: true,
            travel_metres: travel,
        };
        app.world_mut().resource_mut::<Time>().advance_by(frame);
        app.update();
        let velocity = app.world().resource::<CameraVelocity>();
        assert_eq!(velocity.metres_per_second, DVec3::X * 1e9 / 0.016);
        assert_eq!(velocity.marker_metres_per_second, travel / 0.016);

        // With no keyboard travel, an orbiting camera has no travel marker.
        app.world_mut().resource_mut::<CameraPose>().eye = DVec3::X * 2e9;
        app.world_mut().resource_mut::<CameraMotion>().travel_metres = DVec3::ZERO;
        app.world_mut().resource_mut::<Time>().advance_by(frame);
        app.update();
        assert_eq!(
            app.world()
                .resource::<CameraVelocity>()
                .marker_metres_per_second,
            DVec3::ZERO
        );
    }

    #[test]
    fn orbit_turn_preserves_the_target() {
        let mut nav = Navigation::default();
        let target = nav.target;
        let before = nav.rotation;
        orbit(&mut nav, Vec2::new(-100.0, 400.0));
        assert_eq!(nav.target, target);
        assert!(before.angle_between(nav.rotation) > 1.0);
    }

    #[test]
    fn mouse_horizontal_look_uses_the_new_direction() {
        let mut nav = Navigation {
            rotation: Quat::IDENTITY,
            ..Navigation::default()
        };
        drag_look(&mut nav, Vec2::new(20.0, 0.0), None);
        assert!((nav.rotation * -Vec3::Z).x > 0.0);
    }

    #[test]
    fn drag_tracks_screen_axes_at_poles_upside_down_and_rolled() {
        for pitch in [0.0, 0.9, std::f32::consts::FRAC_PI_2, 3.0] {
            for roll_angle in [0.0, 0.7] {
                let mut nav = Navigation {
                    rotation: Quat::from_rotation_x(-pitch) * Quat::from_rotation_z(roll_angle),
                    ..Navigation::default()
                };
                let before = nav.rotation;
                drag_look(&mut nav, Vec2::new(20.0, 0.0), None);
                let change = nav.rotation * -Vec3::Z - before * -Vec3::Z;
                assert!(change.dot(before * Vec3::X) > 0.09);
                assert!(change.dot(before * Vec3::Y).abs() < 1e-5);
                let before = nav.rotation;
                drag_look(&mut nav, Vec2::new(0.0, 20.0), None);
                let change = nav.rotation * -Vec3::Z - before * -Vec3::Z;
                assert!(change.dot(before * Vec3::Y) < -0.09);
                assert!(change.dot(before * Vec3::X).abs() < 1e-5);
            }
        }
    }

    #[test]
    fn roll_rotates_the_horizon_without_turning_or_moving_at_any_pitch() {
        for pitch in [0.0, 0.9, std::f32::consts::FRAC_PI_2, 3.0] {
            let mut nav = Navigation {
                rotation: Quat::from_rotation_x(-pitch),
                distance: 10.0 * AU_METRES,
                ..Navigation::default()
            };
            let before = nav.rotation;
            let target = nav.target;
            roll(&mut nav, 0.5);
            let after = nav.rotation;
            assert_eq!(nav.target, target);
            assert!((before * Vec3::Z).distance(after * Vec3::Z) < 1e-6);
            assert!((before * Vec3::Y).distance(after * Vec3::Y) > 0.4);
        }
    }

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
