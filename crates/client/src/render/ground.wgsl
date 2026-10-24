// Ground chunks (`ground.rs`), shaded by `body.wgsl`. Each vertex blends
// towards its parent level's surface as the distance where the parent takes
// over nears, so chunks neither pop as they split nor crack against coarser
// neighbours: at that distance a chunk is its parent's shape exactly.
#import bevy_pbr::{
    forward_io::VertexOutput,
    mesh_functions,
    mesh_view_bindings::view,
    view_transformations::position_world_to_clip,
}

struct Vertex {
    @builtin(instance_index) instance_index: u32,
    // Metres from the chunk's centre, in the planet's axes.
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    // The offset to the parent level's surface, and the parent level's
    // error in metres; zero for a root chunk.
    @location(2) morph: vec4<f32>,
}

// Metres per render unit, where blending starts as a share of the parent's
// distance, distance per metre of error, and a root chunk's error.
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var<uniform> ground: vec4<f32>;

#ifdef LEVELS
fn level_tint(level: u32) -> vec3<f32> {
    var tints = array<vec3<f32>, 6>(
        vec3(1.0, 0.35, 0.3),
        vec3(1.0, 0.8, 0.3),
        vec3(0.4, 1.0, 0.4),
        vec3(0.3, 0.9, 1.0),
        vec3(0.45, 0.5, 1.0),
        vec3(1.0, 0.45, 1.0),
    );
    return tints[level % 6u];
}
#endif

@vertex
fn vertex(v: Vertex) -> VertexOutput {
    let world_from_local = mesh_functions::get_world_from_local(v.instance_index);
    // Measured from the parent's surface, which both sides of a level's
    // boundary share.
    let parent = mesh_functions::mesh_position_local_to_world(
        world_from_local,
        vec4(v.position + v.morph.xyz, 1.0),
    );
    let takes_over = v.morph.w * ground.z;
    let metres = distance(parent.xyz, view.world_position) * ground.x;
    var blend = 0.0;
    if takes_over > 0.0 {
        blend = smoothstep(ground.y * takes_over, takes_over, metres);
    }
    var out: VertexOutput;
    out.world_position = mesh_functions::mesh_position_local_to_world(
        world_from_local,
        vec4(v.position + v.morph.xyz * blend, 1.0),
    );
    out.position = position_world_to_clip(out.world_position.xyz);
    out.world_normal = mesh_functions::mesh_normal_local_to_world(v.normal, v.instance_index);
#ifdef VERTEX_OUTPUT_INSTANCE_INDEX
    out.instance_index = v.instance_index;
#endif
#ifdef LEVELS
    // Errors shrink fourfold per level.
    var level = 0u;
    if v.morph.w > 0.0 {
        level = u32(round(log2(ground.w / v.morph.w) / 2.0)) + 1u;
    }
    out.color = vec4(level_tint(level), 1.0);
#endif
    return out;
}
