struct Settings {
    rects: array<vec4<f32>, 8>,
    info: vec4<f32>,
};
@group(0) @binding(0) var scene: texture_2d<f32>;
@group(0) @binding(1) var<uniform> settings: Settings;

fn sample_scene(pixel: vec2<i32>) -> vec4<f32> {
    let size = vec2<i32>(textureDimensions(scene));
    return textureLoad(scene, clamp(pixel, vec2<i32>(0), size - vec2<i32>(1)), 0);
}

@fragment
fn fragment(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let pixel = vec2<i32>(position.xy);
    var inside = false;
    for (var i = 0u; i < 8u; i += 1u) {
        if f32(i) < settings.info.x {
            let rect = settings.rects[i];
            inside = inside || (all(position.xy >= rect.xy) && all(position.xy < rect.zw));
        }
    }
    if !inside { return sample_scene(pixel); }
    let step = i32(max(round(settings.info.y), 1.0));
    // A small Gaussian-like 3x3 kernel. The window's existing translucent
    // fill remains unchanged; only the scene beneath it is softened.
    var colour = sample_scene(pixel) * 4.0;
    colour += (sample_scene(pixel + vec2<i32>(step, 0)) + sample_scene(pixel - vec2<i32>(step, 0))
        + sample_scene(pixel + vec2<i32>(0, step)) + sample_scene(pixel - vec2<i32>(0, step))) * 2.0;
    colour += sample_scene(pixel + vec2<i32>(step, step)) + sample_scene(pixel + vec2<i32>(step, -step))
        + sample_scene(pixel + vec2<i32>(-step, step)) + sample_scene(pixel - vec2<i32>(step, step));
    return colour / 16.0;
}
