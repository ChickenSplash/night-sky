//! The displayed camera: its pose, the rig that flies it towards
//! `Navigation`, and its measured velocity.
use super::controls::CameraMotion;
use crate::flight::{Flight, Pose};
use bevy::{math::DVec3, prelude::*};
use space_simulation::LY_METRES;

/// Camera velocity in galactic axes, measured from displayed positions rather
/// than the requested movement, so flights and turns are accounted for.
#[derive(Resource, Default)]
pub struct CameraVelocity {
    pub metres_per_second: DVec3,
    pub marker_metres_per_second: DVec3,
    previous: Option<(DVec3, DVec3)>,
    previous_follow: Option<(String, DVec3)>,
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
    /// Exact target-to-eye offset while following a craft, in galactic axes.
    /// Kept separate so adding it to a light-year-sized position cannot round it.
    pub followed: Option<FollowPose>,
}

pub struct FollowPose {
    pub id: String,
    pub eye_offset: DVec3,
}

impl CameraPose {
    /// The tracked hull and its label share this exact camera-relative centre.
    pub fn craft_relative(&self, id: &str) -> Option<DVec3> {
        self.followed
            .as_ref()
            .filter(|follow| follow.id == id)
            .map(|follow| -follow.eye_offset)
    }

    /// Metres per render unit. Keeps the camera about 1,000 units from its target, so
    /// GPU clip-space values stay small at every zoom level (perspective is
    /// scale-invariant, so the image does not change).
    pub fn render_scale(&self) -> f64 {
        self.scale_distance.max(scale_floor(self.distance)) / 1000.0
    }
}

/// Scale distance used from this zoom out, in metres. A closer zoom brings
/// the floor down with it, so what is zoomed on stays past the near plane.
const SCALE_FLOOR: f64 = 20_000_000.0;

/// The least scale distance at a zoom, in metres.
pub fn scale_floor(distance: f64) -> f64 {
    distance.clamp(super::MIN_DISTANCE, SCALE_FLOOR)
}

#[derive(Resource, Default)]
pub struct CameraRig {
    pub(super) shown: Option<Pose>,
    /// Page star position the shown pose is relative to.
    pub(super) frame_ly: DVec3,
    pub(super) flight: Option<Flight>,
    pub(super) selected: Option<usize>,
    pub(super) warned_stall: bool,
}

impl CameraRig {
    /// Ends any flight, so the next placement jumps straight to `Navigation`.
    pub fn land(&mut self) {
        self.flight = None;
    }
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

pub fn measure_velocity(
    time: Res<Time>,
    rig: Res<CameraRig>,
    pose: Res<CameraPose>,
    motion: Res<CameraMotion>,
    mut velocity: ResMut<CameraVelocity>,
    fleet: Option<Res<crate::craft::Fleet>>,
) {
    let current = (rig.frame_ly, pose.eye);
    let dt = time.delta_secs_f64();
    if let Some(follow) = &pose.followed
        && let Some(live) = fleet.as_deref().and_then(|fleet| fleet.get(&follow.id))
    {
        // The craft's previous displayed state was transformed by the exact
        // same rebase as its current state. Do not difference rounded origins.
        let displacement = velocity
            .previous_follow
            .as_ref()
            .filter(|(id, _)| *id == follow.id)
            .map_or(DVec3::ZERO, |(_, offset)| {
                live.display_delta_m + follow.eye_offset - *offset
            });
        velocity.metres_per_second = if dt > 0.0 {
            displacement / dt
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
        velocity.previous_follow = Some((follow.id.clone(), follow.eye_offset));
        velocity.previous = Some(current);
        return;
    }
    velocity.previous_follow = None;
    let previous = velocity.previous.replace(current);
    if dt <= 0.0 {
        velocity.metres_per_second = DVec3::ZERO;
        velocity.marker_metres_per_second = DVec3::ZERO;
        return;
    }
    let exact = motion.steered && !motion.orbiting && !rig.flying();
    velocity.metres_per_second = match previous {
        // Free flight knows its displacement exactly, whatever rounding the
        // eye far from its origin or a re-anchoring frame switch adds.
        _ if exact => motion.moved_metres / dt,
        // Switching a star-relative frame subtracts interstellar-sized
        // numbers. Do not interpret the rounding residual as motion.
        Some((frame, _)) if frame != rig.frame_ly => {
            velocity.marker_metres_per_second = DVec3::ZERO;
            velocity.metres_per_second = DVec3::ZERO;
            return;
        }
        // Subtract local eyes first: adding the distant galactic origin to
        // each position would lose metre-scale motion to f64 precision.
        Some((_, eye)) => (pose.eye - eye) / dt,
        None => DVec3::ZERO,
    };
    velocity.marker_metres_per_second = if motion.orbiting {
        motion.travel_metres / dt
    } else {
        velocity.metres_per_second
    };
}
