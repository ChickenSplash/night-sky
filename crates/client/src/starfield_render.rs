//! The render-world half of `starfield.rs`. Each star is one 20-byte record
//! in a vertex buffer uploaded once per batch, and drawn as an instance of a
//! three-corner triangle whose corners the shader builds from `vertex_index`.
//! Bevy's material path has no per-instance data, so this is a small pipeline
//! of its own in the transparent 3D pass, sharing Bevy's view bind groups.
//!
//! Each frame `starfield::sync` lists the draws (`StarFrame`) and queues new
//! and dropped batches (`StarUploads`). Both are taken during extraction, so
//! a frame's draws and buffers always agree.
use crate::scene::SpaceCamera;
use bevy::{
    asset::uuid_handle,
    core_pipeline::core_3d::{CORE_3D_DEPTH_FORMAT, Transparent3d},
    ecs::system::{SystemParamItem, lifetimeless::SRes},
    image::BevyDefault,
    mesh::VertexBufferLayout,
    pbr::{MeshPipeline, MeshPipelineKey, SetMeshViewBindGroup, SetMeshViewBindingArrayBindGroup},
    prelude::*,
    render::{
        Extract, ExtractSchedule, Render, RenderApp, RenderStartup, RenderSystems,
        extract_component::ExtractComponentPlugin,
        render_phase::{
            AddRenderCommand, DrawFunctions, PhaseItem, PhaseItemExtraIndex, RenderCommand,
            RenderCommandResult, SetItemPipeline, TrackedRenderPass, ViewSortedRenderPhases,
        },
        render_resource::{binding_types::uniform_buffer, *},
        renderer::{RenderDevice, RenderQueue},
        sync_world::MainEntity,
        view::{ExtractedView, ViewTarget},
    },
};
use bytemuck::{Pod, Zeroable};
use std::{
    collections::HashMap,
    sync::{Mutex, PoisonError},
};

pub const SHADER: Handle<Shader> = uuid_handle!("d12d0ce2-812e-4260-a528-74d3af35d480");
/// Temperatures in each colour table, log-spaced from 1,000 to 50,000 K.
pub const PALETTE: usize = 32;

/// One star as the GPU stores it.
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
#[repr(C)]
pub struct StarInstance {
    /// Parsecs from its draw's origin: the block centre, or the eye for
    /// precise sprites.
    pub position: [f32; 3],
    pub luminosity: f32,
    pub temperature: f32,
}

/// Uniforms shared by every star draw this frame.
#[derive(Clone, Default, ShaderType)]
pub struct Globals {
    /// Parsecs per render unit, faintest visible luminosity per squared
    /// parsec, display light per luminosity per squared parsec, and one for
    /// realistic colours and visual brightness from `palette`.
    pub settings: Vec4,
    /// Debug switches (`RenderDebug`): one to skip rasterising points, then
    /// the footprint radius in physical pixels.
    pub debug: Vec4,
    /// Each temperature's realistic colour with unit luminance, then its
    /// visual efficiency: light per bolometric luminosity.
    pub palette: [Vec4; PALETTE],
    /// Each temperature's wireframe colour (`spectral_colour`), linear.
    pub wireframe: [Vec4; PALETTE],
}

/// One draw: a batch, or this frame's precise sprites.
#[derive(Clone, Copy, Debug)]
pub struct StarDraw {
    /// `None` for `StarFrame::precise`.
    pub batch: Option<u64>,
    /// Render-space position of the stars' origin, then render units per
    /// parsec. Formed from f64 camera-relative offsets.
    pub origin: Vec4,
    /// Points closer than this times their distance from the origin are
    /// hidden, as are those within the body range in parsecs. Both zero for
    /// precise sprites.
    pub near: Vec4,
}

#[derive(Clone, Copy, ShaderType)]
pub(crate) struct DrawUniform {
    origin: Vec4,
    near: Vec4,
}

/// What to draw this frame, rebuilt by `starfield::sync`.
#[derive(Resource, Clone, Default)]
pub struct StarFrame {
    pub globals: Option<Globals>,
    pub draws: Vec<StarDraw>,
    /// Precise sprites, placed relative to the eye each frame.
    pub precise: Vec<StarInstance>,
    /// Draw as single-pixel points (`starfield_points.rs`), not triangles.
    pub points: bool,
}

/// Batches to upload or free, taken once by the render world.
#[derive(Resource, Default)]
pub struct StarUploads(Mutex<Uploads>);

#[derive(Default)]
struct Uploads {
    added: Vec<(u64, Vec<StarInstance>)>,
    removed: Vec<u64>,
}

impl StarUploads {
    pub fn add(&self, id: u64, stars: Vec<StarInstance>) {
        self.lock().added.push((id, stars));
    }

