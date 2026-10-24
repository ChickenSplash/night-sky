//! Atmosphere test (debug window, `tools` tab): measures the selected planet's
//! air from fixed views, for before-and-after comparisons of the atmosphere
//! shader, such as the lookup tables against the direct march. Each view is
//! measured with Earth's and Venus's air at two step counts and with the air
//! switched off, with the clock paused. Each run is logged and appended to
//! `data/logs/atmosphere-test.jsonl`.

use super::gpu_test::{commit, gpu_times, mean, percentile, save};
use crate::navigation::presentation::PresentationMode;
use crate::render::atmosphere::{AtmosphereChange, AtmosphereTuning, Preset};
use crate::render::debug::RenderDebug;
use crate::scene::CameraRig;
use crate::{Clock, Navigation, Selection, Universe};
use bevy::{
    diagnostic::DiagnosticsStore, math::DVec3, platform::time::Instant, prelude::*,
    window::PrimaryWindow,
};
use serde::Serialize;
use space_simulation::{member_position, planet_position};

/// After each change of view or air: pipelines compile and the exposure eases.
pub const SETTLE_S: f64 = 1.5;
pub const MEASURE_S: f64 = 5.0;
const PRESETS: [Preset; 2] = [Preset::Earth, Preset::Venus];
const VIEW_STEPS: [u32; 2] = [16, 24];
/// The orbit view's distance from the planet's centre, in radii.
const ORBIT_RADII: f64 = 3.0;
/// The high view's height, as a share of the top of the air.
const HIGH: f64 = 0.9;
const LOW_M: f64 = 1_000.0;
/// The star's elevation over the ground below the camera.
const SUN_ELEVATION_DEG: f64 = 30.0;
/// The pass drawing the shells, as the atmosphere tab's readout.
const AIR_PASSES: [&str; 1] = ["main_transparent_pass_3d"];

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum View {
    /// The lit disc and its terminator, from three radii.
    Orbit,
    /// Near the top of the air, facing the horizon.
    High,
    /// A kilometre up, facing the horizon.
    Low,
}

impl View {
    const ALL: [Self; 3] = [Self::Orbit, Self::High, Self::Low];

    pub fn label(self) -> &'static str {
        match self {
            Self::Orbit => "orbit",
            Self::High => "high",
            Self::Low => "low",
        }
    }
}

pub fn preset_label(preset: Preset) -> &'static str {
    Preset::ALL
        .iter()
        .find(|(p, _)| *p == preset)
        .map_or("", |(_, label)| label)
}

#[derive(Clone, Copy, Debug, Serialize)]
pub struct Case {
    pub view: View,
    pub preset: Preset,
    /// None with the air switched off.
    pub view_steps: Option<u32>,
}

/// Every view, with each preset off and then at each step count.
fn cases() -> Vec<Case> {
    let mut cases = Vec::new();
    for view in View::ALL {
        for preset in PRESETS {
            let steps = std::iter::once(None).chain(VIEW_STEPS.map(Some));
            cases.extend(steps.map(|view_steps| Case {
                view,
                preset,
                view_steps,
            }));
        }
    }
    cases
}

#[derive(Serialize)]
pub struct Measured {
    #[serde(flatten)]
    pub case: Case,
    pub frame_mean_ms: f64,
    pub frame_p95_ms: f64,
    pub gpu_total_ms: f64,
    pub transparent_ms: f64,
}

#[derive(Serialize)]
pub struct AtmosphereResults {
    pub commit: String,
    pub unix_time: u64,
    pub planet: String,
    pub radius_km: f64,
    pub size: [u32; 2],
    pub msaa: bool,
    pub fov_deg: f32,
    pub light_steps: u32,
    pub jitter: bool,
    pub tables: bool,
    pub stop_opaque: bool,
    pub multiple: bool,
    pub cases: Vec<Measured>,
}

/// What the test changes, put back when it ends.
pub struct Saved {
    tuning: AtmosphereTuning,
    atmospheres: bool,
    paused: bool,
}

pub struct Run {
    planet: String,
    saved: Saved,
    cases: Vec<Case>,
    measured: Vec<Measured>,
    since: Instant,
    measuring: bool,
    frames: Vec<f64>,
    gpu: Vec<f64>,
    transparent: Vec<f64>,
}

