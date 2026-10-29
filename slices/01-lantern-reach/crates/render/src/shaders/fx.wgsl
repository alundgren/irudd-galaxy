// Ribbons (lanes, borders, rings), beams and glows. Output is premultiplied alpha;
// additive effects write zero alpha.

struct VIn {
    @location(0) pos: vec3<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) color: vec4<f32>,
    // x: kind (0 ribbon, 1 dashed, 2 glow, 3 beam), y: dash, z: gap.
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
    if kind > 2.5 {
        // Beam: hot white core, coloured falloff, a slow shimmer along its length.
        let a = abs(in.uv.y);
        let shimmer = 0.8 + 0.2 * sin(in.uv.x * 2.2 - g.camera_pos.w * 9.0);
        let glow = exp(-a * a * 5.0) * shimmer;
        let core = exp(-a * a * 40.0);
        let rgb = c.rgb * glow * 1.4 + vec3<f32>(1.0, 0.96, 0.85) * core;
        return vec4<f32>(rgb * c.a, 0.0);
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
