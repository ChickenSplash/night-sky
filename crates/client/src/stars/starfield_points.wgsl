// Galactic stars as single-pixel points (`stars/points.rs`), native only.
// Each star is one vertex, drawn additively into an image at `SUPERSAMPLE`
// times the view's resolution per axis; `starfield_points_composite.wgsl`
// turns each point into the raster path's Gaussian. Cutoff, near hiding,
// colour and HDR safety cap follow `starfield.wgsl`: keep them in step.
#import bevy_render::view::View

struct Globals {
    settings: vec4<f32>,
    debug: vec4<f32>,
    palette: array<vec4<f32>, 32>,
    wireframe: array<vec4<f32>, 32>,
}

struct Draw {
    origin: vec4<f32>,
    near: vec4<f32>,
}

@group(0) @binding(0) var<uniform> view: View;
@group(0) @binding(1) var<uniform> globals: Globals;
@group(0) @binding(2) var<uniform> draw: Draw;
#ifdef MULTISAMPLED
@group(0) @binding(3) var depth: texture_depth_multisampled_2d;
#else
@group(0) @binding(3) var depth: texture_depth_2d;
#endif

const PI: f32 = 3.14159265;
const SIGMA_PX: f32 = 0.55;

struct Star {
    @location(0) position: vec3<f32>,
    @location(1) luminosity: f32,
    @location(2) shape: vec2<f32>,
}

const AU_PC: f32 = 4.84813681e-6;

// Multiple systems wide enough to show apart are drawn star by star instead.
fn split(star: Star, distance_pc: f32) -> bool {
    return draw.near.z > 0.0 && distance_pc < draw.near.w
        && exp2(star.shape.y) * AU_PC > draw.near.z * distance_pc;
}


struct Output {
    @builtin(position) position: vec4<f32>,
    // The light under the star's Gaussian.
    @location(0) @interpolate(flat) light: vec3<f32>,
}

@vertex
fn vertex(star: Star) -> Output {
    var out: Output;
    let world = draw.origin.xyz + star.position * draw.origin.w;
    var clip = view.clip_from_world * vec4(world, 1.0);
    let distance_pc = length(world - view.world_position) * globals.settings.x;
    let ratio = star.luminosity / max(distance_pc * distance_pc, 1e-16);
    out.position = vec4(2.0, 2.0, 0.0, 1.0);
    if clip.w <= 0.0 || distance_pc < max(draw.near.x * length(star.position), draw.near.y)
        || split(star, distance_pc) || ratio < globals.settings.y || globals.debug.x > 0.5 {
        return out;
    }
    let ndc = clip.xyz / clip.w;
    let at = vec2((ndc.x * 0.5 + 0.5) * view.viewport.z, (0.5 - ndc.y * 0.5) * view.viewport.w);
    if any(at < vec2(0.0)) || any(at >= view.viewport.zw) { return out; }
    // Bodies in front hide stars (reverse Z: nearer is greater).
    let texel = vec2<i32>(floor(at)) + vec2<i32>(view.viewport.xy);
    if ndc.z < textureLoad(depth, texel, 0) { return out; }
    let x = clamp(log(star.shape.x / 1000.0) / log(50.0) * 31.0, 0.0, 31.0);
    let i = min(u32(x), 30u);
    var shown = globals.settings.z * ratio;
    var colour: vec3<f32>;
    if globals.settings.w > 0.5 {
        let entry = mix(globals.palette[i], globals.palette[i + 1u], x - f32(i));
        colour = entry.rgb;
        shown *= entry.w;
    } else {
        colour = mix(globals.wireframe[i], globals.wireframe[i + 1u], x - f32(i)).rgb;
    }
    let area = 2.0 * PI * SIGMA_PX * SIGMA_PX;
    let peak = shown / area;
    var bounded = min(peak, 20000.0 / max(max(colour.r, colour.g), max(colour.b, 1.0)));
    if globals.settings.w < 0.5 {
        bounded = select(1.0 + log(max(peak, 1.0)), peak, peak <= 1.0);
    }
    out.light = colour * bounded * area;
    out.position = clip;
    return out;
}

@fragment
fn fragment(in: Output) -> @location(0) vec4<f32> {
    return vec4(in.light, 0.0);
}
