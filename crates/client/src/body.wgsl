// Realistic bodies under the wireframes. Colours arrive already exposed, so the
// HDR target never sees raw radiance.
#import bevy_pbr::{forward_io::VertexOutput, mesh_view_bindings::view}

struct Body {
    colour: vec4<f32>,
    // Direction to the star in world axes, and night-side ambient. A negative
    // ambient marks a star, which emits instead of reflecting.
    light: vec4<f32>,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> body: Body;

// Linear limb darkening, scaled so the disc keeps its mean brightness.
const LIMB: f32 = 0.6;

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let normal = normalize(in.world_normal);
    if body.light.w < 0.0 {
        let eye = normalize(view.world_position - in.world_position.xyz);
        let mu = max(dot(normal, eye), 0.0);
        let limb = (1.0 - LIMB * (1.0 - mu)) / (1.0 - LIMB / 3.0);
        return vec4(body.colour.rgb * limb, 1.0);
    }
    let lit = max(dot(normal, body.light.xyz), 0.0);
    return vec4(body.colour.rgb * (lit + body.light.w), 1.0);
}
