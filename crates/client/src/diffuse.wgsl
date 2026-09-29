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
const STEPS: i32 = 64;
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

// As `Spiral::modulation`.
fn modulation(strength: f32, x: f32, z: f32) -> f32 {
    let r = length(vec2(x, z));
    if strength <= 0.0 || r <= 0.0 { return 1.0; }
    let e = glow.spiral_extent;
    let weight = smoothstep(e.x * 0.7, e.x, r) * (1.0 - smoothstep(e.y * 0.7, e.y, r));
    let winding = log(r / glow.spiral.w) / glow.spiral.y;
    let angle = glow.spiral.x * (atan2(z, x) - winding - glow.spiral.z);
    let profile = pow(max(0.5 * (1.0 + cos(angle)), 1e-9), e.z);
    return 1.0 + strength / e.w * weight * (profile - e.w);
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
fn disc(d: vec4<f32>, ring: bool, radius: f32, x: f32, z: f32, y0: f32, y1: f32) -> f32 {
    let l = d.x;
    let s = radius / l;
    let surface = select(exp(-s) / (2.0 * PI * l * l), s * s * exp(-s) / (12.0 * PI * l * l), ring);
    return d.z * surface * vertical(y0, y1, d.y) / (2.0 * d.y) * modulation(d.w, x, z);
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

@fragment
fn fragment(in: Output) -> @location(0) vec4<f32> {
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

    var light = vec3(0.0);
    var depth = vec3(0.0);
    // Geometric steps resolve nearby structure and still reach the far side.
    let ratio = pow(max(glow.eye.w, 2.0), 1.0 / f32(STEPS));
    var near = 0.0;
    var far = 1.0;
    for (var i = 0; i < STEPS; i = i + 1) {
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
            emission += disc(glow.thin, false, r, q.x, q.z, y0, y1) * unresolved(2, crossing);
            emission += disc(glow.arms, true, r, q.x, q.z, y0, y1) * unresolved(3, crossing);
            emission += disc(glow.thick, false, r, q.x, q.z, y0, y1) * unresolved(4, crossing);
            let d = glow.dust;
            let hole = glow.dust_hole;
            extinction = d.x * exp((glow.spiral.w - r) / d.y) * vertical(y0, y1, d.z)
                * smoothstep(hole.x, hole.y, r) * modulation(d.w, q.x, q.z);
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
    return vec4(light * pixel_sr * glow.extent.z, 0.0);
}
