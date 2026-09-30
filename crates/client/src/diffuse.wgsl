// The light of stars too faint to be points, ray-integrated through the
// galaxy model (`galaxy.rs`) and dimmed and reddened by its dust. Each step
// adds each component's stars per cubic parsec times their unresolved light
// per star at that distance (`population::unresolved_light`), so the glow and
// the points never count a star twice. Discs are integrated exactly in height
// along each step, so thin layers never alias; the nucleus is closed-form.
#import bevy_pbr::{
    mesh_functions,
    mesh_view_bindings::view,
    forward_io::Vertex,
    view_transformations::{
        direction_view_to_world, frag_coord_to_ndc, position_ndc_to_view,
        position_world_to_clip,
    },
}

// Distances in the unresolved-light table, as `population::GLOW_DISTANCES`.
const DISTANCES: i32 = 28;
const MAX_STEPS: i32 = 128;
const PI: f32 = 3.14159265;
// Extinction per magnitude of visual extinction, in red, green and blue
// (Cardelli et al. 1989, R_V = 3.1), as optical depth.
const REDDENING = vec3<f32>(0.75, 1.0, 1.32) * 0.921034;

struct Glow {
    // Eye in parsecs from the centre; w: farthest ray length.
    eye: vec4<f32>,
    // Plummer radius and stars.
    nucleus: vec4<f32>,
    // Long, vertical and cross scales, then stars.
    bar: vec4<f32>,
    // Cosine and sine of the bar angle.
    bar_axis: vec4<f32>,
    // Scale length, height, stars and arm strength.
    thin: vec4<f32>,
    arms: vec4<f32>,
    thick: vec4<f32>,
    // Arms, tangent of the pitch, phase and reference radius.
    spiral: vec4<f32>,
    // Inner and outer radii, sharpness and profile mean.
    spiral_extent: vec4<f32>,
    // Core radius, flattening, edge and stars over the profile's volume.
    halo: vec4<f32>,
    // Midplane extinction, scale length, height and arm strength.
    dust: vec4<f32>,
    // Hole radii, central extinction and its radial scale.
    dust_hole: vec4<f32>,
    // Central height, galaxy radius, display light per unit of flux.
    extent: vec4<f32>,
    // Unresolved light per star: component × distance.
    light: array<vec4<f32>, 168>,
}
@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> glow: Glow;

struct Output {
    @builtin(position) position: vec4<f32>,
}

@vertex
fn vertex(v: Vertex) -> Output {
    var out: Output;
    let world = mesh_functions::mesh_position_local_to_world(
        mesh_functions::get_world_from_local(v.instance_index), vec4(v.position, 1.0));
    out.position = position_world_to_clip(world.xyz);
    return out;
}

// Unresolved light per star of a component at a distance.
fn unresolved(component: i32, distance: f32) -> vec3<f32> {
    let x = clamp(log2(max(distance, 1e-3)) * 0.30103 * 4.0 + 4.0, 0.0, f32(DISTANCES - 1));
    let i = min(i32(x), DISTANCES - 2);
    let base = component * DISTANCES + i;
    return mix(glow.light[base].rgb, glow.light[base + 1].rgb, x - f32(i));
}

// The common arm profile at a position. All three disc populations and the
// dust share it, with different strengths; calculate the expensive angle,
// trigonometry and power only once for each sample.
fn spiral_deviation(r: f32, x: f32, z: f32) -> f32 {
    let e = glow.spiral_extent;
    if r <= e.x * 0.7 || r >= e.y { return 0.0; }
    let weight = smoothstep(e.x * 0.7, e.x, r) * (1.0 - smoothstep(e.y * 0.7, e.y, r));
    let winding = log(r / glow.spiral.w) / glow.spiral.y;
    let angle = glow.spiral.x * (atan2(z, x) - winding - glow.spiral.z);
    let profile = pow(max(0.5 * (1.0 + cos(angle)), 1e-9), e.z);
    return weight * (profile / e.w - 1.0);
}

