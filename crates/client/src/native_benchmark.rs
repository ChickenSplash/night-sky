//! Native experiment measurement. Wall-clock frame intervals, never Bevy's capped delta.
use bevy::{app::AppExit, math::DVec3, prelude::*, window::PrimaryWindow};
use serde::{Deserialize, Serialize};
use space_simulation::LY_METRES;
use std::{path::PathBuf, time::Instant};

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Spec {
    pub mode: String,
    pub workload: String,
    pub presentation: String,
    pub warmup_seconds: f64,
    pub measure_seconds: f64,
}

#[derive(Resource)]
pub struct Benchmark {
    pub spec: Spec,
    path: PathBuf,
    started: Instant,
    measuring: Option<Instant>,
    previous: Option<Instant>,
    stable: Option<Instant>,
    last_status: f64,
    frames: Vec<f64>,
    scene_ready_seconds: Option<f64>,
    panel_ready_seconds: Option<f64>,
    #[cfg(feature = "cef-ui")]
    paint_start: [u64; 3],
}

pub fn requested() -> Option<Benchmark> {
    let args: Vec<_> = std::env::args().collect();
    let index = args.iter().position(|a| a == "--native-benchmark")?;
    let path = PathBuf::from(args.get(index + 1).expect("Benchmark needs a config path"))
        .canonicalize()
        .expect("Benchmark config does not exist");
    let data = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../data")
        .canonicalize()
        .expect("Project data directory does not exist");
    assert!(
        path.starts_with(data),
        "Benchmark runtime files must stay inside project data/"
    );
    let spec: Spec =
        serde_json::from_slice(&std::fs::read(&path).expect("Cannot read benchmark config"))
            .expect("Invalid benchmark config");
    validate(&spec).expect("Invalid benchmark settings");
    #[cfg(not(feature = "cef-ui"))]
    assert_eq!(spec.mode, "baseline", "CEF mode requires a CEF build");
    #[cfg(feature = "cef-ui")]
    assert_ne!(
        spec.mode, "baseline",
        "Baseline requires the feature-free release build"
    );
    assert_eq!(
        args.iter().any(|a| a == "--cef-html-ui"),
        spec.mode != "baseline",
        "CEF launch flag and mode must agree"
    );
    std::fs::write(path.with_extension("pid"), std::process::id().to_string())
        .expect("Write benchmark PID");
    Some(Benchmark {
        spec,
        path,
        started: Instant::now(),
        measuring: None,
        previous: None,
        stable: None,
        last_status: -1.0,
        frames: Vec::new(),
        scene_ready_seconds: None,
        panel_ready_seconds: None,
        #[cfg(feature = "cef-ui")]
        paint_start: [0; 3],
    })
}

fn validate(s: &Spec) -> Result<(), &'static str> {
    if ![
        "baseline",
        "runtime",
        "idle",
        "active",
        "animation",
        "interactive",
    ]
    .contains(&s.mode.as_str())
        || !["stationary", "flight"].contains(&s.workload.as_str())
        || !["uncapped", "normal"].contains(&s.presentation.as_str())
        || !s.warmup_seconds.is_finite()
        || !(3.0..=120.0).contains(&s.warmup_seconds)
        || !s.measure_seconds.is_finite()
        || !(3.0..=120.0).contains(&s.measure_seconds)
    {
        return Err("Unsupported mode or timing");
    }
    Ok(())
}

pub fn at_home(nav: &mut crate::Navigation) {
    nav.distance = 120.0 * LY_METRES;
    nav.visit = None;
    nav.eye = Some(DVec3::ZERO);
    nav.rotation = Quat::from_rotation_y(std::f32::consts::FRAC_PI_2);
    nav.target = -DVec3::X * nav.distance;
    nav.move_speed = 1_000.0 * LY_METRES;
    nav.set_fov(50.0);
    nav.realistic = true;
    nav.physical = true;
    nav.galaxy_glow = true;
}

pub fn interactive_navigation(benchmark: Option<Res<Benchmark>>) -> bool {
    benchmark.is_none()
}

pub fn install(app: &mut App, benchmark: Benchmark) {
    #[cfg(feature = "cef-ui")]
    bevy_cef_core::paint_metrics::enable();
    // Bevy defaults to 60 Hz reactive updates when unfocused. Tests deliberately
    // never steal focus, so disable that throttle for BOTH presentation modes.
    app.insert_resource(bevy::winit::WinitSettings::continuous())
        .insert_resource(benchmark)
        .add_systems(
            Update,
            drive
                .after(crate::scene::controls)
                .before(crate::scene::nearby_system),
        )
        .add_systems(Last, measure);
}

fn drive(bench: Res<Benchmark>, mut nav: ResMut<crate::Navigation>) {
    let elapsed = bench.measuring.map_or(0.0, |s| s.elapsed().as_secs_f64());
    at_home(&mut nav);
    nav.fly = false;
    nav.following = false;
    nav.view = crate::views::View::Orbit;
    if bench.spec.workload == "flight" {
        let eye = -DVec3::X * (1_000.0 * LY_METRES * elapsed);
        nav.eye = Some(eye);
        nav.target = eye - (nav.rotation * Vec3::Z).as_dvec3() * nav.distance;
    }
}

