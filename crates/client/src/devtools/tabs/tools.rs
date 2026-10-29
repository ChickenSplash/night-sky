//! The `tools` tab: the GPU, atmosphere and core load tests and bug reports.
use crate::devtools::DevTools;
use crate::devtools::atmosphere_test::{AtmosphereTest, preset_label};
use crate::devtools::core_load::CoreLoad;
use crate::devtools::gpu_test::GpuTest;
use crate::format::{grouped, number};
use crate::scene::{CameraPose, CameraRig};
use crate::stars::stream::{Starfield, StreamMetrics};
use crate::ui::terminal;
use crate::{Clock, Navigation, Universe};
use bevy::prelude::*;
use bevy_egui::egui;

/// Returns whether Save bug report was clicked.
pub fn tools_tab(
    ui: &mut egui::Ui,
    tools: &DevTools,
    (core_load, gpu, air): (&mut CoreLoad, &mut GpuTest, &mut AtmosphereTest),
    field: &Starfield,
    metrics: &StreamMetrics,
) -> bool {
    gpu_test(ui, gpu, field, metrics);
    atmosphere_test(ui, air);
    core_load_test(ui, core_load, field);
    terminal::heading(ui, "bug report");
    let report = terminal::button(ui, "save bug report");
    terminal::note(
        ui,
        "Saves state, recent log lines and a screenshot under data/logs/bug-*.",
    );
    if !tools.report.is_empty() {
        ui.label(
            egui::RichText::new(&tools.report)
                .font(terminal::font())
                .color(terminal::CYAN),
        );
    }
    report
}

fn gpu_test(ui: &mut egui::Ui, test: &mut GpuTest, field: &Starfield, metrics: &StreamMetrics) {
    use crate::devtools::gpu_test::{MEASURE_S, SETTLE_S};
    terminal::heading(ui, "gpu test");
    if terminal::button(ui, "measure the galactic centre view") {
        *test = GpuTest::Requested;
    }
    terminal::note(
        ui,
        "Loads the view 500 ly from the centre, facing it, from nothing; once every star is uploaded, measures 20 s. Keep still with the window focused.",
    );
    let status = match test {
        GpuTest::Idle => return,
        GpuTest::Requested => "starting".to_string(),
        GpuTest::Loading(started) => format!(
            "loading, {:.0} s: {} cells pending, {} uploading",
            started.elapsed().as_secs_f64(),
            grouped(field.pending),
            grouped(metrics.mesh_queue)
        ),
        GpuTest::Settling { since, .. } => format!(
            "settling, {:.0} s left",
            (SETTLE_S - since.elapsed().as_secs_f64()).max(0.0)
        ),
        GpuTest::Measuring { since, .. } => format!(
            "measuring, {:.0} s left",
            (MEASURE_S - since.elapsed().as_secs_f64()).max(0.0)
        ),
        GpuTest::Done(r) => {
            terminal::facts_with_details(
                ui,
                "debug-gpu-test",
                &[
                    (
                        "commit",
                        r.commit.clone(),
                        Some("Build measured; +dirty means uncommitted changes."),
                    ),
                    (
                        "fps",
                        number(r.fps, 1),
                        Some("Frames over the 20 s measured."),
                    ),
                    (
                        "frame",
                        format!(
                            "{:.2} ms, p95 {:.2}, p99 {:.2}",
                            r.frame_mean_ms, r.frame_p95_ms, r.frame_p99_ms
                        ),
                        Some("Mean and slowest frame times."),
                    ),
                    (
                        "star pass",
                        format!("{} ms", number(r.star_pass_ms, 2)),
                        Some(
                            "Mean GPU time of the main view's transparent pass and, for points, the star points pass.",
                        ),
                    ),
                    (
                        "gpu total",
                        format!("{} ms", number(r.gpu_total_ms, 2)),
                        Some("Mean GPU time of every pass, as in the perf tab."),
                    ),
                    (
                        "stars",
                        format!(
                            "{} drawn of {}",
                            grouped(r.drawn_stars),
                            grouped(r.uploaded_stars)
                        ),
                        Some("Stars in batches inside the view, of all uploaded."),
                    ),
                    (
                        "star memory",
                        format!("{} MB", number(r.star_memory_mb, 0)),
                        Some("GPU memory holding the uploaded stars."),
                    ),
                    (
                        "view",
                        format!(
                            "{} x {}, MSAA {}, magnitude {:.1}, {}",
                            r.size[0],
                            r.size[1],
                            if r.msaa { "on" } else { "off" },
                            r.magnitude,
                            if r.points { "points" } else { "triangles" }
                        ),
                        Some(
                            "Window size in physical pixels, the 4x MSAA switch, the magnitude limit and whether stars were points or triangles.",
                        ),
                    ),
                    (
                        "load",
                        format!("{} s", number(r.load_s, 1)),
                        Some("From the click until every star was uploaded."),
                    ),
                ],
            );
            terminal::note(ui, "Each run is appended to data/logs/gpu-test.jsonl.");
            return;
        }
    };
    terminal::facts_with_details(ui, "debug-gpu-test", &[("status", status, None)]);
}

