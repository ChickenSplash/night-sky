//! Natively, galactic stars are drawn by compute splatting while
//! `RenderDebug::splat_stars` is on, instead of as `starfield_render`'s
//! instanced triangles: at the galactic centre most stars are far below a
//! pixel apart, and setting up a triangle each cost more than shading them.
//!
//! The `StarSplat` node, between the main opaque and transmissive passes,
//! clears a buffer of fixed-point light per pixel and runs
//! `starfield_splat.wgsl` once per draw of `StarFrame`, one thread per star,
//! depth-tested against the opaque bodies: each adds the raster path's
//! Gaussian, sampled at the 3x3 pixels around it. One full-screen draw in the
//! transparent pass (`starfield_composite.wgsl`) then adds that light to the
//! frame. It reads the same batch buffers, globals and draw uniforms as
//! `starfield_render`.
use crate::{
    scene::SpaceCamera,
    starfield_render::{DrawUniform, Globals, StarBuffers},
};
use bevy::{
    asset::{load_internal_asset, uuid_handle},
    core_pipeline::core_3d::{
        CORE_3D_DEPTH_FORMAT, Transparent3d,
        graph::{Core3d, Node3d},
    },
    ecs::{
        query::QueryItem,
        system::{SystemParamItem, lifetimeless::SRes},
    },
    image::BevyDefault,
    prelude::*,
    render::{
        Render, RenderApp, RenderStartup, RenderSystems,
        diagnostic::RecordDiagnostics,
        render_graph::{
            NodeRunError, RenderGraphContext, RenderGraphExt, RenderLabel, ViewNode, ViewNodeRunner,
        },
        render_phase::{
            AddRenderCommand, DrawFunctions, PhaseItem, PhaseItemExtraIndex, RenderCommand,
            RenderCommandResult, SetItemPipeline, TrackedRenderPass, ViewSortedRenderPhases,
        },
        render_resource::{
            binding_types::{
                storage_buffer_read_only_sized, storage_buffer_sized, texture_depth_2d,
                texture_depth_2d_multisampled, uniform_buffer,
            },
            *,
        },
        renderer::{RenderContext, RenderDevice, RenderQueue},
        sync_world::MainEntity,
        view::{
            ExtractedView, ViewDepthTexture, ViewTarget, ViewUniform, ViewUniformOffset,
            ViewUniforms,
        },
    },
};
use std::collections::HashMap;

const SPLAT_SHADER: Handle<Shader> = uuid_handle!("6f0c4a8e-2b3d-4e51-9a7c-1d2e3f405162");
const COMPOSITE_SHADER: Handle<Shader> = uuid_handle!("8a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d");
/// Threads per workgroup in `starfield_splat.wgsl`.
const WORKGROUP: u32 = 256;

pub struct StarSplatPlugin;

impl Plugin for StarSplatPlugin {
    fn build(&self, app: &mut App) {
        load_internal_asset!(app, SPLAT_SHADER, "starfield_splat.wgsl", Shader::from_wgsl);
        load_internal_asset!(
            app,
            COMPOSITE_SHADER,
            "starfield_composite.wgsl",
            Shader::from_wgsl
        );
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        render_app
            .init_resource::<Splat>()
            .init_resource::<SpecializedComputePipelines<SplatPipeline>>()
            .init_resource::<SpecializedRenderPipelines<SplatPipeline>>()
            .add_render_command::<Transparent3d, DrawComposite>()
            .add_systems(RenderStartup, init_pipeline)
            .add_systems(
                Render,
                (
                    queue.in_set(RenderSystems::QueueMeshes),
                    prepare.in_set(RenderSystems::PrepareResources),
                    bind.in_set(RenderSystems::PrepareBindGroups),
                ),
            )
            .add_render_graph_node::<ViewNodeRunner<SplatNode>>(Core3d, StarSplat)
            .add_render_graph_edges(
                Core3d,
                (
                    Node3d::MainOpaquePass,
                    StarSplat,
                    Node3d::MainTransmissivePass,
                ),
            );
    }
}

