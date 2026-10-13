//! Slight scene-only blur beneath translucent UI windows, before egui draws
//! sharp text and controls. No extra camera or multisampled depth sampling.
use crate::scene::SpaceCamera;
use bevy::{
    asset::{load_internal_asset, uuid_handle},
    core_pipeline::{
        FullscreenShader,
        core_3d::graph::{Core3d, Node3d},
    },
    ecs::query::QueryItem,
    image::BevyDefault,
    prelude::*,
    render::{
        Render, RenderApp, RenderStartup, RenderSystems,
        diagnostic::RecordDiagnostics,
        extract_component::{ExtractComponent, ExtractComponentPlugin},
        extract_resource::{ExtractResource, ExtractResourcePlugin},
        render_graph::{
            NodeRunError, RenderGraphContext, RenderGraphExt, RenderLabel, ViewNode, ViewNodeRunner,
        },
        render_resource::{
            binding_types::{texture_2d, uniform_buffer},
            *,
        },
        renderer::{RenderContext, RenderDevice, RenderQueue},
        view::ViewTarget,
    },
};
use bevy_egui::{EguiContexts, EguiPostUpdateSet, egui};

const SHADER: Handle<Shader> = uuid_handle!("82fd48c4-a478-4d32-a96a-80f8fc93b6ed");
const MAX_WINDOWS: usize = 8;
const REGIONS_ID: &str = "window-blur-regions";

/// Called by shared frames and the filter modal. Collect drains these each
/// frame, so closed windows never leave blur rectangles behind.
pub fn register(ctx: &egui::Context, id: egui::Id, rect: egui::Rect) {
    ctx.data_mut(|data| {
        let regions =
            data.get_temp_mut_or_default::<Vec<(egui::Id, egui::Rect)>>(egui::Id::new(REGIONS_ID));
        if let Some((_, previous)) = regions.iter_mut().find(|(window, _)| *window == id) {
            *previous = rect;
        } else if regions.len() < MAX_WINDOWS && rect.is_positive() {
            regions.push((id, rect));
        }
    });
}

#[derive(Resource, Clone, Default, ExtractResource, ShaderType)]
struct Settings {
    rects: [Vec4; MAX_WINDOWS],
    /// Number of rectangles, tap spacing in physical pixels, unused, unused.
    info: Vec4,
}

pub struct WindowBlurPlugin;

impl Plugin for WindowBlurPlugin {
    fn build(&self, app: &mut App) {
        load_internal_asset!(app, SHADER, "window_blur.wgsl", Shader::from_wgsl);
        app.init_resource::<Settings>()
            .add_plugins((
                ExtractResourcePlugin::<Settings>::default(),
                ExtractComponentPlugin::<BlurCamera>::default(),
            ))
            .add_systems(Update, mark_cameras)
            .add_systems(PostUpdate, collect.after(EguiPostUpdateSet::EndPass));
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        render_app
            .add_systems(RenderStartup, init_pipeline)
            .init_resource::<BlurUniform>()
            .add_systems(
                Render,
                prepare_uniform.in_set(RenderSystems::PrepareResources),
            )
            .add_render_graph_node::<ViewNodeRunner<BlurNode>>(Core3d, WindowBlur)
            .add_render_graph_edges(
                Core3d,
                (
                    Node3d::EndMainPassPostProcessing,
                    WindowBlur,
                    bevy_egui::render::graph::NodeEgui::EguiPass,
                ),
            );
    }
}

#[derive(Component, Clone, Copy, ExtractComponent)]
struct BlurCamera;

fn mark_cameras(
    mut commands: Commands,
    cameras: Query<Entity, (With<SpaceCamera>, Without<BlurCamera>)>,
) {
    for camera in &cameras {
        commands.entity(camera).insert(BlurCamera);
    }
}

fn collect(mut contexts: EguiContexts, mut settings: ResMut<Settings>) {
    *settings = Settings::default();
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };
    let rects = ctx
        .data_mut(|data| data.remove_temp::<Vec<(egui::Id, egui::Rect)>>(egui::Id::new(REGIONS_ID)))
        .unwrap_or_default();
    let scale = ctx.pixels_per_point();
    let rects: Vec<_> = rects.into_iter().map(|(_, rect)| rect).collect();
    *settings = region_settings(&rects, scale);
}

fn region_settings(rects: &[egui::Rect], scale: f32) -> Settings {
    let mut settings = Settings::default();
    for (index, rect) in rects.iter().take(MAX_WINDOWS).enumerate() {
        settings.rects[index] =
            Vec4::new(rect.left(), rect.top(), rect.right(), rect.bottom()) * scale;
    }
    settings.info = Vec4::new(rects.len().min(MAX_WINDOWS) as f32, 2.0 * scale, 0.0, 0.0);
    settings
}

