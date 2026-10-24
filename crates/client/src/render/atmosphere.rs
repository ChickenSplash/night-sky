//! Planetary atmospheres: a scattering shell around a planet's disc
//! (`atmosphere.wgsl`). The shell mesh only chooses which pixels run the
//! shader; the shader finds the ground and the top of the air itself.
use super::{
    MAX_LIGHTS,
    atmosphere_tables::{AtmosphereTables, Tables},
    bodies::{Meshes, Pool, SpaceCameraCell, grow, hide, place},
    resolve::Frame,
};
use crate::scene::SpaceCamera;
use bevy::{
    math::DVec3,
    mesh::MeshVertexBufferLayoutRef,
    pbr::{Material, MaterialPipeline, MaterialPipelineKey},
    prelude::*,
    render::render_resource::{
        AsBindGroup, CompareFunction, Face, RenderPipelineDescriptor, SpecializedMeshPipelineError,
    },
    shader::ShaderRef,
};
use std::ops::RangeInclusive;

/// The shell mesh's radius over the top of the air: the sphere mesh's facets
/// lie slightly inside the sphere they approximate.
const MESH_MARGIN: f32 = 1.01;
/// Within this many near-plane distances of the shell mesh, the camera draws
/// it from inside: the near plane would otherwise cut a hole in the side
/// below. Generous, as oblique points are clipped closer than their distance.
const NEAR_MARGIN: f32 = 10.0;
/// Steps along a pixel's line of sight, and from each towards every star.
pub const VIEW_STEPS: RangeInclusive<u32> = 4..=64;
pub const LIGHT_STEPS: RangeInclusive<u32> = 2..=32;
/// Slider and command ranges.
pub const PRESSURE_BAR: RangeInclusive<f64> = 1e-4..=1000.0;
pub const HEIGHT_KM: RangeInclusive<f64> = 0.1..=200.0;
pub const GAS: RangeInclusive<f64> = 0.0..=10.0;
pub const HAZE: RangeInclusive<f64> = 0.0..=1000.0;
/// The top of the air, in scale heights of its gas or haze, whichever is taller.
const TOP_HEIGHTS: f64 = 12.5;
/// Rayleigh scattering of Earth's air per bar at the ground, for 680, 550 and
/// 440 nm (Hillaire 2020, at 1.01325 bar).
const AIR_PER_BAR: DVec3 = DVec3::new(5.726e-6, 13.381e-6, 32.667e-6);
/// Earth's continental haze at the ground: Mie scattering, and the share of
/// the light it stops that it absorbs rather than scatters (0.444e-6 per
/// metre of absorption).
const HAZE_SCATTERING: f64 = 3.996e-6;
const EARTH_HAZE_ABSORBED: f64 = 0.1;
/// The ground's albedo where the planet gives none.
const GROUND_ALBEDO: f32 = 0.3;
pub const HAZE_ABSORBED: RangeInclusive<f64> = 1e-4..=0.5;

/// A planet's air, physical: coefficients per metre, heights in metres.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Atmosphere {
    /// Height of the top of the air, above which it is taken as empty.
    pub top: f64,
    /// Rayleigh scattering by gas at the ground, per channel, and its scale height.
    pub rayleigh: DVec3,
    pub rayleigh_height: f64,
    /// Mie scattering and grey absorption by haze at the ground, its scale
    /// height and its phase asymmetry (Cornette-Shanks `g`).
    pub mie: DVec3,
    pub mie_absorption: f64,
    pub mie_height: f64,
    pub mie_asymmetry: f64,
    /// Absorption peaking in a layer, such as ozone, falling linearly to
    /// zero `layer_width` either side of `layer_height`.
    pub absorption: DVec3,
    pub layer_height: f64,
    pub layer_width: f64,
}

