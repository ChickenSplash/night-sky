//! Manual camera pipeline: linear exposure in the scene, then one display curve
//! after bloom. No metering or adaptation. Future automatic exposure can supply
//! the same stop value without changing the rendering paths.
use crate::{Navigation, scene::SpaceCamera};
use bevy::{
    asset::{load_internal_asset, uuid_handle},
    core_pipeline::{
        FullscreenShader,
        core_3d::graph::{Core3d, Node3d},
    },
    ecs::query::QueryItem,
    prelude::*,
    render::{
        RenderApp, RenderStartup,
        diagnostic::RecordDiagnostics,
        extract_component::{
            ComponentUniforms, DynamicUniformIndex, ExtractComponent, ExtractComponentPlugin,
            UniformComponentPlugin,
        },
        render_graph::{
            NodeRunError, RenderGraphContext, RenderGraphExt, RenderLabel, ViewNode, ViewNodeRunner,
        },
        render_resource::{
            binding_types::{texture_2d, uniform_buffer},
            *,
        },
        renderer::RenderContext,
        view::ViewTarget,
    },
};

pub const STOPS: std::ops::RangeInclusive<f64> = -64.0..=16.0;
const SHADER: Handle<Shader> = uuid_handle!("37845990-7da2-4e71-85f7-61115fe77e33");
const HDR_SHADER: Handle<Shader> = uuid_handle!("b3f1c0a2-5d7e-4a96-8e21-4c6f9d0b7a35");
/// Stops above white that full compression fits below white. A sunlit planet
/// at a galaxy-friendly exposure is about 24.
const FULL_STOPS: f64 = 30.0;
/// The knee falls from white to this as compression goes to 100%. Light below
/// it, such as the galaxy, is never changed.
const LOWEST_KNEE: f64 = 0.4;

/// The display curve for a compression amount: linear below `knee`, then a
/// logarithmic shoulder with slope one at the knee that reaches white at
/// `white`. Zero `strength` is linear clipping, which small amounts approach.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shoulder {
    pub knee: f64,
    pub white: f64,
    pub strength: f64,
}

impl Shoulder {
    pub fn new(amount: f64) -> Self {
        let amount = amount.clamp(0.0, 1.0);
        let knee = 1.0 - (1.0 - LOWEST_KNEE) * amount;
        let white = (FULL_STOPS * amount).exp2();
        // ln(1 + b·span) = b: the shoulder reaches white exactly at `white`.
        let span = (white - knee) / (1.0 - knee).max(f64::MIN_POSITIVE);
        let strength = if amount <= 0.0 || span <= 1.0 {
            0.0
        } else {
            let (mut low, mut high) = (1e-9, 200.0);
            for _ in 0..100 {
                let mid = 0.5 * (low + high);
                if (mid * span).ln_1p() > mid {
                    low = mid;
                } else {
                    high = mid;
                }
            }
            low
        };
        Self {
            knee,
            white,
            strength,
        }
    }

    /// CPU mirror of `exposure.wgsl`, per channel.
    #[cfg(test)]
    pub fn apply(&self, light: f64) -> f64 {
        if self.strength <= 0.0 {
            return light.clamp(0.0, 1.0);
        }
        if light <= self.knee {
            return light.max(0.0);
        }
        let y = (light - self.knee) / (1.0 - self.knee);
        self.knee + (1.0 - self.knee) * ((self.strength * y).ln_1p() / self.strength).min(1.0)
    }
}

/// Fixed reference sensitivity, independent of location, selection and time.
/// Zero stops keeps the former dark-space calibration for magnitude-0 points.
pub fn gain(stops: f64) -> f64 {
    stops.exp2() / crate::sky::reference_radiance()
}

pub fn facts(nav: &Navigation) -> Vec<(&'static str, String, Option<&'static str>)> {
    vec![
        (
            "exposure",
            format!("{:+.2} stops", nav.exposure),
            Some(
                "Manual sensitivity relative to a fixed dark-space reference. +1 stop doubles light everywhere; -1 halves it. No automatic adjustment on moving, selecting, following or entering a system. The bottom-right slider applies immediately.",
            ),
        ),
        (
            "compression",
            format!("{:.0}%", nav.compression * 100.0),
            Some(
                "One scene-wide highlight curve, per pixel after exposure and bloom. Zero clips at white as before, and small amounts look almost the same. Increasing it lowers the knee (from white to 40% of white) below which light is never changed, so the galaxy and faint stars keep their look, and fits more light above white into the display: up to 30 stops at 100%. Set Exposure for the galaxy first, then raise this until bright planets show their shading. Very large brightness gaps still lose contrast: a day/night line squeezed from 24 stops into the screen's 8 looks hard.",
            ),
        ),
    ]
}

