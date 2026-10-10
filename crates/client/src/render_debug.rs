//! Debug-only switches for A/B performance tests, set from the debug window's
//! `render` tab or `set_debug`. They are not product settings and start in the
//! normal rendering state. Natively, Bevy's render diagnostics also record GPU
//! time and shader invocations per pass for the `perf` tab.
use crate::scene::SpaceCamera;
use bevy::prelude::*;
use serde::Deserialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DebugSwitch {
    /// Stop aiming the star loader at the camera; loaded cells stay put.
    FreezeStream,
    /// Draw galactic star points at all.
    GalacticPoints,
    /// Rasterise and shade star points. Off, they still run the vertex shader.
    PointPixels,
    /// A 1.25 px point footprint instead of 2 px. Clips a little of each
    /// point's light; for measuring fill cost only.
    CompactPoints,
    /// 4x MSAA on the 3D and egui cameras.
    Msaa,
    Bloom,
}

#[derive(Resource, Clone, Copy, Debug, PartialEq)]
pub struct RenderDebug {
    pub freeze_stream: bool,
    pub galactic_points: bool,
    pub point_pixels: bool,
    pub compact_points: bool,
    pub msaa: bool,
    pub bloom: bool,
}

impl Default for RenderDebug {
    fn default() -> Self {
        Self {
            freeze_stream: false,
            galactic_points: true,
            point_pixels: true,
            compact_points: false,
            msaa: true,
            bloom: true,
        }
    }
}

impl RenderDebug {
    pub const SWITCHES: [(DebugSwitch, &str); 6] = [
        (DebugSwitch::FreezeStream, "Freeze star streaming"),
        (DebugSwitch::GalacticPoints, "Galactic star points"),
        (DebugSwitch::PointPixels, "Shade point pixels"),
        (DebugSwitch::CompactPoints, "Compact 1.25 px points"),
        (DebugSwitch::Msaa, "4x MSAA"),
        (DebugSwitch::Bloom, "Bloom"),
    ];

    pub fn get(&self, switch: DebugSwitch) -> bool {
        match switch {
            DebugSwitch::FreezeStream => self.freeze_stream,
            DebugSwitch::GalacticPoints => self.galactic_points,
            DebugSwitch::PointPixels => self.point_pixels,
            DebugSwitch::CompactPoints => self.compact_points,
            DebugSwitch::Msaa => self.msaa,
            DebugSwitch::Bloom => self.bloom,
        }
    }

    pub fn set(&mut self, switch: DebugSwitch, on: bool) {
        *match switch {
            DebugSwitch::FreezeStream => &mut self.freeze_stream,
            DebugSwitch::GalacticPoints => &mut self.galactic_points,
            DebugSwitch::PointPixels => &mut self.point_pixels,
            DebugSwitch::CompactPoints => &mut self.compact_points,
            DebugSwitch::Msaa => &mut self.msaa,
            DebugSwitch::Bloom => &mut self.bloom,
        } = on;
    }

    /// The star point footprint radius in physical pixels.
    pub fn point_radius_px(&self) -> f32 {
        if self.compact_points { 1.25 } else { 2.0 }
    }
}

pub struct RenderDebugPlugin;

impl Plugin for RenderDebugPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<RenderDebug>()
            .add_systems(Update, apply_msaa);
        // Timestamp and pipeline-statistics queries exist only natively.
        #[cfg(not(target_arch = "wasm32"))]
        app.add_plugins(bevy::render::diagnostic::RenderDiagnosticsPlugin);
    }
}

type WindowCameras = Or<(With<SpaceCamera>, With<bevy_egui::PrimaryEguiContext>)>;

/// Cameras sharing the window keep the same MSAA, so the egui overlay switches too.
fn apply_msaa(
    mut commands: Commands,
    debug: Res<RenderDebug>,
    cameras: Query<Entity, WindowCameras>,
) {
    if !debug.is_changed() {
        return;
    }
    let msaa = if debug.msaa { Msaa::Sample4 } else { Msaa::Off };
    for camera in &cameras {
        commands.entity(camera).insert(msaa);
    }
}
