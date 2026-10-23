//! Spatially batched emission patches, analytically integrated in the shader.
//! The glow is smooth, so a camera of its own draws it into an image at a
//! fraction of the view's resolution, without MSAA, and only when the view
//! has changed. A full-view quad then scales that image up into the 3D view.
use crate::{
    Navigation,
    render::sky::radiance_gain,
    scene::{CameraPose, CameraRig, SpaceCamera},
};
use bevy::{
    asset::{RenderAssetUsages, load_internal_asset, uuid_handle},
    camera::{
        RenderTarget,
        visibility::{NoFrustumCulling, RenderLayers},
    },
    core_pipeline::tonemapping::{DebandDither, Tonemapping},
    image::ImageSampler,
    light::cluster::{ClusterConfig, Clusters},
    math::DVec3,
    mesh::{MeshVertexBufferLayoutRef, PrimitiveTopology},
    pbr::{Material, MaterialPipeline, MaterialPipelineKey},
    prelude::*,
    render::{
        Render, RenderApp, RenderSystems,
        render_resource::{
            AsBindGroup, PipelineCache, RenderPipelineDescriptor, ShaderType,
            SpecializedMeshPipelineError, TextureFormat,
        },
        view::{Hdr, NoIndirectDrawing},
    },
    shader::ShaderRef,
};
use space_simulation::{
    galaxy::{COMPONENTS, GalaxyModel, PC_METRES},
    glow::{DISTANCES, patches, unresolved_table},
};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

const SHADER: Handle<Shader> = uuid_handle!("e8cb8743-06fa-43ab-89e4-1150b5f8e738");
const COMPOSITE_SHADER: Handle<Shader> = uuid_handle!("5d0c7f3e-2b8a-4f61-9c4e-7a1d3b9e6f20");
/// Seen only by the glow camera.
const LAYER: usize = 30;
/// Frames still drawn after a change, so asset and pipeline preparation
/// (one frame behind with pipelined rendering) cannot leave a stale image.
const SETTLE: u8 = 3;
/// View pixels per glow pixel, in each direction.
const DOWNSCALE: u32 = 8;

#[derive(Clone, ShaderType)]
struct Settings {
    /// Unused, unused, display value per visual solar luminosity per square
    /// parsec per steradian (`sky::radiance_gain`), unused.
    values: Vec4,
    origin: Vec4,
    right: Vec4,
    up: Vec4,
    forward: Vec4,
    /// tan(half vertical FOV), view aspect and glow image size in pixels.
    projection: Vec4,
    light: [Vec4; COMPONENTS * DISTANCES],
}
#[derive(Asset, TypePath, AsBindGroup, Clone)]
struct GlowMaterial {
    #[uniform(0)]
    settings: Settings,
}
impl Material for GlowMaterial {
    fn vertex_shader() -> ShaderRef {
        SHADER.into()
    }
    fn fragment_shader() -> ShaderRef {
        SHADER.into()
    }
    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Add
    }
    fn specialize(
        _: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        layout: &MeshVertexBufferLayoutRef,
        _: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        descriptor.vertex.buffers = vec![layout.0.get_layout(&[
            Mesh::ATTRIBUTE_POSITION.at_shader_location(0),
            Mesh::ATTRIBUTE_UV_0.at_shader_location(1),
            Mesh::ATTRIBUTE_COLOR.at_shader_location(2),
        ])?];
        descriptor.primitive.cull_mode = None;
        if let Some(depth) = &mut descriptor.depth_stencil {
            depth.depth_write_enabled = false;
        }
        Ok(())
    }
}

#[derive(Clone, Default, PartialEq, ShaderType)]
struct CompositeSettings {
    /// View origin and size in pixels.
    viewport: Vec4,
    /// Current over drawn exposure, then unused.
    scale: Vec4,
}
#[derive(Asset, TypePath, AsBindGroup, Clone)]
struct CompositeMaterial {
    #[uniform(0)]
    settings: CompositeSettings,
    #[texture(1)]
    #[sampler(2)]
    image: Handle<Image>,
}
impl Material for CompositeMaterial {
    fn vertex_shader() -> ShaderRef {
        COMPOSITE_SHADER.into()
    }
    fn fragment_shader() -> ShaderRef {
        COMPOSITE_SHADER.into()
    }
    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Add
    }
    fn depth_bias(&self) -> f32 {
        crate::render::BACKGROUND_DISTANCE
    }
    fn specialize(
        _: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        layout: &MeshVertexBufferLayoutRef,
        _: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        descriptor.vertex.buffers = vec![
            layout
                .0
                .get_layout(&[Mesh::ATTRIBUTE_POSITION.at_shader_location(0)])?,
        ];
        descriptor.primitive.cull_mode = None;
        if let Some(depth) = &mut descriptor.depth_stencil {
            depth.depth_write_enabled = false;
        }
        Ok(())
    }
}

