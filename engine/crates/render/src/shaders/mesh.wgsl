// Cel-shaded meshes with inverted-hull ink outlines.

struct Instance {
    model: mat4x4<f32>,
    normal_mat: mat4x4<f32>,
    tint: vec4<f32>,
    // rgb: shadow band multiplier, w: band count (2 or 3).
    shadow: vec4<f32>,
    // rgb: highlight band multiplier, w: hatch strength.
    highlight: vec4<f32>,
    // rgb: outline colour, w: outline width in physical pixels.
    outline: vec4<f32>,
    emissive: vec4<f32>,
    // x: morph weight (0 the mesh, 1 its morph target), y: the uniform scale the
    // hatching is spaced for (0: the instance's own).
    morph: vec4<f32>,
};

@group(1) @binding(0) var<storage, read> instances: array<Instance>;

struct VIn {
    @location(0) pos: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) outline_normal: vec3<f32>,
    @location(3) color: vec4<f32>,
    // x: minor part span, y: major part span, both in mesh units.
    @location(4) span: vec2<f32>,
    // Minor span of the vertex's own facet, in mesh units.
    @location(5) facet: f32,
    // Where position, normal and paint go as the instance's morph weight rises.
    @location(6) morph_pos: vec3<f32>,
    @location(7) morph_normal: vec3<f32>,
    @location(8) morph_color: vec4<f32>,
    // How much of the paint is a fresh cut: x here, y at the morph target.
    @location(9) bite: vec2<f32>,
    @builtin(instance_index) ii: u32,
};

struct VOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) world: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) color: vec4<f32>,
    @location(3) @interpolate(flat) ii: u32,
    @location(4) @interpolate(flat) span: vec2<f32>,
    // Object-space position: hatching is laid out in it, so the strokes stay on the
    // surface as the object turns, moves or the camera pans.
    @location(5) local: vec3<f32>,
    @location(6) @interpolate(flat) facet: f32,
    // One hatch period in object units: about 7 pixels at the hatch scale.
    @location(7) @interpolate(flat) hatch: f32,
    @location(8) bite: f32,
};

// A vertex at the instance's morph weight.
struct Morphed {
    pos: vec3<f32>,
    normal: vec3<f32>,
    color: vec4<f32>,
    bite: f32,
};

fn morphed(v: VIn, w: f32) -> Morphed {
    var m: Morphed;
    m.pos = mix(v.pos, v.morph_pos, w);
    m.normal = mix(v.normal, v.morph_normal, w);
    m.color = mix(v.color, v.morph_color, w);
    m.bite = mix(v.bite.x, v.bite.y, w);
    return m;
}

// Min and max column lengths of the instance matrix (uniform scale collapses them).
fn instance_extent(m: mat4x4<f32>) -> vec2<f32> {
    let sx = length(m[0].xyz);
    let sy = length(m[1].xyz);
    let sz = length(m[2].xyz);
    return vec2<f32>(min(sx, min(sy, sz)), max(sx, max(sy, sz)));
}

// Pixels covered by a world-space length at `world`. Orthographic scale is exact;
// perspective divides by the point's clip w. The largest axis is the one most
// parallel to the screen, so a top-down camera ignores the view direction.
fn feature_px(world: vec3<f32>, span: f32) -> f32 {
    let w = max(abs((g.view_proj * vec4<f32>(world, 1.0)).w), 1e-4);
    let ax = (g.view_proj * vec4<f32>(1.0, 0.0, 0.0, 0.0)).xy / w;
    let ay = (g.view_proj * vec4<f32>(0.0, 1.0, 0.0, 0.0)).xy / w;
    let az = (g.view_proj * vec4<f32>(0.0, 0.0, 1.0, 0.0)).xy / w;
    let unit = max(
        max(length(ax * g.viewport.xy * 0.5), length(ay * g.viewport.xy * 0.5)),
        length(az * g.viewport.xy * 0.5),
    );
    return span * unit;
}

// Screen-space width of `x` across a pixel, capped so a crease between two flat
// facets (where the 2x2 quad straddles them) never smears a band edge. Never under
// BAND_SOFT: a flat facet has no gradient, and without it a facet turning slowly
// (a rock being mined) would flip a whole band at once instead of fading across.
const BAND_SOFT: f32 = 0.02;

fn aa_width(x: f32) -> f32 {
    return max(min(fwidth(x), 0.05), BAND_SOFT);
}

// A hard step at `edge`, anti-aliased over `w`.
fn band(edge: f32, x: f32, w: f32) -> f32 {
    if w <= 1e-5 {
        return step(edge, x);
    }
    return smoothstep(edge - w * 0.5, edge + w * 0.5, x);
}

