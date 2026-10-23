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
    // 1 to jitter the view steps; 1 to end each ray where the air turns
    // opaque (with TABLES only).
    switches: vec4<f32>,
    // Direction to each star, and the exposed colour of its flux.
    lights: array<vec4<f32>, 4>,
    suns: array<vec4<f32>, 4>,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> air: Air;
// With TABLES, optical depth to the top of the air (`atmosphere_tables.rs`).
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var transmittance: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var transmittance_sampler: sampler;

const PI: f32 = 3.14159265;
// The table's size, as `atmosphere_tables.rs`.
const TABLE: vec2<f32> = vec2(256.0, 64.0);
// Optical depth beyond which nothing more is seen: e^-10 lets through less
// than 1/20,000 of the light.
const OPAQUE: f32 = 10.0;

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

// A coordinate from 0 to 1 onto the table's texel centres.
fn texel(x: f32, size: f32) -> f32 {
    return 0.5 / size + clamp(x, 0.0, 1.0) * (1.0 - 1.0 / size);
}

// Where the table holds the ray from radius `r` at cosine `mu` from the
// zenith (Bruneton's parametrisation, so the horizon gets the most texels).
fn transmittance_uv(r_in: f32, mu: f32, top: f32) -> vec2<f32> {
    let r = clamp(r_in, 1.0, top);
    let horizon = sqrt(top * top - 1.0);
    let rho = sqrt(max(r * r - 1.0, 0.0));
    let d = max(-r * mu + sqrt(max(r * r * (mu * mu - 1.0) + top * top, 0.0)), 0.0);
    let near = top - r;
    let far = rho + horizon;
    return vec2(
        texel((d - near) / max(far - near, 1e-12), TABLE.x),
        texel(rho / horizon, TABLE.y),
    );
}

// Optical depth from `p` towards a star, or none at all when the planet
// is in the way.
fn towards_star(p: vec3<f32>, towards: vec3<f32>, top: f32) -> vec4<f32> {
    let b = dot(p, towards);
    let r2 = dot(p, p);
    if b < 0.0 && r2 - b * b < 1.0 {
        return vec4(0.0, 0.0, 0.0, 0.0);
    }
#ifdef TABLES
    let r = sqrt(r2);
    let uv = transmittance_uv(r, b / r, top);
    return vec4(textureSampleLevel(transmittance, transmittance_sampler, uv, 0.0).rgb, 1.0);
#else
    let far = -b + sqrt(max(b * b - r2 + top * top, 0.0));
    let count = air.steps.z;
    let ds = far / count;
    var depth = vec3(0.0);
    for (var i = 0.0; i < count; i += 1.0) {
        let q = p + towards * ((i + 0.5) * ds);
        depth += medium(length(q) - 1.0).extinction * ds;
    }
    return vec4(depth, 1.0);
#endif
}

#ifdef TABLES
// Optical depth from `p` along `way` to the top of the air.
fn to_top(p: vec3<f32>, way: vec3<f32>, top: f32) -> vec3<f32> {
    let r = length(p);
    let uv = transmittance_uv(r, dot(p, way) / r, top);
    return textureSampleLevel(transmittance, transmittance_sampler, uv, 0.0).rgb;
}

// Optical depth from `start` to `t` along the ray: the difference of the
// two points' depths to the top of the air, taken forwards, or backwards
// for a ray that reaches the ground, whose backward line never meets it.
// `base` is `start`'s depth that way.
fn depth_to(nearest: vec3<f32>, dir: vec3<f32>, t: f32, base: vec3<f32>, reaches: bool, top: f32) -> f32 {
    let p = nearest + dir * t;
    let d = select(base - to_top(p, dir, top), to_top(p, -dir, top) - base, reaches);
    return min(d.r, min(d.g, d.b));
}

