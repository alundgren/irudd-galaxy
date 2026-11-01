// Ribbons (lanes, borders, rings), beams and glows. Output is premultiplied alpha;
// additive effects write zero alpha.

struct VIn {
    @location(0) pos: vec3<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) color: vec4<f32>,
    // x: kind (0 ribbon, 1 dashed, 2 glow, 3 beam, 4 dotted). Dashed: y dash, z gap.
    // Beam: y width, z length (world units). Dotted, all in half-widths (uv.x too):
    // y spacing, z fade length, w where the line ends.
    @location(3) params: vec4<f32>,
};

struct VOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) params: vec4<f32>,
};

@vertex
fn vs_fx(v: VIn) -> VOut {
    var o: VOut;
    o.clip = g.view_proj * vec4<f32>(v.pos, 1.0);
    // A beam runs from a nose to the surface it lands on. Depth-tested at one sample
    // against that surface, a facet bulging towards the camera cut its end with a
    // jagged edge, so beams draw over what they cross.
    if v.params.x > 2.5 && v.params.x < 3.5 {
        o.clip.z = 0.0;
    }
    o.uv = v.uv;
    o.color = v.color;
    o.params = v.params;
    return o;
}

@fragment
fn fs_fx(in: VOut) -> @location(0) vec4<f32> {
    let kind = in.params.x;
    let c = in.color;
    // Derivatives first, in uniform control flow.
    let aa = max(fwidth(in.uv.y), 1e-4);
    let aax = max(fwidth(in.uv.x), 1e-4);
    // Dotted: the distance from the nearest dot's centre, in half-widths.
    let spacing = max(in.params.y, 2.0);
    let t = in.uv.x - floor(in.uv.x / spacing) * spacing - spacing * 0.5;
    let dot = length(vec2<f32>(t, in.uv.y));
    let aad = max(fwidth(dot), 1e-4);
    if kind > 3.5 {
        // Round dots with a thin ink rim, smoothed over a pixel; fading out over the
        // last stretch of the line.
        var alpha = 1.0 - smoothstep(1.0 - aad, 1.0 + aad * 0.5, dot);
        let rim = smoothstep(0.62 - aad, 0.62 + aad, dot);
        let rgb = mix(c.rgb, INK, rim * 0.7);
        let left = in.params.w - in.uv.x;
        alpha *= smoothstep(0.0, max(in.params.z, 1e-3), left);
        alpha *= c.a;
        return vec4<f32>(rgb * alpha, alpha);
    }
    if kind > 2.5 {
        // Beam: hot white core, coloured falloff, a slow shimmer along its length
        // measured in beam widths (world units alias to stripes when zoomed out), and
        // soft rounded-off ends.
        let a = abs(in.uv.y);
        let width = max(in.params.y, 1e-3);
        let shimmer = 0.8 + 0.2 * sin(in.uv.x / width * 1.6 - g.camera_pos.w * 9.0);
        let cap = width * 0.6;
        let ends = smoothstep(0.0, cap, in.uv.x) * smoothstep(0.0, cap, in.params.z - in.uv.x);
        let glow = exp(-a * a * 5.0) * shimmer;
        let core = exp(-a * a * 40.0);
        let rgb = c.rgb * glow * 1.4 + vec3<f32>(1.0, 0.96, 0.85) * core;
        return vec4<f32>(rgb * c.a * ends, 0.0);
    }
    if kind > 1.5 {
        let d = length(in.uv);
        let a = exp(-d * d * 3.5) * clamp(1.0 - d, 0.0, 1.0);
        return vec4<f32>(c.rgb * a * c.a, 0.0);
    }
    // Ribbon: soft anti-aliased edges and a darker ink rim.
    let across = abs(in.uv.y);
    var alpha = 1.0 - smoothstep(1.0 - aa * 1.5, 1.0, across);
    let ink = smoothstep(0.55 - aa, 0.55 + aa, across);
    let rgb = mix(c.rgb, c.rgb * 0.35 + INK * 0.4, ink * 0.75);
    if kind > 0.5 {
        let period = in.params.y + in.params.z;
        let t = in.uv.x - floor(in.uv.x / period) * period;
        alpha *= 1.0 - smoothstep(in.params.y - aax, in.params.y + aax, t);
    }
    alpha *= c.a;
    return vec4<f32>(rgb * alpha, alpha);
}