impl Atmosphere {
    /// The air in units of a planet's radius, as the shader takes it.
    pub fn scaled(&self, radius: f64) -> Air {
        let per = |v: DVec3| (v * radius).as_vec3();
        let over = |h: f64| (h / radius) as f32;
        Air {
            top: 1.0 + over(self.top),
            rayleigh: per(self.rayleigh).extend(over(self.rayleigh_height)),
            mie: per(self.mie).extend((self.mie_absorption * radius) as f32),
            shape: Vec4::new(
                over(self.mie_height),
                self.mie_asymmetry as f32,
                over(self.layer_height),
                over(self.layer_width),
            ),
            absorption: per(self.absorption),
            ground: Vec3::splat(GROUND_ALBEDO),
        }
    }
}

/// An atmosphere as the debug sliders set it, by hand until the simulation
/// stage generates one per planet.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize)]
pub struct AirSettings {
    pub pressure_bar: f64,
    /// Scale height of the gas.
    pub scale_height_km: f64,
    /// Rayleigh scattering per bar at the ground, against Earth's air. Folds
    /// in the gas (carbon dioxide about 2.4, hydrogen about 0.2) and its
    /// number density per bar, higher in colder air.
    pub gas: f64,
    /// Haze at the ground, against Earth's continental haze, whatever the
    /// pressure.
    pub haze: f64,
    pub haze_height_km: f64,
    /// The haze's scattering per channel (red, green, blue), from 0 to 1. Its
    /// absorption stays grey.
    pub haze_colour: [f64; 3],
    /// Cornette-Shanks `g`: how far forward the haze scatters.
    pub haze_asymmetry: f64,
    /// The share of the light the haze stops that it absorbs rather than
    /// scatters (one less its single scattering albedo): 0.1 for Earth's
    /// dust, about 0.001 for clouds of droplets. Under thick haze it sets
    /// how much light gets through by scattering many times.
    pub haze_absorbed: f64,
    /// Absorption peaking in a layer, per metre, such as ozone or methane.
    pub absorption: [f64; 3],
    pub layer_km: f64,
    pub layer_width_km: f64,
}

impl AirSettings {
    pub fn preset(preset: Preset) -> Self {
        let earth = Self {
            pressure_bar: 1.01325,
            scale_height_km: 8.0,
            gas: 1.0,
            haze: 1.0,
            haze_height_km: 1.2,
            haze_colour: [1.0; 3],
            haze_asymmetry: 0.8,
            haze_absorbed: EARTH_HAZE_ABSORBED,
            absorption: [0.650e-6, 1.881e-6, 0.085e-6],
            layer_km: 25.0,
            layer_width_km: 15.0,
        };
        let clear = Self {
            absorption: [0.0; 3],
            ..earth
        };
        // Rough values, to judge the look: surface pressures and scale
        // heights as measured, haze tuned by eye.
        match preset {
            Preset::Earth => earth,
            // Carbon dioxide at 737 K, under thick sulphuric acid cloud.
            Preset::Venus => Self {
                pressure_bar: 92.0,
                scale_height_km: 15.9,
                gas: 0.95,
                haze: 300.0,
                haze_height_km: 15.0,
                haze_colour: [1.0, 0.92, 0.65],
                haze_asymmetry: 0.7,
                // Sulphuric acid droplets barely absorb: this lets about 3%
                // of the starlight reach the ground, averaged over the
                // planet, as measured on Venus (`atmosphere_reference.rs`).
                haze_absorbed: 0.005,
                ..clear
            },
            // Thin carbon dioxide at 210 K, with reddish dust.
            Preset::Mars => Self {
                pressure_bar: 0.006,
                scale_height_km: 11.1,
                gas: 3.3,
                haze: 11.0,
                haze_height_km: 11.1,
                haze_colour: [1.0, 0.6, 0.35],
                haze_asymmetry: 0.75,
                ..clear
            },
            // Nitrogen at 94 K, its low gravity giving a deep column, under
            // a tall orange haze.
            Preset::Titan => Self {
                pressure_bar: 1.5,
                scale_height_km: 21.0,
                gas: 3.0,
                haze: 20.0,
                haze_height_km: 60.0,
                haze_colour: [1.0, 0.6, 0.25],
                haze_asymmetry: 0.6,
                ..clear
            },
            // Hydrogen at 165 K above its 1 bar level, ammonia haze and
            // methane taking red light throughout.
            Preset::HydrogenGiant => Self {
                pressure_bar: 1.0,
                scale_height_km: 27.0,
                gas: 0.4,
                haze: 5.0,
                haze_height_km: 27.0,
                haze_colour: [1.0, 0.95, 0.85],
                haze_asymmetry: 0.7,
                haze_absorbed: EARTH_HAZE_ABSORBED,
                absorption: [2e-6, 0.5e-6, 0.0],
                layer_km: 0.0,
                layer_width_km: 300.0,
            },
        }
    }

