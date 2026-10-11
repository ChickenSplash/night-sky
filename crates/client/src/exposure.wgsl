#define_import_path night_sky::exposure

// One highlight shoulder shared by all realistic light paths. Do this AFTER
// lighting/phase/surface detail, but BEFORE storing fragments in half floats.
// No histogram, spatial adaptation, per-object key or shadow lift.
fn highlight(rgb: vec3<f32>, compression: f32) -> vec3<f32> {
    let light = max(rgb, vec3(0.0));
    let peak = max(max(light.r, light.g), light.b);
    if peak <= 0.0 { return vec3(0.0); }
    let amount = clamp(compression, 0.0, 1.0);
    if amount <= 0.0 {
        // Deliberately overexposed at zero compression. Bound only the GPU
        // output, never the CPU radiance before lighting is evaluated.
        return light * min(1.0, 20000.0 / peak);
    }
    let knee = 1.0 - 0.82 * amount;
    if peak <= knee { return light; }
    let room = 1.0 - knee;
    let range = exp2(5.0 * amount);
    let excess = (peak - knee) * range / room;
    // Stable for the tiny positive excess near the knee as well as for huge
    // exposed radiances. The derivative at the knee is one.
    let log_excess = select(log(1.0 + excess), excess, excess < 0.0001);
    let mapped = knee + room * (1.0 - exp(-log_excess / range));
    // Peak-based scaling preserves colour without a second per-channel clip.
    return light * (mapped / peak);
}