fn atmosphere_test(ui: &mut egui::Ui, test: &mut AtmosphereTest) {
    terminal::heading(ui, "atmosphere test");
    if terminal::button(ui, "measure the selected planet's air")
        && !matches!(test, AtmosphereTest::Running(_))
    {
        *test = AtmosphereTest::Requested;
    }
    terminal::note(
        ui,
        "From orbit, high in the air and 1 km up facing the horizon, with Earth's and Venus's air off and at 16 and 24 view steps: 18 views of 6.5 s, about 2 min. Pauses the clock; keep still with the window focused.",
    );
    let r = match test {
        AtmosphereTest::Idle => return,
        AtmosphereTest::Requested => {
            terminal::facts_with_details(
                ui,
                "debug-atmosphere-test",
                &[("status", "starting".to_string(), None)],
            );
            return;
        }
        AtmosphereTest::Running(run) => {
            terminal::facts_with_details(
                ui,
                "debug-atmosphere-test",
                &[("status", run.status(), None)],
            );
            return;
        }
        AtmosphereTest::Failed(error) => {
            terminal::facts_with_details(
                ui,
                "debug-atmosphere-test",
                &[("status", error.clone(), None)],
            );
            return;
        }
        AtmosphereTest::Done(r) => r,
    };
    // One row per view and preset: the GPU total with the air off, then
    // what the air adds at each step count.
    let mut rows: Vec<(String, String)> = Vec::new();
    for off in r.cases.iter().filter(|m| m.case.view_steps.is_none()) {
        let on = r
            .cases
            .iter()
            .filter(|m| {
                m.case.view == off.case.view
                    && m.case.preset == off.case.preset
                    && m.case.view_steps.is_some()
            })
            .map(|m| {
                format!(
                    "{} +{:.2}",
                    m.case.view_steps.unwrap_or(0),
                    m.gpu_total_ms - off.gpu_total_ms
                )
            })
            .collect::<Vec<_>>()
            .join(" · ");
        rows.push((
            format!(
                "{} · {}",
                off.case.view.label(),
                preset_label(off.case.preset)
            ),
            format!("off {} ms · {on}", number(off.gpu_total_ms, 2)),
        ));
    }
    let detail = "GPU time of every pass with the air off, then what the air adds at 16 and 24 view steps, in ms.";
    let mut facts: Vec<(&str, String, Option<&str>)> = vec![(
        "commit",
        r.commit.clone(),
        Some("Build measured; +dirty means uncommitted changes."),
    )];
    facts.extend(
        rows.iter()
            .map(|(label, value)| (label.as_str(), value.clone(), Some(detail))),
    );
    facts.push((
        "view",
        format!(
            "{} x {}, MSAA {}, FOV {:.0}°, {} light steps",
            r.size[0],
            r.size[1],
            if r.msaa { "on" } else { "off" },
            r.fov_deg,
            r.light_steps
        ),
        Some("Window size in physical pixels, the 4x MSAA switch, the field of view and the steps towards each star."),
    ));
    terminal::facts_with_details(ui, "debug-atmosphere-test", &facts);
    terminal::note(
        ui,
        "Each run is appended to data/logs/atmosphere-test.jsonl, with frame times and the transparent pass per view.",
    );
}

