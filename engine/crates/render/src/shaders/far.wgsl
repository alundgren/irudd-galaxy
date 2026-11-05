// Distant things behind the play layer: fields of marks drawn per pixel from a hashed
// grid, and landmark shapes (a lit disc, a cloudy wash). See `far.rs`.

struct FarGlobals {
    // xy: the logical viewport, zw: the world raster in pixels.
    size: vec4<f32>,
    // xy: where the camera's target is on screen, logical pixels.
    centre: vec4<f32>,
    // Plane units per logical pixel: (x per px.x, y per px.x, x per px.y, y per px.y).
    inv: vec4<f32>,
    // Logical pixels per plane unit: (px.x per x, px.y per x, px.x per y, px.y per y).
    fwd: vec4<f32>,
    // The sun in view space: x right, y up, z towards the eye.
    sun: vec4<f32>,
};

struct Far {
    // Field: xy offset (world units), z cell (world units), w chance.
    // Shape: xy centre (logical px), z radius (logical px), w stretch.
    a: vec4<f32>,
    // Colour; alpha is peak opacity.
    b: vec4<f32>,
    // Field: x, y size (logical px), z angle (radians), w mark (0 dust, 1 speck, 2 hatch).
    // Shape: x angle, y outline (0 disc, 1 wash), z ink rim (logical px), w hatch.
    c: vec4<f32>,
    // The haze colour; w how far.
    d: vec4<f32>,
    // Field: x seed, y the largest mark (half a cell, logical px).
    // Shape: rgb shadow, w seed.
    e: vec4<f32>,
    // Shape: ink.
    f: vec4<f32>,
};

@group(0) @binding(0) var<uniform> fg: FarGlobals;
@group(0) @binding(1) var<storage, read> items: array<Far>;

fn pcg(v: u32) -> u32 {
    let state = v * 747796405u + 2891336453u;
    let word = ((state >> ((state >> 28u) + 4u)) ^ state) * 277803737u;
    return (word >> 22u) ^ word;
}

// Uniform in [0, 1) for grid cell `k`, purpose `n` and seed `seed`.
fn rnd(k: vec2<i32>, seed: u32, n: u32) -> f32 {
    let h = pcg(bitcast<u32>(k.x) ^ pcg(bitcast<u32>(k.y) ^ pcg(seed ^ (n * 0x9e3779b9u))));
    return f32(h >> 8u) / 16777216.0;
}

fn to_plane(d: vec2<f32>) -> vec2<f32> {
    return vec2<f32>(fg.inv.x * d.x + fg.inv.z * d.y, fg.inv.y * d.x + fg.inv.w * d.y);
}

fn to_px(d: vec2<f32>) -> vec2<f32> {
    return vec2<f32>(fg.fwd.x * d.x + fg.fwd.z * d.y, fg.fwd.y * d.x + fg.fwd.w * d.y);
}

fn logical(frag: vec2<f32>) -> vec2<f32> {
    return frag * fg.size.xy / fg.size.zw;
}

fn clip_of(px: vec2<f32>) -> vec4<f32> {
    return vec4<f32>(px.x / fg.size.x * 2.0 - 1.0, 1.0 - px.y / fg.size.y * 2.0, 0.0, 1.0);
}

struct FOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) @interpolate(flat) ii: u32,
};

@vertex
fn vs_field(@builtin(vertex_index) vi: u32, @builtin(instance_index) ii: u32) -> FOut {
    let uv = vec2<f32>(f32((vi << 1u) & 2u), f32(vi & 2u));
    var o: FOut;
    o.clip = vec4<f32>(uv * 2.0 - 1.0, 0.0, 1.0);
    o.ii = ii;
    return o;
}

@fragment
fn fs_field(in: FOut) -> @location(0) vec4<f32> {
    let it = items[in.ii];
    let px = logical(in.clip.xy);
    let u = it.a.xy + to_plane(px - fg.centre.xy);
    let cell = it.a.z;
    let c = u / cell;
    // A mark is at most half a cell across, so only the four cells nearest the pixel
    // can reach it.
    let base = vec2<i32>(floor(c - 0.5));
    let seed = u32(it.e.x);
    let mark = u32(it.c.w + 0.5);
    let dir = vec2<f32>(cos(it.c.z), -sin(it.c.z));
    let across = vec2<f32>(-dir.y, dir.x);
    var cover = 0.0;
    for (var j = 0; j < 2; j++) {
        for (var i = 0; i < 2; i++) {
            let k = base + vec2<i32>(i, j);
            if rnd(k, seed, 0u) >= it.a.w {
                continue;
            }
            let p = (vec2<f32>(k) + vec2<f32>(rnd(k, seed, 1u), rnd(k, seed, 2u))) * cell;
            let d = to_px(p - u);
            let r = min(mix(it.c.x, it.c.y, rnd(k, seed, 3u)), it.e.y);
            let bright = mix(0.45, 1.0, rnd(k, seed, 4u));
            var a = 0.0;
            if mark == 0u {
                let q = length(d) / max(r, 1e-3);
                a = exp(-q * q * 2.5) * (1.0 - smoothstep(0.75, 1.0, q));
            } else if mark == 1u {
                a = clamp(r + 0.5 - length(d), 0.0, 1.0) * min(1.0, r * 2.0);
            } else {
                let half = r * 0.5;
                let along = abs(dot(d, dir));
                let side = abs(dot(d, across));
                a = clamp(1.1 - side, 0.0, 1.0) * clamp(half + 0.5 - along, 0.0, 1.0);
            }
            cover = max(cover, a * bright);
        }
    }
    let alpha = cover * it.b.a;
    if alpha <= 0.0 {
        discard;
    }
    let col = mix(it.b.rgb, it.d.rgb, it.d.w);
    return vec4<f32>(col * alpha, alpha);
}