// Mean of exp(-|y| / h) with y linear from y0 to y1.
fn vertical(y0: f32, y1: f32, h: f32) -> f32 {
    let a = abs(y0);
    let b = abs(y1);
    if y0 * y1 < 0.0 {
        return h * (2.0 - exp(-a / h) - exp(-b / h)) / (a + b);
    }
    if abs(b - a) < 1e-3 * h {
        return exp(-0.5 * (a + b) / h);
    }
    return h * (exp(-a / h) - exp(-b / h)) / (b - a);
}

// Stars per cubic parsec of an exponential disc or ring, as `Disc::density`,
// its height averaged along the step.
fn disc(d: vec4<f32>, ring: bool, radius: f32, arm: f32, y0: f32, y1: f32) -> f32 {
    let l = d.x;
    let s = radius / l;
    let surface = select(exp(-s) / (2.0 * PI * l * l), s * s * exp(-s) / (12.0 * PI * l * l), ring);
    return d.z * surface * vertical(y0, y1, d.y) / (2.0 * d.y) * (1.0 + d.w * arm);
}

// Stars per cubic parsec of the bar, as `GalaxyModel::component_density`.
fn bulge(p: vec3<f32>) -> f32 {
    let b = glow.bar;
    let c = glow.bar_axis.x;
    let s = glow.bar_axis.y;
    let along = (p.x * c + p.z * s) / b.x;
    let side = (p.z * c - p.x * s) / b.z;
    let scaled = length(vec3(along, side, p.y / b.y));
    return b.w / (8.0 * PI * b.x * b.y * b.z) * exp(-scaled);
}

fn halo(p: vec3<f32>) -> f32 {
    let h = glow.halo;
    let squared = dot(p.xz, p.xz) + (p.y / h.y) * (p.y / h.y);
    if squared > h.z * h.z { return 0.0; }
    let core = squared + h.x * h.x;
    return h.w / (core * core);
}

// Emission integrated across one interval, and its visual extinction in
// magnitudes. The height profiles are integrated exactly; radial structure is
// sampled within intervals small enough to pass the error check below.
fn slab(eye: vec3<f32>, dir: vec3<f32>, near: f32, far: f32) -> vec4<f32> {
    let ds = far - near;
    let middle = 0.5 * (near + far);
    let y0 = eye.y + dir.y * near;
    let y1 = eye.y + dir.y * far;
    var crossing = middle;
    if y0 * y1 < 0.0 { crossing = clamp(-eye.y / dir.y, near, far); }
    let q = eye + dir * crossing;
    let m = eye + dir * middle;
    let r = length(q.xz);
    var emission = vec3(0.0);
    var dust = 0.0;
    if dot(q, q) < glow.extent.y * glow.extent.y {
        let arm = spiral_deviation(r, q.x, q.z);
        emission += disc(glow.thin, false, r, arm, y0, y1) * unresolved(2, crossing);
        emission += disc(glow.arms, true, r, arm, y0, y1) * unresolved(3, crossing);
        emission += disc(glow.thick, false, r, arm, y0, y1) * unresolved(4, crossing);
        let d = glow.dust;
        let hole = glow.dust_hole;
        dust = d.x * exp((glow.spiral.w - r) / d.y) * vertical(y0, y1, d.z)
            * smoothstep(hole.x, hole.y, r) * (1.0 + d.w * arm);
        let close = min(length((eye + dir * near).xz), length((eye + dir * far).xz));
        if min(r, close) < 1500.0 {
            let parts = clamp(i32(ceil(ds / 50.0)), 1, 32);
            for (var j = 0; j < parts; j = j + 1) {
                let start = near + ds * f32(j) / f32(parts);
                let end = near + ds * f32(j + 1) / f32(parts);
                let centre = eye + dir * 0.5 * (start + end);
                dust += hole.z * exp(-length(centre.xz) / hole.w)
                    * vertical(eye.y + dir.y * start, eye.y + dir.y * end, glow.extent.x)
                    / f32(parts);
            }
        } else {
            dust += hole.z * exp(-r / hole.w) * vertical(y0, y1, glow.extent.x);
        }
    }
    emission += bulge(m) * unresolved(1, middle) + halo(m) * unresolved(5, middle);
    return vec4(emission * ds, dust * ds);
}