#[derive(Debug, Hash, PartialEq, Eq, Clone, RenderLabel)]
struct WindowBlur;

#[derive(Default)]
struct BlurNode;

impl ViewNode for BlurNode {
    type ViewQuery = (&'static ViewTarget, &'static BlurCamera);

    fn run(
        &self,
        _: &mut RenderGraphContext,
        context: &mut RenderContext,
        (target, _): QueryItem<Self::ViewQuery>,
        world: &World,
    ) -> Result<(), NodeRunError> {
        let settings = world.resource::<Settings>();
        if settings.info.x == 0.0 {
            return Ok(());
        }
        let blur = world.resource::<BlurPipeline>();
        let cache = world.resource::<PipelineCache>();
        let pipeline_id = if target.is_hdr() { blur.hdr } else { blur.sdr };
        let Some(pipeline) = cache.get_render_pipeline(pipeline_id) else {
            return Ok(());
        };
        let uniform = &world.resource::<BlurUniform>().0;
        let write = target.post_process_write();
        let group = context.render_device().create_bind_group(
            "window_blur",
            &cache.get_bind_group_layout(&blur.layout),
            &BindGroupEntries::sequential((write.source, uniform.binding().unwrap())),
        );
        let diagnostics = context.diagnostic_recorder();
        let mut pass = context.begin_tracked_render_pass(RenderPassDescriptor {
            label: Some("window_blur"),
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
        let span = diagnostics.pass_span(&mut pass, "window_blur");
        pass.set_render_pipeline(pipeline);
        pass.set_bind_group(0, &group, &[]);
        pass.draw(0..3, 0..1);
        span.end(&mut pass);
        Ok(())
    }
}

#[derive(Resource)]
struct BlurPipeline {
    layout: BindGroupLayoutDescriptor,
    hdr: CachedRenderPipelineId,
    sdr: CachedRenderPipelineId,
}

#[derive(Resource, Default)]
struct BlurUniform(UniformBuffer<Settings>);

fn prepare_uniform(
    settings: Res<Settings>,
    mut uniform: ResMut<BlurUniform>,
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
) {
    if settings.info.x > 0.0 {
        uniform.0.set(settings.clone());
        uniform.0.write_buffer(&device, &queue);
    }
}

fn init_pipeline(
    mut commands: Commands,
    fullscreen: Res<FullscreenShader>,
    cache: Res<PipelineCache>,
) {
    let layout = BindGroupLayoutDescriptor::new(
        "window_blur",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::FRAGMENT,
            (
                texture_2d(TextureSampleType::Float { filterable: false }),
                uniform_buffer::<Settings>(false),
            ),
        ),
    );
    let pipeline = |format| {
        cache.queue_render_pipeline(RenderPipelineDescriptor {
            label: Some("window_blur".into()),
            layout: vec![layout.clone()],
            vertex: fullscreen.to_vertex_state(),
            fragment: Some(FragmentState {
                shader: SHADER,
                targets: vec![Some(ColorTargetState {
                    format,
                    blend: None,
                    write_mask: ColorWrites::ALL,
                })],
                ..default()
            }),
            ..default()
        })
    };
    commands.insert_resource(BlurPipeline {
        hdr: pipeline(ViewTarget::TEXTURE_FORMAT_HDR),
        sdr: pipeline(TextureFormat::bevy_default()),
        layout,
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_regions_follow_display_scale_and_reset_on_close() {
        let rect = egui::Rect::from_min_max(egui::pos2(20.0, 30.0), egui::pos2(200.0, 300.0));
        let settings = region_settings(&[rect], 2.0);
        assert_eq!(settings.rects[0], Vec4::new(40.0, 60.0, 400.0, 600.0));
        assert_eq!(settings.info, Vec4::new(1.0, 4.0, 0.0, 0.0));
        assert_eq!(region_settings(&[], 1.0).info.x, 0.0);
        let ctx = egui::Context::default();
        register(&ctx, egui::Id::new(0), rect);
        let moved = rect.translate(egui::vec2(10.0, 0.0));
        register(&ctx, egui::Id::new(0), moved);
        for index in 1..20 {
            register(&ctx, egui::Id::new(index), rect);
        }
        let take = || {
            ctx.data_mut(|data| {
                data.remove_temp::<Vec<(egui::Id, egui::Rect)>>(egui::Id::new(REGIONS_ID))
            })
        };
        let regions = take().unwrap();
        assert_eq!(regions.len(), MAX_WINDOWS);
        assert_eq!(
            regions[0].1, moved,
            "multipass layout replaces the earlier bounds"
        );
        assert!(
            take().is_none(),
            "closed windows must not retain their blur region"
        );
    }
}