impl Run {
    pub fn status(&self) -> String {
        let index = self.measured.len();
        let Some(case) = self.cases.get(index) else {
            return String::new();
        };
        let air = case
            .view_steps
            .map_or("off".to_string(), |steps| format!("{steps} steps"));
        let left = if self.measuring {
            format!(
                "measuring, {:.0} s left",
                (MEASURE_S - self.since.elapsed().as_secs_f64()).max(0.0)
            )
        } else {
            "settling".to_string()
        };
        format!(
            "{} of {}: {}, {}, {air}, {left}",
            index + 1,
            self.cases.len(),
            case.view.label(),
            preset_label(case.preset),
        )
    }
}

#[derive(Resource, Default)]
pub enum AtmosphereTest {
    #[default]
    Idle,
    Requested,
    Running(Box<Run>),
    Done(Box<AtmosphereResults>),
    Failed(String),
}

/// Puts the camera and the air in place for `case`, or says why it cannot.
fn apply(
    case: Case,
    planet: &str,
    (nav, rig): (&mut Navigation, &mut CameraRig),
    (universe, clock): (&Universe, &Clock),
    (tuning, debug): (&mut AtmosphereTuning, &mut RenderDebug),
) -> Result<(), String> {
    let lost = || "The planet is no longer selected".to_string();
    if nav.selection != Some(Selection::Planet(planet.to_owned())) {
        return Err(lost());
    }
    let system = nav.system(&universe.0).ok_or_else(lost)?;
    let body = system
        .planets
        .iter()
        .find(|p| p.id == planet)
        .ok_or_else(lost)?;
    let seconds = clock.0.seconds;
    let at = DVec3::from_array(planet_position(system, body, seconds));
    let host = DVec3::from_array(member_position(system, body.page(), &body.host, seconds));
    let page = body.page();
    let radius = body.radius_km * 1e3;
    tuning.apply(&AtmosphereChange {
        preset: Some(case.preset),
        view_steps: case.view_steps,
        ..default()
    });
    debug.atmospheres = case.view_steps.is_some();
    // The ground below the camera sees the star at SUN_ELEVATION_DEG, and
    // the camera faces along the horizon at right angles to it.
    let sun = (host - at).try_normalize().unwrap_or(DVec3::X);
    let side = sun
        .cross(DVec3::Y)
        .try_normalize()
        .unwrap_or_else(|| sun.cross(DVec3::Z).normalize());
    let from_sun = (90.0 - SUN_ELEVATION_DEG).to_radians();
    let up = sun * from_sun.cos() + side * from_sun.sin();
    let along = up.cross(sun).normalize();
    let (eye, forward, screen_up, distance) = match case.view {
        View::Orbit => (
            at + up * ORBIT_RADII * radius,
            -up,
            along,
            ORBIT_RADII * radius,
        ),
        View::High | View::Low => {
            let height = match case.view {
                View::High => HIGH * tuning.air.atmosphere().top,
                _ => LOW_M,
            };
            let from_centre = radius + height;
            // Looking down by the horizon's dip, so it sits mid-screen.
            let dip = (radius / from_centre).acos();
            let forward = along * dip.cos() - up * dip.sin();
            (at + up * from_centre, forward, up, radius)
        }
    };
    nav.presentation_mode = PresentationMode::Simulation;
    nav.page = page;
    nav.following = false;
    nav.fly = false;
    nav.eye = Some(eye);
    nav.distance = distance;
    nav.target = eye + forward * distance;
    nav.rotation = Transform::default()
        .looking_to(forward.as_vec3(), screen_up.as_vec3())
        .rotation;
    rig.land();
    Ok(())
}

fn restore(
    saved: &Saved,
    clock: &mut Clock,
    tuning: &mut AtmosphereTuning,
    debug: &mut RenderDebug,
) {
    *tuning = saved.tuning;
    debug.atmospheres = saved.atmospheres;
    clock.0.paused = saved.paused;
}

