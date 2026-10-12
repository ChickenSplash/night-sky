//! Debug-only switches for A/B performance tests, set from the debug window's
//! `perf` tab or `set_debug`. They are not product settings and start in the
//! normal rendering state. Natively, Bevy's render diagnostics also record GPU
//! time and shader invocations per pass for the `perf` tab.
use crate::scene::SpaceCamera;
use bevy::prelude::*;
use serde::Deserialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DebugSwitch {
    /// Stop aiming the star loader at the camera; loaded cells stay put.
    FreezeStream,
    /// Draw galactic star points at all.
    GalacticPoints,
    /// Rasterise and shade star points. Off, they still run the vertex shader.
    PointPixels,
    /// 4x MSAA on the 3D and egui cameras.
    Msaa,
    Bloom,
    /// Hide star batches whose grid block lies outside the view.
    CullBlocks,
    /// Natively, draw galactic stars as single-pixel points shaped by a
    /// composite pass, rather than as triangles (`stars/points.rs`).
    /// No effect in the browser.
    PointStars,
    /// Draw nearby multiple systems star by star once their stars could show
    /// apart, rather than as one point until arrival (`starfield.rs`).
    SplitMultiples,
}

#[derive(Resource, Clone, Copy, Debug, PartialEq)]
pub struct RenderDebug {
    pub freeze_stream: bool,
    pub galactic_points: bool,
    pub point_pixels: bool,
    pub msaa: bool,
    pub bloom: bool,
    pub cull_blocks: bool,
    pub point_stars: bool,
    pub split_multiples: bool,
}

impl Default for RenderDebug {
    fn default() -> Self {
        Self {
            freeze_stream: false,
            galactic_points: true,
            point_pixels: true,
            msaa: true,
            bloom: true,
            cull_blocks: true,
            point_stars: cfg!(not(target_arch = "wasm32")),
            split_multiples: true,
        }
    }
}

impl RenderDebug {
    pub const SWITCHES: [(DebugSwitch, &str); 8] = [
        (DebugSwitch::FreezeStream, "Freeze star streaming"),
        (DebugSwitch::GalacticPoints, "Galactic star points"),
        (DebugSwitch::PointPixels, "Shade point pixels"),
        (DebugSwitch::Msaa, "4x MSAA"),
        (DebugSwitch::Bloom, "Bloom"),
        (DebugSwitch::CullBlocks, "Cull blocks outside the view"),
        (DebugSwitch::PointStars, "Stars as points"),
        (DebugSwitch::SplitMultiples, "Split multiple stars"),
    ];

    pub fn get(&self, switch: DebugSwitch) -> bool {
        match switch {
            DebugSwitch::FreezeStream => self.freeze_stream,
            DebugSwitch::GalacticPoints => self.galactic_points,
            DebugSwitch::PointPixels => self.point_pixels,
            DebugSwitch::Msaa => self.msaa,
            DebugSwitch::Bloom => self.bloom,
            DebugSwitch::CullBlocks => self.cull_blocks,
            DebugSwitch::PointStars => self.point_stars,
            DebugSwitch::SplitMultiples => self.split_multiples,
        }
    }

    pub fn set(&mut self, switch: DebugSwitch, on: bool) {
        *match switch {
            DebugSwitch::FreezeStream => &mut self.freeze_stream,
            DebugSwitch::GalacticPoints => &mut self.galactic_points,
            DebugSwitch::PointPixels => &mut self.point_pixels,
            DebugSwitch::Msaa => &mut self.msaa,
            DebugSwitch::Bloom => &mut self.bloom,
            DebugSwitch::CullBlocks => &mut self.cull_blocks,
            DebugSwitch::PointStars => &mut self.point_stars,
            DebugSwitch::SplitMultiples => &mut self.split_multiples,
        } = on;
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
