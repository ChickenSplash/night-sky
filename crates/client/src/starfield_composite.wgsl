// Adds the splatted star light (`starfield_splat.wgsl`) to the frame.

// Red, green and blue per pixel, row by row over the viewport, in `SCALE`ths.
@group(0) @binding(0) var<storage, read> light: array<u32>;
// xy: viewport origin in the target, zw: its size, in physical pixels.
@group(0) @binding(1) var<uniform> viewport: vec4<u32>;

const SCALE: f32 = 65536.0;

@vertex
fn vertex(@builtin(vertex_index) corner: u32) -> @builtin(position) vec4<f32> {
    // One triangle covering the screen, at the far plane (reverse Z).
    let uv = vec2(f32((corner << 1u) & 2u), f32(corner & 2u));
    return vec4(uv * 2.0 - 1.0, 0.0, 1.0);
}

@fragment
fn fragment(@builtin(position) at: vec4<f32>) -> @location(0) vec4<f32> {
    let size = vec2<i32>(viewport.zw);
    let pixel = vec2<i32>(floor(at.xy)) - vec2<i32>(viewport.xy);
    if any(pixel < vec2(0)) || any(pixel >= size) { discard; }
    let index = 3u * u32(pixel.y * size.x + pixel.x);
    let sum = vec3(f32(light[index]), f32(light[index + 1u]), f32(light[index + 2u]));
    // Additive, as the raster path: zero alpha under premultiplied blending.
    return vec4(sum / SCALE, 0.0);
}
