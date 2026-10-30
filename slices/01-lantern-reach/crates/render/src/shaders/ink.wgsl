// Inner ink. The mesh pass has already stored, per pixel, an octahedral normal,
// a 16-bit view depth and the unlit paint. Paint alpha is the facet's minor
// axis in pixels (48 saturates); 0 is no surface. This pass draws a line where
// neighbouring samples disagree by more than the art bible's crease angle, where
// the paint changes, or where depth breaks the local plane by a step that is
// large relative to the view depth. A continuous slope is not an edge. Lines
// fade out when the smaller feature is only a few pixels across. Silhouette
// pixels (a neighbour with no surface) are left to the inverted-hull outline.
// Core-WebGPU targets (Rgba16Float normals and depth, Rgba8Unorm paint) and
// textureLoad, no optional features, so the pass is the same on DX12, Vulkan and
// Metal.

struct Ink {
    // x: crease cosine (ink when the normal dot is below this).
    // y: linear colour distance that counts as a paint border.
    // z: minimum world-space depth break. Scaled by view depth / 400 in the shader.
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

fn inside(q: vec2<i32>, dim: vec2<u32>) -> bool {
    return q.x >= 0 && q.y >= 0 && u32(q.x) < dim.x && u32(q.y) < dim.y;
}

@fragment
fn fs_ink(in: VOut) -> @location(0) vec4<f32> {
    let dim = textureDimensions(geom_tex);
    let p = vec2<i32>(in.clip.xy);
    let a0 = textureLoad(albedo_tex, p, 0);
    // 0 is the clear colour. A surface stores at least 1/255.
    if a0.a < 0.5 / 255.0 {
        discard;
    }
    let g0 = textureLoad(geom_tex, p, 0);
    let n0 = oct_decode(g0.xy);
    let d0 = unpack_depth(g0);
    // No inner line on a feature smaller than a few pixels.
    let px0 = a0.a * 48.0;
    if px0 < 3.0 {
        discard;
    }
    let radius = max(ink.params.w * clamp(px0 / 20.0, 0.55, 1.0), 1.0);
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
        if !inside(q, dim) {
            continue;
        }
        let a1 = textureLoad(albedo_tex, q, 0);
        // A missing neighbour is the silhouette. The hull outline owns that.
        if a1.a < 0.5 / 255.0 {
            continue;
        }
        let px1 = a1.a * 48.0;
        let fade = smoothstep(3.0, 14.0, min(px0, px1));
        if fade < 0.04 {
            continue;
        }
        let g1 = textureLoad(geom_tex, q, 0);
        let n1 = oct_decode(g1.xy);
        let agree = clamp(dot(n0, n1), -1.0, 1.0);
        let d1 = unpack_depth(g1);
        let crease = agree < ink.params.x;
        let paint = length(a0.rgb - a1.rgb) > ink.params.y;
        // Plane test: the second difference is ~0 on any continuous slope and
        // ~the step height across a break. Scale the bible's world step by
        // view depth so a far surface does not ink its own gradient.
        var depth_break = false;
        let qm = p - o;
        if inside(qm, dim) {
            let am = textureLoad(albedo_tex, qm, 0);
            if am.a >= 0.5 / 255.0 {
                let dm = unpack_depth(textureLoad(geom_tex, qm, 0));
                let second = abs(d1 + dm - 2.0 * d0);
                let limit = max(ink.params.z * max(d0, 1.0) / 400.0, 0.08);
                depth_break = second > limit && d0 <= d1;
            }
        }
        if !crease && !paint && !depth_break {
            continue;
        }
        // One side of a crease or paint border, so the stroke does not cover
        // both faces. A pure depth break stays on the nearer pixel (above).
        if crease || paint {
            let mark = g0.x + g0.y * 2.0;
            let mark1 = g1.x + g1.y * 2.0;
            if mark + 0.004 < mark1 {
                continue;
            }
        }
        edge = max(edge, fade);
    }
    if edge < 0.04 {
        discard;
    }
    // Linear ink, #2a1a2e, premultiplied. The sRGB target encodes it.
    // Alpha fades with feature size so a medium crease is a tint, not a fill.
    let col = vec3<f32>(0.0232, 0.0103, 0.0273);
    return vec4<f32>(col * edge, edge);
}
