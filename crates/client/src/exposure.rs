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
                "One scene-wide display curve, after exposure and bloom. Zero clips linearly at the display's limits. Increasing it lifts shadows and rolls off highlights, compressing brightness differences while preserving their order. Black stays black and middle grey stays fixed. It cannot recover light already lost to the HDR buffer's finite precision or safety cap.",
            ),
        ),
    ]
}

pub struct ExposurePlugin;

impl Plugin for ExposurePlugin {
    fn build(&self, app: &mut App) {
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
    /// x: compression; a vec4 keeps WebGL uniform alignment at 16 bytes.
    settings: Vec4,
}

fn sync(mut commands: Commands, nav: Res<Navigation>, cameras: Query<Entity, With<SpaceCamera>>) {
    for camera in &cameras {
        if nav.realistic {
            commands.entity(camera).insert(DisplayCurve {
                settings: Vec4::new(nav.compression as f32, 0.0, 0.0, 0.0),
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

    /// CPU mirror of exposure.wgsl, for the display curve's invariants.
    fn curve(light: f64, amount: f64) -> f64 {
        if light <= 0.0 {
            return 0.0;
        }
        let power = (-3.0 * amount).exp2();
        let t = (light / 0.18).powf(power) * (0.18 / 0.82);
        light.clamp(0.0, 1.0) * (1.0 - amount) + t / (1.0 + t) * amount
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
    fn compression_preserves_black_middle_grey_and_brightness_order() {
        for amount in [0.0, 0.25, 0.5, 1.0] {
            assert_eq!(curve(0.0, amount), 0.0);
            assert!((curve(0.18, amount) - 0.18).abs() < 1e-12);
            let mut previous = -1.0;
            for i in -80..=60 {
                let value = curve((f64::from(i) / 2.0).exp2(), amount);
                assert!((0.0..=1.0).contains(&value));
                assert!(value >= previous);
                previous = value;
            }
        }
        assert_eq!(curve(2.0, 0.0), 1.0);
        assert!(curve(1e-5, 1.0) > curve(1e-5, 0.0));
        assert!(curve(10.0, 1.0) < curve(100.0, 1.0));
        assert!(curve(100.0, 1.0) < 1.0);
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