// Where the air between `start` and the point turns opaque in its least
// dimmed channel, found by halving, or `end` if it never does.
fn opaque_at(nearest: vec3<f32>, dir: vec3<f32>, start: f32, end: f32, reaches: bool, top: f32) -> f32 {
    let base = to_top(nearest + dir * start, select(dir, -dir, reaches), top);
    if depth_to(nearest, dir, end, base, reaches, top) < OPAQUE {
        return end;
    }
    var low = start;
    var high = end;
    for (var i = 0; i < 10; i++) {
        let mid = 0.5 * (low + high);
        if depth_to(nearest, dir, mid, base, reaches, top) < OPAQUE {
            low = mid;
        } else {
            high = mid;
        }
    }
    return high;
}
#endif

// Where each view step takes its sample, from 0 (its start) to 1 (its end).
// Neighbouring pixels sample at different points (Jimenez 2014's interleaved
// gradient noise), so sharp changes along the ray, such as the edge of the
// planet's shadow at twilight, blur into fine grain instead of bands.
// Midway without jitter.
fn jitter(pixel: vec2<f32>) -> f32 {
    if air.switches.x == 0.0 {
        return 0.5;
    }
    return fract(52.9829189 * fract(dot(floor(pixel), vec2(0.06711056, 0.00583715))));
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let dir = normalize(in.world_position.xyz - view.world_position);
    let eye = -air.centre.xyz / air.centre.w;
    let top = air.steps.x;
    // Underground nothing is seen, not even the air beyond the far side.
    if dot(eye, eye) < 1.0 {
        return vec4(0.0);
    }
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
    let reaches = d2 < 1.0 && ground > start;
    if reaches {
        end = min(end, ground);
    }
    if end <= start {
        return vec4(0.0);
    }
    // Past where the air turns opaque nothing is seen, ground or not, so the
    // ray ends there. Rays either side of the horizon then end alike in
    // thick air, and every step goes where light can be seen.
    var stopped = false;
#ifdef TABLES
    if air.switches.y != 0.0 {
        let opaque = opaque_at(nearest, dir, start, end, reaches, top);
        stopped = opaque < end;
        end = opaque;
    }
#endif
    // Steps bunch up near a camera low in the air; from the top of the air
    // and above they are even.
    let count = air.steps.y;
    let height = clamp((length(eye) - 1.0) / (top - 1.0), 0.0, 1.0);
    let power = mix(2.0, 1.0, height);
    let span = end - start;
    let offset = jitter(in.position.xy);
    var last = start;
    var depth = vec3(0.0);
    var light = vec3(0.0);
    for (var i = 0.0; i < count && last < end; i += 1.0) {
        let to = min(start + span * pow((i + 1.0) / count, power), end);
        let ds = to - last;
        let p = nearest + dir * mix(last, to, offset);
        last = to;
        let m = medium(length(p) - 1.0);
        // Light scattered across the step, dimmed by the air in front of
        // each point of it (Hillaire 2015), so thick steps keep their light.
        let x = m.extinction * ds;
        let within = select(
            (1.0 - exp(-x)) / max(x, vec3(1e-4)),
            1.0 - 0.5 * x,
            x < vec3(1e-4),
        );
        let seen = exp(-depth) * within * ds;
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
            light += sun * exp(-path.xyz) * seen * scattering;
        }
        depth += x;
    }
    // The ground behind was lit with undimmed starlight, so dim it by the
    // air between it and the stars as well, weighted by each star's share
    // of its light.
    var sunlit = 1.0;
    if reaches && !stopped {
        let surface = normalize(nearest + dir * end);
        var lit = 0.0;
        var total = 0.0;
        for (var j = 0u; j < 4u; j++) {
            let sun = air.suns[j].rgb;
            let towards = air.lights[j].xyz;
            let facing = dot(surface, towards);
            if all(sun == vec3(0.0)) || facing <= 0.0 {
                continue;
            }
            let path = towards_star(surface, towards, top);
            let weight = dot(sun, vec3(1.0 / 3.0)) * facing;
            lit += weight * dot(exp(-path.xyz), vec3(1.0 / 3.0));
            total += weight;
        }
        if total > 0.0 {
            sunlit = lit / total;
        }
    }
    let through = dot(exp(-depth), vec3(1.0 / 3.0)) * sunlit;
    return vec4(encode(light), 1.0 - through);
}
