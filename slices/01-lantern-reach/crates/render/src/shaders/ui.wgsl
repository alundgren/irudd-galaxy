// Rounded UI panels: signed-distance rounded boxes with an ink border and a soft shadow.

struct Screen {
    // xy: viewport in physical pixels.
    viewport: vec4<f32>,
};

struct Panel {
    // x, y, w, h in physical pixels.
    rect: vec4<f32>,
    // x: corner radius, y: border width, z: shadow offset (0 = none).
    params: vec4<f32>,
    fill: vec4<f32>,
    border: vec4<f32>,
};

@group(0) @binding(0) var<uniform> screen: Screen;
@group(0) @binding(1) var<storage, read> panels: array<Panel>;

struct VOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) px: vec2<f32>,
    @location(1) @interpolate(flat) ii: u32,
};

@vertex
fn vs_panel(@builtin(vertex_index) vi: u32, @builtin(instance_index) ii: u32) -> VOut {
    let p = panels[ii];
    let margin = p.params.z * 3.0 + 2.0;
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(0.0, 0.0), vec2<f32>(1.0, 0.0), vec2<f32>(1.0, 1.0),
        vec2<f32>(0.0, 0.0), vec2<f32>(1.0, 1.0), vec2<f32>(0.0, 1.0),
    );
    let c = corners[vi];
    let lo = p.rect.xy - vec2<f32>(margin);
    let size = p.rect.zw + vec2<f32>(margin * 2.0);
    let px = lo + c * size;
    var o: VOut;
    o.clip = vec4<f32>(px.x / screen.viewport.x * 2.0 - 1.0, 1.0 - px.y / screen.viewport.y * 2.0, 0.0, 1.0);
    o.px = px;
    o.ii = ii;
    return o;
}

fn sd_round_box(p: vec2<f32>, half: vec2<f32>, r: f32) -> f32 {
    let q = abs(p) - half + vec2<f32>(r);
    return length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0) - r;
}

@fragment
fn fs_panel(in: VOut) -> @location(0) vec4<f32> {
    let p = panels[in.ii];
    let half = p.rect.zw * 0.5;
    let centre = p.rect.xy + half;
    let r = min(p.params.x, min(half.x, half.y));
    let d = sd_round_box(in.px - centre, half, r);
    let inside = 1.0 - smoothstep(-0.5, 0.5, d);
    let border = smoothstep(-p.params.y - 0.5, -p.params.y + 0.5, d);
    let body = mix(p.fill, p.border, border * step(0.01, p.params.y));
    let a = inside * body.a;
    var out = vec4<f32>(body.rgb * a, a);
    if p.params.z > 0.0 {
        let off = vec2<f32>(p.params.z * 0.4, p.params.z);
        let ds = sd_round_box(in.px - centre - off, half, r);
        let sa = (1.0 - smoothstep(-p.params.z, p.params.z, ds)) * 0.45;
        out = out + vec4<f32>(0.0232, 0.0103, 0.0273, 1.0) * sa * (1.0 - out.a);
    }
    return out;
}