#[derive(Debug, Hash, PartialEq, Eq, Clone, RenderLabel)]
struct StarSplat;

#[derive(Resource)]
struct SplatPipeline {
    /// The splat's view, light and depth bindings, with a single-sampled and
    /// a multisampled depth texture.
    splat: [BindGroupLayoutDescriptor; 2],
    /// One draw's stars.
    stars: BindGroupLayoutDescriptor,
    composite: BindGroupLayoutDescriptor,
}

fn init_pipeline(mut commands: Commands) {
    let splat = |multisampled: bool| {
        let depth = if multisampled {
            texture_depth_2d_multisampled()
        } else {
            texture_depth_2d()
        };
        BindGroupLayoutDescriptor::new(
            "star_splat_layout",
            &BindGroupLayoutEntries::sequential(
                ShaderStages::COMPUTE,
                (
                    uniform_buffer::<ViewUniform>(true),
                    uniform_buffer::<Globals>(false),
                    uniform_buffer::<DrawUniform>(true),
                    storage_buffer_sized(false, None),
                    depth,
                ),
            ),
        )
    };
    commands.insert_resource(SplatPipeline {
        splat: [splat(false), splat(true)],
        stars: BindGroupLayoutDescriptor::new(
            "star_splat_stars_layout",
            &BindGroupLayoutEntries::single(
                ShaderStages::COMPUTE,
                storage_buffer_read_only_sized(false, None),
            ),
        ),
        composite: BindGroupLayoutDescriptor::new(
            "star_composite_layout",
            &BindGroupLayoutEntries::sequential(
                ShaderStages::FRAGMENT,
                (
                    storage_buffer_read_only_sized(false, None),
                    uniform_buffer::<UVec4>(false),
                ),
            ),
        ),
    });
}

/// Keyed by whether the depth texture is multisampled.
impl SpecializedComputePipeline for SplatPipeline {
    type Key = bool;

    fn specialize(&self, multisampled: bool) -> ComputePipelineDescriptor {
        ComputePipelineDescriptor {
            label: Some("star_splat".into()),
            layout: vec![
                self.splat[usize::from(multisampled)].clone(),
                self.stars.clone(),
            ],
            shader: SPLAT_SHADER,
            shader_defs: if multisampled {
                vec!["MULTISAMPLED".into()]
            } else {
                Vec::new()
            },
            entry_point: Some("splat".into()),
            ..default()
        }
    }
}

/// Keyed by MSAA samples and whether the target is HDR.
impl SpecializedRenderPipeline for SplatPipeline {
    type Key = (u32, bool);