/// What the glow image was drawn for.
#[derive(Clone, Copy)]
struct View {
    /// Galactic parsecs from the central black hole.
    eye: DVec3,
    forward: Vec3,
    up: Vec3,
    tan_half_fov: f32,
    size: UVec2,
    exposure: f32,
}
impl View {
    /// Whether the drawn image still matches to a fraction of a glow pixel.
    /// Exposure only rescales it, within a range half floats keep exact.
    fn matches(&self, drawn: &View) -> bool {
        let tolerance = 0.25 * 2.0 * self.tan_half_fov / self.size.y.div_ceil(DOWNSCALE) as f32;
        // Parallax of the nearest structure: the nuclear cluster, or ~100 pc.
        let reach = self.eye.length().clamp(1.0, 100.0);
        let ratio = self.exposure / drawn.exposure;
        self.size == drawn.size
            && self.tan_half_fov == drawn.tan_half_fov
            && (self.forward - drawn.forward).length() < tolerance
            && (self.up - drawn.up).length() < tolerance
            && self.eye.distance(drawn.eye) < f64::from(tolerance) * reach
            && (0.5..2.0).contains(&ratio)
    }
}

#[derive(Resource)]
struct Glow {
    /// Batch origins, in galactic parsecs, and their materials.
    batches: Vec<(DVec3, Handle<GlowMaterial>)>,
    camera: Entity,
    composite: Entity,
    composite_material: Handle<CompositeMaterial>,
    image: Handle<Image>,
    drawn: Option<View>,
    settle: u8,
    /// The star point magnitude limit its light leaves out.
    magnitude: f64,
}

/// Each population's light per star below `magnitude`, by distance.
fn unresolved_light(magnitude: f64) -> [Vec4; COMPONENTS * DISTANCES] {
    let light: Vec<_> = unresolved_table(magnitude)
        .into_iter()
        .map(Vec4::from_array)
        .collect();
    light.try_into().expect("a table per population")
}

/// Whether any render pipeline is still compiling, reported by the render world.
#[derive(Resource, Clone, Default)]
struct Compiling(Arc<AtomicBool>);

pub struct GlowPlugin;
impl Plugin for GlowPlugin {
    fn build(&self, app: &mut App) {
        load_internal_asset!(app, SHADER, "glow.wgsl", Shader::from_wgsl);
        load_internal_asset!(
            app,
            COMPOSITE_SHADER,
            "glow_composite.wgsl",
            Shader::from_wgsl
        );
        let compiling = Compiling::default();
        app.add_plugins((
            MaterialPlugin::<GlowMaterial>::default(),
            MaterialPlugin::<CompositeMaterial>::default(),
        ))
        .insert_resource(compiling.clone())
        .add_systems(
            PostUpdate,
            sync.after(crate::scene::draw)
                .before(bevy::camera::visibility::VisibilitySystems::VisibilityPropagate),
        );
        if let Some(render) = app.get_sub_app_mut(RenderApp) {
            render
                .insert_resource(compiling)
                .add_systems(Render, report_compiling.in_set(RenderSystems::Cleanup));
        }
    }
}

fn report_compiling(cache: Res<PipelineCache>, compiling: Res<Compiling>) {
    compiling.0.store(
        cache.waiting_pipelines().next().is_some(),
        Ordering::Relaxed,
    );
}

fn glow_size(view: UVec2) -> UVec2 {
    UVec2::new(view.x.div_ceil(DOWNSCALE), view.y.div_ceil(DOWNSCALE))
}