fn core_load_test(ui: &mut egui::Ui, load: &mut CoreLoad, field: &Starfield) {
    terminal::heading(ui, "core load test");
    if terminal::button(ui, "load the galactic centre from scratch") {
        *load = CoreLoad::Requested;
    }
    terminal::note(
        ui,
        "Jumps 1 pc from the black hole with no stars loaded and times loading every cell; keep still until it finishes.",
    );
    let (time, cells, stars) = match *load {
        CoreLoad::Idle | CoreLoad::Requested => return,
        CoreLoad::Running(started) => (
            format!("{} s so far", number(started.elapsed().as_secs_f64(), 1)),
            format!(
                "{} loaded, {} pending",
                grouped(field.cells.len()),
                grouped(field.pending)
            ),
            grouped(field.stars),
        ),
        CoreLoad::Done {
            cells,
            stars,
            seconds,
        } => (
            format!("{} s", number(seconds, 2)),
            grouped(cells),
            grouped(stars),
        ),
    };
    terminal::facts_with_details(
        ui,
        "debug-core-load",
        &[
            (
                "time",
                time,
                Some("From the click until no cells are pending."),
            ),
            (
                "cells",
                cells,
                Some("Cells loaded, of every brightness tier, around the camera."),
            ),
            ("stars", stars, Some("Stars generated in those cells.")),
        ],
    );
}

pub fn save_bug_report(
    commands: &mut Commands,
    universe: &Universe,
    nav: &Navigation,
    clock: &Clock,
    (rig, pose): (&CameraRig, &CameraPose),
    starfield: &Starfield,
) -> String {
    let (galaxy, clock) = (&universe.0, &clock.0);
    let system = nav.system(galaxy);
    let state = format!(
        "System ID: {}\nArrived: {}\nList: {}\nPage: {}\nPresentation mode: {:?}\nFollowing: {}\nSelection: {:?}\nSource: {}\nSystems: {}\nTasks: {}\nSimulated seconds: {:.3}\nRate: {:.3}\nPaused: {}\nDesired target (m): {:?}\nDesired range (m): {:.3}\nDisplayed eye relative to page star (m): {:?}\nDisplayed range (m): {:.3}\nFlight progress: {:?}\nGalaxy: {}\n",
        system.map_or("none", |s| s.id.as_str()),
        nav.arrived,
        nav.list().is_some(),
        nav.page,
        nav.presentation_mode,
        nav.following,
        nav.selection,
        galaxy.source,
        galaxy.systems.len(),
        galaxy
            .systems
            .iter()
            .map(|s| s.planets.len())
            .sum::<usize>(),
        clock.seconds,
        clock.rate,
        clock.paused,
        nav.target,
        nav.distance,
        pose.eye,
        pose.distance,
        rig.progress(),
        starfield.summary(),
    );
    match crate::devtools::diagnostics::save_bug_report(&state) {
        Ok(path) => {
            info!("Bug report saved to {}", path.display());
            commands
                .spawn(bevy::render::view::screenshot::Screenshot::primary_window())
                .observe(bevy::render::view::screenshot::save_to_disk(
                    path.with_extension("png"),
                ));
            format!("Saved {}", path.display())
        }
        Err(err) => format!("Could not save bug report: {err}"),
    }
}
