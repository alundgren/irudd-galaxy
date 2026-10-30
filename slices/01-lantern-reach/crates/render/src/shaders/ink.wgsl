// Inner ink. The mesh pass has already stored, per pixel, an octahedral normal,
// a 16-bit view depth and the unlit paint. This pass draws a line where two
// surface pixels disagree by more than the art bible's crease angle, or where
// the paint changes. Silhouette pixels (a neighbour with no surface) are left
// to the inverted-hull outline. Rgba8 targets and textureLoad, no optional
// features, so the pass is the same on DX12, Vulkan and Metal.

struct Ink {
    // x: crease cosine (ink when the normal dot is below this).
    // y: linear colour distance that counts as a paint border.
    // z: depth step in world units for a step whose normals still agree.
    // w: sample radius in pixels.
    params: vec4<f32>,
};

@group(0) @binding(0) var<uniform> ink: Ink;
@group(0) @binding(1) var geom_tex: texture_2d<f32>;
@group(0) @binding(2) var albedo_tex: texture_2d<f32>;

struct VOut {
    @builtin(position) clip: vec4<f32>,
};

@vertex
fn vs_ink(@builtin(vertex_index) vi: u32) -> VOut {
    let uv = vec2<f32>(f32((vi << 1u) & 2u), f32(vi & 2u));
    var o: VOut;
    o.clip = vec4<f32>(uv * 2.0 - 1.0, 0.0, 1.0);
    return o;
}

fn sign_nz(v: vec2<f32>) -> vec2<f32> {
    return vec2<f32>(select(-1.0, 1.0, v.x >= 0.0), select(-1.0, 1.0, v.y >= 0.0));
}

fn oct_decode(e: vec2<f32>) -> vec3<f32> {
    let f = e * 2.0 - 1.0;
    var n = vec3<f32>(f.x, f.y, 1.0 - abs(f.x) - abs(f.y));
    if n.z < 0.0 {
        let xy = (1.0 - abs(n.yx)) * sign_nz(n.xy);
        n = vec3<f32>(xy, n.z);
    }
    return normalize(n);
}

fn unpack_depth(g: vec4<f32>) -> f32 {
    let hi = u32(g.b * 255.0 + 0.5);
    let lo = u32(g.a * 255.0 + 0.5);
    return f32(hi * 256u + lo) / 65535.0 * 2048.0;
}

@fragment
fn fs_ink(in: VOut) -> @location(0) vec4<f32> {
    let dim = textureDimensions(geom_tex);
    let p = vec2<i32>(in.clip.xy);
    let a0 = textureLoad(albedo_tex, p, 0);
    if a0.a < 0.5 {
        discard;
    }
    let g0 = textureLoad(geom_tex, p, 0);
    let n0 = oct_decode(g0.xy);
    let d0 = unpack_depth(g0);
    let radius = max(ink.params.w, 1.0);
    var edge = 0.0;
    for (var i = 0u; i < 8u; i++) {
        let ang = f32(i) * 0.7853982;
        let o = vec2<i32>(
            i32(round(cos(ang) * radius)),
            i32(round(sin(ang) * radius)),
        );
        if o.x == 0 && o.y == 0 {
            continue;
        }
        let q = p + o;
        if q.x < 0 || q.y < 0 || u32(q.x) >= dim.x || u32(q.y) >= dim.y {
            continue;
        }
        let a1 = textureLoad(albedo_tex, q, 0);
        // A missing neighbour is the silhouette. The hull outline owns that.
        if a1.a < 0.5 {
            continue;
        }
        let g1 = textureLoad(geom_tex, q, 0);
        let agree = clamp(dot(n0, oct_decode(g1.xy)), -1.0, 1.0);
        if agree < ink.params.x {
            edge = 1.0;
        }
        if length(a0.rgb - a1.rgb) > ink.params.y {
            edge = 1.0;
        }
        if agree > 0.92 && abs(d0 - unpack_depth(g1)) > ink.params.z {
            edge = 1.0;
        }
    }
    if edge < 0.5 {
        discard;
    }
    // Linear ink, #2a1a2e, fully opaque so a bright hull does not wash it out.
    // The sRGB target encodes it.
    let col = vec3<f32>(0.0232, 0.0103, 0.0273);
    return vec4<f32>(col, 1.0);
}
