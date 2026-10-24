//! Debug-only switches for A/B performance tests, set from the debug window's
//! `perf` tab or `set_debug`. They are not product settings and start in the
//! normal rendering state. Bevy's render diagnostics also record GPU
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
    /// Draw galactic stars as single-pixel points shaped by a composite pass,
    /// rather than as triangles (`stars/points.rs`).
    PointStars,
    /// Draw nearby multiple systems star by star once their stars could show
    /// apart, rather than as one point until arrival (`starfield.rs`).
    SplitMultiples,
    /// Draw planetary atmospheres (`atmosphere.rs`).
    Atmospheres,
    /// Move each pixel's atmosphere samples along its steps, so steps show
    /// as fine grain rather than bands.
    AtmosphereJitter,
    /// Read the starlight reaching the air from a table built per air,
    /// rather than marching towards each star (`atmosphere/tables.rs`).
    AtmosphereTables,
    /// End each line of sight where the air in front turns opaque, whether
    /// or not ground lies beyond. Needs the tables.
    AtmosphereStopOpaque,
    /// Add the light scattered more than once, from a table built per air
    /// (`atmosphere/multiple.rs`). Needs the tables.
    AtmosphereMultiple,
    /// Light the surface with filtered sunlight and skylight from the ground
    /// row of the scattering table. Needs the tables, independently of sky multiple scattering.
    AtmosphereGround,
    /// Draw bodies flattened by their spin, rather than as spheres.
    Oblateness,
    /// Draw planets' ground as chunks refined near the camera, rather than as
    /// one sphere mesh.
    Ground,
    /// Tint ground chunks by their level of detail.
    GroundLevels,
}

impl DebugSwitch {
    pub fn detail(self) -> &'static str {
        match self {
            Self::FreezeStream => {
                "Stop aiming star streaming at the camera. The loader keeps its last aim; pending work can finish. Useful for A/B tests with a stable star population."
            }
            Self::GalacticPoints => {
                "Draw galactic star points. Turn off to isolate other rendering costs; generated systems and the diffuse galaxy glow are separate."
            }
            Self::PointPixels => {
                "Rasterise and shade star-point pixels. When off, points still run the vertex shader, separating vertex cost from pixel fill cost."
            }
            Self::Msaa => {
                "Enable 4x multisample anti-aliasing on both window cameras. Turn off to compare its GPU cost and edge quality."
            }
            Self::Bloom => {
                "Spread bright light into a soft glow around stars and bodies. Turn off to compare the post-processing cost."
            }
            Self::CullBlocks => {
                "Skip drawing star grid blocks outside the camera view. Disabling this draws all uploaded blocks for a culling-cost comparison."
            }
            Self::PointStars => {
                "Use single-pixel points and a composite pass instead of triangles for galactic stars."
            }
            Self::SplitMultiples => {
                "Resolve nearby multiple-star systems into individual stars once their separation is visible, rather than drawing one combined point until arrival."
            }
            Self::Atmospheres => {
                "Draw the scattering shell of planetary atmospheres. Turn off to compare the look and its GPU cost."
            }
            Self::AtmosphereJitter => {
                "Sample the atmosphere at a different point of each step in neighbouring pixels, so too few steps show as fine grain instead of coloured bands. Turn off to compare."
            }
            Self::AtmosphereTables => {
                "Read the starlight reaching each point of the air from a lookup table built once per air, instead of marching towards each star. Should look the same and cost less; turn off to compare."
            }
            Self::AtmosphereStopOpaque => {
                "End each line of sight where the air in front of the camera turns opaque, so thick air such as Venus's hides its ground and all the steps go where light can be seen. Uses the lookup tables, so does nothing with them off. Turn off to compare."
            }
            Self::AtmosphereMultiple => {
                "Add the light that scatters more than once in the air, read from a table built once per air, so thick air such as Venus's glows through instead of turning black, and twilight lasts longer. Uses the lookup tables, so does nothing with them off. Turn off to compare."
            }
            Self::AtmosphereGround => {
                "Light the ground with filtered sunlight and diffuse sky light, including its colour, from the atmosphere's scattering table. Applies to chunks and the sphere fallback once the table is ready. Needs atmospheres and lookup tables, independently of the sky's multiple-scattering switch. Turn off for the old grey surface dimming."
            }
            Self::Oblateness => {
                "Draw stars and planets flattened at the poles by their spin, with their air following the same shape. Turn off to draw every body as a sphere of its mean radius."
            }
            Self::Ground => {
                "Draw each planet's ground as a cube-sphere of chunks, refined near the camera so its facets stay under half a pixel from the true surface, where the air's ground lies. Turn off to draw the old single sphere mesh, whose facets sit up to 0.06% of the radius inside it."
            }
            Self::GroundLevels => {
                "Tint each ground chunk by its level of detail, cycling through six colours, to see where chunks split and that neighbours meet without cracks. Does nothing with the ground off."
            }
        }
    }
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
    pub atmospheres: bool,
    pub atmosphere_jitter: bool,
    pub atmosphere_tables: bool,
    pub atmosphere_stop_opaque: bool,
    pub atmosphere_multiple: bool,
    pub atmosphere_ground: bool,
    pub oblateness: bool,
    pub ground: bool,
    pub ground_levels: bool,
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
            point_stars: true,
            split_multiples: true,
            atmospheres: true,
            atmosphere_jitter: true,
            atmosphere_tables: true,
            atmosphere_stop_opaque: true,
            atmosphere_multiple: true,
            atmosphere_ground: true,
            oblateness: true,
            ground: true,
            ground_levels: false,
        }
    }
}