    /// The physical air these settings describe.
    pub fn atmosphere(&self) -> Atmosphere {
        let colour = DVec3::from_array(self.haze_colour);
        let tallest = self.scale_height_km.max(self.haze_height_km);
        Atmosphere {
            top: (tallest * TOP_HEIGHTS).max(self.layer_km + self.layer_width_km) * 1e3,
            rayleigh: AIR_PER_BAR * self.gas * self.pressure_bar,
            rayleigh_height: self.scale_height_km * 1e3,
            mie: colour * HAZE_SCATTERING * self.haze,
            mie_absorption: HAZE_SCATTERING * self.haze * self.haze_absorbed
                / (1.0 - self.haze_absorbed),
            mie_height: self.haze_height_km * 1e3,
            mie_asymmetry: self.haze_asymmetry,
            absorption: DVec3::from_array(self.absorption),
            layer_height: self.layer_km * 1e3,
            layer_width: self.layer_width_km * 1e3,
        }
    }
}

/// Starting points for the sliders.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Preset {
    Earth,
    Venus,
    Mars,
    Titan,
    HydrogenGiant,
}

impl Preset {
    pub const ALL: [(Self, &str); 5] = [
        (Self::Earth, "Earth"),
        (Self::Venus, "Venus"),
        (Self::Mars, "Mars"),
        (Self::Titan, "Titan"),
        (Self::HydrogenGiant, "hydrogen giant"),
    ];
}

/// The prototype's air, applied to the selected planet, and the shader's
/// steps. Set from the debug window's `atmosphere` tab or `set_atmosphere`.
#[derive(Resource, Clone, Copy, Debug, PartialEq, serde::Serialize)]
pub struct AtmosphereTuning {
    /// The preset last applied; the sliders may have moved since.
    pub preset: Preset,
    pub air: AirSettings,
    pub view_steps: u32,
    pub light_steps: u32,
}

impl Default for AtmosphereTuning {
    fn default() -> Self {
        Self {
            preset: Preset::Earth,
            air: AirSettings::preset(Preset::Earth),
            view_steps: 16,
            light_steps: 8,
        }
    }
}

/// A change to the tuning: a preset first, then any values over it.
#[derive(Clone, Debug, Default, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct AtmosphereChange {
    #[serde(default)]
    pub preset: Option<Preset>,
    #[serde(default)]
    pub pressure_bar: Option<f64>,
    #[serde(default)]
    pub scale_height_km: Option<f64>,
    #[serde(default)]
    pub gas: Option<f64>,
    #[serde(default)]
    pub haze: Option<f64>,
    #[serde(default)]
    pub haze_height_km: Option<f64>,
    #[serde(default)]
    pub haze_colour: Option<[f64; 3]>,
    #[serde(default)]
    pub haze_absorbed: Option<f64>,
    #[serde(default)]
    pub view_steps: Option<u32>,
    #[serde(default)]
    pub light_steps: Option<u32>,
}

