// A planet's atmosphere: single scattering marched along each pixel's line of
// sight, in units of the planet's radius. The shell mesh only picks pixels;
// the ground and the top of the air are found here. Positions are taken from
// the ray's closest approach to the centre, so the limb holds in f32 from far
// away. Output is premultiplied: scattered light added, what lies behind
// dimmed by the transmittance.
#import bevy_pbr::{forward_io::VertexOutput, mesh_view_bindings::view}
#import night_sky::hdr::encode

struct Air {
    // The planet's centre from the camera, in render units, and its radius.
    centre: vec4<f32>,
    // Top of the air in radii, and steps along the view and towards a star.
    steps: vec4<f32>,
    // Coefficients per radius. Rayleigh scattering and its scale height; Mie
    // scattering and absorption; Mie scale height and asymmetry, then the
    // absorbing layer's height and half width; the layer's absorption.
    rayleigh: vec4<f32>,
    mie: vec4<f32>,
    shape: vec4<f32>,
    absorption: vec4<f32>,
    // Direction to each star, and the exposed colour of its flux.
    lights: array<vec4<f32>, 4>,
    suns: array<vec4<f32>, 4>,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> air: Air;

const PI: f32 = 3.14159265;

struct Medium {
    rayleigh: f32,
    mie: f32,
    extinction: vec3<f32>,
}

fn medium(height: f32) -> Medium {
    let rayleigh = exp(-max(height, 0.0) / air.rayleigh.w);
    let mie = exp(-max(height, 0.0) / air.shape.x);
    let layer = max(0.0, 1.0 - abs(height - air.shape.z) / air.shape.w);
    let extinction = air.rayleigh.rgb * rayleigh + (air.mie.rgb + air.mie.w) * mie
        + air.absorption.rgb * layer;
    return Medium(rayleigh, mie, extinction);
}

fn rayleigh_phase(mu: f32) -> f32 {
    return 3.0 / (16.0 * PI) * (1.0 + mu * mu);
}

fn mie_phase(mu: f32, g: f32) -> f32 {
    let g2 = g * g;
    let denominator = (2.0 + g2) * pow(max(1.0 + g2 - 2.0 * g * mu, 1e-6), 1.5);
    return 3.0 / (8.0 * PI) * (1.0 - g2) * (1.0 + mu * mu) / denominator;
}

// Optical depth from `p` towards a star, or none at all when the planet
// is in the way.
fn towards_star(p: vec3<f32>, towards: vec3<f32>, top: f32) -> vec4<f32> {
    let b = dot(p, towards);
    let r2 = dot(p, p);
    if b < 0.0 && r2 - b * b < 1.0 {
        return vec4(0.0, 0.0, 0.0, 0.0);
    }
    let far = -b + sqrt(max(b * b - r2 + top * top, 0.0));
    let count = air.steps.z;
    let ds = far / count;
    var depth = vec3(0.0);
    for (var i = 0.0; i < count; i += 1.0) {
        let q = p + towards * ((i + 0.5) * ds);
        depth += medium(length(q) - 1.0).extinction * ds;
    }
    return vec4(depth, 1.0);
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let dir = normalize(in.world_position.xyz - view.world_position);
    let eye = -air.centre.xyz / air.centre.w;
    let top = air.steps.x;
    // Distance along the ray to its closest approach, and that point.
    let closest = -dot(eye, dir);
    let nearest = eye + dir * closest;
    let d2 = dot(nearest, nearest);
    if d2 >= top * top {
        return vec4(0.0);
    }
    // The march runs between `start` and `end`, measured from the closest
    // approach: from the camera or where the ray enters the air, to where it
    // leaves or reaches the ground.
    let half = sqrt(top * top - d2);
    let start = max(-half, -closest);
    var end = half;
    // The ground counts only ahead of the camera: a ray climbing away from
    // the planet can have its line cross the ground behind.
    let ground = -sqrt(max(1.0 - d2, 0.0));
    if d2 < 1.0 && ground > start {
        end = min(end, ground);
    }
    if end <= start {
        return vec4(0.0);
    }
    let count = air.steps.y;
    let ds = (end - start) / count;
    var depth = vec3(0.0);
    var light = vec3(0.0);
    for (var i = 0.0; i < count; i += 1.0) {
        let p = nearest + dir * (start + (i + 0.5) * ds);
        let m = medium(length(p) - 1.0);
        let seen = exp(-(depth + 0.5 * m.extinction * ds));
        for (var j = 0u; j < 4u; j++) {
            let sun = air.suns[j].rgb;
            if all(sun == vec3(0.0)) {
                continue;
            }
            let towards = air.lights[j].xyz;
            let path = towards_star(p, towards, top);
            if path.w == 0.0 {
                continue;
            }
            let mu = dot(dir, towards);
            let scattering = air.rayleigh.rgb * m.rayleigh * rayleigh_phase(mu)
                + air.mie.rgb * m.mie * mie_phase(mu, air.shape.y);
            light += sun * exp(-path.xyz) * seen * scattering * ds;
        }
        depth += m.extinction * ds;
    }
    let through = exp(-depth);
    return vec4(encode(light), 1.0 - dot(through, vec3(1.0 / 3.0)));
}