/// A half-float image for the glow camera, filtered when scaled up.
fn target(size: UVec2) -> Image {
    let mut image = Image::new_target_texture(size.x, size.y, TextureFormat::Rgba16Float, None);
    image.sampler = ImageSampler::linear();
    image
}

fn spawn(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<GlowMaterial>,
    composites: &mut Assets<CompositeMaterial>,
    images: &mut Assets<Image>,
    (low, magnitude): (UVec2, f64),
) -> Glow {
    let image = images.add(target(low));
    let camera = commands
        .spawn((
            Camera3d::default(),
            Camera {
                order: -1,
                is_active: false,
                clear_color: ClearColorConfig::Custom(Color::NONE),
                ..default()
            },
            RenderTarget::Image(image.clone().into()),
            Hdr,
            Tonemapping::None,
            DebandDither::Disabled,
            Msaa::Off,
            NoIndirectDrawing,
            RenderLayers::layer(LAYER),
            // Bevy adds these only to active cameras, so the first active
            // frame would draw without its view bind group.
            Clusters::default(),
            ClusterConfig::default(),
        ))
        .id();
    let composite_material = composites.add(CompositeMaterial {
        settings: CompositeSettings::default(),
        image: image.clone(),
    });
    let composite = commands
        .spawn((
            Mesh3d(meshes.add(Rectangle::new(2.0, 2.0))),
            MeshMaterial3d(composite_material.clone()),
            Transform::default(),
            GlobalTransform::default(),
            Visibility::Hidden,
            NoFrustumCulling,
        ))
        .id();

    let template = GlowMaterial {
        settings: Settings {
            values: Vec4::ZERO,
            origin: Vec4::ZERO,
            right: Vec4::X,
            up: Vec4::Y,
            forward: -Vec4::Z,
            projection: Vec4::ONE,
            light: unresolved_light(magnitude),
        },
    };
    let mut blocks = std::collections::BTreeMap::<[i32; 3], Vec<_>>::new();
    for patch in patches(&GalaxyModel::milky_way(), 0.75) {
        let key = patch.position_pc.map(|p| (p / 16000.0).floor() as i32);
        blocks.entry(key).or_default().push(patch);
    }
    let mut batches = Vec::new();
    for (key, block) in blocks {
        let centre = DVec3::from_array(key.map(|k| (f64::from(k) + 0.5) * 16000.0));
        let material = materials.add(template.clone());
        let mut positions = Vec::new();
        let mut corners = Vec::new();
        let mut colours = Vec::new();
        for patch in block {
            let offset = (DVec3::from_array(patch.position_pc) - centre)
                .as_vec3()
                .to_array();
            for corner in [
                [-1.0, -1.0],
                [1.0, -1.0],
                [1.0, 1.0],
                [-1.0, -1.0],
                [1.0, 1.0],
                [-1.0, 1.0],
            ] {
                positions.push(offset);
                corners.push(corner);
                colours.push([
                    patch.sigma_pc[0] as f32,
                    patch.sigma_pc[1] as f32,
                    patch.stars as f32,
                    patch.component as f32,
                ]);
            }
        }
        let mesh = Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
        )
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, corners)
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, colours);
        // The shader places patches itself, so the transform stays identity.
        commands.spawn((
            Mesh3d(meshes.add(mesh)),
            MeshMaterial3d(material.clone()),
            Transform::default(),
            GlobalTransform::default(),
            Visibility::Visible,
            NoFrustumCulling,
            RenderLayers::layer(LAYER),
        ));
        batches.push((centre, material));
    }
    Glow {
        batches,
        camera,
        composite,
        composite_material,
        image,
        drawn: None,
        settle: SETTLE,
        magnitude,
    }
}

type GlowAssets<'w> = (
    ResMut<'w, Assets<Mesh>>,
    ResMut<'w, Assets<GlowMaterial>>,
    ResMut<'w, Assets<CompositeMaterial>>,
    ResMut<'w, Assets<Image>>,
);

