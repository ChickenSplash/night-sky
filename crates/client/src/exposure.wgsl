// The display pass for the realistic scene. Runs per pixel on the resolved HDR
// image after bloom, before the egui camera, with built-in tone mapping off.
// No curve: bodies are balanced on the CPU (`render::resolve`), so this only
// decodes stored headroom, clips at white and dithers.
#import night_sky::hdr::decode

@group(0) @binding(0) var source: texture_2d<f32>;

@fragment
fn fragment(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let input = textureLoad(source, vec2<i32>(position.xy), 0);
    // Per channel, so very bright light turns white rather than staying saturated.
    let mapped = clamp(decode(input.rgb), vec3(0.0), vec3(1.0));
    // Bevy's deband dither, as its tone-mapping pass would apply: wide, gentle
    // gradients such as a corona otherwise show 8-bit rings.
    var dither = vec3(dot(vec2(171.0, 231.0), position.xy));
    dither = (fract(dither / vec3(103.0, 71.0, 97.0)) - 0.5) / 255.0;
    let shown = pow(max(pow(mapped, vec3(1.0 / 2.2)) + dither, vec3(0.0)), vec3(2.2));
    return vec4(shown, input.a);
}