// Light from a constant-emissivity slab, attenuated within the slab itself.
fn slab_light(s: vec4<f32>) -> vec3<f32> {
    let optical = s.w * REDDENING;
    let through = select(vec3(1.0), (1.0 - exp(-optical)) / max(optical, vec3(1e-20)),
        optical > vec3(1e-4));
    return s.rgb * through;
}

fn strongest(v: vec3<f32>) -> f32 {
    return max(v.r, max(v.g, v.b));
}

// Test path: compare each interval against its two halves. Subdivide where
// emission or attenuation is changing too fast for one slab, then grow the
// step again in smooth/empty space. No camera-centred shell boundaries have
// authority over the result. Red means the work budget ended prematurely.
fn adaptive(eye: vec3<f32>, dir: vec3<f32>, nucleus: f32, closest: f32) -> vec4<f32> {
    var near = 0.0;
    var step = 4.0;
    let end = glow.eye.w;
    var depth = vec3(0.0);
    var light = vec3(0.0);
    for (var i = 0; i < 768; i = i + 1) {
        if near >= end || min(depth.r, min(depth.g, depth.b)) > 30.0 { break; }
        let far = min(near + step, end);
        let mid = 0.5 * (near + far);
        let whole = slab(eye, dir, near, far);
        let first = slab(eye, dir, near, mid);
        let second = slab(eye, dir, mid, far);
        let fine = slab_light(first) + exp(-first.w * REDDENING) * slab_light(second);
        let coarse = slab_light(whole);
        let difference = strongest(abs(fine - coarse));
        let tolerance = 0.002 * max(strongest(fine), 0.01);
        let dust_difference = abs(first.w + second.w - whole.w);
        let dusty = depth.g < 10.0 && dust_difference > 0.004;
        if step > 1.0 && (difference > tolerance || dusty) {
            step *= 0.5;
            continue;
        }
        light += exp(-depth) * fine;
        if closest >= near && closest < far {
            let t = (closest - near) / (far - near) * 2.0;
            let to_star = first.w * min(t, 1.0) + second.w * max(t - 1.0, 0.0);
            light += exp(-(depth + to_star * REDDENING)) * nucleus
                * unresolved(0, max(closest, glow.nucleus.x));
        }
        depth += (first.w + second.w) * REDDENING;
        near = far;
        if difference < tolerance * 0.125 && dust_difference < 0.0005 {
            step *= 1.8;
        }
    }
    if near < end && min(depth.r, min(depth.g, depth.b)) <= 30.0 {
        return vec4(1.0, 0.0, 0.0, 1.0);
    }
    return vec4(light, 0.0);
}

