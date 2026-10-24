use super::*;
use crate::render::atmosphere::{AirSettings, Preset, tables::MultipleTables};

fn shell() -> Shell {
    Shell {
        fade: 1.0,
        centre: Vec3::Z * -10.0,
        radius: 1.0,
        pole: Quat::from_rotation_z(0.7) * Vec3::Y,
        flattening: 0.1,
        air: AirSettings::preset(Preset::Earth)
            .atmosphere()
            .scaled(6.371e6),
        lights: [Vec3::X.extend(0.0); MAX_LIGHTS],
        suns: [Vec4::new(2.0, 3.0, 4.0, 0.0); MAX_LIGHTS],
    }
}

fn tables(ready: bool) -> Tables {
    let mut images = Assets::<Image>::default();
    Tables {
        transmittance: images.add(Image::default()),
        multiple: ready.then(|| MultipleTables {
            thinned: images.add(Image::default()),
            sky: images.add(Image::default()),
            flow: images.add(Image::default()),
        }),
    }
}

#[test]
fn surface_waits_for_tables_and_obeys_each_required_switch() {
    let shell = shell();
    let debug = RenderDebug::default();
    for tables in [None, Some(tables(false))] {
        let light = SurfaceLight::new(&shell, &debug, tables);
        assert!(light.table.is_none());
        assert_eq!(light.uniform, SurfaceUniform::default());
    }
    for switch in [
        crate::render::debug::DebugSwitch::Atmospheres,
        crate::render::debug::DebugSwitch::AtmosphereTables,
        crate::render::debug::DebugSwitch::AtmosphereGround,
    ] {
        let mut debug = debug;
        debug.set(switch, false);
        let light = SurfaceLight::new(&shell, &debug, Some(tables(true)));
        assert!(light.table.is_none());
        assert_eq!(light.uniform, SurfaceUniform::default());
    }
}

#[test]
fn ground_uses_the_sky_images_ground_row_and_exposed_flux_without_extra_albedo() {
    let shell = shell();
    // Ground lighting has its own switch, independent of scattering in the sky.
    let debug = RenderDebug {
        atmosphere_multiple: false,
        ..default()
    };
    let tables = tables(true);
    let sky = tables.multiple.as_ref().unwrap().sky.clone();
    let light = SurfaceLight::new(&shell, &debug, Some(tables));
    assert_eq!(light.table, Some(sky));
    assert_eq!(light.uniform.suns, shell.suns);
    assert_eq!(light.uniform.pole.truncate(), shell.pole);
    assert!((light.uniform.pole.w - 1.0 / 0.9).abs() < 1e-6);
}

#[test]
fn normals_recover_the_shells_rounded_radial_direction_on_a_spheroid() {
    let pole = shell().pole;
    let stretch = 1.0 / (1.0 - shell().flattening);
    let scale = |v: Vec3, amount| v + pole * v.dot(pole) * (amount - 1.0);
    for point in [Vec3::X, Vec3::Y, Vec3::Z, Vec3::ONE.normalize(), -pole] {
        // A point on the unit sphere is unrounded to the true spheroid.
        let on_spheroid = scale(point, 1.0 / stretch);
        let normal = scale(point, stretch).normalize();
        let recovered = scale(normal, 1.0 / stretch).normalize();
        let shell_radial = scale(on_spheroid, stretch).normalize();
        assert!(recovered.distance(shell_radial) < 1e-6);
    }
}

#[test]
fn bodies_without_an_atmosphere_do_not_keep_another_planets_lighting() {
    let lighting = SurfaceLighting(vec![SurfaceLight::new(
        &shell(),
        &RenderDebug::default(),
        Some(tables(true)),
    )]);
    assert!(lighting.for_body(Some(0)).table.is_some());
    for atmosphere in [None, Some(1)] {
        let light = lighting.for_body(atmosphere);
        assert!(light.table.is_none());
        assert_eq!(light.uniform, SurfaceUniform::default());
    }
}

#[test]
fn shell_skips_legacy_sunlight_dimming_only_when_surface_lighting_is_ready() {
    let shell = shell();
    for ready in [false, true] {
        let light = SurfaceLight::new(&shell, &RenderDebug::default(), Some(tables(ready)));
        let material = shell.material(
            Vec3::ZERO,
            0.01,
            (16, 8),
            [true, true, true, light.table.is_some()],
            Some(tables(ready)),
        );
        assert_eq!(material.switches.z, if ready { 1.0 } else { 0.0 });
    }
}

#[test]
fn shell_and_surface_use_the_same_point_disc_presentation_fade() {
    let mut shell = shell();
    shell.fade = 0.37;
    let light = SurfaceLight::new(&shell, &RenderDebug::default(), Some(tables(true)));
    let material = shell.material(Vec3::ZERO, 0.01, (16, 8), [true; 4], Some(tables(true)));
    assert_eq!(light.uniform.blend.x, shell.fade);
    assert_eq!(material.switches.w, shell.fade);
}

