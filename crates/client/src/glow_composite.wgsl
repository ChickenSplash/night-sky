// Scales the low-resolution galaxy glow up over the 3D view.
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
    return vec4(textureSampleLevel(glow, glow_sampler, uv, 0.0).rgb * settings.scale.x, 0.0);
}
