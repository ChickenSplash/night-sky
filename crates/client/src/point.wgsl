// Point sprites for bodies smaller than a point on screen, added over the
// frame. The spread integrates to one per square pixel, so the colour carries
// the body's whole exposed light.
#import bevy_pbr::forward_io::VertexOutput

struct Point {
    colour: vec4<f32>,
    // Half side in pixels, core and halo spread in pixels, and the halo's share.
    shape: vec4<f32>,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> point: Point;

const TAU: f32 = 6.28318530718;

fn gaussian(r2: f32, sigma: f32) -> f32 {
    let s2 = sigma * sigma;
    return exp(-r2 / (2.0 * s2)) / (TAU * s2);
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let p = (in.uv * 2.0 - 1.0) * point.shape.x;
    let r2 = dot(p, p);
    let spread = mix(gaussian(r2, point.shape.y), gaussian(r2, point.shape.z), point.shape.w);
    // Bevy blends additive materials as premultiplied alpha: zero alpha adds.
    return vec4(point.colour.rgb * spread, 0.0);
}
