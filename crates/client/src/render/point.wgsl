// Camera-facing sprites added over the frame (`render::Point`). For a body
// smaller than a point on screen: its light spread by the camera's point
// spread (`PSF_SIGMA_PX`), widened by its disc. Around a disc: the same point
// spread blurring its limb, and for a star its corona. Either way, eye-like
// streaks when far over white. Light above white is stored as glare.
#import bevy_pbr::forward_io::VertexOutput
#import night_sky::hdr::encode

struct Point {
    // A point's whole light per square pixel, or a corona's scale: the disc's
    // brightness times `CORONA_SHARE`.
    colour: vec4<f32>,
    // Half side in pixels, a point's Gaussian sigma in pixels, the disc
    // radius in pixels (zero for a point) and a corona's seed.
    shape: vec4<f32>,
    // The disc's brightness at its limb, and the point spread's sigma in pixels.
    edge: vec4<f32>,
    // Streak colour, and the longest streak's length in pixels.
    streaks: vec4<f32>,
    // xy: a star's pole, or a planet's direction to its star, projected onto
    // the sprite (+y down); z: one for a star, whose whole limb shines.
    axis: vec4<f32>,
    // A disc's outline, an ellipse (`resolve::Outline`): xy its short axis
    // on the sprite, z that axis over the long one.
    outline: vec4<f32>,
    // The outline's centre from the sprite's, in pixels (+y down).
    offset: vec4<f32>,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> point: Point;

const TAU: f32 = 6.28318530718;
const PI: f32 = 3.14159265359;
// Angular cells for the streaks; most stay dark.
const RAYS: f32 = 220.0;

fn hash(n: f32) -> f32 {
    return fract(sin(n * 12.9898 + 78.233) * 43758.5453);
}

// Smooth noise in [0, 1] around a circle of `cells` cells, with an octave
// at three times the cells, so rays never show straight-edged facets.
fn ring_noise(angle: f32, cells: f32, seed: f32) -> f32 {
    return 0.7 * ring_octave(angle, cells, seed) + 0.3 * ring_octave(angle, cells * 3.0, seed + 5.0);
}

fn ring_octave(angle: f32, cells: f32, seed: f32) -> f32 {
    let a = angle / TAU * cells;
    let i = floor(a);
    let f = a - i;
    let s = f * f * f * (f * (f * 6.0 - 15.0) + 10.0);
    let lo = hash((i - cells * floor(i / cells)) + seed * 97.0);
    let j = i + 1.0;
    let hi = hash((j - cells * floor(j / cells)) + seed * 97.0);
    return mix(lo, hi, s);
}

// Complementary error function for x >= 0 (Abramowitz and Stegun 7.1.26).
fn erfc(x: f32) -> f32 {
    let t = 1.0 / (1.0 + 0.3275911 * x);
    let poly = t * (0.254829592 + t * (-0.284496736 + t * (1.421413741
        + t * (-1.453152027 + t * 1.061405429))));
    return poly * exp(-x * x);
}

fn gaussian(r2: f32, sigma: f32) -> f32 {
    let s2 = sigma * sigma;
    return exp(-r2 / (2.0 * s2)) / (TAU * s2);
}

// Wrapped angular distance.
fn apart(a: f32, b: f32) -> f32 {
    let d = a - b;
    return d - TAU * round(d / TAU);
}

// Baumbach's K and F corona, in millionths of the disc, given `x` disc radii
// out: a steep inner corona and a slow outer one. The outer corona gathers
// into helmet streamers near the star's equator, narrowing outwards; fine
// plumes stand at the poles; faint rays run through all of it. Keep the
// bounds in step with `render::CORONA_*_MAX`.
fn corona(x: f32, n: vec2<f32>) -> f32 {
    let seed = point.shape.w;
    let tilt = length(point.axis.xy);
    let pole = select(vec2(0.0, -1.0), point.axis.xy / tilt, tilt > 1e-4);
    // Angle from the pole, the equator at a quarter turn either side.
    let angle = atan2(n.x * pole.y - n.y * pole.x, dot(n, pole));
    // Seen pole-on the streamers ring the whole disc.
    let spread = mix(PI, 0.45, tilt);
    var streamers = 0.25;
    for (var i = 0; i < 4; i++) {
        let k = f32(i) + seed * 13.0;
        let side = select(-1.0, 1.0, i % 2 == 0);
        let centre = side * PI * 0.5 + (hash(k) - 0.5) * 2.0 * spread;
        let width = (0.22 + 0.2 * hash(k + 5.0)) * pow(x, -0.8);
        let d = apart(angle, centre);
        streamers += (0.6 + 1.4 * hash(k + 9.0)) * exp(-d * d / (2.0 * width * width));
    }
    let polar = smoothstep(0.55, 1.0, abs(cos(angle))) * tilt;
    let plumes = 1.0 + 0.6 * polar * (ring_noise(angle, 90.0, seed + 3.0) * 2.0 - 1.0);
    let rays = 1.0 + 0.25 * (ring_noise(angle, 40.0, seed) * 2.0 - 1.0);
    let outer = 0.0532 * pow(x, -2.5) * streamers;
    let inner = 1.425 * pow(x, -7.0) * plumes;
    return (outer + inner + 2.565 * pow(x, -17.0)) * rays;
}

// Eye-like streaks: thin rays of uneven length and strength from `start`,
// the same pattern for every source, fixed to the screen like the eye's.
fn streaks(r: f32, n: vec2<f32>, start: f32) -> f32 {
    let reach_px = point.streaks.w;
    let t = (r - start) / max(reach_px, 1e-4);
    if reach_px <= 0.0 || t < 0.0 || t >= 1.0 {
        return 0.0;
    }
    let a = (atan2(n.y, n.x) / TAU + 0.5) * RAYS;
    let i = floor(a);
    // Rays stay about a pixel wide, so each cell's share of the turn narrows outwards.
    let width = (0.45 + 0.003 * r) / max(r * TAU / RAYS, 1e-4);
    var total = 0.0;
    for (var k = -1; k <= 1; k++) {
        let cell = i + f32(k);
        let key = cell - RAYS * floor(cell / RAYS);
        let strength = pow(hash(key), 4.0);
        let reach = 0.3 + 0.7 * hash(key + 311.0);
        let centre = cell + 0.5 + (hash(key + 619.0) - 0.5) * 0.6;
        let along = clamp(1.0 - t / reach, 0.0, 1.0);
        let d = (a - centre) / width;
        total += strength * along * along * exp(-0.5 * d * d);
    }
    return total * smoothstep(0.0, 0.04, t);
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    var p = (in.uv * 2.0 - 1.0) * point.shape.x - point.offset.xy;
    let limb = point.shape.z;
    // Stretched along its short axis, a disc's outline is a circle of its
    // long radius. The limb's blur and the corona narrow there by the same
    // share.
    if limb > 0.0 && point.outline.z > 0.0 && point.outline.z < 1.0 {
        p += point.outline.xy * dot(p, point.outline.xy) * (1.0 / point.outline.z - 1.0);
    }
    let r = length(p);
    let n = p / max(r, 1e-4);
    var light = vec3(0.0);
    var start = 1.5 * point.shape.y;
    // Streaks leave only the lit part of a planet's limb.
    var shining = 1.0;
    if limb > 0.0 {
        start = limb;
        if point.axis.z < 0.5 {
            shining = smoothstep(-0.1, 0.4, dot(n, point.axis.xy));
        }
        // The disc itself hides everything inside its limb.
        if r >= limb {
            let blur = 0.5 * erfc((r - limb) / (point.edge.w * 1.41421356));
            let lit = select(max(dot(n, point.axis.xy), 0.0), 1.0, point.axis.z > 0.5);
            light += point.edge.rgb * blur * lit;
            if point.axis.z > 0.5 {
                light += point.colour.rgb * corona(r / limb, n);
            }
        }
    } else {
        light += point.colour.rgb * gaussian(r * r, point.shape.y);
    }
    light += point.streaks.rgb * streaks(r, n, start) * shining;
    // Fade out before the sprite's edge, which reaches past all visible light.
    light *= 1.0 - smoothstep(0.6, 1.0, r / point.shape.x);
    // Bevy blends additive materials as premultiplied alpha: zero alpha adds.
    return vec4(encode(light), 0.0);
}
