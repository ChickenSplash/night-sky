// An approximate ray integral of old-disc and bright young spiral-arm light,
// excluding the nearest 25 pc. Thin-plane light is strongest toward the centre.
#import bevy_pbr::{
    mesh_functions,
    forward_io::Vertex,
    view_transformations::position_world_to_clip,
}

struct Glow {
    eye: vec4<f32>,
    disc: vec4<f32>,
    arm: vec4<f32>,
}
@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> glow: Glow;

struct Output {
    @builtin(position) position: vec4<f32>,
    @location(0) direction: vec3<f32>,
}

@vertex
fn vertex(v: Vertex) -> Output {
    var out: Output;
    let world = mesh_functions::mesh_position_local_to_world(
        mesh_functions::get_world_from_local(v.instance_index), vec4(v.position, 1.0));
    out.position = position_world_to_clip(world.xyz);
    out.direction = v.position;
    return out;
}

@fragment
fn fragment(in: Output) -> @location(0) vec4<f32> {
    let dir = normalize(in.direction);
    var light = vec3(0.0);
    // Geometric intervals resolve nearby disc crossings, while still reaching
    // the far side of the galaxy. Each sample represents a path length in pc.
    var near = 25.0;
    for (var i = 0; i < 12; i = i + 1) {
        let far = min(60000.0, near * 1.8 + 45.0);
        let p = glow.eye.xyz + dir * (near + far) * 0.5;
        let radius = length(p.xz);
        if radius < glow.disc.w {
            let disc = exp(-radius / glow.disc.x);
            let old = disc * exp(-abs(p.y) / glow.disc.y);
            let winding = log(max(radius, 1.0) / glow.arm.y) / tan(glow.arm.x);
            let angle = 4.0 * (atan2(p.z, p.x) - winding - glow.arm.z);
            let arm = pow(0.5 * (1.0 + cos(angle)), 4.0);
            let young = disc * arm * exp(-abs(p.y) / glow.disc.z);
            let ds = far - near;
            light += ds * (vec3(0.00000045, 0.00000032, 0.00000023) * old
                + vec3(0.0000014, 0.0000020, 0.0000033) * young);
        }
        near = far;
    }
    return vec4(light * glow.arm.w, 0.0);
}