fn too_small(world: vec3<f32>, span: vec2<f32>, model: mat4x4<f32>) -> bool {
    if span.y <= 0.0 {
        return false;
    }
    let s = instance_extent(model);
    return feature_px(world, span.y * s.y) < 1.0;
}

@vertex
fn vs_main(v: VIn) -> VOut {
    let inst = instances[v.ii];
    let m = morphed(v, inst.morph.x);
    let world = inst.model * vec4<f32>(m.pos, 1.0);
    var o: VOut;
    o.world = world.xyz;
    o.normal = (inst.normal_mat * vec4<f32>(m.normal, 0.0)).xyz;
    o.color = m.color;
    o.bite = m.bite;
    o.ii = v.ii;
    o.span = v.span;
    o.facet = v.facet;
    o.local = m.pos;
    var hatch_scale = instance_extent(inst.model).y;
    if inst.morph.y > 0.0 {
        hatch_scale = inst.morph.y;
    }
    o.hatch = 7.0 * g.viewport.z / max(feature_px(inst.model[3].xyz, hatch_scale), 1e-4);
    // Facets under about a pixel are dropped: decals and greebles, not the hull.
    if too_small(world.xyz, v.span, inst.model) {
        o.clip = vec4<f32>(0.0, 0.0, 2.0, 1.0);
        return o;
    }
    o.clip = g.view_proj * world;
    return o;
}

@fragment
fn fs_main(in: VOut) -> @location(0) vec4<f32> {
    let inst = instances[in.ii];
    let n = normalize(in.normal);
    let albedo = in.color.rgb * inst.tint.rgb;
    // A fresh cut darkens in shadow without taking the shadow's colour cast.
    let bite = clamp(in.bite, 0.0, 1.0);
    let shadow_rgb = mix(inst.shadow.rgb, vec3<f32>(dot(inst.shadow.rgb, vec3<f32>(0.2126, 0.7152, 0.0722))), bite);

    // Half-Lambert from the key light, quantised into tone bands. Each band edge is
    // a hard step, smoothed over one pixel so a curved terminator doesn't crawl.
    let light = dot(n, normalize(g.sun_dir.xyz)) * 0.5 + 0.5;
    let light_w = aa_width(light);
    var tone = vec3<f32>(1.0);
    var shadowed = 0.0;
    if inst.shadow.w > 2.5 {
        let hi = band(0.86, light, light_w);
        shadowed = 1.0 - band(0.52, light, light_w);
        tone = mix(mix(vec3<f32>(1.0), inst.highlight.rgb, hi), shadow_rgb, shadowed);
    } else {
        shadowed = 1.0 - band(0.55, light, light_w);
        tone = mix(vec3<f32>(1.0), shadow_rgb, shadowed);
    }

    // Point lights add a stepped warm pool.
    var lamp = vec3<f32>(0.0);
    let count = u32(g.counts.x);
    for (var i = 0u; i < count; i++) {
        let p = g.points[i];
        let d = p.pos_radius.xyz - in.world;
        let dist = max(length(d), 1e-4);
        let att = clamp(1.0 - dist / p.pos_radius.w, 0.0, 1.0);
        let k = max(dot(n, d / dist), 0.0) * att * att;
        let kw = aa_width(k);
        lamp += p.color.rgb * (band(0.08, k, kw) * 0.35 + band(0.35, k, kw) * 0.35);
    }

    var col = albedo * (tone * g.sun_color.rgb + g.ambient.rgb + lamp);

    // A hard rim on surfaces turning away from the camera.
    var to_eye = -g.camera_fwd.xyz;
    if g.camera_fwd.w < 0.5 {
        to_eye = normalize(g.camera_pos.xyz - in.world);
    }
    let rim = 1.0 - max(dot(n, to_eye), 0.0);
    let rim_on = band(0.72, rim, aa_width(rim)) * band(0.4, light, light_w);
    col += g.rim.rgb * g.rim.a * rim_on * (albedo + 0.15);

    // Sketched hatching in the shadow band: diagonal strokes laid out in object
    // space (about 7 pixels apart at the hatch scale), so they stick to the surface,
    // with anti-aliased sides. A fresh cut isn't hatched.
    let c = (in.local.x - in.local.y) / max(in.hatch, 1e-4);
    let u = fract(c);
    // A stroke covers the first 28% of each period. Measure from its start, wrapped
    // to the middle of the gap, and soften both sides over about a pixel.
    let a = u - select(0.0, 1.0, u > 0.64);
    let px = clamp(fwidth(c) * 0.5, 1e-4, 0.14);
    let stroke = smoothstep(-px, px, a) * (1.0 - smoothstep(0.28 - px, 0.28 + px, a));
    let hatch = stroke * shadowed * inst.highlight.w * (1.0 - bite);
    col = mix(col, col * 0.5 + INK * 0.5, hatch * 0.55);

    // Self-lit paint (windows, engines) ignores the light.
    col = mix(col, in.color.rgb * inst.tint.rgb * 1.1, in.color.a);
    col += inst.emissive.rgb * inst.emissive.a;
    return vec4<f32>(col, 1.0);
}

