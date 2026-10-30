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
};

@group(1) @binding(0) var<storage, read> instances: array<Instance>;

struct VIn {
    @location(0) pos: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) outline_normal: vec3<f32>,
    @location(3) color: vec4<f32>,
    // x: minor facet span, y: major facet span, both in mesh units.
    @location(4) span: vec2<f32>,
    @builtin(instance_index) ii: u32,
};

struct VOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) world: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) color: vec4<f32>,
    @location(3) @interpolate(flat) ii: u32,
    @location(4) @interpolate(flat) span: vec2<f32>,
};

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
    let world = inst.model * vec4<f32>(v.pos, 1.0);
    var o: VOut;
    o.world = world.xyz;
    o.normal = (inst.normal_mat * vec4<f32>(v.normal, 0.0)).xyz;
    o.color = v.color;
    o.ii = v.ii;
    o.span = v.span;
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

    // Half-Lambert from the key light, quantised into tone bands.
    let light = dot(n, normalize(g.sun_dir.xyz)) * 0.5 + 0.5;
    var tone = vec3<f32>(1.0);
    var shadowed = 0.0;
    if inst.shadow.w > 2.5 {
        if light > 0.86 {
            tone = inst.highlight.rgb;
        } else if light < 0.52 {
            tone = inst.shadow.rgb;
            shadowed = 1.0;
        }
    } else if light < 0.55 {
        tone = inst.shadow.rgb;
        shadowed = 1.0;
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
        lamp += p.color.rgb * (step(0.08, k) * 0.35 + step(0.35, k) * 0.35);
    }

    var col = albedo * (tone * g.sun_color.rgb + g.ambient.rgb + lamp);

    // A hard rim on surfaces turning away from the camera.
    var to_eye = -g.camera_fwd.xyz;
    if g.camera_fwd.w < 0.5 {
        to_eye = normalize(g.camera_pos.xyz - in.world);
    }
    let rim = 1.0 - max(dot(n, to_eye), 0.0);
    col += g.rim.rgb * g.rim.a * step(0.72, rim) * step(0.4, light) * (albedo + 0.15);

    // Sketched hatching in the shadow band, in screen space.
    let spacing = 7.0 * g.viewport.z;
    let s = fract((in.clip.x + in.clip.y) / spacing);
    let hatch = step(s, 0.28) * shadowed * inst.highlight.w;
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
    let world = inst.model * vec4<f32>(v.pos, 1.0);
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

// Geometry buffer for the inner-ink pass: octahedral normal, 16-bit view depth
// packed into two unorm bytes, and the unlit paint. Flat per face, so a crease
// is a step between neighbouring pixels rather than a gradient.

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
        minor_px = feature_px(in.world, in.span.x * instance_extent(inst.model).x);
    }
    var o: GeomOut;
    o.geom = vec4<f32>(oct, f32(q >> 8u) / 255.0, f32(q & 255u) / 255.0);
    o.albedo = vec4<f32>(albedo, clamp(minor_px / 48.0, 1.0 / 255.0, 1.0));
    return o;
}