struct SOut {
    @builtin(position) clip: vec4<f32>,
    // In the shape's own frame, in radii: x along, y up.
    @location(0) uv: vec2<f32>,
    // Logical pixels from the centre, screen axes.
    @location(1) px: vec2<f32>,
    @location(2) @interpolate(flat) ii: u32,
};

@vertex
fn vs_shape(@builtin(vertex_index) vi: u32, @builtin(instance_index) ii: u32) -> SOut {
    let it = items[ii];
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, -1.0), vec2<f32>(1.0, 1.0),
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, 1.0), vec2<f32>(-1.0, 1.0),
    );
    let corner = corners[vi];
    let r = max(it.a.z, 1e-3);
    var ext = vec2<f32>(r + it.c.z + 2.0);
    if it.c.y > 0.5 {
        ext = vec2<f32>(r, r * it.a.w) * 1.05 + 2.0;
    }
    let local = corner * ext;
    // Turned anticlockwise on screen (screen y runs down).
    let ca = cos(it.c.x);
    let sa = sin(it.c.x);
    let off = vec2<f32>(local.x * ca - local.y * sa, -(local.x * sa + local.y * ca));
    var o: SOut;
    o.clip = clip_of(it.a.xy + off);
    o.uv = local / r;
    o.px = off;
    o.ii = ii;
    return o;
}

// Smooth value noise in [0, 1).
fn noise(p: vec2<f32>, seed: u32) -> f32 {
    let i = vec2<i32>(floor(p));
    let f = fract(p);
    let w = f * f * (3.0 - 2.0 * f);
    let a = rnd(i, seed, 7u);
    let b = rnd(i + vec2<i32>(1, 0), seed, 7u);
    let c = rnd(i + vec2<i32>(0, 1), seed, 7u);
    let d = rnd(i + vec2<i32>(1, 1), seed, 7u);
    return mix(mix(a, b, w.x), mix(c, d, w.x), w.y);
}

// A hatch stroke covering 28% of each period, anti-aliased, as on meshes.
fn strokes(c: f32) -> f32 {
    let u = fract(c);
    let a = u - select(0.0, 1.0, u > 0.64);
    let px = clamp(fwidth(c) * 0.5, 1e-4, 0.14);
    return smoothstep(-px, px, a) * (1.0 - smoothstep(0.28 - px, 0.28 + px, a));
}

@fragment
fn fs_shape(in: SOut) -> @location(0) vec4<f32> {
    let it = items[in.ii];
    let seed = u32(it.e.w);
    // Derivatives first, while every pixel of the quad is still running.
    let l = length(in.uv);
    let z = sqrt(max(1.0 - l * l, 0.0));
    let light = dot(vec3<f32>(in.uv, z), normalize(fg.sun.xyz)) * 0.5 + 0.5;
    let w = max(fwidth(light), 0.004);
    let disc_strokes = strokes((in.px.x - in.px.y) / 7.0);
    let wash_strokes = strokes((in.px.x + in.px.y) / 9.0);
    var col: vec3<f32>;
    var alpha: f32;
    if it.c.y < 0.5 {
        // A disc: a ball lit by the scene's sun in tone bands, hatched in shadow, with
        // an ink rim.
        let r = max(it.a.z, 1e-3);
        let aa = 1.0 / r;
        let body = clamp((1.0 - l) / aa + 0.5, 0.0, 1.0);
        let cover = clamp((1.0 + it.c.z / r - l) / aa + 0.5, 0.0, 1.0);
        let shadowed = 1.0 - smoothstep(0.52 - w, 0.52 + w, light);
        let hi = smoothstep(0.86 - w, 0.86 + w, light);
        let tone = mix(mix(vec3<f32>(1.0), vec3<f32>(1.1, 1.06, 0.98), hi), it.e.rgb, shadowed);
        col = it.b.rgb * tone;
        let hatch = disc_strokes * shadowed * it.c.w;
        col = mix(col, col * 0.5 + it.f.rgb * 0.5, hatch * 0.55);
        col = mix(it.f.rgb, col, body);
        alpha = cover * it.b.a;
    } else {
        // A wash: a soft cloudy streak, lightly hatched where it is thick.
        let q = vec2<f32>(in.uv.x, in.uv.y / max(it.a.w, 0.05));
        let n = noise(in.uv * 2.5, seed) * 0.6 + noise(in.uv * 5.5 + 17.0, seed) * 0.4;
        let a = (1.0 - smoothstep(0.3, 1.0, length(q))) * mix(0.35, 1.0, n);
        let hatch = wash_strokes * smoothstep(0.35, 0.7, a) * it.c.w;
        col = mix(it.b.rgb, it.b.rgb * 0.55 + it.f.rgb * 0.45, hatch * 0.6);
        alpha = a * it.b.a;
    }
    if alpha <= 0.0 {
        discard;
    }
    col = mix(col, it.d.rgb, it.d.w);
    return vec4<f32>(col * alpha, alpha);
}

// Puts the depth back to far after distant meshes, so the play layer draws over them.
@vertex
fn vs_depth_reset(@builtin(vertex_index) vi: u32) -> @builtin(position) vec4<f32> {
    let uv = vec2<f32>(f32((vi << 1u) & 2u), f32(vi & 2u));
    return vec4<f32>(uv * 2.0 - 1.0, 1.0, 1.0);
}

@fragment
fn fs_depth_reset() -> @location(0) vec4<f32> {
    return vec4<f32>(0.0);
}
