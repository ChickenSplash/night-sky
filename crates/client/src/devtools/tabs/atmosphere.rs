//! The `atmosphere` tab: optional override air on the selected planet, its
//! presets and shader steps, and what it costs the GPU.
use super::perf::{gpu_total, pass_times};
use crate::actions::UiCommand;
use crate::render::atmosphere::{
    self, AirSettings, AtmosphereChange, AtmosphereTuning, GAS, HAZE, HAZE_ABSORBED, HEIGHT_KM,
    PRESSURE_BAR, Preset,
};
use crate::ui::kit::Slider;
use crate::ui::terminal;
use bevy::diagnostic::DiagnosticsStore;
use bevy::prelude::*;
use bevy_egui::egui;

/// The transparent 3D pass, where the shell draws.
const PASS: &str = "main_transparent_pass_3d";

pub fn atmosphere(
    ui: &mut egui::Ui,
    tuning: &AtmosphereTuning,
    diagnostics: Option<&DiagnosticsStore>,
    out: &mut Vec<UiCommand>,
) {
    terminal::heading(ui, "atmosphere");
    terminal::note(
        ui,
        "Global atmosphere rendering and quality switches are in the settings tab. Select a planet to tune its debug air here.",
    );
    gpu(ui, diagnostics);
    if terminal::check(
        ui,
        tuning.enabled,
        "Selected planet override",
        true,
        "Replace only the selected planet's generated air with the settings below. Disable to restore generated air. Editing air settings enables the override; quality settings do not.",
    ) {
        out.push(UiCommand::SetAtmosphere(AtmosphereChange {
            enabled: Some(!tuning.enabled),
            ..default()
        }));
    }
    let mut change = |change: AtmosphereChange| out.push(UiCommand::SetAtmosphere(change));

    terminal::heading(ui, "presets");
    // The preset last applied is lit, marked `*` once a slider has moved.
    let edited = tuning.air != AirSettings::preset(tuning.preset);
    ui.horizontal_wrapped(|ui| {
        for (preset, label) in Preset::ALL {
            let chosen = tuning.enabled && preset == tuning.preset;
            let label = if chosen && edited {
                format!("{label}*")
            } else {
                label.to_owned()
            };
            if terminal::tab(ui, &label, chosen) {
                change(AtmosphereChange {
                    preset: Some(preset),
                    ..default()
                });
            }
        }
    });

    let air = tuning.air;
    terminal::heading(ui, "air");
    let slider =
        |ui: &mut egui::Ui, value: f64, slider: Slider| terminal::slider(ui, &slider, value);
    let row = |label, detail, range, unit, logarithmic| Slider {
        label,
        detail,
        range,
        unit,
        logarithmic,
        integer: false,
    };
    if let Some(v) = slider(
        ui,
        air.pressure_bar,
        row(
            "Pressure",
            "Surface pressure. Gas scattering grows with it; the haze does not.",
            PRESSURE_BAR,
            " bar",
            true,
        ),
    ) {
        change(AtmosphereChange {
            pressure_bar: Some(v),
            ..default()
        });
    }
    if let Some(v) = slider(
        ui,
        air.scale_height_km,
        row(
            "Scale height",
            "Override scale height. Generated air derives this from temperature, molecular mass and gravity.",
            HEIGHT_KM,
            " km",
            true,
        ),
    ) {
        change(AtmosphereChange {
            scale_height_km: Some(v),
            ..default()
        });
    }
    if let Some(v) = slider(
        ui,
        air.gas,
        row(
            "Gas",
            "Rayleigh scattering per bar, against Earth's air. Folds in the gas (carbon dioxide about 2.4, hydrogen about 0.2) and its number density per bar, higher in colder air.",
            GAS,
            "x",
            false,
        ),
    ) {
        change(AtmosphereChange {
            gas: Some(v),
            ..default()
        });
    }
    if let Some(v) = slider(
        ui,
        air.haze,
        row(
            "Haze",
            "Haze at the ground, against Earth's continental haze, whatever the pressure.",
            HAZE,
            "x",
            true,
        ),
    ) {
        change(AtmosphereChange {
            haze: Some(v),
            ..default()
        });
    }
    if let Some(v) = slider(
        ui,
        air.haze_height_km,
        row(
            "Haze height",
            "How far up the haze thins by a factor of e.",
            HEIGHT_KM,
            " km",
            true,
        ),
    ) {
        change(AtmosphereChange {
            haze_height_km: Some(v),
            ..default()
        });
    }
    if let Some(v) = slider(
        ui,
        air.haze_absorbed,
        row(
            "Haze absorbs",
            "The share of the light the haze stops that it absorbs rather than scatters: 0.1 for Earth's dust, about 0.001 for clouds of droplets. Under thick haze it sets how much light gets through by scattering many times.",
            HAZE_ABSORBED,
            "",
            true,
        ),
    ) {
        change(AtmosphereChange {
            haze_absorbed: Some(v),
            ..default()
        });
    }
    for (channel, label) in ["Haze red", "Haze green", "Haze blue"]
        .into_iter()
        .enumerate()
    {
        if let Some(v) = slider(
            ui,
            air.haze_colour[channel],
            row(
                label,
                "How much of this colour the haze scatters, 0 to 1. Its absorption stays grey.",
                0.0..=1.0,
                "",
                false,
            ),
        ) {
            let mut colour = air.haze_colour;
            colour[channel] = v;
            change(AtmosphereChange {
                haze_colour: Some(colour),
                ..default()
            });
        }
    }
    let physical = air.atmosphere();
    // Straight up from the ground, for blue light.
    terminal::facts_with_details(
        ui,
        "debug-atmosphere-air",
        &[
            (
                "top of the air",
                format!("{} km", crate::format::number(physical.top / 1e3, 0)),
                Some(
                    "Above this the air is taken as empty: 12.5 scale heights of the gas or haze, whichever is taller.",
                ),
            ),
            (
                "optical depth up",
                format!(
                    "gas {:.3} (blue), haze {:.3}",
                    physical.rayleigh.z * physical.rayleigh_height,
                    (physical.mie.max_element() + physical.mie_absorption) * physical.mie_height,
                ),
                Some(
                    "Looking straight up from the ground. Above 1, most direct light is lost on the way and the sky is mostly scattered light.",
                ),
            ),
        ],
    );

    terminal::heading(ui, "quality");
    let steps = |label, detail, range: std::ops::RangeInclusive<u32>| Slider {
        label,
        detail,
        range: f64::from(*range.start())..=f64::from(*range.end()),
        unit: "",
        logarithmic: false,
        integer: true,
    };
    if let Some(v) = slider(
        ui,
        f64::from(tuning.view_steps),
        steps(
            "View steps",
            "Steps along each pixel's line of sight. Fewer are cheaper but band the limb.",
            atmosphere::VIEW_STEPS,
        ),
    ) {
        change(AtmosphereChange {
            view_steps: Some(v as u32),
            ..default()
        });
    }
    if let Some(v) = slider(
        ui,
        f64::from(tuning.light_steps),
        steps(
            "Light steps",
            "Steps from each view step towards every star, for the light lost on the way in. Only used with the lookup tables off.",
            atmosphere::LIGHT_STEPS,
        ),
    ) {
        change(AtmosphereChange {
            light_steps: Some(v as u32),
            ..default()
        });
    }
}

