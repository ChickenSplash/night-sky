//! The `settings` tab: display, quality, rendering and diagnostic switches.
use crate::Navigation;
use crate::actions::{Display, UiCommand};
use crate::render::debug::{DebugSwitch, RenderDebug};
use crate::ui::kit;
use crate::ui::terminal::{self};
use bevy::prelude::*;
use bevy_egui::egui;

pub fn settings(
    ui: &mut egui::Ui,
    nav: &Navigation,
    debug: &RenderDebug,
    fullscreen: bool,
    out: &mut Vec<UiCommand>,
) {
    terminal::heading(ui, "display");
    if terminal::check(
        ui,
        fullscreen,
        "Fullscreen",
        true,
        "Fill the window's monitor without borders. F11 also toggles it.",
    ) {
        out.push(UiCommand::SetFullscreen { on: !fullscreen });
    }
    if terminal::check(
        ui,
        nav.overlays,
        "Scene overlays",
        true,
        "Draw everything over the scene that is not in it: the options below, and selection and hover rings, star and barycentre markers and constellation lines. Clicks still select bodies when hidden.",
    ) {
        out.push(UiCommand::SetDisplay {
            option: Display::Overlays,
            on: !nav.overlays,
        });
    }
    // Each overlay's own switch, which applies only while overlays are on.
    ui.indent("scene-overlays", |ui| {
        for (option, on, label, enabled, detail) in [
            (
                Display::Orbits,
                nav.orbits,
                "Orbital paths",
                true,
                "Draw orbital paths for planets and companion stars. Orbits keep running when paths are hidden.",
            ),
            (
                Display::Labels,
                nav.labels,
                "Planet labels",
                true,
                "Show planet names over the scene, including task names where a planet has an attached task.",
            ),
            (
                Display::Markers,
                nav.markers,
                "Body markers",
                true,
                "Show small rings for planets too small to see at physical size. Selection and hover rings remain visible; hiding markers does not disable picking.",
            ),
            (
                Display::VelocityMarker,
                nav.velocity_marker,
                "Velocity marker",
                true,
                "Show the direction of measured camera travel. Turning in place does not create a velocity marker.",
            ),
            (
                Display::PredictedPaths,
                nav.predicted_paths,
                "Craft predicted paths",
                true,
                "Draw the selected craft's predicted coast path, with its apsides and reference changes.",
            ),
        ] {
            if terminal::check(ui, on, label, nav.overlays && enabled, detail) {
                out.push(UiCommand::SetDisplay { option, on: !on });
            }
        }
    });
    if let Some(degrees) = terminal::slider(
        ui,
        &kit::Slider {
            label: "Field of view",
            detail: "Vertical field of view. Wider shows more of the sky at once; narrower magnifies.",
            range: f64::from(*crate::FOV_DEG.start())..=f64::from(*crate::FOV_DEG.end()),
            unit: "°",
            logarithmic: false,
            integer: true,
        },
        f64::from(nav.fov_deg),
    ) {
        out.push(UiCommand::SetFov {
            degrees: degrees as f32,
        });
    }
    terminal::heading(ui, "quality");
    for (switch, label) in [
        (DebugSwitch::Msaa, "4x MSAA"),
        (DebugSwitch::SplitMultiples, "Resolve multiple-star systems"),
        (DebugSwitch::Oblateness, "Flattened bodies"),
        (DebugSwitch::Ground, "Detailed ground"),
    ] {
        let enabled = match switch {
            DebugSwitch::Ground => nav.realistic,
            DebugSwitch::SplitMultiples => debug.galactic_points,
            _ => true,
        };
        switch_row(ui, debug, switch, label, enabled, out);
    }

    terminal::heading(ui, "rendering");
    if terminal::check(
        ui,
        nav.realistic,
        "Realistic lighting",
        true,
        "Draw stars by temperature and physical brightness, and light planets by their stars. When off, bodies use wireframes. Exposure and compression apply only with realistic lighting.",
    ) {
        out.push(UiCommand::SetDisplay {
            option: Display::Realistic,
            on: !nav.realistic,
        });
    }
    ui.indent("realistic-rendering", |ui| {
        switch_row(ui, debug, DebugSwitch::Bloom, "Bloom", nav.realistic, out);
    if terminal::check(
        ui,
        nav.galaxy_glow,
        "Smooth galaxy glow (prototype)",
        nav.realistic,
        "Draw unresolved galactic light as a smooth diffuse glow. This prototype has no dust or detailed spiral structure. Requires realistic lighting.",
    ) {
        out.push(UiCommand::SetDisplay {
            option: Display::GalaxyGlow,
            on: !nav.galaxy_glow,
        });
    }
        if terminal::check(
            ui,
            nav.wireframe,
            "Wireframes over bodies",
            nav.realistic && nav.overlays,
            "Overlay latitude and longitude wireframes on realistically lit bodies. Requires realistic lighting and scene overlays; without realistic lighting, bodies are wireframes.",
        ) {
            out.push(UiCommand::SetDisplay {
                option: Display::Wireframe,
                on: !nav.wireframe,
            });
        }
        switch_row(ui, debug, DebugSwitch::Atmospheres, "Atmospheres", nav.realistic, out);
        ui.indent("atmosphere-rendering", |ui| {
            let air = nav.realistic && debug.atmospheres;
            for (switch, label) in [
                (DebugSwitch::AtmosphereMultiple, "Multiple scattering"),
                (DebugSwitch::AtmosphereGround, "Ground lighting"),
            ] {
                switch_row(ui, debug, switch, label, air && debug.atmosphere_tables, out);
            }
            switch_row(ui, debug, DebugSwitch::GiantDescent, "Giant atmosphere descent", air, out);
        });
    });
    terminal::facts_with_details(ui, "debug-exposure", &crate::render::exposure::facts(nav));
    terminal::note(
        ui,
        if nav.realistic {
            "Exposure and Dynamic Range Compression are manual controls at the bottom right."
        } else {
            "Turn on realistic lighting above to use these."
        },
    );
    if terminal::section_with_default(ui, "debug", false) {
        debug_controls(ui, nav, debug, out);
    }
}