    pub fn remove(&self, id: u64) {
        self.lock().removed.push(id);
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Uploads> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

pub struct StarRenderPlugin;

impl Plugin for StarRenderPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<StarFrame>()
            .init_resource::<StarUploads>()
            .add_plugins(ExtractComponentPlugin::<SpaceCamera>::default());
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        render_app
            .init_resource::<StarBuffers>()
            .init_resource::<SpecializedRenderPipelines<StarPipeline>>()
            .add_render_command::<Transparent3d, DrawStars>()
            .add_systems(RenderStartup, init_pipeline)
            .add_systems(ExtractSchedule, extract)
            .add_systems(
                Render,
                (
                    queue.in_set(RenderSystems::QueueMeshes),
                    prepare.in_set(RenderSystems::PrepareResources),
                    bind.in_set(RenderSystems::PrepareBindGroups),
                ),
            );
    }
}

#[derive(Resource)]
struct StarPipeline {
    mesh_pipeline: MeshPipeline,
    layout: BindGroupLayoutDescriptor,
}

fn init_pipeline(mut commands: Commands, mesh_pipeline: Res<MeshPipeline>) {
    let layout = BindGroupLayoutDescriptor::new(
        "starfield_layout",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::VERTEX_FRAGMENT,
            (
                uniform_buffer::<Globals>(false),
                uniform_buffer::<DrawUniform>(true),
            ),
        ),
    );
    commands.insert_resource(StarPipeline {
        mesh_pipeline: mesh_pipeline.clone(),
        layout,
    });
}

impl SpecializedRenderPipeline for StarPipeline {
    type Key = MeshPipelineKey;

    fn specialize(&self, key: Self::Key) -> RenderPipelineDescriptor {
        let view = self.mesh_pipeline.get_view_layout(key.into());
        let format = if key.contains(MeshPipelineKey::HDR) {
            ViewTarget::TEXTURE_FORMAT_HDR
        } else {
            TextureFormat::bevy_default()
        };
        RenderPipelineDescriptor {
            label: Some("starfield".into()),
            layout: vec![
                view.main_layout.clone(),
                view.binding_array_layout.clone(),
                self.layout.clone(),
            ],
            vertex: VertexState {
                shader: SHADER,
                entry_point: Some("vertex".into()),
                buffers: vec![VertexBufferLayout {
                    array_stride: size_of::<StarInstance>() as u64,
                    step_mode: VertexStepMode::Instance,
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
                            format: VertexFormat::Float32,
                            offset: 16,
                            shader_location: 2,
                        },
                    ],
                }],
                ..default()
            },
            fragment: Some(FragmentState {
                shader: SHADER,
                entry_point: Some("fragment".into()),
                // Additive, as `AlphaMode::Add`: the shader writes zero alpha.
                targets: vec![Some(ColorTargetState {
                    format,
                    blend: Some(BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    write_mask: ColorWrites::ALL,
                })],
                ..default()
            }),
            primitive: PrimitiveState {
                cull_mode: None,
                ..default()
            },
            // Bodies in front hide stars; stars never hide anything.
            depth_stencil: Some(DepthStencilState {
                format: CORE_3D_DEPTH_FORMAT,
                depth_write_enabled: false,
                depth_compare: CompareFunction::GreaterEqual,
                stencil: default(),
                bias: default(),
            }),
            multisample: MultisampleState {
                count: key.msaa_samples(),
                ..default()
            },
            ..default()
        }
    }
}

#[derive(Resource)]
pub(crate) struct StarBuffers {
    pub(crate) batches: HashMap<u64, (Buffer, u32)>,
    pub(crate) frame: StarFrame,
    pub(crate) precise: RawBufferVec<StarInstance>,
    pub(crate) globals: UniformBuffer<Globals>,
    pub(crate) draws: DynamicUniformBuffer<DrawUniform>,
    /// Each draw's dynamic offset into `draws`.
    pub(crate) offsets: Vec<u32>,
    bind_group: Option<BindGroup>,
}

impl Default for StarBuffers {
    fn default() -> Self {
        Self {
            batches: HashMap::new(),
            frame: StarFrame::default(),
            precise: RawBufferVec::new(BufferUsages::VERTEX),
            globals: UniformBuffer::default(),
            draws: DynamicUniformBuffer::default(),
            offsets: Vec::new(),
            bind_group: None,
        }
    }
}

fn extract(
    frame: Extract<Res<StarFrame>>,
    uploads: Extract<Res<StarUploads>>,
    device: Res<RenderDevice>,
    mut buffers: ResMut<StarBuffers>,
) {
    let mut queue = uploads.lock();
    // Added first: a batch made and dropped before extraction is then freed.
    for (id, stars) in queue.added.drain(..) {
        let buffer = device.create_buffer_with_data(&BufferInitDescriptor {
            label: Some("star batch"),
            contents: bytemuck::cast_slice(&stars),
            usage: BufferUsages::VERTEX,
        });
        buffers.batches.insert(id, (buffer, stars.len() as u32));
    }
    for id in queue.removed.drain(..) {
        buffers.batches.remove(&id);
    }
    buffers.frame = frame.clone();
}

