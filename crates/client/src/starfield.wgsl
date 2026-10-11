// Galactic stars, one instance each (`starfield_render.rs`). Each is an
// oversized triangle expanded around its projected centre; the fragment
// shader cuts out a small Gaussian. Every star has the same one- to two-pixel
// profile, as in a perfect camera: brightness shows in its peak, never its size.
#import bevy_render::view::View

@group(0) @binding(0) var<uniform> view: View;

struct Globals {
    // x: parsecs per render unit, y: faintest visible luminosity per squared
    // parsec, z: display light per luminosity per squared parsec, w: one for
    // realistic colour and visual brightness from the palette.
    settings: vec4<f32>,
    // x: one to skip rasterising every point, y: footprint radius in physical
    // pixels (debug switches, `render_debug.rs`).
    debug: vec4<f32>,
    // Unit-luminance colour, then visual efficiency, log-spaced in
    // temperature from 1,000 to 50,000 K (`starfield.rs`).
    palette: array<vec4<f32>, 32>,
    // Wireframe colours at the same temperatures, linear.
    wireframe: array<vec4<f32>, 32>,
}
@group(2) @binding(0) var<uniform> globals: Globals;

struct Draw {
    // xyz: the stars' origin in render space, w: render units per parsec.
    origin: vec4<f32>,
    // x: points closer than this times their offset from the origin are
    // hidden. This is smaller than the precise sprite's visibility radius, so
    // both overlap during the handoff (`starfield.rs`). y: body range in
    // parsecs. Both zero for precise sprites.
    near: vec4<f32>,
}
@group(2) @binding(1) var<uniform> draw: Draw;

const PI: f32 = 3.14159265;
// A point's Gaussian sigma in physical pixels. Its footprint radius is debug.y.
const SIGMA_PX: f32 = 0.55;

struct Star {
    // Parsecs from the draw's origin.
    @location(0) position: vec3<f32>,
    @location(1) luminosity: f32,
    @location(2) temperature: f32,
    @builtin(vertex_index) corner: u32,
}

struct Output {
    @builtin(position) position: vec4<f32>,
    @location(0) corner: vec2<f32>,
    @location(1) colour: vec3<f32>,
    @location(2) peak: f32,
}

@vertex
fn vertex(star: Star) -> Output {
    var out: Output;
    let world = draw.origin.xyz + star.position * draw.origin.w;
    var clip = view.clip_from_world * vec4(world, 1.0);
    let distance_pc = length(world - view.world_position) * globals.settings.x;
    // Luminosity over squared distance is linear in apparent flux, so the
    // magnitude cutoff and brightness need no logarithm (`population.rs`).
    let ratio = star.luminosity / max(distance_pc * distance_pc, 1e-16);
    // The far plane is 1e22 render units; reject bodies behind the camera.
    if clip.w <= 0.0 || distance_pc < max(draw.near.x * length(star.position), draw.near.y)
        || ratio < globals.settings.y || globals.debug.x > 0.5 {
        clip = vec4(2.0, 2.0, 0.0, 1.0);
    }
    // Linear in apparent flux, up to the HDR safety cap. Exposure and bloom operate on
    // this HDR colour; in the wireframe view it is tone-mapped by the normal
    // SDR target.
    let x = clamp(log(star.temperature / 1000.0) / log(50.0) * 31.0, 0.0, 31.0);
    let i = min(u32(x), 30u);
    var light = globals.settings.z * ratio;
    var colour: vec3<f32>;
    if globals.settings.w > 0.5 {
        let entry = mix(globals.palette[i], globals.palette[i + 1u], x - f32(i));
        colour = entry.rgb;
        light *= entry.w;
    } else {
        colour = mix(globals.wireframe[i], globals.wireframe[i + 1u], x - f32(i)).rgb;
    }
    let peak = light / (2.0 * PI * SIGMA_PX * SIGMA_PX);
    if globals.settings.w > 0.5 {
        out.peak = min(peak, 20000.0 / max(max(colour.r, colour.g), max(colour.b, 1.0)));
    } else {
        // Preserve the old wireframe point rendering only.
        out.peak = select(1.0 + log(max(peak, 1.0)), peak, peak <= 1.0);
    }
    let width = max(view.viewport.zw, vec2(1.0, 1.0));
    // Clip space spans two units per viewport width.
    var corner = vec2(-1.0, -1.0);
    if star.corner == 1u { corner = vec2(3.0, -1.0); } else if star.corner == 2u { corner = vec2(-1.0, 3.0); }
    clip = vec4(clip.xy + corner * clip.w * 2.0 * globals.debug.y / width, clip.zw);
    out.position = clip;
    out.corner = corner;
    out.colour = colour;
    return out;
}

@fragment
fn fragment(in: Output) -> @location(0) vec4<f32> {
    let r2 = dot(in.corner, in.corner);
    if r2 > 1.0 { discard; }
    let px2 = r2 * globals.debug.y * globals.debug.y;
    return vec4(in.colour * in.peak * exp(-px2 / (2.0 * SIGMA_PX * SIGMA_PX)), 0.0);
}