impl AtmosphereChange {
    /// Why a typed change would be clamped or do nothing.
    pub fn check(&self) -> Result<(), String> {
        fn within<T: PartialOrd + std::fmt::Display>(
            name: &str,
            value: Option<T>,
            range: RangeInclusive<T>,
        ) -> Result<(), String> {
            match value {
                Some(value) if !range.contains(&value) => Err(format!(
                    "{name} must be {} to {}, not {value}",
                    range.start(),
                    range.end()
                )),
                _ => Ok(()),
            }
        }
        if *self == Self::default() {
            return Err("Give a preset or at least one value".into());
        }
        within("pressure_bar", self.pressure_bar, PRESSURE_BAR)?;
        within("scale_height_km", self.scale_height_km, HEIGHT_KM)?;
        within("gas", self.gas, GAS)?;
        within("haze", self.haze, HAZE)?;
        within("haze_height_km", self.haze_height_km, HEIGHT_KM)?;
        within("haze_absorbed", self.haze_absorbed, HAZE_ABSORBED)?;
        within("view_steps", self.view_steps, VIEW_STEPS)?;
        within("light_steps", self.light_steps, LIGHT_STEPS)?;
        if let Some(colour) = self.haze_colour
            && !colour.iter().all(|c| (0.0..=1.0).contains(c))
        {
            return Err("haze_colour must be three numbers, each 0 to 1".into());
        }
        Ok(())
    }
}

impl AtmosphereTuning {
    /// Applies a change, clamping what the sliders could not reach.
    pub fn apply(&mut self, change: &AtmosphereChange) {
        fn set<T: PartialOrd + Copy>(to: &mut T, value: Option<T>, range: RangeInclusive<T>) {
            if let Some(value) = value {
                *to = clamped(value, range);
            }
        }
        fn clamped<T: PartialOrd + Copy>(value: T, range: RangeInclusive<T>) -> T {
            if value < *range.start() {
                *range.start()
            } else if value > *range.end() {
                *range.end()
            } else {
                value
            }
        }
        if let Some(preset) = change.preset {
            self.preset = preset;
            self.air = AirSettings::preset(preset);
        }
        let air = &mut self.air;
        set(&mut air.pressure_bar, change.pressure_bar, PRESSURE_BAR);
        set(&mut air.scale_height_km, change.scale_height_km, HEIGHT_KM);
        set(&mut air.gas, change.gas, GAS);
        set(&mut air.haze, change.haze, HAZE);
        set(&mut air.haze_height_km, change.haze_height_km, HEIGHT_KM);
        set(&mut air.haze_absorbed, change.haze_absorbed, HAZE_ABSORBED);
        if let Some(colour) = change.haze_colour {
            air.haze_colour = colour.map(|c| clamped(c, 0.0..=1.0));
        }
        set(&mut self.view_steps, change.view_steps, VIEW_STEPS);
        set(&mut self.light_steps, change.light_steps, LIGHT_STEPS);
    }
}

/// An atmosphere in units of its planet's radius.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Air {
    /// Radius of the top of the air.
    pub top: f32,
    /// Rayleigh scattering, and its scale height.
    pub rayleigh: Vec4,
    /// Mie scattering, and Mie absorption.
    pub mie: Vec4,
    /// Mie scale height and asymmetry, absorbing layer height and half width.
    pub shape: Vec4,
    pub absorption: Vec3,
    /// The ground's albedo per channel, for the light it reflects back up
    /// into the air.
    pub ground: Vec3,
}

/// A planet's atmosphere ready for the GPU.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shell {
    pub centre: Vec3,
    /// The planet's equatorial radius, in render units.
    pub radius: f32,
    /// The planet's pole, in world axes, and its flattening: the ground and
    /// every layer of the air are spheroids of this shape.
    pub pole: Vec3,
    pub flattening: f32,
    pub air: Air,
    /// Direction to each star lighting the planet, and the exposed colour of
    /// its flux; zero when unused.
    pub lights: [Vec4; MAX_LIGHTS],
    pub suns: [Vec4; MAX_LIGHTS],
}