#[test]
fn the_uploaded_last_row_preserves_ground_radiance_and_does_not_blend_in_the_sky() {
    use crate::render::atmosphere::{
        multiple::{Multiple, SIZE},
        tables::image,
    };
    use bevy::math::DVec3;
    let ground: Vec<DVec3> = (0..SIZE)
        .map(|i| {
            let mu = -1.0 + 2.0 * i as f64 / (SIZE - 1) as f64;
            DVec3::new(mu.max(0.0), 0.03 + mu.max(0.0) * 0.1, 0.001)
        })
        .collect();
    let table = Multiple {
        sky: vec![DVec3::splat(100.0); SIZE * SIZE],
        up: vec![DVec3::ZERO; SIZE * SIZE],
        ground,
    };
    let texels: Vec<_> = table.sky.iter().chain(&table.ground).copied().collect();
    let image = image([SIZE, SIZE + 1], &texels);
    let data = image.data.as_ref().unwrap();
    let at = |x: usize, y: usize| {
        DVec3::from_array(std::array::from_fn(|c| {
            let offset = ((y * SIZE + x) * 4 + c) * 2;
            half::f16::from_le_bytes(data[offset..offset + 2].try_into().unwrap()).to_f64()
        }))
    };
    for mu in [-1.0, -0.33, 0.0, 0.017, 0.42, 0.99, 1.0] {
        // Normalised texture coordinates as body.wgsl uses, converted back
        // to texel coordinates for bilinear sampling of the uploaded image.
        let u = (0.5 + (0.5 + 0.5 * mu) * (SIZE - 1) as f64) / SIZE as f64;
        let v = (SIZE as f64 + 0.5) / (SIZE + 1) as f64;
        let x = u * SIZE as f64 - 0.5;
        let y = v * (SIZE + 1) as f64 - 0.5;
        assert!((y - SIZE as f64).abs() < 1e-12);
        let i = (x as usize).min(SIZE - 2);
        let f = x - i as f64;
        let got = at(i, SIZE) * (1.0 - f) + at(i + 1, SIZE) * f;
        let expected = table.ground_at(mu);
        assert!(
            (got - expected).abs().max_element() < 3e-4,
            "{mu}: {got} {expected}"
        );
    }
}

/// Validate the actual WGSL through Bevy's shader composer without a GPU or
/// desktop window. Only the engine's vertex output and view are stubbed; the
/// project shaders and HDR encoding are the exact embedded source.
#[test]
#[expect(
    clippy::result_large_err,
    reason = "Bevy's ShaderCache callback fixes this error type"
)]
fn surface_and_shell_shaders_validate_with_their_debug_variants() {
    use bevy::{
        render::render_resource::{DownlevelFlags, WgpuFeatures},
        shader::{ShaderCache, ShaderDefVal},
    };
    let mut cache =
        ShaderCache::<(), ()>::new(WgpuFeatures::empty(), DownlevelFlags::all(), |_, _, _| {
            Ok(())
        });
    let mut shaders = Assets::<Shader>::default();
    for (path, source) in [
        (
            "forward_io.wgsl",
            "#define_import_path bevy_pbr::forward_io\nstruct VertexOutput { @builtin(position) position: vec4<f32>, @location(0) world_position: vec4<f32>, @location(1) world_normal: vec3<f32>, @location(5) color: vec4<f32>, }",
        ),
        (
            "view.wgsl",
            "#define_import_path bevy_pbr::mesh_view_bindings\nstruct View { world_position: vec3<f32>, }\n@group(0) @binding(0) var<uniform> view: View;",
        ),
        ("hdr.wgsl", include_str!("../../hdr.wgsl")),
    ] {
        let shader = Shader::from_wgsl(source, path);
        let handle = shaders.add(shader.clone());
        cache.set_shader(handle.id(), shader);
    }
    for (path, source, variants) in [
        (
            "body.wgsl",
            include_str!("../../body.wgsl"),
            vec![vec![], vec!["LEVELS"]],
        ),
        (
            "atmosphere.wgsl",
            include_str!("../atmosphere.wgsl"),
            vec![vec![], vec!["TABLES"], vec!["TABLES", "MULTIPLE"]],
        ),
    ] {
        let shader = Shader::from_wgsl(source, path);
        let handle = shaders.add(shader.clone());
        cache.set_shader(handle.id(), shader);
        for variant in variants {
            let mut defs = vec![ShaderDefVal::UInt("MATERIAL_BIND_GROUP".into(), 3)];
            defs.extend(variant.iter().map(|name| ShaderDefVal::from(*name)));
            let result = cache.get(&(), 0, handle.id(), &defs);
            assert!(result.is_ok(), "{path} {variant:?}: {result:?}");
        }
    }
}
