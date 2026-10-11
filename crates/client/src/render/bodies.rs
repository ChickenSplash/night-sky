//! Pooled mesh entities that put each frame's resolved bodies and sprites on
//! screen, and the camera switch into realistic rendering.
use super::{
    BodyMaterial, PSF_SIGMA_PX, PointMaterial, SPACE_BLUE,
    lights::{Lights, Photometry},
    resolve::{Frame, resolve},
};
use crate::{Navigation, scene::SpaceCamera};
use bevy::{
    camera::visibility::NoFrustumCulling,
    core_pipeline::tonemapping::Tonemapping,
    pbr::{Material, MeshMaterial3d},
    post_process::bloom::Bloom,
    prelude::*,
    render::view::Hdr,
};
use big_space::prelude::*;

#[derive(Component, Clone, Copy)]
pub(super) struct Body;

#[derive(Component, Clone, Copy)]
pub(super) struct PointSprite;

#[derive(Resource)]
pub(super) struct Meshes {
    sphere: Handle<Mesh>,
    square: Handle<Mesh>,
}

/// Reused entities, grown on demand and hidden when unused.
#[derive(Resource)]
pub(super) struct Pool<M: Material>(Vec<(Entity, Handle<M>)>);

impl<M: Material> Default for Pool<M> {
    fn default() -> Self {
        Self(Vec::new())
    }
}

pub(super) fn setup(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>) {
    commands.insert_resource(Meshes {
        sphere: meshes.add(Sphere::new(1.0).mesh().uv(48, 24)),
        square: meshes.add(Rectangle::new(2.0, 2.0)),
    });
}

/// HDR, bloom and a black sky only while realistic, so the wireframe look is
/// unchanged.
/// Cameras sharing a window must agree on HDR, or the overlay presents a stale
/// frame, so the egui camera switches too, without tone mapping.
pub(super) fn switch_camera(
    mut commands: Commands,
    nav: Res<Navigation>,
    mut clear: ResMut<ClearColor>,
    debug: Res<crate::render::debug::RenderDebug>,
    space: Query<(Entity, Has<Hdr>, Has<Bloom>), With<SpaceCamera>>,
    overlay: Query<Entity, With<bevy_egui::PrimaryEguiContext>>,
) {
    let Ok((entity, hdr, bloom)) = space.single() else {
        return;
    };
    let wants_bloom = nav.realistic && debug.bloom;
    if bloom != wants_bloom {
        if wants_bloom {
            commands.entity(entity).insert(Bloom::NATURAL);
        } else {
            commands.entity(entity).remove::<Bloom>();
        }
    }
    if nav.realistic == hdr {
        return;
    }
    clear.0 = if nav.realistic {
        Color::BLACK
    } else {
        SPACE_BLUE
    };
    for overlay in &overlay {
        if nav.realistic {
            commands.entity(overlay).insert((Hdr, Tonemapping::None));
        } else {
            commands.entity(overlay).remove::<Hdr>();
        }
    }
    if nav.realistic {
        commands.entity(entity).insert((Hdr, Tonemapping::None));
    } else {
        commands.entity(entity).remove::<Hdr>();
    }
}

pub(super) fn shade(
    nav: Res<Navigation>,
    lights: Res<Lights>,
    photometry: Res<Photometry>,
    mut frame: ResMut<Frame>,
    camera: Query<&Camera, With<SpaceCamera>>,
) {
    // Physical pixels per logical pixel: the point spread is set in physical ones.
    let scale = camera
        .single()
        .ok()
        .and_then(|c| Some(c.physical_viewport_size()?.x as f32 / c.logical_viewport_size()?.x))
        .filter(|s| s.is_finite() && *s > 0.0)
        .unwrap_or(1.0);
    *frame = if nav.realistic {
        resolve(
            &lights,
            &photometry,
            nav.exposure,
            nav.compression,
            PSF_SIGMA_PX / scale,
        )
    } else {
        Frame::default()
    };
}

type Placement = (
    &'static mut Transform,
    &'static mut GlobalTransform,
    &'static mut CellCoord,
    &'static mut Visibility,
);

type SpaceCameraCell = (
    &'static CellCoord,
    &'static ChildOf,
    &'static GlobalTransform,
);