#[derive(Component, Clone, Copy)]
pub(super) struct AtmosphereShell;

#[derive(Asset, TypePath, AsBindGroup, Clone, Default, PartialEq)]
#[bind_group_data(AtmosphereKey)]
pub struct AtmosphereMaterial {
    /// The planet's centre from the camera, in render units, and its radius.
    #[uniform(0)]
    centre: Vec4,
    /// Top of the air in radii, and view and light steps.
    #[uniform(0)]
    steps: Vec4,
    #[uniform(0)]
    rayleigh: Vec4,
    #[uniform(0)]
    mie: Vec4,
    #[uniform(0)]
    shape: Vec4,
    #[uniform(0)]
    absorption: Vec4,
    /// 1 to jitter the view steps, and 1 to end each ray where the air turns
    /// opaque (with the tables only).
    #[uniform(0)]
    switches: Vec4,
    /// The pole, and how far to stretch along it to make the planet round:
    /// its equatorial radius over its polar.
    #[uniform(0)]
    pole: Vec4,
    #[uniform(0)]
    lights: [Vec4; MAX_LIGHTS],
    #[uniform(0)]
    suns: [Vec4; MAX_LIGHTS],
    /// Optical depth to the top of the air (`atmosphere_tables.rs`); without
    /// it the shader marches towards each star instead.
    #[texture(1)]
    #[sampler(2)]
    transmittance: Option<Handle<Image>>,
    /// The multiple scattering's tables (`atmosphere_multiple.rs`), read
    /// with the transmittance's sampler: the thinned air's transmittance for
    /// the starlight, the light scattered more than once and its flow.
    #[texture(3)]
    thinned: Option<Handle<Image>>,
    #[texture(4)]
    multiple: Option<Handle<Image>>,
    #[texture(5)]
    flow: Option<Handle<Image>>,
    /// The camera is inside the shell mesh, or so close that the near plane
    /// would cut its near side: its far side is drawn instead, over everything.
    inside: bool,
}

#[repr(C)]
#[derive(Eq, PartialEq, Hash, Copy, Clone)]
pub struct AtmosphereKey {
    inside: bool,
    tables: bool,
    multiple: bool,
}

impl From<&AtmosphereMaterial> for AtmosphereKey {
    fn from(material: &AtmosphereMaterial) -> Self {
        Self {
            inside: material.inside,
            tables: material.transmittance.is_some(),
            multiple: material.multiple.is_some(),
        }
    }
}

impl Material for AtmosphereMaterial {
    fn fragment_shader() -> ShaderRef {
        super::ATMOSPHERE_SHADER.into()
    }

    /// Scattered light is added; what lies behind is dimmed by the
    /// transmittance, carried in alpha.
    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Premultiplied
    }

    /// Sorted at the shell's near side, not its centre, so its planet's limb
    /// sprites, which share the centre, always draw first, beneath it.
    fn depth_bias(&self) -> f32 {
        self.centre.w * self.steps.x * MESH_MARGIN
    }

    fn specialize(
        _pipeline: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &MeshVertexBufferLayoutRef,
        key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        if key.bind_group_data.inside {
            descriptor.primitive.cull_mode = Some(Face::Front);
            if let Some(depth) = descriptor.depth_stencil.as_mut() {
                depth.depth_compare = CompareFunction::Always;
            }
        }
        if let Some(fragment) = descriptor.fragment.as_mut() {
            if key.bind_group_data.tables {
                fragment.shader_defs.push("TABLES".into());
            }
            if key.bind_group_data.tables && key.bind_group_data.multiple {
                fragment.shader_defs.push("MULTIPLE".into());
            }
        }
        Ok(())
    }
}