@fragment
fn fragment(in: Output) -> @location(0) vec4<f32> {
    let debug = i32(glow.extent.w);
    if debug == 1 { return vec4(0.0); }
    // Each pixel's own view ray, from the camera, never interpolated across
    // the sphere's triangles. The sphere is never rotated, so world axes are
    // galactic axes.
    let ndc = frag_coord_to_ndc(in.position).xy;
    let dir = normalize(direction_view_to_world(position_ndc_to_view(vec3(ndc, 1.0))));
    let forward = normalize(direction_view_to_world(vec3(0.0, 0.0, -1.0)));
    // This pixel's own solid angle, smaller away from the centre of view.
    let tan_half = 1.0 / view.clip_from_view[1][1];
    let pixel = 2.0 * tan_half / max(view.viewport.w, 1.0);
    let cosine = max(dot(dir, forward), 0.0);
    let pixel_sr = pixel * pixel * cosine * cosine * cosine;

    let eye = glow.eye.xyz;
    let radius_pc = glow.extent.y;
    // The nucleus in closed form: stars per square parsec along the whole ray,
    // added at its closest approach.
    let closest = -dot(eye, dir);
    let a = glow.nucleus.x;
    let c2 = a * a + max(dot(eye, eye) - closest * closest, 0.0);
    let u = closest / sqrt(c2 + closest * closest);
    let nucleus = glow.nucleus.y * a * a * (2.0 + u * (3.0 - u * u)) / (4.0 * PI * c2 * c2);

    if debug == 6 {
        let result = adaptive(eye, dir, nucleus, closest);
        if result.a > 0.0 {
            return vec4(result.rgb, 0.0);
        }
        return vec4(result.rgb * pixel_sr * glow.extent.z, 0.0);
    }

    var light = vec3(0.0);
    var depth = vec3(0.0);
    // Geometric steps resolve nearby structure and still reach the far side.
    var steps = 64;
    if debug == 4 { steps = 32; }
    if debug == 5 { steps = 128; }
    let ratio = pow(max(glow.eye.w, 2.0), 1.0 / f32(steps));
    var near = 0.0;
    var far = 1.0;
    for (var i = 0; i < MAX_STEPS; i = i + 1) {
        if i >= steps { break; }
        far = far * ratio;
        let ds = far - near;
        let y0 = eye.y + dir.y * near;
        let y1 = eye.y + dir.y * far;
        let middle = 0.5 * (near + far);
        // Disc light and dust are weighed where the step crosses the plane.
        var crossing = middle;
        if y0 * y1 < 0.0 {
            crossing = clamp(-eye.y / dir.y, near, far);
        }
        let q = eye + dir * crossing;
        let m = eye + dir * middle;
        let r = length(q.xz);
        var emission = vec3(0.0);
        var extinction = 0.0;
        if dot(q, q) < radius_pc * radius_pc {
            let arm = spiral_deviation(r, q.x, q.z);
            emission += disc(glow.thin, false, r, arm, y0, y1) * unresolved(2, crossing);
            emission += disc(glow.arms, true, r, arm, y0, y1) * unresolved(3, crossing);
            emission += disc(glow.thick, false, r, arm, y0, y1) * unresolved(4, crossing);
            let d = glow.dust;
            let hole = glow.dust_hole;
            extinction = d.x * exp((glow.spiral.w - r) / d.y) * vertical(y0, y1, d.z)
                * smoothstep(hole.x, hole.y, r) * (1.0 + d.w * arm);
            // The molecular zone is only ~150 pc wide. A galactic-scale
            // step may cross its entire core; subdivide only these central
            // segments rather than treating their midpoint as the column.
            let close = min(length((eye + dir * near).xz), length((eye + dir * far).xz));
            if min(r, close) < 1500.0 {
                let parts = clamp(i32(ceil(ds / 50.0)), 1, 32);
                for (var j = 0; j < parts; j = j + 1) {
                    let start = near + ds * f32(j) / f32(parts);
                    let end = near + ds * f32(j + 1) / f32(parts);
                    let centre = eye + dir * (start + end) * 0.5;
                    extinction += hole.z * exp(-length(centre.xz) / hole.w)
                        * vertical(eye.y + dir.y * start, eye.y + dir.y * end, glow.extent.x)
                        / f32(parts);
                }
            } else {
                extinction += hole.z * exp(-r / hole.w) * vertical(y0, y1, glow.extent.x);
            }
        }
        emission += bulge(m) * unresolved(1, middle) + halo(m) * unresolved(5, middle);
        if debug == 2 { extinction = 0.0; }
        let kappa = extinction * REDDENING;
        let optical = kappa * ds;
        // A uniform, emitting and absorbing step.
        let through = select(vec3(ds), (1.0 - exp(-optical)) / max(kappa, vec3(1e-30)),
            optical > vec3(1e-4));
        let transmitted = exp(-depth);
        light += transmitted * emission * through;
        if closest >= near && closest < far {
            light += transmitted * exp(-0.5 * optical) * nucleus
                * unresolved(0, max(closest, a));
        }
        depth += optical;
        near = far;
        if min(depth.r, min(depth.g, depth.b)) > 30.0 { break; }
    }
    if debug == 3 {
        return vec4(vec3(1.0 - exp(-depth.g)) * 0.3, 0.0);
    }
    let display = light * pixel_sr * glow.extent.z;
    if debug == 2 { return vec4(min(display, vec3(10.0)), 0.0); }
    return vec4(display, 0.0);
}