fn switch_row(
    ui: &mut egui::Ui,
    debug: &RenderDebug,
    switch: DebugSwitch,
    label: &str,
    enabled: bool,
    out: &mut Vec<UiCommand>,
) {
    let on = debug.get(switch);
    if terminal::check(ui, on, label, enabled, switch.detail()) {
        out.push(UiCommand::SetDebug { switch, on: !on });
    }
}

fn debug_controls(
    ui: &mut egui::Ui,
    nav: &Navigation,
    debug: &RenderDebug,
    out: &mut Vec<UiCommand>,
) {
    terminal::heading(ui, "streaming");
    switch_row(
        ui,
        debug,
        DebugSwitch::FreezeStream,
        "Freeze star streaming",
        true,
        out,
    );
    terminal::heading(ui, "stars");
    switch_row(
        ui,
        debug,
        DebugSwitch::GalacticPoints,
        "Galactic star points",
        true,
        out,
    );
    ui.indent("star-debug", |ui| {
        for (switch, label) in [
            (DebugSwitch::PointPixels, "Shade point pixels"),
            (DebugSwitch::PointStars, "Stars as points"),
            (DebugSwitch::CullBlocks, "Cull blocks outside the view"),
        ] {
            switch_row(ui, debug, switch, label, debug.galactic_points, out);
        }
    });
    terminal::heading(ui, "atmospheres");
    let air = nav.realistic && debug.atmospheres;
    switch_row(
        ui,
        debug,
        DebugSwitch::AtmosphereTables,
        "Atmosphere lookup tables",
        air,
        out,
    );
    ui.indent("table-debug", |ui| {
        switch_row(
            ui,
            debug,
            DebugSwitch::AtmosphereStopOpaque,
            "Stop atmosphere at opaque",
            air && debug.atmosphere_tables,
            out,
        );
    });
    terminal::heading(ui, "ground");
    switch_row(
        ui,
        debug,
        DebugSwitch::GroundLevels,
        "Colour chunks by detail level",
        nav.realistic && debug.ground,
        out,
    );
    terminal::note(
        ui,
        "Implementation switches for A/B tests; reset on restart. Freeze streaming first to hold the star population still.",
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The commands a click on the switch labelled `label` sends.
    fn click(nav: &Navigation, label: &str) -> Vec<UiCommand> {
        click_with(nav, false, label)
    }

    fn click_with(nav: &Navigation, fullscreen: bool, label: &str) -> Vec<UiCommand> {
        click_switch(nav, &RenderDebug::default(), fullscreen, label, false)
    }

    fn click_switch(
        nav: &Navigation,
        debug: &RenderDebug,
        fullscreen: bool,
        label: &str,
        debug_open: bool,
    ) -> Vec<UiCommand> {
        let ctx = egui::Context::default();
        let frame = |events| {
            let mut commands = Vec::new();
            let output = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(800.0, 2500.0),
                    )),
                    events,
                    ..default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        ui.data_mut(|data| {
                            data.insert_persisted(
                                ui.make_persistent_id(("section", "debug")),
                                debug_open,
                            )
                        });
                        settings(ui, nav, debug, fullscreen, &mut commands);
                    });
                },
            );
            (output, commands)
        };
        let (output, _) = frame(vec![]);
        let pos = output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::epaint::Shape::Text(text) if text.galley.job.text == label => {
                    Some(text.pos + text.galley.size() * 0.5)
                }
                _ => None,
            })
            .unwrap_or_else(|| panic!("missing display switch: {label}"));
        let button = |pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        frame(vec![egui::Event::PointerMoved(pos), button(true)]);
        frame(vec![button(false)]).1
    }

    const OVERLAYS: [(&str, Display); 6] = [
        ("Orbital paths", Display::Orbits),
        ("Planet labels", Display::Labels),
        ("Body markers", Display::Markers),
        ("Velocity marker", Display::VelocityMarker),
        ("Craft predicted paths", Display::PredictedPaths),
        ("Wireframes over bodies", Display::Wireframe),
    ];

    #[test]
    fn display_switches_send_commands_from_settings_tab() {
        let nav = Navigation {
            wireframe: true,
            ..default()
        };
        for (label, option) in OVERLAYS.into_iter().chain([
            ("Scene overlays", Display::Overlays),
            ("Realistic lighting", Display::Realistic),
        ]) {
            assert_eq!(
                click(&nav, label),
                vec![UiCommand::SetDisplay { option, on: false }],
                "{label}"
            );
        }
    }

    #[test]
    fn fullscreen_switch_toggles_the_window_mode() {
        let nav = Navigation::default();
        for on in [false, true] {
            assert_eq!(
                click_with(&nav, on, "Fullscreen"),
                vec![UiCommand::SetFullscreen { on: !on }]
            );
        }
    }

    #[test]
    fn overlay_switches_wait_for_scene_overlays() {
        let nav = Navigation {
            overlays: false,
            ..default()
        };
        for (label, _) in OVERLAYS {
            assert!(click(&nav, label).is_empty(), "{label}");
        }
        assert!(!nav.overlay(nav.orbits));
        assert_eq!(
            click(&nav, "Scene overlays"),
            vec![UiCommand::SetDisplay {
                option: Display::Overlays,
                on: true
            }]
        );
    }

    const SWITCHES: [(&str, DebugSwitch); 17] = [
        ("4x MSAA", DebugSwitch::Msaa),
        ("Resolve multiple-star systems", DebugSwitch::SplitMultiples),
        ("Flattened bodies", DebugSwitch::Oblateness),
        ("Detailed ground", DebugSwitch::Ground),
        ("Bloom", DebugSwitch::Bloom),
        ("Atmospheres", DebugSwitch::Atmospheres),
        ("Multiple scattering", DebugSwitch::AtmosphereMultiple),
        ("Ground lighting", DebugSwitch::AtmosphereGround),
        ("Giant atmosphere descent", DebugSwitch::GiantDescent),
        ("Freeze star streaming", DebugSwitch::FreezeStream),
        ("Galactic star points", DebugSwitch::GalacticPoints),
        ("Shade point pixels", DebugSwitch::PointPixels),
        ("Stars as points", DebugSwitch::PointStars),
        ("Cull blocks outside the view", DebugSwitch::CullBlocks),
        ("Atmosphere lookup tables", DebugSwitch::AtmosphereTables),
        (
            "Stop atmosphere at opaque",
            DebugSwitch::AtmosphereStopOpaque,
        ),
        ("Colour chunks by detail level", DebugSwitch::GroundLevels),
    ];

    #[test]
    fn all_render_switches_send_commands_from_settings() {
        let nav = Navigation::default();
        let debug = RenderDebug::default();
        for (label, switch) in SWITCHES {
            assert_eq!(
                click_switch(&nav, &debug, false, label, true),
                vec![UiCommand::SetDebug {
                    switch,
                    on: !debug.get(switch)
                }],
                "{label}"
            );
        }
    }

    #[test]
    fn rendering_children_wait_for_realistic_lighting() {
        let nav = Navigation {
            realistic: false,
            ..default()
        };
        let debug = RenderDebug::default();
        for label in [
            "Bloom",
            "Smooth galaxy glow (prototype)",
            "Wireframes over bodies",
            "Atmospheres",
            "Multiple scattering",
            "Ground lighting",
            "Giant atmosphere descent",
            "Detailed ground",
            "Atmosphere lookup tables",
            "Stop atmosphere at opaque",
            "Colour chunks by detail level",
        ] {
            assert!(
                click_switch(&nav, &debug, false, label, true).is_empty(),
                "{label}"
            );
        }
        // Independent controls still work in wireframe mode.
        assert_eq!(
            click(&nav, "4x MSAA"),
            vec![UiCommand::SetDebug {
                switch: DebugSwitch::Msaa,
                on: false
            }]
        );
        assert_eq!(
            click(&nav, "Flattened bodies"),
            vec![UiCommand::SetDebug {
                switch: DebugSwitch::Oblateness,
                on: false
            }]
        );
    }

    #[test]
    fn subsystem_children_wait_for_their_actual_dependencies() {
        let nav = Navigation::default();
        for (debug, labels) in [
            (
                RenderDebug {
                    atmospheres: false,
                    ..default()
                },
                &[
                    "Multiple scattering",
                    "Ground lighting",
                    "Giant atmosphere descent",
                    "Atmosphere lookup tables",
                    "Stop atmosphere at opaque",
                ][..],
            ),
            (
                RenderDebug {
                    atmosphere_tables: false,
                    ..default()
                },
                &[
                    "Multiple scattering",
                    "Ground lighting",
                    "Stop atmosphere at opaque",
                ][..],
            ),
            (
                RenderDebug {
                    galactic_points: false,
                    ..default()
                },
                &[
                    "Resolve multiple-star systems",
                    "Shade point pixels",
                    "Stars as points",
                    "Cull blocks outside the view",
                ][..],
            ),
            (
                RenderDebug {
                    ground: false,
                    ..default()
                },
                &["Colour chunks by detail level"][..],
            ),
        ] {
            for label in labels {
                assert!(
                    click_switch(&nav, &debug, false, label, true).is_empty(),
                    "{label}"
                );
            }
        }
        // Table-independent atmosphere features stay usable without tables.
        let debug = RenderDebug {
            atmosphere_tables: false,
            ..default()
        };
        assert_eq!(
            click_switch(&nav, &debug, false, "Giant atmosphere descent", true),
            vec![UiCommand::SetDebug {
                switch: DebugSwitch::GiantDescent,
                on: false
            }]
        );
    }

    #[test]
    fn settings_have_four_sections_and_each_switch_once_with_debug_closed_by_default() {
        let ctx = egui::Context::default();
        let draw = |debug_open| {
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(800.0, 2500.0),
                    )),
                    ..default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        if debug_open {
                            ui.data_mut(|data| {
                                data.insert_persisted(
                                    ui.make_persistent_id(("section", "debug")),
                                    true,
                                )
                            });
                        }
                        settings(
                            ui,
                            &Navigation::default(),
                            &RenderDebug::default(),
                            false,
                            &mut Vec::new(),
                        );
                    });
                },
            )
        };
        let labels = |output: egui::FullOutput| {
            output
                .shapes
                .into_iter()
                .filter_map(|shape| match shape.shape {
                    egui::epaint::Shape::Text(text) => Some(text.galley.job.text.clone()),
                    _ => None,
                })
                .collect::<Vec<_>>()
        };
        let closed = labels(draw(false));
        for section in ["display", "quality", "rendering"] {
            assert!(
                closed.iter().any(|label| label.contains(section)),
                "{section}"
            );
        }
        assert!(!closed.iter().any(|label| label == "Freeze star streaming"));
        let open = labels(draw(true));
        for (label, _) in SWITCHES {
            assert_eq!(
                open.iter().filter(|text| text.as_str() == label).count(),
                1,
                "{label}"
            );
        }
    }
}
