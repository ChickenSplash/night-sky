// Adds the star points (`starfield_points.wgsl`) to the frame. Each output
// pixel sums the point light of the 6x6 supersampled pixels covering it and
// its neighbours, weighted by the raster path's 0.55 px Gaussian at their
// centres, so a point lands within a quarter pixel of its star. Light above
// white is stored as glare (`hdr.wgsl`); in the SDR wireframe view it clips
// at white either way.
#import night_sky::hdr::encode

@group(0) @binding(0) var points: texture_2d<f32>;
// xy: viewport origin in the target, zw: its size, in physical pixels.
@group(0) @binding(1) var<uniform> viewport: vec4<u32>;

const PI: f32 = 3.14159265;
const SIGMA_PX: f32 = 0.55;
// Supersampled pixels per output pixel, per axis. Keep in step with `SUPERSAMPLE`.
const SUPERSAMPLE: i32 = 2;

@vertex
fn vertex(@builtin(vertex_index) corner: u32) -> @builtin(position) vec4<f32> {
    // One triangle covering the screen, at the far plane (reverse Z).
    let uv = vec2(f32((corner << 1u) & 2u), f32(corner & 2u));
    return vec4(uv * 2.0 - 1.0, 0.0, 1.0);
}

@fragment
fn fragment(@builtin(position) at: vec4<f32>) -> @location(0) vec4<f32> {
    let pixel = vec2<i32>(floor(at.xy)) - vec2<i32>(viewport.xy);
    let size = vec2<i32>(viewport.zw) * SUPERSAMPLE;
    let first = (pixel - 1) * SUPERSAMPLE;
    let span = 3 * SUPERSAMPLE;
    let spread = -1.0 / (2.0 * SIGMA_PX * SIGMA_PX);
    var total = vec3(0.0);
    for (var y = 0; y < span; y++) {
        // Offset of the supersampled pixel's centre from this pixel's, in pixels.
        let dy = (f32(y) + 0.5) / f32(SUPERSAMPLE) - 1.5;
        let sy = first.y + y;
        if sy < 0 || sy >= size.y { continue; }
        for (var x = 0; x < span; x++) {
            let sx = first.x + x;
            if sx < 0 || sx >= size.x { continue; }
            let dx = (f32(x) + 0.5) / f32(SUPERSAMPLE) - 1.5;
            total += textureLoad(points, vec2(sx, sy), 0).rgb * exp(spread * (dx * dx + dy * dy));
        }
    }
    // Light over the Gaussian's area gives its peak. Additive, as the raster
    // path: zero alpha under premultiplied blending.
    return vec4(encode(total / (2.0 * PI * SIGMA_PX * SIGMA_PX)), 0.0);
}