pub struct ExposurePlugin;

impl Plugin for ExposurePlugin {
    fn build(&self, app: &mut App) {
        load_internal_asset!(app, HDR_SHADER, "hdr.wgsl", Shader::from_wgsl);
        load_internal_asset!(app, SHADER, "exposure.wgsl", Shader::from_wgsl);
        app.add_plugins((
            ExtractComponentPlugin::<DisplayCurve>::default(),
            UniformComponentPlugin::<DisplayCurve>::default(),
        ))
        .add_systems(Update, sync);
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        render_app
            .add_systems(RenderStartup, init_pipeline)
            .add_render_graph_node::<ViewNodeRunner<CurveNode>>(Core3d, ManualDisplay)
            .add_render_graph_edges(
                Core3d,
                (
                    Node3d::Tonemapping,
                    ManualDisplay,
                    Node3d::EndMainPassPostProcessing,
                ),
            );
    }
}

#[derive(Component, Clone, Copy, ExtractComponent, ShaderType)]
struct DisplayCurve {
    /// Knee, white point, shoulder strength (`Shoulder`), unused; a vec4 keeps
    /// WebGL uniform alignment at 16 bytes.
    settings: Vec4,
}

fn sync(mut commands: Commands, nav: Res<Navigation>, cameras: Query<Entity, With<SpaceCamera>>) {
    for camera in &cameras {
        if nav.realistic {
            let curve = Shoulder::new(nav.compression);
            commands.entity(camera).insert(DisplayCurve {
                settings: Vec4::new(
                    curve.knee as f32,
                    curve.white as f32,
                    curve.strength as f32,
                    0.0,
                ),
            });
        } else {
            commands.entity(camera).remove::<DisplayCurve>();
        }
    }
}

#[derive(Debug, Hash, PartialEq, Eq, Clone, RenderLabel)]
struct ManualDisplay;

#[derive(Default)]
struct CurveNode;

impl ViewNode for CurveNode {
    type ViewQuery = (
        &'static ViewTarget,
        &'static DynamicUniformIndex<DisplayCurve>,
    );