/// Grows a pool to `count` hidden entities in the camera's grid.
fn grow<M: Material + Default>(
    commands: &mut Commands,
    pool: &mut Pool<M>,
    materials: &mut Assets<M>,
    mesh: &Handle<Mesh>,
    count: usize,
    (cell, parent): (&CellCoord, &ChildOf),
    marker: impl Component + Copy,
) {
    if pool.0.len() < count {
        // The grid has no visibility of its own, which would hide its children.
        commands
            .entity(parent.parent())
            .insert_if_new(Visibility::default());
    }
    while pool.0.len() < count {
        let material = materials.add(M::default());
        let entity = commands
            .spawn((
                marker,
                Mesh3d(mesh.clone()),
                MeshMaterial3d(material.clone()),
                Transform::default(),
                *cell,
                Visibility::Hidden,
                NoFrustumCulling,
                ChildOf(parent.parent()),
            ))
            .id();
        pool.0.push((entity, material));
    }
}

/// Shows an entity in the camera's cell: the local translation is the render
/// position. Sets the global transform too, because propagation has run.
fn place(
    (mut transform, mut global, mut cell, mut visibility): (
        Mut<Transform>,
        Mut<GlobalTransform>,
        Mut<CellCoord>,
        Mut<Visibility>,
    ),
    camera: CellCoord,
    placed: Transform,
) {
    *cell = camera;
    *transform = placed;
    *global = GlobalTransform::from(placed);
    *visibility = Visibility::Inherited;
}

fn hide(mut visibility: Mut<Visibility>) {
    if *visibility != Visibility::Hidden {
        *visibility = Visibility::Hidden;
    }
}

pub(super) fn sync_bodies(
    mut commands: Commands,
    frame: Res<Frame>,
    meshes: Res<Meshes>,
    mut pool: ResMut<Pool<BodyMaterial>>,
    mut materials: ResMut<Assets<BodyMaterial>>,
    camera: Query<SpaceCameraCell, With<SpaceCamera>>,
    mut bodies: Query<Placement, (With<Body>, Without<SpaceCamera>)>,
) {
    let Ok((cell, parent, _)) = camera.single() else {
        return;
    };
    let count = frame.bodies.len();
    grow(
        &mut commands,
        &mut pool,
        &mut materials,
        &meshes.sphere,
        count,
        (cell, parent),
        Body,
    );
    for (index, (entity, material)) in pool.0.iter().enumerate() {
        let Ok((transform, global, body_cell, visibility)) = bodies.get_mut(*entity) else {
            continue;
        };
        let Some(body) = frame.bodies.get(index) else {
            hide(visibility);
            continue;
        };
        place(
            (transform, global, body_cell, visibility),
            *cell,
            Transform::from_translation(body.centre).with_scale(Vec3::splat(body.radius)),
        );
        // Only touch changed materials: every change re-uploads the bind group.
        let changed = BodyMaterial {
            colour: body.colour.extend(1.0),
            light: body.light,
            spin: Vec4::from(body.rotation),
            surface: body.surface,
            lights: body.lights,
            tints: body.tints,
        };
        let key = |m: &BodyMaterial| (m.colour, m.light, m.spin, m.surface, m.lights, m.tints);
        if materials
            .get(material)
            .is_some_and(|m| key(m) != key(&changed))
            && let Some(m) = materials.get_mut(material)
        {
            *m = changed;
        }
    }
}

pub(super) fn sync_points(
    mut commands: Commands,
    frame: Res<Frame>,
    meshes: Res<Meshes>,
    mut pool: ResMut<Pool<PointMaterial>>,
    mut materials: ResMut<Assets<PointMaterial>>,
    camera: Query<SpaceCameraCell, With<SpaceCamera>>,
    mut points: Query<Placement, (With<PointSprite>, Without<SpaceCamera>)>,
) {
    let Ok((cell, parent, view)) = camera.single() else {
        return;
    };
    let count = frame.rims.len() + frame.points.len();
    grow(
        &mut commands,
        &mut pool,
        &mut materials,
        &meshes.square,
        count,
        (cell, parent),
        PointSprite,
    );
    // Sprites face the camera: they share its rotation.
    let facing = view.rotation();
    for (index, (entity, material)) in pool.0.iter().enumerate() {
        let Ok((transform, global, point_cell, visibility)) = points.get_mut(*entity) else {
            continue;
        };
        // Rims first: points come and go as bodies cross `POINT_PIXELS`, and a
        // sprite moved to another pooled entity shows its old material for a frame.
        let Some(point) = frame.rims.iter().chain(&frame.points).nth(index) else {
            hide(visibility);
            continue;
        };
        place(
            (transform, global, point_cell, visibility),
            *cell,
            Transform::from_translation(point.centre)
                .with_rotation(facing)
                .with_scale(Vec3::splat(point.half)),
        );
        let changed = point.material(facing);
        if materials.get(material).is_some_and(|m| *m != changed)
            && let Some(m) = materials.get_mut(material)
        {
            *m = changed;
        }
    }
}