fn queue(
    draw_functions: Res<DrawFunctions<Transparent3d>>,
    pipeline: Res<StarPipeline>,
    mut pipelines: ResMut<SpecializedRenderPipelines<StarPipeline>>,
    cache: Res<PipelineCache>,
    buffers: Res<StarBuffers>,
    mut phases: ResMut<ViewSortedRenderPhases<Transparent3d>>,
    views: Query<(&ExtractedView, &Msaa), With<SpaceCamera>>,
) {
    let draw_function = draw_functions.read().id::<DrawStars>();
    if buffers.frame.points {
        return;
    }
    for (view, msaa) in &views {
        let Some(phase) = phases.get_mut(&view.retained_view_entity) else {
            continue;
        };
        let key = MeshPipelineKey::from_msaa_samples(msaa.samples())
            | MeshPipelineKey::from_hdr(view.hdr);
        let id = pipelines.specialize(&cache, &pipeline, key);
        // No entity: the batch range carries the draw's index. Bevy's batching
        // finds no mesh for the placeholder and leaves the range alone.
        for index in 0..buffers.frame.draws.len() as u32 {
            phase.add(Transparent3d {
                distance: 0.0,
                pipeline: id,
                entity: (Entity::PLACEHOLDER, MainEntity::from(Entity::PLACEHOLDER)),
                draw_function,
                batch_range: index..index + 1,
                extra_index: PhaseItemExtraIndex::None,
                indexed: false,
            });
        }
    }
}

fn prepare(device: Res<RenderDevice>, queue: Res<RenderQueue>, mut buffers: ResMut<StarBuffers>) {
    let buffers = &mut *buffers;
    let Some(globals) = buffers.frame.globals.clone() else {
        return;
    };
    buffers.globals.set(globals);
    buffers.globals.write_buffer(&device, &queue);
    buffers.draws.clear();
    buffers.offsets = buffers
        .frame
        .draws
        .iter()
        .map(|draw| {
            buffers.draws.push(&DrawUniform {
                origin: draw.origin,
                near: draw.near,
            })
        })
        .collect();
    buffers.draws.write_buffer(&device, &queue);
    buffers.precise.clear();
    for &star in &buffers.frame.precise {
        buffers.precise.push(star);
    }
    buffers.precise.write_buffer(&device, &queue);
}

fn bind(
    device: Res<RenderDevice>,
    pipeline: Res<StarPipeline>,
    cache: Res<PipelineCache>,
    mut buffers: ResMut<StarBuffers>,
) {
    let (Some(globals), Some(draws)) = (buffers.globals.binding(), buffers.draws.binding()) else {
        buffers.bind_group = None;
        return;
    };
    let bind_group = device.create_bind_group(
        "starfield_bind_group",
        &cache.get_bind_group_layout(&pipeline.layout),
        &BindGroupEntries::sequential((globals, draws)),
    );
    buffers.bind_group = Some(bind_group);
}

type DrawStars = (
    SetItemPipeline,
    SetMeshViewBindGroup<0>,
    SetMeshViewBindingArrayBindGroup<1>,
    DrawStarInstances,
);

struct DrawStarInstances;

impl<P: PhaseItem> RenderCommand<P> for DrawStarInstances {
    type Param = SRes<StarBuffers>;
    type ViewQuery = ();
    type ItemQuery = ();

    fn render<'w>(
        item: &P,
        _view: (),
        _entity: Option<()>,
        buffers: SystemParamItem<'w, '_, Self::Param>,
        pass: &mut TrackedRenderPass<'w>,
    ) -> RenderCommandResult {
        let buffers = buffers.into_inner();
        let index = item.batch_range().start as usize;
        let (Some(draw), Some(&offset), Some(bind_group)) = (
            buffers.frame.draws.get(index),
            buffers.offsets.get(index),
            &buffers.bind_group,
        ) else {
            return RenderCommandResult::Skip;
        };
        let (buffer, count) = match draw.batch {
            Some(id) => match buffers.batches.get(&id) {
                Some((buffer, count)) => (buffer, *count),
                None => return RenderCommandResult::Skip,
            },
            None => match buffers.precise.buffer() {
                Some(buffer) => (buffer, buffers.frame.precise.len() as u32),
                None => return RenderCommandResult::Skip,
            },
        };
        if count == 0 {
            return RenderCommandResult::Skip;
        }
        pass.set_bind_group(2, bind_group, &[offset]);
        pass.set_vertex_buffer(0, buffer.slice(..));
        pass.draw(0..3, 0..count);
        RenderCommandResult::Success
    }
}
