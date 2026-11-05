// Ribbons (lanes, borders, rings), beams, particles and glows. Output is premultiplied
// alpha; additive effects write zero alpha.

struct VIn {
    @location(0) pos: vec3<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) color: vec4<f32>,
    // x: kind (0 ribbon, 1 dashed, 2 glow, 3 beam, 4 dotted, 5 particle). Dashed: y dash,
    // z gap. Beam: y width, z length (world units), w glow strength. Dotted, all in
    // half-widths (uv.x too): y spacing, z fade length, w where the line ends. Particle:
    // y shape (0 dot, 1 spark, 2 puff, 3 chip), z 1 when additive; uv -1..1 on the quad.
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
    // Particles: distance from the quad's centre, round and square.
    let round = length(in.uv);
    let square = max(abs(in.uv.x), abs(in.uv.y));
    let aar = max(fwidth(round), 1e-4);
    let aas = max(fwidth(square), 1e-4);
    if kind > 4.5 {
        let shape = in.params.y;
        var a = 0.0;
        var rgb = c.rgb;
        if shape > 2.5 {
            // A chip: a square with an ink rim, painted like a tiny model.
            a = 1.0 - smoothstep(1.0 - aas, 1.0, square);
            rgb = mix(c.rgb, INK, smoothstep(0.6 - aas, 0.6 + aas, square) * 0.75);
        } else if shape > 1.5 {
            // A puff: a round cel disc with an ink rim.
            a = 1.0 - smoothstep(1.0 - aar, 1.0, round);
            rgb = mix(c.rgb, INK, smoothstep(0.72 - aar, 0.72 + aar, round) * 0.6);
        } else if shape > 0.5 {
            // A spark: a soft streak, hottest at its head (uv.x = 1), with a white core.
            let across = exp(-in.uv.y * in.uv.y * 3.0);
            let head = 0.25 + 0.75 * smoothstep(-1.0, 0.7, in.uv.x);
            let ends = 1.0 - smoothstep(0.8, 1.0, abs(in.uv.x));
            a = across * head * ends;
            rgb = mix(c.rgb, vec3<f32>(1.0, 0.97, 0.9), exp(-in.uv.y * in.uv.y * 18.0) * 0.6);
        } else {
            // A dot: soft and round.
            a = exp(-round * round * 3.5) * clamp(1.0 - round, 0.0, 1.0) * 1.6;
        }
        a = clamp(a, 0.0, 1.0) * c.a;
        if in.params.z > 0.5 {
            return vec4<f32>(rgb * a, 0.0);
        }
        return vec4<f32>(rgb * a, a);
    }
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
        // params.w scales the soft outer glow; the hot core stays.
        let glow = exp(-a * a * 5.0) * shimmer * in.params.w;
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