/// The frame's GPU time and the transparent pass's share, to compare with
/// the switch on and off.
fn gpu(ui: &mut egui::Ui, diagnostics: Option<&DiagnosticsStore>) {
    let passes = pass_times(diagnostics);
    if passes.is_empty() {
        terminal::note(ui, "Waiting for render diagnostics…");
        return;
    }
    let pass = passes
        .get(PASS)
        .and_then(|p| p.gpu)
        .map_or("·".into(), |ms| {
            format!("{} ms", crate::format::number(ms, 2))
        });
    terminal::facts_with_details(
        ui,
        "debug-atmosphere-gpu",
        &[
            (
                "gpu frame",
                format!("{} ms", crate::format::number(gpu_total(&passes), 2)),
                Some(
                    "GPU time of the whole frame. Toggle Atmospheres in the settings tab to compare its share.",
                ),
            ),
            (
                "transparent pass",
                pass,
                Some(
                    "The pass the shell draws in, along with star points, limbs and the galaxy glow.",
                ),
            ),
        ],
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The commands a click on the text `label` sends.
    fn click(label: &str) -> Vec<UiCommand> {
        let ctx = egui::Context::default();
        let tuning = AtmosphereTuning::default();
        let frame = |events| {
            let mut commands = Vec::new();
            let output = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(800.0, 1000.0),
                    )),
                    events,
                    ..default()
                },
                |ctx| {
                    egui::CentralPanel::default()
                        .show(ctx, |ui| atmosphere(ui, &tuning, None, &mut commands));
                },
            );
            (output, commands)
        };
        let (output, _) = frame(vec![]);
        let pos = output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::epaint::Shape::Text(text) if text.galley.job.text.contains(label) => {
                    Some(text.pos + text.galley.size() * 0.5)
                }
                _ => None,
            })
            .unwrap_or_else(|| panic!("missing control: {label}"));
        let button = |pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        frame(vec![egui::Event::PointerMoved(pos), button(true)]);
        frame(vec![button(false)]).1
    }

    #[test]
    fn presets_and_the_switch_send_commands() {
        assert_eq!(
            click("Venus"),
            vec![UiCommand::SetAtmosphere(AtmosphereChange {
                preset: Some(Preset::Venus),
                ..default()
            })]
        );
        assert_eq!(
            click("Selected planet override"),
            vec![UiCommand::SetAtmosphere(AtmosphereChange {
                enabled: Some(true),
                ..default()
            })]
        );
    }
}
