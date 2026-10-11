//! The central black hole, kept separate from the removed diffuse glow.

use crate::scene::{CameraPose, CameraRig, SpaceCamera};
use bevy::{camera::visibility::NoFrustumCulling, math::DVec3, prelude::*};
use big_space::prelude::*;
use space_simulation::galaxy::{GalaxyModel, PC_METRES};

#[derive(Resource)]
struct BlackHole(Entity);

pub struct BlackHolePlugin;

impl Plugin for BlackHolePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            PostUpdate,
            sync.after(crate::scene::draw)
                .before(bevy::camera::visibility::VisibilitySystems::VisibilityPropagate),
        );
    }
}

fn sync(
    mut commands: Commands,
    sphere: Option<Res<BlackHole>>,
    mut model: Local<Option<GalaxyModel>>,
    camera_state: (Res<CameraPose>, Res<CameraRig>),
    assets: (ResMut<Assets<Mesh>>, ResMut<Assets<StandardMaterial>>),
    camera: Query<(&CellCoord, &ChildOf, &GlobalTransform), With<SpaceCamera>>,
    mut placement: Query<
        (
            &mut Transform,
            &mut GlobalTransform,
            &mut CellCoord,
            &mut Visibility,
        ),
        Without<SpaceCamera>,
    >,
) {
    let (pose, rig) = camera_state;
    let (mut meshes, mut materials) = assets;
    let Ok((cell, parent, camera_transform)) = camera.single() else {
        return;
    };
    let model = model.get_or_insert_with(GalaxyModel::milky_way);
    if sphere.is_none() {
        commands
            .entity(parent.parent())
            .insert_if_new(Visibility::default());
        let entity = commands
            .spawn((
                Mesh3d(meshes.add(Sphere::new(1.0).mesh().uv(48, 24))),
                MeshMaterial3d(materials.add(StandardMaterial {
                    base_color: Color::BLACK,
                    unlit: true,
                    ..default()
                })),
                Transform::default(),
                GlobalTransform::default(),
                *cell,
                Visibility::Hidden,
                NoFrustumCulling,
                ChildOf(parent.parent()),
            ))
            .id();
        commands.insert_resource(BlackHole(entity));
        return;
    }
    let eye = DVec3::from_array(model.home_pc)
        + rig.galactic_ly(&pose) * (space_simulation::LY_METRES / PC_METRES);
    if let Ok((mut transform, mut global, mut grid_cell, mut visibility)) =
        placement.get_mut(sphere.expect("black hole exists").0)
    {
        *grid_cell = *cell;
        let delta = -eye * (PC_METRES / pose.render_scale());
        *transform = Transform::from_translation(camera_transform.translation() + delta.as_vec3())
            .with_scale(Vec3::splat(
                (model.black_hole_radius_metres() / pose.render_scale()) as f32,
            ));
        *global = GlobalTransform::from(*transform);
        *visibility = Visibility::Inherited;
    }
}
