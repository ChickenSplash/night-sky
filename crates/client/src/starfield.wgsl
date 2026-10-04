// Many galactic stars per mesh. Each point is an oversized triangle expanded
// around its projected centre; the fragment shader cuts out a small Gaussian.
// Every star has the same one- to two-pixel profile, as in a perfect camera:
// brightness shows in its peak, never its size.
#import bevy_pbr::{
    mesh_functions,
    mesh_view_bindings::view,
    view_transformations::position_world_to_clip,
}

// x: parsecs per render unit, y: faintest visible luminosity per squared
// parsec, z: display light per luminosity per squared parsec, w: one for
// realistic colour and visual brightness from the palette.
struct Settings { values: vec4<f32> }
@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> settings: Settings;
// x: batch points closer than this times their offset from the mesh origin are
// hidden. This is smaller than the precise sprite's visibility radius, so both
// overlap during the handoff (`starfield.rs`). y: body range in parsecs.
// Both zero for sprites.
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var<uniform> near: vec4<f32>;
// Unit-luminance colour, then visual efficiency, log-spaced in temperature
// from 1,000 to 50,000 K (`starfield.rs`).
struct Palette { colours: array<vec4<f32>, 32> }
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var<uniform> palette: Palette;
const PI: f32 = 3.14159265;
// A point's footprint radius and Gaussian sigma, in physical pixels.
const RADIUS_PX: f32 = 2.0;
const SIGMA_PX: f32 = 0.55;
// Point peaks up to this are linear; beyond, they grow logarithmically.
const KNEE: f32 = 1.0;

// Smooth, unbounded compression with a continuous slope at the knee, so faint
// stars stay linear and a bright giant cannot flood its footprint or the bloom.
fn compress(peak: f32) -> f32 {
    if peak <= KNEE { return peak; }
    return KNEE * (1.0 + log(peak / KNEE));
}

struct Vertex {
    @location(0) position: vec3<f32>,
    // Corner index, then temperature in kelvin.
    @location(1) corner: vec2<f32>,
    @location(2) colour: vec4<f32>,
    @builtin(instance_index) instance_index: u32,
}

struct Output {
    @builtin(position) position: vec4<f32>,
    @location(0) corner: vec2<f32>,
    @location(1) colour: vec3<f32>,
    @location(2) peak: f32,
}

@vertex
fn vertex(v: Vertex) -> Output {
    var out: Output;
    let world = mesh_functions::mesh_position_local_to_world(
        mesh_functions::get_world_from_local(v.instance_index), vec4(v.position, 1.0));
    var clip = position_world_to_clip(world.xyz);
    let offset = world.xyz - view.world_position;
    let distance_pc = length(offset) * settings.values.x;
    // Luminosity over squared distance is linear in apparent flux, so the
    // magnitude cutoff and brightness need no logarithm (`population.rs`).
    let ratio = v.colour.a / max(distance_pc * distance_pc, 1e-16);
    // The far plane is 1e22 render units; reject bodies behind the camera.
    if clip.w <= 0.0 || distance_pc < max(near.x * length(v.position), near.y)
        || ratio < settings.values.y {
        clip = vec4(2.0, 2.0, 0.0, 1.0);
    }
    // Linear in apparent flux, up to the knee. Exposure and bloom operate on
    // this HDR colour; in the wireframe view it is tone-mapped by the normal
    // SDR target.
    var colour = v.colour.rgb;
    var light = settings.values.z * ratio;
    if settings.values.w > 0.5 {
        let x = clamp(log(v.corner.y / 1000.0) / log(50.0) * 31.0, 0.0, 31.0);
        let i = min(u32(x), 30u);
        let entry = mix(palette.colours[i], palette.colours[i + 1u], x - f32(i));
        colour = entry.rgb;
        light *= entry.w;
    }
    out.peak = compress(light / (2.0 * PI * SIGMA_PX * SIGMA_PX));
    let width = max(view.viewport.zw, vec2(1.0, 1.0));
    // Clip space spans two units per viewport width.
    var corner = vec2(-1.0, -1.0);
    if v.corner.x > 1.5 { corner = vec2(-1.0, 3.0); } else if v.corner.x > 0.5 { corner = vec2(3.0, -1.0); }
    clip = vec4(clip.xy + corner * clip.w * 2.0 * RADIUS_PX / width, clip.zw);
    out.position = clip;
    out.corner = corner;
    out.colour = colour;
    return out;
}

@fragment
fn fragment(in: Output) -> @location(0) vec4<f32> {
    let r2 = dot(in.corner, in.corner);
    if r2 > 1.0 { discard; }
    let px2 = r2 * RADIUS_PX * RADIUS_PX;
    return vec4(in.colour * in.peak * exp(-px2 / (2.0 * SIGMA_PX * SIGMA_PX)), 0.0);
}
