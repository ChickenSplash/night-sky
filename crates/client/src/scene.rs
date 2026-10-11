use crate::flight::{Flight, Pose, STAR_JUMP_SECONDS};
use crate::presentation::{PresentationMode, TableLayout};
use crate::render::{self, Lights, Photometry, PlanetLight, StarLight};
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
/// Free flight this close to a loaded star makes it the visited system.
const APPROACH_METRES: f64 = 0.02 * LY_METRES;
/// The camera leaves a system beyond this multiple of its `system_range`, so
/// the boundary cannot flicker.
const LEAVE_FACTOR: f64 = 2.0;
/// Keep nearby streamed points well clear of the camera's near clip plane even
/// when the selected camera range is interstellar.
const PRECISE_SCALE_PC: f64 = 20.0 * LY_METRES / PC_METRES;
pub const MIN_SPEED: f64 = 1.0;
pub const MAX_SPEED: f64 = 10_000.0 * LY_METRES;

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
    /// Keyboard controls, eased towards the pressed keys so starts, stops and
    /// turns take a moment: travel in camera axes as a fraction of
    /// `move_speed` (x, y, z), and roll as a fraction of `ROLL_SPEED` (w).
    thrust: Vec4,
}

/// Time constant for keyboard controls: about 95% of a change within 0.15 s.
const THRUST_EASE_SECONDS: f32 = 0.05;
/// Q/E roll rate at full thrust, in radians per second.
const ROLL_SPEED: f32 = 1.5;