    fn specialize(&self, (samples, hdr): (u32, bool)) -> RenderPipelineDescriptor {
        RenderPipelineDescriptor {
            label: Some("star_composite".into()),
            layout: vec![self.composite.clone()],
            vertex: VertexState {
                shader: COMPOSITE_SHADER,
                entry_point: Some("vertex".into()),
                ..default()
            },
            fragment: Some(FragmentState {
                shader: COMPOSITE_SHADER,
                entry_point: Some("fragment".into()),
                // Additive: the shader writes zero alpha.
                targets: vec![Some(ColorTargetState {
                    format: if hdr {
                        ViewTarget::TEXTURE_FORMAT_HDR
                    } else {
                        TextureFormat::bevy_default()
                    },
                    blend: Some(BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    write_mask: ColorWrites::ALL,
                })],
                ..default()
            }),
            // The splat already depth-tested each star.
            depth_stencil: Some(DepthStencilState {
                format: CORE_3D_DEPTH_FORMAT,
                depth_write_enabled: false,
                depth_compare: CompareFunction::Always,
                stencil: default(),
                bias: default(),
            }),
            multisample: MultisampleState {
                count: samples,
                ..default()
            },
            ..default()
        }
    }
}

/// The space camera's splat resources. There is one space camera.
#[derive(Resource, Default)]
struct Splat {
    /// Red, green and blue fixed-point light per pixel.
    light: Option<Buffer>,
    /// The viewport `light` covers: origin, then size, in physical pixels.
    viewport: UniformBuffer<UVec4>,
    pipeline: Option<CachedComputePipelineId>,
    /// The view, globals, draws, light and depth.
    group: Option<BindGroup>,
    composite: Option<BindGroup>,
    /// Each batch's stars, kept while the batch is.
    batches: HashMap<u64, BindGroup>,
    /// This frame's precise sprites.
    precise: Option<BindGroup>,
}

#[allow(clippy::too_many_arguments)]
fn queue(
    draw_functions: Res<DrawFunctions<Transparent3d>>,
    pipeline: Res<SplatPipeline>,
    mut computes: ResMut<SpecializedComputePipelines<SplatPipeline>>,
    mut renders: ResMut<SpecializedRenderPipelines<SplatPipeline>>,
    cache: Res<PipelineCache>,
    buffers: Res<StarBuffers>,
    mut splat: ResMut<Splat>,
    mut phases: ResMut<ViewSortedRenderPhases<Transparent3d>>,
    views: Query<(&ExtractedView, &Msaa), With<SpaceCamera>>,
) {
    splat.pipeline = None;
    if !buffers.frame.splat {
        return;
    }
    let draw_function = draw_functions.read().id::<DrawComposite>();
    for (view, msaa) in &views {
        let Some(phase) = phases.get_mut(&view.retained_view_entity) else {
            continue;
        };
        splat.pipeline = Some(computes.specialize(&cache, &pipeline, msaa.samples() > 1));
        phase.add(Transparent3d {
            distance: 0.0,
            pipeline: renders.specialize(&cache, &pipeline, (msaa.samples(), view.hdr)),
            entity: (Entity::PLACEHOLDER, MainEntity::from(Entity::PLACEHOLDER)),
            draw_function,
            batch_range: 0..1,
            extra_index: PhaseItemExtraIndex::None,
            indexed: false,
        });
    }
}

fn prepare(
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
    buffers: Res<StarBuffers>,
    mut splat: ResMut<Splat>,
    views: Query<&ExtractedView, With<SpaceCamera>>,
) {
    let (Some(view), true) = (views.iter().next(), buffers.frame.splat) else {
        // Free the light buffer while splatting is off.
        splat.light = None;
        return;
    };
    let viewport = view.viewport;
    let size = u64::from(viewport.z) * u64::from(viewport.w) * 3 * 4;
    if splat.light.is_none() || *splat.viewport.get() != viewport {
        splat.light = Some(device.create_buffer(&BufferDescriptor {
            label: Some("star_splat_light"),
            size: size.max(16),
            usage: BufferUsages::STORAGE | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        }));
        splat.viewport.set(viewport);
        splat.viewport.write_buffer(&device, &queue);
    }
}

#[allow(clippy::too_many_arguments)]
fn bind(
    device: Res<RenderDevice>,
    pipeline: Res<SplatPipeline>,
    cache: Res<PipelineCache>,
    buffers: Res<StarBuffers>,
    view_uniforms: Res<ViewUniforms>,
    mut splat: ResMut<Splat>,
    views: Query<(&ViewDepthTexture, &Msaa), With<SpaceCamera>>,
) {
    let splat = &mut *splat;
    splat.group = None;
    splat.composite = None;
    splat.precise = None;
    let stars_layout = cache.get_bind_group_layout(&pipeline.stars);
    splat
        .batches
        .retain(|id, _| buffers.batches.contains_key(id));
    let (Some(light), Some((depth, msaa))) = (&splat.light, views.iter().next()) else {
        splat.batches.clear();
        return;
    };
    let (Some(view), Some(globals), Some(draws), Some(viewport)) = (
        view_uniforms.uniforms.binding(),
        buffers.globals.binding(),
        buffers.draws.binding(),
        splat.viewport.binding(),
    ) else {
        return;
    };
    splat.group = Some(device.create_bind_group(
        "star_splat_bind_group",
        &cache.get_bind_group_layout(&pipeline.splat[usize::from(msaa.samples() > 1)]),
        &BindGroupEntries::sequential((
            view,
            globals,
            draws,
            light.as_entire_binding(),
            depth.view(),
        )),
    ));
    splat.composite = Some(device.create_bind_group(
        "star_composite_bind_group",
        &cache.get_bind_group_layout(&pipeline.composite),
        &BindGroupEntries::sequential((light.as_entire_binding(), viewport)),
    ));
    for (id, (buffer, _)) in &buffers.batches {
        splat.batches.entry(*id).or_insert_with(|| {
            device.create_bind_group(
                "star_splat_batch",
                &stars_layout,
                &BindGroupEntries::single(buffer.as_entire_binding()),
            )
        });
    }
    splat.precise = buffers.precise.buffer().map(|buffer| {
        device.create_bind_group(
            "star_splat_precise",
            &stars_layout,
            &BindGroupEntries::single(buffer.as_entire_binding()),
        )
    });
}

#[derive(Default)]
struct SplatNode;

impl ViewNode for SplatNode {
    type ViewQuery = (&'static ViewUniformOffset, &'static SpaceCamera);

