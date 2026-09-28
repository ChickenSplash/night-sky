// Many galactic stars per mesh. Each point is an oversized triangle expanded
// around its projected centre; the fragment shader cuts out a small Gaussian.
#import bevy_pbr::{
    mesh_functions,
    mesh_view_bindings::view,
    view_transformations::position_world_to_clip,
}

struct Settings { values: vec4<f32> }
@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> settings: Settings;
// x: batch points closer than this times their offset from the mesh origin are
// hidden. This is smaller than the precise sprite's visibility radius, so both
// overlap during the handoff (`starfield.rs`). y: body range in parsecs.
// Both zero for sprites.
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var<uniform> near: vec4<f32>;

struct Vertex {
    @location(0) position: vec3<f32>,
    @location(1) corner: vec2<f32>,
    @location(2) colour: vec4<f32>,
    @builtin(instance_index) instance_index: u32,
}

struct Output {
    @builtin(position) position: vec4<f32>,
    @location(0) corner: vec2<f32>,
    @location(1) colour: vec4<f32>,
    @location(2) distance_pc: f32,
}

@vertex
fn vertex(v: Vertex) -> Output {
    var out: Output;
    let world = mesh_functions::mesh_position_local_to_world(
        mesh_functions::get_world_from_local(v.instance_index), vec4(v.position, 1.0));
    var clip = position_world_to_clip(world.xyz);
    out.distance_pc = distance(world.xyz, view.world_position) * settings.values.x;
    // The far plane is 1e22 render units; reject bodies behind the camera.
    if clip.w <= 0.0 || out.distance_pc < max(near.x * length(v.position), near.y) {
        clip = vec4(2.0, 2.0, 0.0, 1.0);
    }
    let width = max(view.viewport.zw, vec2(1.0, 1.0));
    clip = vec4(clip.xy + v.corner * clip.w * 5.0 / width, clip.zw);
    out.position = clip;
    out.corner = v.corner;
    out.colour = v.colour;
    return out;
}

@fragment
fn fragment(in: Output) -> @location(0) vec4<f32> {
    let r2 = dot(in.corner, in.corner);
    if r2 > 1.0 { discard; }
    // Luminosity over squared distance is linear in apparent flux, so the
    // magnitude cutoff and brightness need no logarithm (`population.rs`).
    let ratio = in.colour.a / max(in.distance_pc * in.distance_pc, 1e-16);
    if ratio < settings.values.y { discard; }
    // One linear apparent-flux scale. Exposure and bloom operate on this HDR
    // colour; in the wireframe view it is tone-mapped by the normal SDR target.
    let intensity = min(20000.0, settings.values.z * ratio);
    return vec4(in.colour.rgb * intensity * exp(-3.0 * r2) * settings.values.w, 0.0);
}
