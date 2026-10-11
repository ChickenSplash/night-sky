// Point sprites for bodies smaller than a point on screen, and coronas around
// star discs, added over the frame. A point's spread integrates to one per
// square pixel, so its colour carries the body's whole exposed light. A corona's
// colour is its brightness just outside the limb.
#import bevy_pbr::forward_io::VertexOutput

struct Point {
    colour: vec4<f32>,
    // Half side in pixels, core and halo spread in pixels, and the halo's share.
    shape: vec4<f32>,
    // Disc radius in pixels and falloff for a corona; zero for a point.
    corona: vec4<f32>,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> point: Point;

const TAU: f32 = 6.28318530718;

fn gaussian(r2: f32, sigma: f32) -> f32 {
    let s2 = sigma * sigma;
    return exp(-r2 / (2.0 * s2)) / (TAU * s2);
}

// Falls off as a power of distance outside the limb and fades out at the
// sprite's edge. The disc itself hides the part behind it.
fn corona(r: f32) -> f32 {
    let limb = point.corona.x;
    if r < limb {
        return 0.0;
    }
    let edge = 1.0 - smoothstep(0.6, 1.0, r / point.shape.x);
    return pow(limb / r, point.corona.y) * edge;
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let p = (in.uv * 2.0 - 1.0) * point.shape.x;
    let r2 = dot(p, p);
    var spread: f32;
    if point.corona.x > 0.0 {
        spread = corona(sqrt(r2));
    } else {
        spread = mix(gaussian(r2, point.shape.y), gaussian(r2, point.shape.z), point.shape.w);
    }
    // Bevy blends additive materials as premultiplied alpha: zero alpha adds.
    return vec4(point.colour.rgb * spread, 0.0);
}
