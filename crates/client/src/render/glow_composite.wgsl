// Scales the low-resolution galaxy glow up over the 3D view. Light above
// white is stored as glare (`hdr.wgsl`).
#import night_sky::hdr::encode
struct Settings { viewport: vec4<f32>, scale: vec4<f32> }
@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> settings: Settings;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var glow: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var glow_sampler: sampler;
struct Vertex { @location(0) position: vec3<f32> }
@vertex fn vertex(v: Vertex) -> @builtin(position) vec4<f32> {
    // At the far plane, so nearer bodies still hide the glow.
    return vec4(v.position.xy, 0.000001, 1.0);
}
@fragment fn fragment(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let uv = (position.xy - settings.viewport.xy) / settings.viewport.zw;
    // Cubic B-spline from four bilinear taps: plain bilinear shows each glow
    // pixel as a diamond around bright, compact features.
    let size = vec2<f32>(textureDimensions(glow));
    let p = uv * size - 0.5;
    let i = floor(p);
    let f = p - i;
    let w0 = (1.0 - f) * (1.0 - f) * (1.0 - f) / 6.0;
    let w1 = (4.0 - 6.0 * f * f + 3.0 * f * f * f) / 6.0;
    let w3 = f * f * f / 6.0;
    let w2 = 1.0 - w0 - w1 - w3;
    let g0 = w0 + w1;
    let g1 = w2 + w3;
    let h0 = (i - 0.5 + w1 / g0) / size;
    let h1 = (i + 1.5 + w3 / g1) / size;
    let colour = g0.y * (g0.x * textureSampleLevel(glow, glow_sampler, h0, 0.0).rgb
            + g1.x * textureSampleLevel(glow, glow_sampler, vec2(h1.x, h0.y), 0.0).rgb)
        + g1.y * (g0.x * textureSampleLevel(glow, glow_sampler, vec2(h0.x, h1.y), 0.0).rgb
            + g1.x * textureSampleLevel(glow, glow_sampler, h1, 0.0).rgb);
    return vec4(encode(colour * settings.scale.x), 0.0);
}
