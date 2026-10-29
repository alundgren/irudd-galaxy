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
    @builtin(instance_index) ii: u32,
};

struct VOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) world: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) color: vec4<f32>,
    @location(3) @interpolate(flat) ii: u32,
};

@vertex
fn vs_main(v: VIn) -> VOut {
    let inst = instances[v.ii];
    let world = inst.model * vec4<f32>(v.pos, 1.0);
    var o: VOut;
    o.clip = g.view_proj * world;
    o.world = world.xyz;
    o.normal = (inst.normal_mat * vec4<f32>(v.normal, 0.0)).xyz;
    o.color = v.color;
    o.ii = v.ii;
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
    var clip = g.view_proj * world;
    let n_world = (inst.normal_mat * vec4<f32>(v.outline_normal, 0.0)).xyz;
    // Push the back faces out by a constant number of pixels along the projected normal.
    let n_px = (g.view_proj * vec4<f32>(n_world, 0.0)).xy * g.viewport.xy;
    let len = length(n_px);
    if len > 1e-5 {
        let offset = n_px / len * inst.outline.w * 2.0 / g.viewport.xy * clip.w;
        clip = vec4<f32>(clip.xy + offset, clip.zw);
    }
    var o: OOut;
    o.clip = clip;
    o.color = vec4<f32>(inst.outline.rgb, 1.0);
    return o;
}

@fragment
fn fs_outline(in: OOut) -> @location(0) vec4<f32> {
    return in.color;
}
