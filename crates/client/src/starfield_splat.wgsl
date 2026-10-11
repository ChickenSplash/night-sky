// Galactic stars splatted by a compute pass (`starfield_splat.rs`), native
// only. One thread per star adds its Gaussian, sampled at the 3x3 pixels
// around it exactly as the raster path shades them, as fixed-point atomics;
// `starfield_composite.wgsl` adds the sum to the frame. Cutoff, near hiding,
// colour and the knee follow `starfield.wgsl`: keep them in step.
#import bevy_render::view::View

struct Globals {
    settings: vec4<f32>,
    debug: vec4<f32>,
    palette: array<vec4<f32>, 32>,
    assisted: array<vec4<f32>, 32>,
}

struct Draw {
    origin: vec4<f32>,
    // x, y: near hiding as `starfield.wgsl`; z: stars in the draw.
    near: vec4<f32>,
}

@group(0) @binding(0) var<uniform> view: View;
@group(0) @binding(1) var<uniform> globals: Globals;
@group(0) @binding(2) var<uniform> draw: Draw;
// Red, green and blue per pixel, row by row over the viewport, in `SCALE`ths.
@group(0) @binding(3) var<storage, read_write> light: array<atomic<u32>>;
#ifdef MULTISAMPLED
@group(0) @binding(4) var depth: texture_depth_multisampled_2d;
#else
@group(0) @binding(4) var depth: texture_depth_2d;
#endif
// Five floats per star: position in parsecs from the draw's origin,
// luminosity and temperature (`StarInstance`).
@group(1) @binding(0) var<storage, read> stars: array<f32>;

const PI: f32 = 3.14159265;
const SIGMA_PX: f32 = 0.55;
const KNEE: f32 = 1.0;
// Fixed-point steps per unit of display light. Keep in step with the composite.
const SCALE: f32 = 65536.0;
// A single deposit's ceiling (256 units, above any compressed star's peak),
// so a pixel holds 256 of the brightest before its sum could wrap around.
const MOST: f32 = 16777216.0;

fn compress(peak: f32) -> f32 {
    if peak <= KNEE { return peak; }
    return KNEE * (1.0 + log(peak / KNEE));
}

// PCG: dithers rounding, so faint light is not lost to truncation.
fn hash(seed: u32) -> f32 {
    let state = seed * 747796405u + 2891336453u;
    let word = ((state >> ((state >> 28u) + 4u)) ^ state) * 277803737u;
    return f32((word >> 22u) ^ word) / 4294967296.0;
}

fn deposit(pixel: vec2<i32>, size: vec2<i32>, rgb: vec3<f32>, seed: u32) {
    if any(pixel < vec2(0)) || any(pixel >= size) { return; }
    let index = 3u * u32(pixel.y * size.x + pixel.x);
    let dither = hash(seed ^ index);
    let steps = min(rgb * SCALE + dither, vec3(MOST));
    atomicAdd(&light[index], u32(steps.r));
    atomicAdd(&light[index + 1u], u32(steps.g));
    atomicAdd(&light[index + 2u], u32(steps.b));
}

@compute @workgroup_size(256)
fn splat(@builtin(global_invocation_id) id: vec3<u32>) {
    let star = id.x;
    if star >= u32(draw.near.z) || globals.debug.x > 0.5 { return; }
    let base = 5u * star;
    let position = vec3(stars[base], stars[base + 1u], stars[base + 2u]);
    let luminosity = stars[base + 3u];
    let temperature = stars[base + 4u];
    let world = draw.origin.xyz + position * draw.origin.w;
    let clip = view.clip_from_world * vec4(world, 1.0);
    let distance_pc = length(world - view.world_position) * globals.settings.x;
    let ratio = luminosity / max(distance_pc * distance_pc, 1e-16);
    if clip.w <= 0.0 || distance_pc < max(draw.near.x * length(position), draw.near.y)
        || ratio < globals.settings.y {
        return;
    }
    let ndc = clip.xyz / clip.w;
    let size = vec2<i32>(view.viewport.zw);
    // Pixel coordinates, pixel centres at half steps.
    let at = vec2((ndc.x * 0.5 + 0.5) * view.viewport.z, (0.5 - ndc.y * 0.5) * view.viewport.w);
    if any(at < vec2(-1.0)) || any(at > view.viewport.zw + 1.0) { return; }
    // Bodies in front hide stars (reverse Z: nearer is greater).
    let texel = clamp(vec2<i32>(floor(at)), vec2(0), size - 1);
    if ndc.z < textureLoad(depth, texel + vec2<i32>(view.viewport.xy), 0) { return; }
    let x = clamp(log(temperature / 1000.0) / log(50.0) * 31.0, 0.0, 31.0);
    let i = min(u32(x), 30u);
    var shown = globals.settings.z * ratio;
    var colour: vec3<f32>;
    if globals.settings.w > 0.5 {
        let entry = mix(globals.palette[i], globals.palette[i + 1u], x - f32(i));
        colour = entry.rgb;
        shown *= entry.w;
    } else {
        colour = mix(globals.assisted[i], globals.assisted[i + 1u], x - f32(i)).rgb;
    }
    let rgb = colour * compress(shown / (2.0 * PI * SIGMA_PX * SIGMA_PX));
    // The Gaussian at the centres of the nearest pixel and its neighbours.
    // Beyond them it is under 3% of the peak, as at the raster footprint's edge.
    let centre = vec2<i32>(floor(at));
    let offset = at - (floor(at) + 0.5);
    let spread = -1.0 / (2.0 * SIGMA_PX * SIGMA_PX);
    let wx = exp(spread * (vec3(-1.0, 0.0, 1.0) - offset.x) * (vec3(-1.0, 0.0, 1.0) - offset.x));
    let wy = exp(spread * (vec3(-1.0, 0.0, 1.0) - offset.y) * (vec3(-1.0, 0.0, 1.0) - offset.y));
    let seed = star * 9u + bitcast<u32>(draw.origin.x);
    for (var y = 0; y < 3; y++) {
        for (var x = 0; x < 3; x++) {
            deposit(centre + vec2(x - 1, y - 1), size, rgb * wx[x] * wy[y], seed + u32(3 * y + x));
        }
    }
}
