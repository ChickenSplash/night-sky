// The only display curve for the realistic scene. Runs per pixel on the
// resolved HDR image after bloom, before the egui camera, with built-in tone
// mapping off. Highlights only: light below the knee is never changed.
#import night_sky::hdr::decode

@group(0) @binding(0) var source: texture_2d<f32>;
// x: knee, y: white point in display units, z: shoulder strength
// (`exposure::Shoulder`), w: unused.
struct Curve { settings: vec4<f32> }
@group(0) @binding(1) var<uniform> curve: Curve;

fn shoulder(x: vec3<f32>) -> vec3<f32> {
    let knee = curve.settings.x;
    let strength = curve.settings.z;
    if strength <= 0.0 {
        return clamp(x, vec3(0.0), vec3(1.0));
    }
    // Slope one at the knee, logarithmic above it, white at the white point.
    let y = max(x - knee, vec3(0.0)) / (1.0 - knee);
    let above = knee + (1.0 - knee) * min(log(1.0 + strength * y) / strength, vec3(1.0));
    return select(x, above, x > vec3(knee));
}

@fragment
fn fragment(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let input = textureLoad(source, vec2<i32>(position.xy), 0);
    // Per channel, so zero compression is exactly the old linear clipping and
    // very bright light still turns white rather than staying saturated.
    let mapped = shoulder(decode(input.rgb));
    // Bevy's deband dither, as its tone-mapping pass would apply: wide, gentle
    // gradients such as a compressed corona otherwise show 8-bit rings.
    var dither = vec3(dot(vec2(171.0, 231.0), position.xy));
    dither = (fract(dither / vec3(103.0, 71.0, 97.0)) - 0.5) / 255.0;
    let shown = pow(max(pow(mapped, vec3(1.0 / 2.2)) + dither, vec3(0.0)), vec3(2.2));
    return vec4(shown, input.a);
}
