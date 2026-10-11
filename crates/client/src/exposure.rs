//! Manual camera pipeline: linear exposure in the scene, per-body balance
//! (`render::resolve`), then one display pass after bloom that decodes stored
//! headroom, clips at white and dithers. No metering or adaptation. Future
//! automatic exposure can supply the same stop value.
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
        extract_component::{ExtractComponent, ExtractComponentPlugin},
        render_graph::{
            NodeRunError, RenderGraphContext, RenderGraphExt, RenderLabel, ViewNode, ViewNodeRunner,
        },
        render_resource::{binding_types::texture_2d, *},
        renderer::RenderContext,
        view::ViewTarget,
    },
};

pub const STOPS: std::ops::RangeInclusive<f64> = -64.0..=16.0;
const SHADER: Handle<Shader> = uuid_handle!("37845990-7da2-4e71-85f7-61115fe77e33");
const HDR_SHADER: Handle<Shader> = uuid_handle!("b3f1c0a2-5d7e-4a96-8e21-4c6f9d0b7a35");
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
                "How far each body (star, corona, planet) is exposed for itself instead of at the shared Exposure. Its key is how strongly it is lit, not how it looks on screen: a planet's fully sunlit point, a star's surface. 0% is one physical exposure for everything. 50% keeps half of each body's stops above its correct exposure, so a planet 20 stops too bright shows 10 stops too bright. 100% shows every body correctly exposed, so they all look equally bright. Shading within a body (phase, terminator, limb darkening) is untouched, bodies are never brightened beyond physical, and the galaxy and galactic stars always follow Exposure alone. Set Exposure for the galaxy, then raise this to see the system.",
            ),
        ),
    ]
}

pub struct ExposurePlugin;

impl Plugin for ExposurePlugin {
    fn build(&self, app: &mut App) {
        load_internal_asset!(app, HDR_SHADER, "hdr.wgsl", Shader::from_wgsl);
        load_internal_asset!(app, SHADER, "exposure.wgsl", Shader::from_wgsl);
        app.add_plugins(ExtractComponentPlugin::<ManualCamera>::default())
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

/// Marks the realistic space camera, whose display pass this is.
#[derive(Component, Clone, Copy, ExtractComponent)]
struct ManualCamera;

fn sync(mut commands: Commands, nav: Res<Navigation>, cameras: Query<Entity, With<SpaceCamera>>) {
    for camera in &cameras {
        if nav.realistic {
            commands.entity(camera).insert(ManualCamera);
        } else {
            commands.entity(camera).remove::<ManualCamera>();
        }
    }
}

#[derive(Debug, Hash, PartialEq, Eq, Clone, RenderLabel)]
struct ManualDisplay;

#[derive(Default)]
struct CurveNode;

impl ViewNode for CurveNode {
    type ViewQuery = (&'static ViewTarget, &'static ManualCamera);

    fn run(
        &self,
        _graph: &mut RenderGraphContext,
        context: &mut RenderContext,
        (target, _): QueryItem<Self::ViewQuery>,
        world: &World,
    ) -> Result<(), NodeRunError> {
        let curve = world.resource::<CurvePipeline>();
        let cache = world.resource::<PipelineCache>();
        let Some(pipeline) = cache.get_render_pipeline(curve.pipeline) else {
            return Ok(());
        };
        if !target.is_hdr() {
            return Ok(());
        }
        // Flip only once the pipeline is ready.
        let write = target.post_process_write();
        let group = context.render_device().create_bind_group(
            "manual_display",
            &cache.get_bind_group_layout(&curve.layout),
            &BindGroupEntries::single(write.source),
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
        pass.set_bind_group(0, &group, &[]);
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
        &BindGroupLayoutEntries::single(
            ShaderStages::FRAGMENT,
            texture_2d(TextureSampleType::Float { filterable: false }),
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
