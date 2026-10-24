// Realistic bodies under the wireframes. Colours arrive already exposed, in
// f32 uniforms; light above half-float range is stored encoded (`hdr.wgsl`).
#import bevy_pbr::{forward_io::VertexOutput, mesh_view_bindings::view}
#import night_sky::hdr::encode

struct SurfaceAir {
    // World pole and equatorial/polar stretch; zero means bare lighting.
    pole: vec4<f32>,
    // Exposed stellar flux. The table already includes surface albedo / pi.
    suns: array<vec4<f32>, 4>,
}

struct Body {
    colour: vec4<f32>,
    // Direction to the star in world axes, and night-side ambient. A negative
    // ambient marks a star, which emits instead of reflecting.
    light: vec4<f32>,
    // Rotation of the surface, as a quaternion.
    spin: vec4<f32>,
    // Granules per radius, on-screen radius in pixels and granulation contrast.
    surface: vec4<f32>,
    // A planet's direction to each star lighting it, brightest first, and the
    // exposed colour each gives a surface facing it; zero when unused.
    lights: array<vec4<f32>, 4>,
    tints: array<vec4<f32>, 4>,
    air: SurfaceAir,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> body: Body;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var air_ground: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(3) var air_sampler: sampler;

// The last row of the 32 x 33 sky image is reflected ground radiance,
// including both thinned direct sunlight and diffuse skylight, per unit flux.
// Recover the rounded planet's radial direction from its true normal, without
// subtracting large world positions near the surface. The shell uses the same
// pole stretch for its star directions and spherical atmosphere tables.
fn atmospheric_ground(normal: vec3<f32>) -> vec3<f32> {
    let pole = body.air.pole.xyz;
    let stretch = body.air.pole.w;
    let radial = normalize(normal + pole * dot(normal, pole) * (1.0 / stretch - 1.0));
    let size = vec2<f32>(textureDimensions(air_ground));
    var light = vec3(0.0);
    for (var i = 0u; i < 4u; i++) {
        if all(body.air.suns[i].rgb == vec3(0.0)) {
            continue;
        }
        let towards = body.lights[i].xyz;
        let rounded = normalize(towards + pole * dot(towards, pole) * (stretch - 1.0));
        let mu = clamp(dot(radial, rounded), -1.0, 1.0);
        let uv = vec2((0.5 + (0.5 + 0.5 * mu) * (size.x - 1.0)) / size.x,
            (size.y - 0.5) / size.y);
        light += body.air.suns[i].rgb * textureSampleLevel(air_ground, air_sampler, uv, 0.0).rgb;
    }
    return light;
}

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
        return vec4(encode(body.colour.rgb * limb * surface), 1.0);
    }
    // Every star lights the side facing it in its own colour: a binary's
    // planet has a second, fainter day side towards the far star.
    var shade = body.colour.rgb * body.light.w;
    if body.air.pole.w > 0.0 {
        shade = atmospheric_ground(normal);
    } else {
        for (var i = 0u; i < 4u; i++) {
            shade += body.tints[i].rgb * max(dot(normal, body.lights[i].xyz), 0.0);
        }
    }
#ifdef LEVELS
    // Ground chunks tinted by level (`ground.wgsl`), faintly on the night side.
    let day = body.tints[0].rgb;
    let lit = max(max(shade.r, shade.g), shade.b) + 0.1 * max(max(day.r, day.g), day.b);
    shade = in.color.rgb * lit;
#endif
    return vec4(encode(shade), 1.0);
}