fn ease_thrust(thrust: Vec4, wanted: Vec4, dt: f32) -> Vec4 {
    let eased = wanted + (thrust - wanted) * (-dt / THRUST_EASE_SECONDS).exp();
    if eased.distance_squared(wanted) < 1e-6 {
        wanted
    } else {
        eased
    }
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

/// The main 3D camera. Extracted so the star pipeline draws only in its view.
#[derive(Component, Clone, bevy::render::extract_component::ExtractComponent)]
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

impl CameraRig {
    /// Ends any flight, so the next placement jumps straight to `Navigation`.
    pub fn land(&mut self) {
        self.flight = None;
    }
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
    nav.location_planet = None;
    nav.following = false;
}

/// Whether the camera is at a system `distance` metres from its star: within
/// `range` to arrive, and until `LEAVE_FACTOR` times it once there.
fn within(arrived: bool, distance: f64, range: f64) -> bool {
    distance < if arrived { LEAVE_FACTOR * range } else { range }
}

/// Track where the camera physically is (`Navigation::arrived`). Arriving by
/// free flight uses the same star body and planetary system as clicking Visit;
/// only cells already streamed in are eligible, and it never starts a flight or
/// re-frames the camera. Leaving a system in simulation mode, however it was
/// reached, returns to open space without moving the eye.
pub fn locate(
    #[cfg(target_arch = "wasm32")] benchmark: Option<
        Res<crate::devtools::benchmark::BenchmarkFlight>,
    >,
    field: Res<crate::stars::stream::Starfield>,
    mut metrics: ResMut<crate::stars::stream::StreamMetrics>,
    lists: Res<Universe>,
    rig: Res<CameraRig>,
    pose: Res<CameraPose>,
    mut nav: ResMut<Navigation>,
) {
    let eye_ly = rig.galactic_ly(&pose);
    let arrived = nav.arrived;
    // The constellation frames a whole list from afar, so it is never at a star.
    nav.arrived = rig.placed()
        && nav.presentation_mode != PresentationMode::Constellation
        && nav.system(&lists.0).is_some_and(|s| {
            let distance = (page_origin_ly(s, nav.page) - eye_ly).length() * LY_METRES;
            within(arrived, distance, system_range(s, nav.page))
        });
    #[cfg(target_arch = "wasm32")]
    if benchmark.is_some() {
        return;
    }
    // Flights and presets are deliberate: wait until they land.
    if nav.presentation_mode != PresentationMode::Simulation
        || rig.flying()
        || nav.fly
        || !rig.placed()
        || nav.arrived
    {
        return;
    }
    if nav.system(&lists.0).is_some() {
        rebase(&mut nav, &rig, pose.eye, &lists.0, None, 0);
        // Let place_camera switch the displayed frame before another handoff.
        return;
    }
    let eye_pc =
        DVec3::from_array(GalaxyModel::milky_way().home_pc) + eye_ly * (LY_METRES / PC_METRES);
    let reach_pc = APPROACH_METRES / PC_METRES;
    let Some((_, cell, star)) = field.nearest(eye_pc, reach_pc, &mut metrics) else {
        return;
    };
    let (id, position_pc) = (star.id(cell), star.position_pc);
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
    rebase(&mut nav, &rig, pose.eye, &lists.0, Some(visit), page);
    nav.arrived = true;
}

/// Measure planets after camera placement, so a followed planet and the eye
/// use the same simulation instant even at very high time rates.
pub fn locate_planet(
    lists: Res<Universe>,
    rig: Res<CameraRig>,
    pose: Res<CameraPose>,
    clock: Res<Clock>,
    mut nav: ResMut<Navigation>,
) {
    nav.location_planet = nav.location(&lists.0).and_then(|system| {
        // Never cancel huge galactic coordinates for metre-scale measurements.
        if rig.frame_ly != page_origin_ly(system, nav.page) {
            return None;
        }
        let eye = page_frame(system, nav.page).inverse() * pose.eye;
        nearby_planet(
            system,
            nav.page,
            eye,
            clock.0.seconds,
            nav.location_planet.as_deref(),
            nav.presentation_mode,
        )
    });
}

/// Enter within 20 physical radii; remain until 30. No visual exaggeration,
/// selection or following is involved. Table/constellation are not physical.
fn nearby_planet(
    system: &space_shared::StarSystem,
    page: u32,
    eye: DVec3,
    seconds: f64,
    previous: Option<&str>,
    mode: PresentationMode,
) -> Option<String> {
    if mode != PresentationMode::Simulation {
        return None;
    }
    let distance = |planet: &space_shared::Planet| {
        (eye - DVec3::from_array(position(&planet.orbit, system.mass(page), seconds))).length()
    };
    if let Some(planet) = system
        .on_page(page)
        .find(|planet| Some(planet.id.as_str()) == previous)
        && distance(planet) < planet.radius_km * 1000.0 * 30.0
    {
        return Some(planet.id.clone());
    }
    system
        .on_page(page)
        .filter_map(|planet| {
            let d = distance(planet);
            (d < planet.radius_km * 1000.0 * 20.0).then_some((d, &planet.id))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0).then_with(|| a.1.cmp(b.1)))
        .map(|(_, id)| id.clone())
}

impl CameraRig {
    pub fn flying(&self) -> bool {
        self.flight.is_some()
    }

    /// Whether the camera has been placed yet. Before that, its pose means nothing.
    pub fn placed(&self) -> bool {
        self.shown.is_some()
    }

    /// The displayed camera in galactic axes, in light years from home.
    pub fn galactic_ly(&self, pose: &CameraPose) -> DVec3 {
        self.frame_ly + pose.eye / LY_METRES
    }

    pub fn rotation(&self) -> Quat {
        self.shown.map_or(Quat::IDENTITY, |pose| pose.rotation)
    }

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
    commands.init_resource::<CameraVelocity>();
    commands.init_resource::<CameraMotion>();
    commands.init_resource::<Markers>();
    commands.spawn_big_space_default(|root| {
        root.spawn_spatial((
            SpaceCamera,
            FloatingOrigin,
            Camera3d {
                // Star points test their depth against it (`stars/points.rs`).
                // WebGL cannot sample multisampled depth textures: retain
                // its default render-attachment-only depth buffer.
                #[cfg(not(target_arch = "wasm32"))]
                depth_texture_usages:
                    (bevy::render::render_resource::TextureUsages::RENDER_ATTACHMENT
                        | bevy::render::render_resource::TextureUsages::TEXTURE_BINDING)
                        .into(),
                ..default()
            },
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
    mut nav: ResMut<Navigation>,
    mut clock: ResMut<Clock>,
    mut camera_motion: ResMut<CameraMotion>,
    mut ui: MessageWriter<crate::actions::UiCommand>,
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
        .map(|ctx| (crate::ui::pointer_captured(ctx), ctx.wants_keyboard_input()))
        .unwrap_or_default();
    // Flights are short; camera input resumes when they land. Constellations
    // orbit their auto-framed centre, but cannot zoom or translate.
    let free = nav.presentation_mode == PresentationMode::Simulation && !rig.flying();
    let rotating = matches!(
        nav.presentation_mode,
        PresentationMode::Simulation | PresentationMode::Constellation
    ) && !rig.flying();
    if !pointer && rotating {
        let shift = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
        if buttons.pressed(MouseButton::Left) && shift {
            drag_fov(&mut nav, motion.delta.y);
        } else if buttons.pressed(MouseButton::Left) && free {
            let tilt = nav
                .system(&universe.0)
                .map_or(DQuat::IDENTITY, |s| page_frame(s, nav.frame_page(s)));
            drag_look(&mut nav, motion.delta, Some(tilt.inverse() * pose.eye));
        } else if buttons.pressed(MouseButton::Right) {
            nav.eye = None;
            orbit(&mut nav, motion.delta);
            camera_motion.orbiting = free;
        }
        if free && !shift {
            nav.move_speed = scroll_speed(nav.move_speed, scroll);
        } else if free && shift && scroll != 0.0 {
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
    let mut wanted = Vec4::ZERO;
    if !keyboard {
        if keys.just_pressed(KeyCode::Enter) {
            crate::galaxy_nav::visit_selected(&mut nav, &universe.0);
        }
        if keys.just_pressed(KeyCode::Space) {
            clock.0.paused = !clock.0.paused;
        }
        if keys.just_pressed(KeyCode::Escape) {
            ui.write(crate::actions::UiCommand::Up);
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
            nav.following = false;
            wanted.w = roll_input;
        }
        if movement.length_squared() > 0.0 && free {
            nav.following = false;
            wanted = movement.normalize().as_vec3().extend(wanted.w);
        }
    }
    // Flights, tracking and presets take the camera over: no coasting into them.
    camera_motion.thrust = if free && !nav.following {
        ease_thrust(camera_motion.thrust, wanted, time.delta_secs())
    } else {
        Vec4::ZERO
    };
    if camera_motion.thrust.w != 0.0 {
        roll(
            &mut nav,
            camera_motion.thrust.w * ROLL_SPEED * time.delta_secs(),
        );
    }
    let travel = camera_motion.thrust.truncate();
    if travel != Vec3::ZERO {
        let tilt = nav
            .system(&universe.0)
            .map_or(DQuat::IDENTITY, |s| page_frame(s, nav.frame_page(s)));
        let rotation = nav.rotation;
        let step = (rotation * travel).as_dvec3() * nav.move_speed * time.delta_secs_f64();
        camera_motion.travel_metres += tilt * step;
        let eye = nav.eye.get_or_insert_with(|| tilt.inverse() * pose.eye);
        *eye += step;
        nav.target = *eye - (rotation * Vec3::Z).as_dvec3() * nav.distance;
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

/// Shift + left-drag: up narrows the view (zooms in), down widens it.
fn drag_fov(nav: &mut Navigation, delta_y: f32) {
    nav.set_fov(nav.fov_deg + delta_y * 0.15);
}

/// Right-drag keeps its target fixed and turns about the camera's own screen
/// axes, so it follows the view when upside down or rolled.
fn orbit(nav: &mut Navigation, delta: Vec2) {
    if delta == Vec2::ZERO {
        return;
    }
    let turn = Quat::from_rotation_y(-delta.x * 0.005) * Quat::from_rotation_x(-delta.y * 0.005);
    nav.rotation = (nav.rotation * turn).normalize();
}

#[allow(clippy::too_many_arguments)]
pub fn place_camera(
    time: Res<Time>,
    universe: Res<Universe>,
    field: Res<crate::stars::stream::Starfield>,
    mut metrics: ResMut<crate::stars::stream::StreamMetrics>,
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
    if nav.fly || nav.following || nav.presentation_mode != PresentationMode::Simulation {
        nav.eye = None;
    }
    if nav.following
        && let Some(followed) = nav.system(&universe.0).and_then(|system| {
            let planet = system.planets.get(nav.selected_planet(&universe.0)?)?;
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
        layout: f64::from(u8::from(nav.presentation_mode == PresentationMode::Table)),
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
    let retarget = nav.following && nav.selected_planet(&universe.0) != rig.selected;
    let star_jump = rig.shown.is_some() && frame != rig.frame_ly;
    rig.frame_ly = frame;
    rig.selected = nav.selected_planet(&universe.0);
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
                "Flight started: list={} page={} presentation_mode={:?} star travel={:.3} ly duration={:.3} s",
                system.map_or("galaxy", |s| s.id.as_str()),
                nav.page + 1,
                nav.presentation_mode,
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
            "Flight landed: list={} page={} presentation_mode={:?}",
            system.map_or("galaxy", |s| s.id.as_str()),
            nav.page + 1,
            nav.presentation_mode
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
    pose.scale_distance = proximity_scale_distance(&field, eye_pc, shown.distance, &mut metrics);
    if nav.arrived {
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
        p.fov = nav.fov_deg.to_radians();
    }
}

fn proximity_scale_distance(
    field: &crate::stars::stream::Starfield,
    eye_pc: DVec3,
    selected_range: f64,
    metrics: &mut crate::stars::stream::StreamMetrics,
) -> f64 {
    field
        .nearest(eye_pc, PRECISE_SCALE_PC, metrics)
        .map_or(selected_range, |(distance, ..)| {
            selected_range.min(distance * PC_METRES)
        })
        .max(20_000_000.0)
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

/// Within this range of its star the camera arrives at a system. Even unusually
/// wide systems include their initial fitted range and an outer planet being
/// followed. Small systems keep the original cutoff.
fn system_range(system: &StarSystem, page: u32) -> f64 {
    let outer_apoapsis = system
        .on_page(page)
        .map(|p| p.orbit.semi_major_au * (1.0 + p.orbit.eccentricity))
        .fold(0.0, f64::max);
    WIDE_METRES.max(4.0 * outer_apoapsis * space_simulation::AU_METRES)
}

/// While the camera is at the visited system, its planets and body are drawn
/// and this is how far from its star, in metres, they can be. Elsewhere the
/// star is a galactic point like any other, bound by the same magnitude limit,
/// and the planets are hidden.
pub fn body_range(nav: &Navigation, system: &StarSystem, pose: &CameraPose) -> Option<f64> {
    nav.arrived
        .then(|| LEAVE_FACTOR * system_range(system, pose.page))
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
        if nav.orbits && nav.presentation_mode == PresentationMode::Simulation && pose.layout <= 0.5
        {
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
    let planets_shown = body_range(&nav, system, &pose).is_some();
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
    };
    let markers = &mut markers.0;
    if nav.presentation_mode == PresentationMode::Constellation {
        canvas.constellation(&mut gizmos);
    }
    if let Some(range) = body_range(&nav, system, &pose) {
        // Zero where the camera arrives, and so where galactic points become
        // bodies, full at half that distance.
        let arrive = range / LEAVE_FACTOR;
        lights.streaks = ((arrive - pose.eye.length()) / (0.5 * arrive)).clamp(0.0, 1.0) as f32;
    }
    if planets_shown {
        canvas.star(&mut gizmos, &mut lights);
        canvas.planets(&mut gizmos, clock.0.seconds, markers, &mut lights);
        lights.kelvin = system.star(pose.page).map_or(5772.0, |s| s.temperature_k);
    }
}

#[cfg(test)]
mod tests;
