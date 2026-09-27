// Many galactic stars per mesh. Each point is an oversized triangle expanded
// around its projected centre; the fragment shader cuts out a small Gaussian.
#import bevy_pbr::{
    mesh_functions,
    mesh_view_bindings::view,
    view_transformations::position_world_to_clip,
}

struct Settings { values: vec4<f32> }
@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> settings: Settings;

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
    // The far plane is 1e22 render units; reject bodies behind the camera.
    if clip.w <= 0.0 { clip = vec4(2.0, 2.0, 0.0, 1.0); }
    let width = max(view.viewport.zw, vec2(1.0, 1.0));
    clip = vec4(clip.xy + v.corner * clip.w * 5.0 / width, clip.zw);
    out.position = clip;
    out.corner = v.corner;
    out.colour = v.colour;
    out.distance_pc = distance(world.xyz, view.world_position) * settings.values.x;
    return out;
}

@fragment
fn fragment(in: Output) -> @location(0) vec4<f32> {
    let r2 = dot(in.corner, in.corner);
    if r2 > 1.0 { discard; }
    let ratio = in.colour.a / max(in.distance_pc * in.distance_pc, 1e-16);
    let magnitude = 4.83 - 2.5 * log2(max(ratio, 1e-30)) / log2(10.0) - 5.0;
    if magnitude > settings.values.y { discard; }
    // One linear apparent-flux scale. Exposure and bloom operate on this HDR
    // colour; in the wireframe view it is tone-mapped by the normal SDR target.
    let intensity = min(20000.0, settings.values.z * pow(10.0, -0.4 * magnitude));
    return vec4(in.colour.rgb * intensity * exp(-3.0 * r2) * settings.values.w, 0.0);
}
