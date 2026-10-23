//! Galactic stars are drawn as single-pixel points while
//! `RenderDebug::point_stars` is on, instead of `stars::render`'s
//! three-corner triangles: at the galactic centre the cost is per vertex, and
//! a point is one vertex where a triangle is three.
//!
//! The `StarPoints` node, between the main opaque and transmissive passes,
//! draws every batch of `StarFrame` as points (`starfield_points.wgsl`),
//! depth-tested against the opaque bodies, additively into an HDR image at
//! `SUPERSAMPLE` times the view's resolution per axis, without MSAA. One
//! full-screen draw in the transparent pass (`starfield_points_composite.wgsl`)
//! then gives each point the raster path's Gaussian and adds it to the frame.
//! It reads the same batch buffers, globals and draw uniforms as
//! `stars::render`.
use crate::{
    scene::SpaceCamera,
    stars::render::{DrawUniform, Globals, StarBuffers, StarInstance},
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
    mesh::VertexBufferLayout,
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
                texture_2d, texture_depth_2d, texture_depth_2d_multisampled, uniform_buffer,
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

const POINTS_SHADER: Handle<Shader> = uuid_handle!("3c9d5e1f-7a2b-4c8d-9e0f-1a2b3c4d5e6f");
const COMPOSITE_SHADER: Handle<Shader> = uuid_handle!("b4e7f0a1-2c3d-4e5f-8a9b-0c1d2e3f4a5b");
/// Points image pixels per view pixel, per axis, so a point lands within a
/// quarter pixel of its star. Keep in step with the composite shader.
const SUPERSAMPLE: u32 = 2;
const POINTS_FORMAT: TextureFormat = TextureFormat::Rgba16Float;

pub struct StarPointsPlugin;

impl Plugin for StarPointsPlugin {
    fn build(&self, app: &mut App) {
        load_internal_asset!(
            app,
            POINTS_SHADER,
            "starfield_points.wgsl",
            Shader::from_wgsl
        );
        load_internal_asset!(
            app,
            COMPOSITE_SHADER,
            "starfield_points_composite.wgsl",
            Shader::from_wgsl
        );
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        render_app
            .init_resource::<Points>()
            .init_resource::<SpecializedRenderPipelines<PointsPipeline>>()
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
            .add_render_graph_node::<ViewNodeRunner<PointsNode>>(Core3d, StarPoints)
            .add_render_graph_edges(
                Core3d,
                (
                    Node3d::MainOpaquePass,
                    StarPoints,
                    Node3d::MainTransmissivePass,
                ),
            );
    }
}

#[derive(Debug, Hash, PartialEq, Eq, Clone, RenderLabel)]
struct StarPoints;

#[derive(Resource)]
struct PointsPipeline {
    /// The view, globals, draw and depth, with a single-sampled and a
    /// multisampled depth texture.
    points: [BindGroupLayoutDescriptor; 2],
    composite: BindGroupLayoutDescriptor,
}

fn init_pipeline(mut commands: Commands) {
    let points = |multisampled: bool| {
        let depth = if multisampled {
            texture_depth_2d_multisampled()
        } else {
            texture_depth_2d()
        };
        BindGroupLayoutDescriptor::new(
            "star_points_layout",
            &BindGroupLayoutEntries::sequential(
                ShaderStages::VERTEX,
                (
                    uniform_buffer::<ViewUniform>(true),
                    uniform_buffer::<Globals>(false),
                    uniform_buffer::<DrawUniform>(true),
                    depth,
                ),
            ),
        )
    };
    commands.insert_resource(PointsPipeline {
        points: [points(false), points(true)],
        composite: BindGroupLayoutDescriptor::new(
            "star_points_composite_layout",
            &BindGroupLayoutEntries::sequential(
                ShaderStages::FRAGMENT,
                (
                    texture_2d(TextureSampleType::Float { filterable: false }),
                    uniform_buffer::<UVec4>(false),
                ),
            ),
        ),
    });
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Key {
    /// The points, by whether the depth texture is multisampled.
    Points { multisampled: bool },
    /// The composite, by MSAA samples and whether the target is HDR.
    Composite { samples: u32, hdr: bool },
}

impl SpecializedRenderPipeline for PointsPipeline {
    type Key = Key;

    fn specialize(&self, key: Key) -> RenderPipelineDescriptor {
        match key {
            Key::Points { multisampled } => RenderPipelineDescriptor {
                label: Some("star_points".into()),
                layout: vec![self.points[usize::from(multisampled)].clone()],
                vertex: VertexState {
                    shader: POINTS_SHADER,
                    shader_defs: if multisampled {
                        vec!["MULTISAMPLED".into()]
                    } else {
                        Vec::new()
                    },
                    entry_point: Some("vertex".into()),
                    buffers: vec![VertexBufferLayout {
                        array_stride: size_of::<StarInstance>() as u64,
                        step_mode: VertexStepMode::Vertex,
                        attributes: vec![
                            VertexAttribute {
                                format: VertexFormat::Float32x3,
                                offset: 0,
                                shader_location: 0,
                            },
                            VertexAttribute {
                                format: VertexFormat::Float32,
                                offset: 12,
                                shader_location: 1,
                            },
                            VertexAttribute {
                                format: VertexFormat::Float16x2,
                                offset: 16,
                                shader_location: 2,
                            },
                        ],
                    }],
                },
                fragment: Some(FragmentState {
                    shader: POINTS_SHADER,
                    entry_point: Some("fragment".into()),
                    targets: vec![Some(ColorTargetState {
                        format: POINTS_FORMAT,
                        blend: Some(BlendState {
                            color: BlendComponent {
                                src_factor: BlendFactor::One,
                                dst_factor: BlendFactor::One,
                                operation: BlendOperation::Add,
                            },
                            alpha: BlendComponent::OVER,
                        }),
                        write_mask: ColorWrites::COLOR,
                    })],
                    ..default()
                }),
                primitive: PrimitiveState {
                    topology: PrimitiveTopology::PointList,
                    ..default()
                },
                ..default()
            },
            Key::Composite { samples, hdr } => RenderPipelineDescriptor {
                label: Some("star_points_composite".into()),
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
                // The points already tested their depth.
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
            },
        }
    }
}

/// The space camera's point resources. There is one space camera.
#[derive(Resource, Default)]
struct Points {
    /// The supersampled points image.
    image: Option<TextureView>,
    /// The viewport the image covers: origin, then size, in physical pixels.
    viewport: UniformBuffer<UVec4>,
    pipeline: Option<CachedRenderPipelineId>,
    /// The view, globals, draws and depth.
    group: Option<BindGroup>,
    composite: Option<BindGroup>,
}

#[allow(clippy::too_many_arguments)]
fn queue(
    draw_functions: Res<DrawFunctions<Transparent3d>>,
    pipeline: Res<PointsPipeline>,
    mut pipelines: ResMut<SpecializedRenderPipelines<PointsPipeline>>,
    cache: Res<PipelineCache>,
    buffers: Res<StarBuffers>,
    mut points: ResMut<Points>,
    mut phases: ResMut<ViewSortedRenderPhases<Transparent3d>>,
    views: Query<(&ExtractedView, &Msaa), With<SpaceCamera>>,
) {
    points.pipeline = None;
    if !buffers.frame.points {
        return;
    }
    let draw_function = draw_functions.read().id::<DrawComposite>();
    for (view, msaa) in &views {
        let Some(phase) = phases.get_mut(&view.retained_view_entity) else {
            continue;
        };
        let multisampled = msaa.samples() > 1;
        points.pipeline =
            Some(pipelines.specialize(&cache, &pipeline, Key::Points { multisampled }));
        let composite = Key::Composite {
            samples: msaa.samples(),
            hdr: view.hdr,
        };
        phase.add(Transparent3d {
            distance: crate::render::BACKGROUND_DISTANCE,
            pipeline: pipelines.specialize(&cache, &pipeline, composite),
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
    mut points: ResMut<Points>,
    views: Query<&ExtractedView, With<SpaceCamera>>,
) {
    let (Some(view), true) = (views.iter().next(), buffers.frame.points) else {
        // Free the image while points are off.
        points.image = None;
        return;
    };
    let viewport = view.viewport;
    if points.image.is_none() || *points.viewport.get() != viewport {
        let texture = device.create_texture(&TextureDescriptor {
            label: Some("star_points_image"),
            size: Extent3d {
                width: (viewport.z * SUPERSAMPLE).max(1),
                height: (viewport.w * SUPERSAMPLE).max(1),
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: POINTS_FORMAT,
            usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        points.image = Some(texture.create_view(&TextureViewDescriptor::default()));
        points.viewport.set(viewport);
        points.viewport.write_buffer(&device, &queue);
    }
}

fn bind(
    device: Res<RenderDevice>,
    pipeline: Res<PointsPipeline>,
    cache: Res<PipelineCache>,
    buffers: Res<StarBuffers>,
    view_uniforms: Res<ViewUniforms>,
    mut points: ResMut<Points>,
    views: Query<(&ViewDepthTexture, &Msaa), With<SpaceCamera>>,
) {
    let points = &mut *points;
    points.group = None;
    points.composite = None;
    let (Some(image), Some((depth, msaa))) = (&points.image, views.iter().next()) else {
        return;
    };
    let (Some(view), Some(globals), Some(draws), Some(viewport)) = (
        view_uniforms.uniforms.binding(),
        buffers.globals.binding(),
        buffers.draws.binding(),
        points.viewport.binding(),
    ) else {
        return;
    };
    points.group = Some(device.create_bind_group(
        "star_points_bind_group",
        &cache.get_bind_group_layout(&pipeline.points[usize::from(msaa.samples() > 1)]),
        &BindGroupEntries::sequential((view, globals, draws, depth.view())),
    ));
    points.composite = Some(device.create_bind_group(
        "star_points_composite_bind_group",
        &cache.get_bind_group_layout(&pipeline.composite),
        &BindGroupEntries::sequential((image, viewport)),
    ));
}

#[derive(Default)]
struct PointsNode;

impl ViewNode for PointsNode {
    type ViewQuery = (&'static ViewUniformOffset, &'static SpaceCamera);

    fn run<'w>(
        &self,
        _graph: &mut RenderGraphContext,
        render_context: &mut RenderContext<'w>,
        (view_offset, _): QueryItem<'w, '_, Self::ViewQuery>,
        world: &'w World,
    ) -> Result<(), NodeRunError> {
        let points = world.resource::<Points>();
        let buffers = world.resource::<StarBuffers>();
        let (Some(image), Some(group), Some(id)) = (&points.image, &points.group, points.pipeline)
        else {
            return Ok(());
        };
        let Some(render_pipeline) = world.resource::<PipelineCache>().get_render_pipeline(id)
        else {
            return Ok(());
        };
        let diagnostics = render_context.diagnostic_recorder();
        let mut pass = render_context.begin_tracked_render_pass(RenderPassDescriptor {
            label: Some("star_points"),
            color_attachments: &[Some(RenderPassColorAttachment {
                view: image,
                depth_slice: None,
                resolve_target: None,
                ops: Operations {
                    load: LoadOp::Clear(default()),
                    store: StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        let span = diagnostics.pass_span(&mut pass, "star_points");
        pass.set_render_pipeline(render_pipeline);
        for (draw, &offset) in buffers.frame.draws.iter().zip(&buffers.offsets) {
            let (buffer, count) = match draw.batch {
                Some(id) => match buffers.batches.get(&id) {
                    Some((buffer, count)) => (buffer, *count),
                    None => continue,
                },
                None => match buffers.precise.buffer() {
                    Some(buffer) => (buffer, buffers.frame.precise.len() as u32),
                    None => continue,
                },
            };
            if count == 0 {
                continue;
            }
            pass.set_bind_group(0, group, &[view_offset.offset, offset]);
            pass.set_vertex_buffer(0, buffer.slice(..));
            pass.draw(0..count, 0..1);
        }
        span.end(&mut pass);
        Ok(())
    }
}

type DrawComposite = (SetItemPipeline, DrawPointLight);

struct DrawPointLight;

impl<P: PhaseItem> RenderCommand<P> for DrawPointLight {
    type Param = SRes<Points>;
    type ViewQuery = ();
    type ItemQuery = ();

    fn render<'w>(
        _item: &P,
        _view: (),
        _entity: Option<()>,
        points: SystemParamItem<'w, '_, Self::Param>,
        pass: &mut TrackedRenderPass<'w>,
    ) -> RenderCommandResult {
        let Some(group) = &points.into_inner().composite else {
            return RenderCommandResult::Skip;
        };
        pass.set_bind_group(0, group, &[]);
        pass.draw(0..3, 0..1);
        RenderCommandResult::Success
    }
}