fn save(path: &std::path::Path, value: &serde_json::Value) {
    let temp = path.with_extension("tmp");
    std::fs::write(
        &temp,
        serde_json::to_vec(value).expect("Serialize benchmark"),
    )
    .expect("Write benchmark");
    std::fs::rename(temp, path).expect("Commit benchmark file");
}

#[allow(clippy::too_many_arguments)]
fn measure(
    mut bench: ResMut<Benchmark>,
    field: Res<crate::stream::Starfield>,
    metrics: Res<crate::stream::StreamMetrics>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut exit: MessageWriter<AppExit>,
    #[cfg(feature = "cef-ui")] input: Option<Res<crate::cef_input::HtmlInput>>,
) {
    let now = Instant::now();
    let age = bench.started.elapsed().as_secs_f64();
    #[cfg(feature = "cef-ui")]
    let panel_ready =
        input.as_ref().is_some_and(|i| i.usable) && bevy_cef_core::paint_metrics::snapshot()[0] > 0;
    #[cfg(not(feature = "cef-ui"))]
    let panel_ready = false;
    if panel_ready && bench.panel_ready_seconds.is_none() {
        bench.panel_ready_seconds = Some(age);
    }
    let scene_ready = field.stars > 0 && field.pending == 0 && metrics.mesh_queue == 0;
    if scene_ready && bench.scene_ready_seconds.is_none() {
        bench.scene_ready_seconds = Some(age);
    }
    if scene_ready {
        bench.stable.get_or_insert(now);
    } else {
        bench.stable = None;
    }
    let warmed = age >= bench.spec.warmup_seconds
        && bench
            .stable
            .is_some_and(|s| s.elapsed().as_secs_f64() >= 3.0)
        && (bench.spec.mode == "baseline" || panel_ready);
    if bench.measuring.is_none()
        && warmed
        && (bench.spec.mode != "interactive" || bench.path.with_extension("start").exists())
    {
        bench.measuring = Some(now);
        #[cfg(feature = "cef-ui")]
        {
            bench.paint_start = bevy_cef_core::paint_metrics::snapshot();
        }
    }
    let elapsed = bench.measuring.map_or(0.0, |s| s.elapsed().as_secs_f64());
    if bench.measuring.is_some() {
        if let Some(previous) = bench.previous {
            bench
                .frames
                .push(now.duration_since(previous).as_secs_f64() * 1_000.0);
        }
        bench.previous = Some(now);
    }
    let window = windows.single().ok();
    if age - bench.last_status >= 1.0 {
        bench.last_status = age;
        save(
            &bench.path.with_extension("status.json"),
            &serde_json::json!({
                "pid": std::process::id(), "phase": if bench.measuring.is_some() {"measuring"} else if warmed {"ready"} else {"warming"},
                "age_seconds": age, "elapsed_seconds": elapsed, "stars": field.stars,
                "window": window.map(|w| [w.physical_width(), w.physical_height()]),
            }),
        );
    }
    if bench.measuring.is_none() && !warmed && age > bench.spec.warmup_seconds + 120.0 {
        save(
            &bench.path.with_extension("result.json"),
            &serde_json::json!({"error": "Warm-up/readiness timed out", "spec": bench.spec}),
        );
        exit.write(AppExit::error());
    } else if bench.measuring.is_some() && elapsed >= bench.spec.measure_seconds {
        #[cfg(feature = "cef-ui")]
        let paint = {
            let current = bevy_cef_core::paint_metrics::snapshot();
            let delta: Vec<_> = current
                .iter()
                .zip(bench.paint_start)
                .map(|(a, b)| a.saturating_sub(b))
                .collect();
            serde_json::json!({"callbacks": delta[0], "copied_bytes": delta[1], "callback_cpu_ms": delta[2] as f64 / 1e6})
        };
        #[cfg(not(feature = "cef-ui"))]
        let paint = serde_json::Value::Null;
        save(
            &bench.path.with_extension("result.json"),
            &serde_json::json!({
                "spec": bench.spec, "frame_ms": bench.frames, "wall_seconds": elapsed,
                "startup_scene_ready_seconds": bench.scene_ready_seconds,
                "startup_panel_ready_seconds": bench.panel_ready_seconds,
                "warmup_actual_seconds": age - elapsed, "cef_paint": paint,
                "final_stars": field.stars, "final_pending": field.pending,
                "resolution": window.map(|w| [w.physical_width(), w.physical_height()]),
                "present_mode_requested": window.map(|w| format!("{:?}", w.present_mode)),
                "texture_update_gpu_ms": null,
                "notes": ["Frame intervals are main-loop wall time, not GPU timestamps", "Paint time is CPU callback copy/publish, not GPU texture upload"]
            }),
        );
        exit.write(AppExit::Success);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn settings_and_route_are_bounded() {
        let mut s = Spec {
            mode: "baseline".into(),
            workload: "flight".into(),
            presentation: "uncapped".into(),
            warmup_seconds: 15.0,
            measure_seconds: 20.0,
        };
        assert!(validate(&s).is_ok());
        s.measure_seconds = f64::NAN;
        assert!(validate(&s).is_err());
        let mut nav = crate::Navigation::default();
        at_home(&mut nav);
        assert_eq!(nav.eye, Some(DVec3::ZERO));
        assert_eq!(nav.move_speed, 1_000.0 * LY_METRES);
    }
}