/// Before `scene::place_camera`, so each view is placed in the same frame.
pub fn run(
    mut test: ResMut<AtmosphereTest>,
    place: (ResMut<Navigation>, ResMut<CameraRig>),
    scene: (Res<Universe>, ResMut<Clock>, Res<Time>),
    air: (ResMut<AtmosphereTuning>, ResMut<RenderDebug>),
    diagnostics: Option<Res<DiagnosticsStore>>,
    window: Query<&Window, With<PrimaryWindow>>,
) {
    let (mut nav, mut rig) = place;
    let (universe, mut clock, time) = scene;
    let (mut tuning, mut debug) = air;
    match &mut *test {
        AtmosphereTest::Requested => {
            let Some(Selection::Planet(planet)) = nav.selection.clone() else {
                *test = AtmosphereTest::Failed("Select a planet first".into());
                return;
            };
            let saved = Saved {
                tuning: *tuning,
                atmospheres: debug.atmospheres,
                paused: clock.0.paused,
            };
            clock.0.paused = true;
            let cases = cases();
            let applied = apply(
                cases[0],
                &planet,
                (&mut nav, &mut rig),
                (&universe, &clock),
                (&mut tuning, &mut debug),
            );
            *test = match applied {
                Ok(()) => AtmosphereTest::Running(Box::new(Run {
                    planet,
                    saved,
                    cases,
                    measured: Vec::new(),
                    since: Instant::now(),
                    measuring: false,
                    frames: Vec::new(),
                    gpu: Vec::new(),
                    transparent: Vec::new(),
                })),
                Err(error) => {
                    restore(&saved, &mut clock, &mut tuning, &mut debug);
                    AtmosphereTest::Failed(error)
                }
            };
        }
        AtmosphereTest::Running(run) => {
            let seconds = run.since.elapsed().as_secs_f64();
            if !run.measuring {
                if seconds >= SETTLE_S {
                    run.measuring = true;
                    run.since = Instant::now();
                }
                return;
            }
            run.frames.push(time.delta_secs_f64() * 1000.0);
            let (transparent, total) = diagnostics
                .as_deref()
                .map_or((None, None), |d| gpu_times(d, &AIR_PASSES));
            run.transparent.extend(transparent);
            run.gpu.extend(total);
            if seconds < MEASURE_S {
                return;
            }
            let case = run.cases[run.measured.len()];
            run.measured.push(Measured {
                case,
                frame_mean_ms: mean(&run.frames),
                frame_p95_ms: percentile(&run.frames, 0.95),
                gpu_total_ms: mean(&run.gpu),
                transparent_ms: mean(&run.transparent),
            });
            run.frames.clear();
            run.gpu.clear();
            run.transparent.clear();
            run.measuring = false;
            run.since = Instant::now();
            if let Some(next) = run.cases.get(run.measured.len()).copied() {
                if let Err(error) = apply(
                    next,
                    &run.planet,
                    (&mut nav, &mut rig),
                    (&universe, &clock),
                    (&mut tuning, &mut debug),
                ) {
                    restore(&run.saved, &mut clock, &mut tuning, &mut debug);
                    *test = AtmosphereTest::Failed(error);
                }
                return;
            }
            restore(&run.saved, &mut clock, &mut tuning, &mut debug);
            nav.focus_selected(&universe.0);
            let radius_km = nav
                .system(&universe.0)
                .and_then(|system| system.planets.iter().find(|p| p.id == run.planet))
                .map_or(0.0, |planet| planet.radius_km);
            let results = AtmosphereResults {
                commit: commit(),
                unix_time: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0, |d| d.as_secs()),
                planet: std::mem::take(&mut run.planet),
                radius_km,
                size: window
                    .single()
                    .map_or([0, 0], |w| [w.physical_width(), w.physical_height()]),
                msaa: debug.msaa,
                fov_deg: nav.fov_deg,
                light_steps: tuning.light_steps,
                jitter: debug.atmosphere_jitter,
                tables: debug.atmosphere_tables,
                stop_opaque: debug.atmosphere_stop_opaque,
                multiple: debug.atmosphere_multiple,
                cases: std::mem::take(&mut run.measured),
            };
            info!(
                "Atmosphere test ({}) on {}: {} views measured, saved to atmosphere-test.jsonl",
                results.commit,
                results.planet,
                results.cases.len()
            );
            save("atmosphere-test.jsonl", &results);
            *test = AtmosphereTest::Done(Box::new(results));
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_view_and_preset_has_a_baseline_with_the_air_off() {
        let cases = cases();
        assert_eq!(cases.len(), 18);
        for chunk in cases.chunks(3) {
            assert_eq!(chunk[0].view_steps, None);
            assert!(
                chunk
                    .iter()
                    .all(|c| c.view == chunk[0].view && c.preset == chunk[0].preset)
            );
            assert_eq!(chunk[1].view_steps, Some(16));
            assert_eq!(chunk[2].view_steps, Some(24));
        }
    }
}