    fn run<'w>(
        &self,
        _graph: &mut RenderGraphContext,
        render_context: &mut RenderContext<'w>,
        (view_offset, _): QueryItem<'w, '_, Self::ViewQuery>,
        world: &'w World,
    ) -> Result<(), NodeRunError> {
        let splat = world.resource::<Splat>();
        let buffers = world.resource::<StarBuffers>();
        let (Some(light), Some(group), Some(id)) = (&splat.light, &splat.group, splat.pipeline)
        else {
            return Ok(());
        };
        let Some(compute) = world.resource::<PipelineCache>().get_compute_pipeline(id) else {
            return Ok(());
        };
        let diagnostics = render_context.diagnostic_recorder();
        let encoder = render_context.command_encoder();
        encoder.clear_buffer(light, 0, None);
        let mut pass = encoder.begin_compute_pass(&ComputePassDescriptor {
            label: Some("star_splat"),
            timestamp_writes: None,
        });
        let span = diagnostics.pass_span(&mut pass, "star_splat");
        pass.set_pipeline(compute);
        for (draw, &offset) in buffers.frame.draws.iter().zip(&buffers.offsets) {
            let stars = match draw.batch {
                Some(id) => splat.batches.get(&id),
                None => splat.precise.as_ref(),
            };
            let count = draw.near.z as u32;
            let Some(stars) = stars.filter(|_| count > 0) else {
                continue;
            };
            pass.set_bind_group(0, group, &[view_offset.offset, offset]);
            pass.set_bind_group(1, stars, &[]);
            pass.dispatch_workgroups(count.div_ceil(WORKGROUP), 1, 1);
        }
        span.end(&mut pass);
        Ok(())
    }
}

type DrawComposite = (SetItemPipeline, DrawSplatLight);

struct DrawSplatLight;

impl<P: PhaseItem> RenderCommand<P> for DrawSplatLight {
    type Param = SRes<Splat>;
    type ViewQuery = ();
    type ItemQuery = ();

    fn render<'w>(
        _item: &P,
        _view: (),
        _entity: Option<()>,
        splat: SystemParamItem<'w, '_, Self::Param>,
        pass: &mut TrackedRenderPass<'w>,
    ) -> RenderCommandResult {
        let Some(group) = &splat.into_inner().composite else {
            return RenderCommandResult::Skip;
        };
        pass.set_bind_group(0, group, &[]);
        pass.draw(0..3, 0..1);
        RenderCommandResult::Success
    }
}