impl Shell {
    /// Radius of the shell mesh, in render units.
    fn extent(&self) -> f32 {
        self.radius * self.air.top * MESH_MARGIN
    }

    /// `near` is the camera's near plane, in render units; `switches` jitter
    /// the view steps, end each ray where the air turns opaque and add the
    /// light scattered more than once.
    fn material(
        &self,
        eye: Vec3,
        near: f32,
        steps: (u32, u32),
        switches: [bool; 3],
        tables: Option<Tables>,
    ) -> AtmosphereMaterial {
        let from_eye = self.centre - eye;
        let [jitter, opaque] = [switches[0], switches[1]].map(|on| f32::from(u8::from(on)));
        let multiple = tables
            .as_ref()
            .and_then(|t| t.multiple.clone())
            .filter(|_| switches[2]);
        AtmosphereMaterial {
            centre: from_eye.extend(self.radius),
            steps: Vec4::new(self.air.top, steps.0 as f32, steps.1 as f32, 0.0),
            rayleigh: self.air.rayleigh,
            mie: self.air.mie,
            shape: self.air.shape,
            absorption: self.air.absorption.extend(0.0),
            switches: Vec4::new(jitter, opaque, 0.0, 0.0),
            pole: self.pole.extend(1.0 / (1.0 - self.flattening)),
            lights: self.lights,
            suns: self.suns,
            transmittance: tables.map(|t| t.transmittance),
            thinned: multiple.as_ref().map(|m| m.thinned.clone()),
            multiple: multiple.as_ref().map(|m| m.sky.clone()),
            flow: multiple.map(|m| m.flow),
            inside: from_eye.length() < self.extent() + NEAR_MARGIN * near,
        }
    }
}

pub(super) fn sync_atmospheres(
    mut commands: Commands,
    (frame, meshes): (Res<Frame>, Res<Meshes>),
    (debug, tuning): (Res<super::debug::RenderDebug>, Res<AtmosphereTuning>),
    (mut pool, mut materials): (
        ResMut<Pool<AtmosphereMaterial>>,
        ResMut<Assets<AtmosphereMaterial>>,
    ),
    (mut tables, mut images): (ResMut<AtmosphereTables>, ResMut<Assets<Image>>),
    camera: Query<(SpaceCameraCell, &Projection), With<SpaceCamera>>,
    mut shells: Query<super::bodies::Placement, (With<AtmosphereShell>, Without<SpaceCamera>)>,
) {
    let Ok(((cell, parent, view), projection)) = camera.single() else {
        return;
    };
    let near = match projection {
        Projection::Perspective(p) => p.near,
        _ => 0.0,
    };
    let shown: &[Shell] = if debug.atmospheres {
        &frame.shells
    } else {
        &[]
    };
    grow(
        &mut commands,
        &mut pool,
        &mut materials,
        &meshes.sphere,
        shown.len(),
        (cell, parent),
        AtmosphereShell,
    );
    let eye = view.translation();
    for (index, (entity, material)) in pool.0.iter().enumerate() {
        let Ok((transform, global, shell_cell, visibility)) = shells.get_mut(*entity) else {
            continue;
        };
        let Some(shell) = shown.get(index) else {
            hide(visibility);
            continue;
        };
        place(
            (transform, global, shell_cell, visibility),
            *cell,
            Transform::from_translation(shell.centre).with_scale(Vec3::splat(shell.extent())),
        );
        let changed = shell.material(
            eye,
            near,
            (tuning.view_steps, tuning.light_steps),
            [
                debug.atmosphere_jitter,
                debug.atmosphere_stop_opaque,
                debug.atmosphere_multiple,
            ],
            debug
                .atmosphere_tables
                .then(|| tables.get(&shell.air))
                .flatten(),
        );
        if materials.get(material).is_some_and(|m| *m != changed)
            && let Some(m) = materials.get_mut(material)
        {
            *m = changed;
        }
    }
    tables.update(&mut images);
}
