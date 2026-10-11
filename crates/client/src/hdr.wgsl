#define_import_path night_sky::hdr

// Half floats stop at 65504, but a sunlit planet at a galaxy-friendly
// exposure is about 2^24 display units. Bodies, sprites and coronas store
// light above `KNEE` logarithmically; `exposure.wgsl` decodes it before the
// display curve. Below `KNEE` storage is linear, so bloom, additive points and
// the galaxy glow are unchanged. Keep in step with `exposure.rs`.
const KNEE: f32 = 8192.0;
// Stored units per e-fold above the knee: 2^30 stores as about 31,000.
const SLOPE: f32 = 4.0;

fn encode(light: vec3<f32>) -> vec3<f32> {
    let x = max(light, vec3(0.0));
    let above = KNEE * (1.0 + log(max(x, vec3(KNEE)) / KNEE) / SLOPE);
    return select(x, above, x > vec3(KNEE));
}

fn decode(stored: vec3<f32>) -> vec3<f32> {
    let s = max(stored, vec3(0.0));
    let above = KNEE * exp(min((s / KNEE - 1.0) * SLOPE, vec3(70.0)));
    return select(s, above, s > vec3(KNEE));
}
