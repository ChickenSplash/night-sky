// A planet's atmosphere: single scattering marched along each pixel's line of
// sight, in units of the planet's radius, and with MULTIPLE the light
// scattered more than once read from a table. The shell mesh only picks pixels;
// the ground and the top of the air are found here. Positions are taken from
// the ray's closest approach to the centre, so the limb holds in f32 from far
// away. Output is premultiplied: scattered light added, what lies behind
// dimmed by the transmittance.
//
// A flattened planet is made round first: positions and directions are
// stretched along its pole until its ground is a sphere of its equatorial
// radius, which keeps the ground, the top of the air, the limb and the
// planet's shadow exact and the tables spherical. Heights in the air and
// path lengths are then off by at most the flattening, near the poles.
#import bevy_pbr::{forward_io::VertexOutput, mesh_view_bindings::view}
#import night_sky::hdr::encode

struct Air {
    // The planet's centre from the camera, in render units, and its radius.
    centre: vec4<f32>,
    // Top, view/light steps, and depth of an envelope's reference above its
    // black optical lower boundary (zero for real ground).
    steps: vec4<f32>,
    // Coefficients per radius. Rayleigh scattering and its scale height; Mie
    // scattering and absorption; Mie scale height and asymmetry, then the
    // absorbing layer's height and half width; the layer's absorption.
    rayleigh: vec4<f32>,
    mie: vec4<f32>,
    shape: vec4<f32>,
    absorption: vec4<f32>,
    gas_absorption: vec4<f32>,
    // Stop at opacity, ground includes air lighting, presentation fade, and
    // focus the view steps on the densest visible air.
    switches: vec4<f32>,
    // The pole, and the stretch along it that makes the planet round.
    pole: vec4<f32>,
    // Direction to each star, and the exposed colour of its flux.
    lights: array<vec4<f32>, 4>,
    suns: array<vec4<f32>, 4>,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> air: Air;
// With TABLES, optical depth to the top of the air (`tables.rs`).
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var transmittance: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var transmittance_sampler: sampler;
// With MULTIPLE (`multiple.rs`): the transmittance of the air with
// the haze's forward peak taken as no scattering, for the starlight; the
// mean radiance of light scattered more than once per unit starlight, the
// ground's in its last row; and that light's net upward flow.
@group(#{MATERIAL_BIND_GROUP}) @binding(3) var thinned: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(4) var multiple: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(5) var flow: texture_2d<f32>;

const PI: f32 = 3.14159265;
// The table's size, as `tables.rs`.
const TABLE: vec2<f32> = vec2(256.0, 64.0);
// The multiple scattering table's size, as `multiple.rs`.
const MULTIPLE_SIZE: f32 = 32.0;
// Optical depth beyond which nothing more is seen: e^-10 lets through less
// than 1/20,000 of the light.
const OPAQUE: f32 = 10.0;

// Depth textures store log2(1 + depth), avoiding half-float overflow. Decode
// before interpolation: interpolating logarithms would bias transmission.
fn sample_depth(table: texture_2d<f32>, uv: vec2<f32>) -> vec3<f32> {
    let size = vec2<i32>(textureDimensions(table));
    let p = clamp(uv * vec2<f32>(size) - 0.5, vec2(0.0), vec2<f32>(size - 1));
    let lo = vec2<i32>(floor(p));
    let hi = min(lo + 1, size - 1);
    let f = fract(p);
    let a = exp2(textureLoad(table, lo, 0).rgb) - 1.0;
    let b = exp2(textureLoad(table, vec2(hi.x, lo.y), 0).rgb) - 1.0;
    let c = exp2(textureLoad(table, vec2(lo.x, hi.y), 0).rgb) - 1.0;
    let d = exp2(textureLoad(table, hi, 0).rgb) - 1.0;
    return mix(mix(a, b, f.x), mix(c, d, f.x), f.y);
}

struct Medium {
    rayleigh: f32,
    mie: f32,
    extinction: vec3<f32>,
}

fn medium(height: f32) -> Medium {
    let above = max(height, 0.0);
    let potential_height = above / (1.0 + air.gas_absorption.w * above);
    let rayleigh = exp(-potential_height / air.rayleigh.w);
    let mie = exp(-max(height, 0.0) / air.shape.x);
    let layer = max(0.0, 1.0 - abs(height - air.shape.z) / air.shape.w);
    let extinction = (air.rayleigh.rgb + air.gas_absorption.rgb) * rayleigh + (air.mie.rgb + air.mie.w) * mie
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
// is in the way. With MULTIPLE, `thin` reads the thinned air's: starlight
// scattered slightly forwards counts as starlight, as in the table.
fn towards_star(p: vec3<f32>, towards: vec3<f32>, top: f32, thin: bool) -> vec4<f32> {
    let b = dot(p, towards);
    let r2 = dot(p, p);
    if b < 0.0 && r2 - b * b < 1.0 {
        return vec4(0.0, 0.0, 0.0, 0.0);
    }
#ifdef TABLES
    let r = sqrt(r2);
    let uv = transmittance_uv(r, b / r, top);
#ifdef MULTIPLE
    if thin {
        return vec4(sample_depth(thinned, uv), 1.0);
    }
#endif
    return vec4(sample_depth(transmittance, uv), 1.0);
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

fn least(depth: vec3<f32>) -> f32 {
    return min(depth.r, min(depth.g, depth.b));
}

struct Foreground {
    end: f32,
    depth: vec3<f32>,
    opaque: bool,
    complete: bool,
    // Where the mean depth reaches one, or -1 if it never does.
    seen: f32,
}

// Integrate only positive local extinction, never differences of large table
// depths. Limit radial profile changes and tangent curvature to resolve both
// sides of the limb identically. Independent of table readiness.
fn foreground(nearest: vec3<f32>, dir: vec3<f32>, start: f32, end: f32) -> Foreground {
    // The shortest of the other profiles' heights, which do not vary with
    // radius; none leaves the gas alone to set the steps.
    var fixed = 1e30;
    if any(air.mie.rgb > vec3(0.0)) || air.mie.w > 0.0 {
        fixed = air.shape.x;
    }
    if any(air.absorption.rgb > vec3(0.0)) {
        fixed = min(fixed, air.shape.w);
    }
    let span = end - start;
    var t = 0.0;
    var depth = vec3(0.0);
    var seen = -1.0;
    for (var i = 0; i < 1024 && t < span; i++) {
        let along = start + t;
        let r = length(nearest + dir * along);
        // Spherical gas's scale height grows as the square of the radius, so
        // steps lengthen through the thin air high up instead of staying as
        // fine as at the ground.
        let lift = 1.0 + air.gas_absorption.w * max(r - 1.0, 0.0);
        let scale = min(air.rayleigh.w * lift * lift, fixed);
        let rate = max(abs(along) / r, sqrt(scale / r));
        let local_extinction = medium(r - 1.0).extinction;
        let ds = min(min(span - t, 0.25 * scale / rate), 1.0 / max(least(local_extinction), 1e-30));
        let x = medium(length(nearest + dir * (along + 0.5 * ds)) - 1.0).extinction * ds;
        let before = dot(depth, vec3(1.0 / 3.0));
        let after = dot(depth + x, vec3(1.0 / 3.0));
        if seen < 0.0 && after >= 1.0 {
            seen = t + ds * clamp((1.0 - before) / max(after - before, 1e-30), 0.0, 1.0);
        }
        if least(depth + x) >= OPAQUE {
            var low = 0.0;
            var high = ds;
            for (var j = 0; j < 12; j++) {
                let mid = 0.5 * (low + high);
                let local = medium(length(nearest + dir * (along + 0.5 * mid)) - 1.0).extinction * mid;
                if least(depth + local) < OPAQUE {
                    low = mid;
                } else {
                    high = mid;
                }
            }
            let local = medium(length(nearest + dir * (along + 0.5 * high)) - 1.0).extinction * high;
            return Foreground(t + high, depth + local, true, true, seen);
        }
        depth += x;
        t += ds;
    }
    // If the safety budget is exhausted, do not claim an opaque stop or use
    // an incomplete foreground depth for transmission; retain the view march.
    return Foreground(span, depth, false, t >= span, seen);
}

// Where to bunch the view steps, from the start of the march: the densest
// air along it, or nearer where the mean depth reaches one, since little light
// comes from beyond; and the distance either side over which that air thins
// by e. Its scale height grows as the square of the radius in spherical gas.
fn focus(d2: f32, start: f32, span: f32, seen: f32) -> vec2<f32> {
    var centre = clamp(-start, 0.0, span);
    if seen >= 0.0 {
        centre = min(centre, seen);
    }
    let along = start + centre;
    let r = sqrt(d2 + along * along);
    let rate = abs(along) / r;
    let lift = 1.0 + air.gas_absorption.w * (r - 1.0);
    var scale = air.rayleigh.w * lift * lift;
    if any(air.mie.rgb > vec3(0.0)) || air.mie.w > 0.0 {
        scale = min(scale, air.shape.x);
    }
    var width = 2.0 * scale / (rate + sqrt(rate * rate + 2.0 * scale / r));
    if seen >= 0.0 {
        width = min(width, max(seen, 1e-9));
    }
    return vec2(centre, width);
}

#ifdef MULTIPLE
// Where the multiple scattering table holds radius `r` under a star at
// cosine `mu` from the zenith: rows evenly in 1 - e^(-h / 2H), H the air's
// tallest scale height, below the ground's row.
fn multiple_uv(r: f32, mu: f32, top: f32) -> vec2<f32> {
    let scale = 2.0 * max(air.rayleigh.w, air.shape.x);
    let whole = 1.0 - exp(-(top - 1.0) / scale);
    let height = clamp((1.0 - exp(-max(r - 1.0, 0.0) / scale)) / whole, 0.0, 1.0);
    return vec2(
        texel(0.5 + 0.5 * mu, MULTIPLE_SIZE),
        (0.5 + height * (MULTIPLE_SIZE - 1.0)) / (MULTIPLE_SIZE + 1.0),
    );
}
#endif

// Stretched along the pole, so the planet is round.
fn rounded(v: vec3<f32>) -> vec3<f32> {
    return v + air.pole.xyz * (dot(v, air.pole.xyz) * (air.pole.w - 1.0));
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let view_dir = normalize(in.world_position.xyz - view.world_position);
    let dir = normalize(rounded(view_dir));
    let eye = rounded(-air.centre.xyz / air.centre.w);
    let top = air.steps.x;
    // Underground nothing is seen, not even the air beyond the far side.
    if dot(eye, eye) < 1.0 {
        return vec4(0.0, 0.0, 0.0, select(0.0, air.switches.z, air.steps.w > 0.0));
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
    // Past where the air turns opaque no more scattered light is seen,
    // ground or not, so the march ends there. Rays either side of the
    // horizon then end alike in thick air, and every step goes where light
    // can be seen. What lies behind is still dimmed by the whole ray's air.
    let front = foreground(nearest, dir, start, end);
    var span = end - start;
    if air.switches.x != 0.0 && front.opaque {
        span = front.end;
    }
    // Focused, the steps are even in asinh of the distance from the focus,
    // so they are finest there and grow in proportion to the distance away.
    // Otherwise they bunch up near a camera low in the air, and are even from
    // the top of the air and above.
    let count = air.steps.y;
    let focused = air.switches.w != 0.0;
    let around = focus(d2, start, span, front.seen);
    let low = asinh(-around.x / around.y);
    let high = asinh((span - around.x) / around.y);
    let height = clamp((length(eye) - 1.0) / (top - 1.0), 0.0, 1.0);
    let power = mix(2.0, 1.0, height);
    var last = 0.0;
    var depth = vec3(0.0);
    var light = vec3(0.0);
    for (var i = 0.0; i < count && last < span; i += 1.0) {
        var to = min(span * pow((i + 1.0) / count, power), span);
        // Fixed midpoint sampling, without pixel noise.
        var mid = mix(last, to, 0.5);
        if focused {
            to = clamp(around.x + around.y * sinh(mix(low, high, (i + 1.0) / count)), last, span);
            if i + 1.0 >= count {
                to = span;
            }
            mid = clamp(around.x + around.y * sinh(mix(low, high, (i + 0.5) / count)), last, to);
        }
        let ds = to - last;
        let p = nearest + dir * (start + mid);
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
            let towards = normalize(rounded(air.lights[j].xyz));
#ifdef MULTIPLE
            // Light scattered more than once, the same from every direction,
            // and the haze's forward scattering of its net flow up or down;
            // in the planet's shadow too.
            let r = length(p);
            let uv = multiple_uv(r, dot(p, towards) / r, top);
            let mean = textureSampleLevel(multiple, transmittance_sampler, uv, 0.0).rgb;
            let up = textureSampleLevel(flow, transmittance_sampler, uv, 0.0).rgb;
            let along = -dot(dir, p) / r;
            let more = air.rayleigh.rgb * m.rayleigh * mean
                + air.mie.rgb * m.mie * (mean + 3.0 * air.shape.y / (4.0 * PI) * up * along);
            light += sun * seen * max(more, vec3(0.0));
#endif
            let path = towards_star(p, towards, top, true);
            if path.w == 0.0 {
                continue;
            }
            let mu = dot(view_dir, air.lights[j].xyz);
            let scattering = air.rayleigh.rgb * m.rayleigh * rayleigh_phase(mu)
                + air.mie.rgb * m.mie * mie_phase(mu, air.shape.y);
            light += sun * exp(-path.xyz) * seen * scattering;
        }
        depth += x;
    }
    // Without surface table lighting the ground was lit with undimmed starlight, so dim it by the
    // air between it and the stars as well, weighted by each star's share
    // of its light.
    if front.opaque {
        // Everything beyond this deliberately unresolved tail is hidden. This
        // also prevents bright surface/rim light leaking through e^-10.
        depth = vec3(1e3);
    } else if front.complete {
        depth = front.depth;
    }
    var sunlit = 1.0;
    if reaches && air.switches.y == 0.0 {
        let surface = normalize(nearest + dir * ground);
        var lit = 0.0;
        var total = 0.0;
        for (var j = 0u; j < 4u; j++) {
            let sun = air.suns[j].rgb;
            let towards = normalize(rounded(air.lights[j].xyz));
            let facing = dot(surface, towards);
            if all(sun == vec3(0.0)) || facing <= 0.0 {
                continue;
            }
            let path = towards_star(surface, towards, top, false);
            let weight = dot(sun, vec3(1.0 / 3.0)) * facing;
            lit += weight * dot(exp(-path.xyz), vec3(1.0 / 3.0));
            total += weight;
        }
        if total > 0.0 {
            sunlit = lit / total;
        }
    }
    var through = dot(exp(-depth), vec3(1.0 / 3.0)) * sunlit;
    if air.steps.w > 0.0 {
        // No solid surface: the unresolved deep envelope absorbs the view,
        // including before tables are ready. Ease into darkness before crossing
        // its optical boundary, without restricting free-camera movement.
        let resolved = smoothstep(0.0, min(2.0 * air.rayleigh.w, air.steps.w), length(eye) - 1.0);
        light *= resolved;
        through *= resolved;
        if reaches {
            through = 0.0;
        }
    }
    return vec4(encode(light * air.switches.z), (1.0 - through) * air.switches.z);
}
