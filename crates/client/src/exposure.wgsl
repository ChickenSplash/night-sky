// The only display compression for the realistic scene. Runs on the resolved
// HDR image after bloom, before the egui camera, with built-in tone mapping off.
@group(0) @binding(0) var source: texture_2d<f32>;
struct Curve { settings: vec4<f32> }
@group(0) @binding(1) var<uniform> curve: Curve;

@fragment
fn fragment(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let input = textureLoad(source, vec2<i32>(position.xy), 0);
    // Protect the display from half-float overflow in extreme additive crowds.
    let rgb = clamp(input.rgb, vec3(0.0), vec3(60000.0));
    let luminance = dot(rgb, vec3(0.2126, 0.7152, 0.0722));
    if luminance <= 0.0 { return vec4(0.0, 0.0, 0.0, input.a); }
    let amount = clamp(curve.settings.x, 0.0, 1.0);
    // Compress around a fixed middle grey, independent of the camera's view.
    // Zero is linear clipping; one has a gentle, wide logarithmic response.
    let power = exp2(-3.0 * amount);
    let t = pow(luminance / 0.18, power) * (0.18 / 0.82);
    // Scale RGB together for compression, then fit to the display gamut.
    // At zero, each linear channel clips normally, including white highlights.
    let linear = clamp(rgb, vec3(0.0), vec3(1.0));
    let compressed = clamp(rgb * ((t / (1.0 + t)) / luminance), vec3(0.0), vec3(1.0));
    return vec4(mix(linear, compressed, amount), input.a);
}
