// Keep exposed radiance in uniforms until lighting and surface detail have
// been evaluated, then map highlights before half-float storage.
#import bevy_pbr::{forward_io::VertexOutput, mesh_view_bindings::view}
#import night_sky::exposure::highlight

struct Body {
    colour: vec4<f32>,
    // Direction to the star in world axes, and night-side ambient. A negative
    // ambient marks a star, which emits instead of reflecting.
    light: vec4<f32>,
    // Rotation of the surface, as a quaternion.
    spin: vec4<f32>,
    // Granules per radius, on-screen radius, granulation contrast, compression.
    surface: vec4<f32>,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> body: Body;

// Linear limb darkening by channel, stronger in blue, as on the Sun. Each channel
// is scaled so the disc keeps its mean brightness.
const LIMB: vec3<f32> = vec3(0.5, 0.6, 0.75);
// Brightness variation of granules, and of supergranules about 30 times larger.
const GRANULES: f32 = 0.14;
const SUPERGRANULES: f32 = 0.04;
const SUPERGRANULE_SIZE: f32 = 30.0;

fn rotate(q: vec4<f32>, v: vec3<f32>) -> vec3<f32> {
    let t = 2.0 * cross(q.xyz, v);
    return v + q.w * t + cross(q.xyz, t);
}

fn hash(cell: vec3<i32>) -> vec3<f32> {
    let keys = vec3<u32>(1597334673u, 3812015801u, 2798796415u);
    let q = bitcast<vec3<u32>>(cell) * keys;
    let n = (q.x ^ q.y ^ q.z) * keys;
    return vec3<f32>(n >> vec3(8u)) / 16777216.0;
}

// Convection cells: bright centres, dark lanes between them, averaging near zero.
fn cells(p: vec3<f32>) -> f32 {
    let base = vec3<i32>(floor(p));
    let f = fract(p);
    var near = 8.0;
    var next = 8.0;
    for (var z = -1; z <= 1; z++) {
        for (var y = -1; y <= 1; y++) {
            for (var x = -1; x <= 1; x++) {
                let offset = vec3<i32>(x, y, z);
                let d = distance(f, vec3<f32>(offset) + hash(base + offset));
                if d < near {
                    next = near;
                    near = d;
                } else if d < next {
                    next = d;
                }
            }
        }
    }
    return smoothstep(0.0, 0.4, next - near) * 2.0 - 1.4;
}

// A pattern fades out as its cells shrink towards a pixel, so it never aliases.
fn resolved(per_radius: f32) -> f32 {
    return smoothstep(2.0, 6.0, body.surface.y / per_radius);
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let normal = normalize(in.world_normal);
    if body.light.w < 0.0 {
        let eye = normalize(view.world_position - in.world_position.xyz);
        let mu = max(dot(normal, eye), 0.0);
        let limb = (1.0 - LIMB * (1.0 - mu)) / (1.0 - LIMB / 3.0);
        // Granulation fixed to the surface, fading in contrast towards the limb.
        let local = rotate(vec4(-body.spin.xyz, body.spin.w), normal);
        let size = body.surface.x;
        var texture = 0.0;
        let fine = resolved(size);
        if fine > 0.0 {
            texture += GRANULES * fine * cells(local * size);
        }
        let coarse = resolved(size / SUPERGRANULE_SIZE);
        if coarse > 0.0 {
            texture += SUPERGRANULES * coarse * cells(local * size / SUPERGRANULE_SIZE + 17.0);
        }
        let surface = 1.0 + texture * body.surface.z * sqrt(mu);
        return vec4(highlight(body.colour.rgb * limb * surface, body.surface.w), 1.0);
    }
    let lit = max(dot(normal, body.light.xyz), 0.0);
    return vec4(highlight(body.colour.rgb * (lit + body.light.w), body.surface.w), 1.0);
}