struct OOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) color: vec4<f32>,
};

@vertex
fn vs_outline(v: VIn) -> OOut {
    let inst = instances[v.ii];
    let world = inst.model * vec4<f32>(morphed(v, inst.morph.x).pos, 1.0);
    var o: OOut;
    o.color = vec4<f32>(inst.outline.rgb, 1.0);
    if too_small(world.xyz, v.span, inst.model) {
        o.clip = vec4<f32>(0.0, 0.0, 2.0, 1.0);
        return o;
    }
    var clip = g.view_proj * world;
    let n_world = (inst.normal_mat * vec4<f32>(v.outline_normal, 0.0)).xyz;
    // Full art-bible weight on a large facet. A small one keeps a hairline, never
    // a stroke wider than the facet itself, so a distant station stays coloured.
    var width = inst.outline.w;
    if v.span.y > 0.0 {
        let minor_px = feature_px(world.xyz, v.span.x * instance_extent(inst.model).x);
        // Full weight once the facet is a dozen pixels across. Below that, a
        // hairline that cannot paint the shape solid.
        let t = smoothstep(4.0, 12.0, minor_px);
        width = mix(0.65, inst.outline.w, t);
        width = min(width, max(0.5, minor_px * 0.45));
    }
    // Push the back faces out along the projected normal.
    let n_px = (g.view_proj * vec4<f32>(n_world, 0.0)).xy * g.viewport.xy;
    let len = length(n_px);
    if len > 1e-5 {
        let offset = n_px / len * width * 2.0 / g.viewport.xy * clip.w;
        clip = vec4<f32>(clip.xy + offset, clip.zw);
    }
    o.clip = clip;
    return o;
}

@fragment
fn fs_outline(in: OOut) -> @location(0) vec4<f32> {
    return in.color;
}

// Geometry buffer for the inner-ink pass: octahedral normal and a 16-bit view
// depth split into two bytes (stored exactly in a half-float target), and the
// unlit paint. A flat-shaded crease is a step between neighbouring pixels; a
// smooth-shaded skin bends gently and only its real creases ink.

struct GeomOut {
    @location(0) geom: vec4<f32>,
    @location(1) albedo: vec4<f32>,
};

fn sign_nz(v: vec2<f32>) -> vec2<f32> {
    return vec2<f32>(select(-1.0, 1.0, v.x >= 0.0), select(-1.0, 1.0, v.y >= 0.0));
}

fn oct_encode(n: vec3<f32>) -> vec2<f32> {
    let n2 = n / max(abs(n.x) + abs(n.y) + abs(n.z), 1e-5);
    var oct = n2.xy;
    if n2.z < 0.0 {
        oct = (1.0 - abs(n2.yx)) * sign_nz(n2.xy);
    }
    return oct * 0.5 + 0.5;
}

@fragment
fn fs_geom(in: VOut) -> GeomOut {
    let inst = instances[in.ii];
    let oct = oct_encode(normalize(in.normal));
    let fwd = g.camera_fwd.xyz;
    let d = dot(in.world - g.camera_pos.xyz, fwd / max(length(fwd), 1e-5));
    // 0..2048 world units from the camera plane. The play camera sits near 400,
    // and a 16-bit step is about 0.03 units, finer than the ink depth threshold.
    let q = u32(clamp(d / 2048.0, 0.0, 1.0) * 65535.0 + 0.5);
    let albedo = clamp(in.color.rgb * inst.tint.rgb, vec3<f32>(0.0), vec3<f32>(1.0));
    // Alpha carries the facet's minor axis in pixels, scaled so 48px saturates.
    // The clear value is 0, so any surface stays above zero and the ink pass can
    // fade lines out on features only a few pixels across.
    var minor_px = 48.0;
    if in.span.y > 0.0 {
        minor_px = feature_px(in.world, in.facet * instance_extent(inst.model).x);
    }
    var o: GeomOut;
    o.geom = vec4<f32>(oct, f32(q >> 8u) / 255.0, f32(q & 255u) / 255.0);
    o.albedo = vec4<f32>(albedo, clamp(minor_px / 48.0, 1.0 / 255.0, 1.0));
    return o;
}