    fn run(
        &self,
        _graph: &mut RenderGraphContext,
        context: &mut RenderContext,
        (target, index): QueryItem<Self::ViewQuery>,
        world: &World,
    ) -> Result<(), NodeRunError> {
        let curve = world.resource::<CurvePipeline>();
        let cache = world.resource::<PipelineCache>();
        let Some(pipeline) = cache.get_render_pipeline(curve.pipeline) else {
            return Ok(());
        };
        let uniforms = world.resource::<ComponentUniforms<DisplayCurve>>();
        let Some(binding) = uniforms.uniforms().binding() else {
            return Ok(());
        };
        if !target.is_hdr() {
            return Ok(());
        }
        // Flip only once the pipeline and uniforms are ready.
        let write = target.post_process_write();
        let group = context.render_device().create_bind_group(
            "manual_display",
            &cache.get_bind_group_layout(&curve.layout),
            &BindGroupEntries::sequential((write.source, binding)),
        );
        let diagnostics = context.diagnostic_recorder();
        let mut pass = context.begin_tracked_render_pass(RenderPassDescriptor {
            label: Some("manual_display"),
            color_attachments: &[Some(RenderPassColorAttachment {
                view: write.destination,
                depth_slice: None,
                resolve_target: None,
                ops: Operations::default(),
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        let span = diagnostics.pass_span(&mut pass, "manual_display");
        pass.set_render_pipeline(pipeline);
        pass.set_bind_group(0, &group, &[index.index()]);
        pass.draw(0..3, 0..1);
        span.end(&mut pass);
        Ok(())
    }
}

#[derive(Resource)]
struct CurvePipeline {
    layout: BindGroupLayoutDescriptor,
    pipeline: CachedRenderPipelineId,
}

fn init_pipeline(
    mut commands: Commands,
    fullscreen: Res<FullscreenShader>,
    cache: Res<PipelineCache>,
) {
    let layout = BindGroupLayoutDescriptor::new(
        "manual_display",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::FRAGMENT,
            (
                texture_2d(TextureSampleType::Float { filterable: false }),
                uniform_buffer::<DisplayCurve>(true),
            ),
        ),
    );
    let pipeline = cache.queue_render_pipeline(RenderPipelineDescriptor {
        label: Some("manual_display".into()),
        layout: vec![layout.clone()],
        vertex: fullscreen.to_vertex_state(),
        fragment: Some(FragmentState {
            shader: SHADER,
            targets: vec![Some(ColorTargetState {
                format: ViewTarget::TEXTURE_FORMAT_HDR,
                blend: None,
                write_mask: ColorWrites::ALL,
            })],
            ..default()
        }),
        ..default()
    });
    commands.insert_resource(CurvePipeline { layout, pipeline });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn curve(light: f64, amount: f64) -> f64 {
        Shoulder::new(amount).apply(light)
    }

    /// CPU mirror of hdr.wgsl.
    const KNEE: f64 = 8192.0;
    const SLOPE: f64 = 4.0;
    fn encode(x: f64) -> f64 {
        if x <= KNEE {
            x.max(0.0)
        } else {
            KNEE * (1.0 + (x / KNEE).ln() / SLOPE)
        }
    }
    fn decode(s: f64) -> f64 {
        if s <= KNEE {
            s.max(0.0)
        } else {
            KNEE * ((s / KNEE - 1.0) * SLOPE).min(70.0).exp()
        }
    }

    #[test]
    fn hdr_encoding_is_linear_below_its_knee_and_fits_half_floats() {
        for x in [0.0, 1e-4, 0.5, 1.0, 20_000.0f64.min(KNEE)] {
            assert_eq!(encode(x), x);
        }
        for stops in [14, 20, 24, 30, 40] {
            let x = f64::from(stops).exp2();
            assert!(encode(x) < 60_000.0, "2^{stops} overflows half floats");
            assert!((decode(encode(x)) / x - 1.0).abs() < 1e-9);
        }
        // Half-float storage keeps 11 significant bits: under 1% error decoded.
        let stored = encode(2f64.powi(24));
        let step = 2f64.powi(stored.log2().floor() as i32 - 10);
        assert!(decode(stored + step) / decode(stored) < 1.01);
    }

    #[test]
    fn compression_changes_only_highlights_and_starts_from_clipping() {
        for amount in [0.0, 0.01, 0.25, 0.5, 1.0] {
            for light in [0.0, 1e-4, 0.01, 0.1, 0.3, LOWEST_KNEE] {
                assert!((curve(light, amount) - light).abs() < 1e-12);
            }
            let mut previous = -1.0;
            for i in -80..=80 {
                let value = curve((f64::from(i) / 2.0).exp2(), amount);
                assert!((0.0..=1.0).contains(&value));
                assert!(value >= previous);
                previous = value;
            }
        }
        // Zero is clipping; 1% stays within 1% of it everywhere.
        assert_eq!(curve(2.0, 0.0), 1.0);
        for i in -20..=60 {
            let light = (f64::from(i) / 2.0).exp2();
            assert!((curve(light, 0.01) - curve(light, 0.0)).abs() < 0.01);
        }
        // Full compression fits a sunlit planet at galaxy exposure with a
        // visible step per stop, and reaches white at its white point.
        let a = curve(2f64.powi(20), 1.0);
        let b = curve(2f64.powi(24), 1.0);
        assert!(b - a > 0.05 && b < 1.0);
        assert!((curve(FULL_STOPS.exp2(), 1.0) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn shoulder_joins_linear_light_smoothly() {
        for amount in [0.01, 0.25, 1.0] {
            let s = Shoulder::new(amount);
            let d = 1e-7;
            assert!(((s.apply(s.knee + d) - s.knee) / d - 1.0).abs() < 1e-3);
        }
    }

    #[test]
    fn stops_double_or_halve_all_light() {
        assert_eq!(gain(1.0) / gain(0.0), 2.0);
        assert_eq!(gain(-1.0) / gain(0.0), 0.5);
        for stops in [*STOPS.start(), 0.0, *STOPS.end()] {
            assert!(gain(stops).is_finite());
        }
    }

    #[test]
    fn manual_controls_reject_non_finite_values_and_clamp() {
        let mut nav = Navigation::default();
        nav.set_exposure(-100.0);
        assert_eq!(nav.exposure, -64.0);
        nav.set_exposure(100.0);
        assert_eq!(nav.exposure, 16.0);
        nav.set_exposure(f64::NAN);
        assert_eq!(nav.exposure, 16.0);
        nav.set_compression(-1.0);
        assert_eq!(nav.compression, 0.0);
        nav.set_compression(2.0);
        assert_eq!(nav.compression, 1.0);
        nav.set_compression(f64::INFINITY);
        assert_eq!(nav.compression, 1.0);
        let rows = facts(&nav);
        assert_eq!(rows.len(), 2);
        assert!(rows.iter().all(|row| row.2.is_some()));
    }
}