impl RenderDebug {
    pub const SWITCHES: [(DebugSwitch, &str); 17] = [
        (DebugSwitch::FreezeStream, "Freeze star streaming"),
        (DebugSwitch::GalacticPoints, "Galactic star points"),
        (DebugSwitch::PointPixels, "Shade point pixels"),
        (DebugSwitch::Msaa, "4x MSAA"),
        (DebugSwitch::Bloom, "Bloom"),
        (DebugSwitch::CullBlocks, "Cull blocks outside the view"),
        (DebugSwitch::PointStars, "Stars as points"),
        (DebugSwitch::SplitMultiples, "Split multiple stars"),
        (DebugSwitch::Atmospheres, "Atmospheres"),
        (DebugSwitch::AtmosphereJitter, "Jitter atmosphere steps"),
        (DebugSwitch::AtmosphereTables, "Atmosphere lookup tables"),
        (
            DebugSwitch::AtmosphereStopOpaque,
            "Stop atmosphere at opaque",
        ),
        (
            DebugSwitch::AtmosphereMultiple,
            "Atmosphere multiple scattering",
        ),
        (DebugSwitch::AtmosphereGround, "Atmosphere ground lighting"),
        (DebugSwitch::Oblateness, "Flattened bodies"),
        (DebugSwitch::Ground, "Ground chunks"),
        (DebugSwitch::GroundLevels, "Ground chunk levels"),
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
            DebugSwitch::Atmospheres => self.atmospheres,
            DebugSwitch::AtmosphereJitter => self.atmosphere_jitter,
            DebugSwitch::AtmosphereTables => self.atmosphere_tables,
            DebugSwitch::AtmosphereStopOpaque => self.atmosphere_stop_opaque,
            DebugSwitch::AtmosphereMultiple => self.atmosphere_multiple,
            DebugSwitch::AtmosphereGround => self.atmosphere_ground,
            DebugSwitch::Oblateness => self.oblateness,
            DebugSwitch::Ground => self.ground,
            DebugSwitch::GroundLevels => self.ground_levels,
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
            DebugSwitch::Atmospheres => &mut self.atmospheres,
            DebugSwitch::AtmosphereJitter => &mut self.atmosphere_jitter,
            DebugSwitch::AtmosphereTables => &mut self.atmosphere_tables,
            DebugSwitch::AtmosphereStopOpaque => &mut self.atmosphere_stop_opaque,
            DebugSwitch::AtmosphereMultiple => &mut self.atmosphere_multiple,
            DebugSwitch::AtmosphereGround => &mut self.atmosphere_ground,
            DebugSwitch::Oblateness => &mut self.oblateness,
            DebugSwitch::Ground => &mut self.ground,
            DebugSwitch::GroundLevels => &mut self.ground_levels,
        } = on;
    }
}

pub struct RenderDebugPlugin;

impl Plugin for RenderDebugPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<RenderDebug>()
            .add_systems(Update, apply_msaa)
            .add_plugins(bevy::render::diagnostic::RenderDiagnosticsPlugin);
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