#[allow(clippy::too_many_arguments)]
fn sync(
    mut commands: Commands,
    glow: Option<ResMut<Glow>>,
    nav: Res<Navigation>,
    pose: Res<CameraPose>,
    rig: Res<CameraRig>,
    compiling: Res<Compiling>,
    mut assets: GlowAssets,
    space: Query<(&GlobalTransform, &Camera), With<SpaceCamera>>,
    mut cameras: Query<&mut Camera, Without<SpaceCamera>>,
    mut visibility: Query<&mut Visibility>,
) {
    let (meshes, materials, composites, images) = &mut assets;
    let Ok((camera_transform, camera)) = space.single() else {
        return;
    };
    let enabled = nav.realistic && nav.galaxy_glow;
    let size = camera
        .physical_viewport_size()
        .unwrap_or(UVec2::ONE)
        .max(UVec2::ONE);
    let Some(mut glow) = glow else {
        if enabled {
            let low = glow_size(size);
            let glow = spawn(
                &mut commands,
                meshes,
                materials,
                composites,
                images,
                (low, nav.magnitude),
            );
            commands.insert_resource(glow);
        }
        return;
    };
    if let Ok(mut composite) = visibility.get_mut(glow.composite) {
        *composite = if enabled {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
    }
    if !enabled {
        if let Ok(mut glow_camera) = cameras.get_mut(glow.camera) {
            glow_camera.is_active = false;
        }
        glow.drawn = None;
        glow.settle = SETTLE;
        return;
    }

    if glow.magnitude != nav.magnitude {
        glow.magnitude = nav.magnitude;
        let light = unresolved_light(nav.magnitude);
        for (_, handle) in &glow.batches {
            if let Some(material) = materials.get_mut(handle) {
                material.settings.light = light;
            }
        }
        glow.drawn = None;
    }
    let model = GalaxyModel::milky_way();
    let view = View {
        eye: DVec3::from_array(model.home_pc)
            + rig.galactic_ly(&pose) * (space_simulation::LY_METRES / PC_METRES),
        forward: camera_transform.forward().into(),
        up: camera_transform.up().into(),
        tan_half_fov: nav.tan_half_fov() as f32,
        size,
        exposure: {
            let pixel = 2.0 * nav.tan_half_fov() / f64::from(size.y);
            radiance_gain(&nav, pixel * pixel) as f32
        },
    };
    if glow.drawn.is_none_or(|drawn| !view.matches(&drawn)) || compiling.0.load(Ordering::Relaxed) {
        glow.settle = SETTLE;
    }
    let draw = glow.settle > 0;
    if let Ok(mut glow_camera) = cameras.get_mut(glow.camera) {
        glow_camera.is_active = draw;
    }
    if draw {
        glow.settle -= 1;
        let low = glow_size(size);
        // A new image, not a resize, so the camera and composite rebind to it.
        if images.get(&glow.image).is_some_and(|i| i.size() != low) {
            images.remove(&glow.image);
            glow.image = images.add(target(low));
            commands
                .entity(glow.camera)
                .insert(RenderTarget::Image(glow.image.clone().into()));
            if let Some(composite) = composites.get_mut(&glow.composite_material) {
                composite.image = glow.image.clone();
            }
            glow.settle = SETTLE;
        }
        for (centre, handle) in &glow.batches {
            if let Some(material) = materials.get_mut(handle) {
                let settings = &mut material.settings;
                settings.values = Vec4::new(0.0, 0.0, view.exposure, 0.0);
                settings.origin = (*centre - view.eye).as_vec3().extend(0.0);
                settings.right = camera_transform.right().extend(0.0);
                settings.up = view.up.extend(0.0);
                settings.forward = view.forward.extend(0.0);
                settings.projection = Vec4::new(
                    view.tan_half_fov,
                    size.x as f32 / size.y as f32,
                    low.x as f32,
                    low.y as f32,
                );
            }
        }
        glow.drawn = Some(view);
    }

    let Some(drawn) = glow.drawn else {
        return;
    };
    let offset = camera
        .viewport
        .as_ref()
        .map_or(UVec2::ZERO, |v| v.physical_position);
    let settings = CompositeSettings {
        viewport: Vec4::new(
            offset.x as f32,
            offset.y as f32,
            size.x as f32,
            size.y as f32,
        ),
        scale: Vec4::new(view.exposure / drawn.exposure, 0.0, 0.0, 0.0),
    };
    if composites
        .get(&glow.composite_material)
        .is_some_and(|c| c.settings != settings)
        && let Some(composite) = composites.get_mut(&glow.composite_material)
    {
        composite.settings = settings;
    }
}
