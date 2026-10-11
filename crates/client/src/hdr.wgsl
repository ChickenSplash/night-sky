#define_import_path night_sky::hdr

// Camera glare. Every realistic shader stores light above `KNEE` on a
// logarithmic curve: the display clips at white either way, so only bloom
// sees the difference, and an overexposed source's glow grows with its stops
// over white instead of with its raw light. Bright galactic stars, a few
// stops over, bloom almost as before; a star disc 2^30 over white stores
// about 43, not a billion, so its glow stays compact. Slope one at the knee.
// Keep in step with `exposure.rs`.
const KNEE: f32 = 4.0;
const GLARE: f32 = 2.0;

fn encode(light: vec3<f32>) -> vec3<f32> {
    let x = max(light, vec3(0.0));
    let above = KNEE + GLARE * log(1.0 + (max(x, vec3(KNEE)) - KNEE) / GLARE);
    return select(x, above, x > vec3(KNEE));
}
