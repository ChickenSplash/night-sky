// Analytic line integral through a Gaussian ellipsoid. No galaxy raymarch loop.
struct Settings { values: vec4<f32>, origin: vec4<f32>, right: vec4<f32>, up: vec4<f32>, forward: vec4<f32>, projection: vec4<f32>, light: array<vec4<f32>, 192> }
@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> settings: Settings;
struct Vertex { @location(0) position: vec3<f32>, @location(1) corner: vec2<f32>, @location(2) data: vec4<f32>, @builtin(instance_index) instance_index: u32 }
struct Output { @builtin(position) position: vec4<f32>, @location(0) @interpolate(flat) centre: vec3<f32>, @location(1) @interpolate(flat) data: vec4<f32> }
@vertex fn vertex(v: Vertex) -> Output {
    var out: Output;
    let centre = v.position + settings.origin.xyz;
    // Widen patches to at least half a glow pixel, keeping their light, so a
    // patch smaller than a pixel (the nuclear cluster) does not flicker as it
    // crosses pixel centres.
    let pixel = 2.0 * settings.projection.x / settings.projection.w;
    let blur = 0.5 * pixel * length(centre);
    let widened = sqrt(v.data.xy * v.data.xy + blur * blur);
    let sigma = vec3(widened.x, widened.y, widened.x);
    let forward = settings.forward.xyz;
    let right = settings.right.xyz;
    let up = settings.up.xyz;
    let depth = dot(centre, forward);
    let f = forward * sigma;
    let radius = 3.5 * length(f);
    var clip = vec4(v.corner, 0.000001, 1.0);
    if depth > radius {
        // Exact perspective bounds of the truncated ellipsoid. In particular,
        // an edge-on thin patch does not rasterise a huge circular billboard.
        let q = vec2(dot(centre,right), dot(centre,up));
        let r = right*sigma; let u = up*sigma;
        let aa = depth*depth - 12.25*dot(f,f);
        let bb = q*depth - 12.25*vec2(dot(r,f),dot(u,f));
        let cc = q*q - 12.25*vec2(dot(r,r),dot(u,u));
        let half = sqrt(max(bb*bb-aa*cc,vec2(0.0)))/aa;
        let projection = 1.0 / (settings.projection.x * vec2(settings.projection.y, 1.0));
        clip = vec4((bb/aa + v.corner*half)*projection, 0.000001, 1.0);
    } else if depth < -radius {
        clip = vec4(2.0, 2.0, 0.0, 1.0);
    }
    // Cull whole patches outside any side of the view cone, even when their
    // ellipsoid crosses the eye's plane and needs a full-screen bound.
    let tx = settings.projection.x * settings.projection.y;
    let ty = settings.projection.x;
    for (var side = 0; side < 4; side = side + 1) {
        var normal = forward*tx + right;
        if side == 1 { normal = forward*tx - right; }
        if side == 2 { normal = forward*ty + up; }
        if side == 3 { normal = forward*ty - up; }
        if dot(centre,normal) < -3.5*length(normal*sigma) {
            clip = vec4(2.0,2.0,0.0,1.0);
        }
    }
    out.position = clip; out.centre = centre; out.data = vec4(widened, v.data.zw);
    return out;
}
fn erf(x: f32) -> f32 {
    let t = 1.0 / (1.0 + 0.3275911 * abs(x));
    let value = 1.0 - (((((1.061405429 * t - 1.453152027) * t) + 1.421413741) * t - 0.284496736) * t + 0.254829592) * t * exp(-x*x);
    return sign(x) * value;
}
@fragment fn fragment(in: Output) -> @location(0) vec4<f32> {
    // The glow image covers the whole view at lower resolution.
    let position = in.position.xy;
    let ndc = vec2(2.0*position.x/settings.projection.z-1.0, 1.0-2.0*position.y/settings.projection.w);
    let dir = normalize(settings.forward.xyz + settings.right.xyz * ndc.x * settings.projection.x * settings.projection.y + settings.up.xyz * ndc.y * settings.projection.x);
    let sigma = vec3(in.data.x, in.data.y, in.data.x);
    let d = dir / sigma; let c = in.centre / sigma;
    let a = dot(d,d); let b = dot(d,c);
    // Cross-product form avoids cancelling two huge squares for distant, thin
    // patches close to the ray, especially the compact nuclear cluster.
    let transverse = cross(c, d);
    let perpendicular = dot(transverse, transverse) / a;
    if perpendicular > 12.25 { discard; }
    let integral = exp(-0.5 * perpendicular) * sqrt(1.570796327 / a) * (1.0 + erf(b / sqrt(2.0*a)));
    let density = in.data.z / (15.74960995 * sigma.x * sigma.y * sigma.z);
    // Use the emitting part of a nearby patch, not its centre at zero distance.
    let distance = max(max(b / a, 0.0), inverseSqrt(a));
    let x = clamp((log2(distance) * 0.30103 + 1.0) * (31.0 / 6.0), 0.0, 31.0);
    let i = min(i32(x), 30); let base = i32(in.data.w) * 32 + i;
    let luminosity = mix(settings.light[base].rgb, settings.light[base+1].rgb, x-f32(i));
    // Radiance, so independent of pixel size, at the frame's exposure.
    return vec4(luminosity * density * integral * settings.values.z, 0.0);
}
