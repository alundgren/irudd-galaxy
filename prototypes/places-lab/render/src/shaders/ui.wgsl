// Rounded UI panels: signed-distance rounded boxes with an ink border and a soft shadow.

struct Screen {
    // xy: viewport in physical pixels.
    viewport: vec4<f32>,
};

struct Panel {
    // x, y, w, h in physical pixels.
    rect: vec4<f32>,
    // x: corner radius, y: border width, z: shadow offset (0 = none), w: dash period (0 = solid).
    params: vec4<f32>,
    fill: vec4<f32>,
    border: vec4<f32>,
    // x: rotation in radians (clockwise on screen), y: shape (0 rounded box, 1 triangle
    // pointing up the screen).
    shape: vec4<f32>,
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
    let half = p.rect.zw * 0.5;
    let turn = select(0.0, length(half) - min(half.x, half.y), p.shape.x != 0.0);
    let margin = p.params.z * 3.0 + 2.0 + turn;
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

// Isosceles triangle with its tip at the origin and its base `q.y` below it (y runs
// down the screen), half as wide as `q.x`.
fn sd_triangle(p0: vec2<f32>, q: vec2<f32>) -> f32 {
    let p = vec2<f32>(abs(p0.x), p0.y);
    let a = p - q * clamp(dot(p, q) / dot(q, q), 0.0, 1.0);
    let b = p - q * vec2<f32>(clamp(p.x / q.x, 0.0, 1.0), 1.0);
    let s = -sign(q.y);
    let d = min(vec2<f32>(dot(a, a), s * (p.x * q.y - p.y * q.x)), vec2<f32>(dot(b, b), s * (p.y - q.y)));
    return -sqrt(d.x) * sign(d.y);
}

fn shape_sd(rel0: vec2<f32>, half: vec2<f32>, r: f32, shape: vec4<f32>) -> f32 {
    let c = cos(shape.x);
    let s = sin(shape.x);
    let rel = vec2<f32>(c * rel0.x + s * rel0.y, -s * rel0.x + c * rel0.y);
    if shape.y > 0.5 {
        return sd_triangle(vec2<f32>(rel.x, rel.y + half.y), vec2<f32>(half.x, half.y * 2.0)) - r;
    }
    return sd_round_box(rel, half, r);
}

@fragment
fn fs_panel(in: VOut) -> @location(0) vec4<f32> {
    let p = panels[in.ii];
    let half = p.rect.zw * 0.5;
    let centre = p.rect.xy + half;
    let r = min(p.params.x, min(half.x, half.y));
    let d = shape_sd(in.px - centre, half, r, p.shape);
    let inside = 1.0 - smoothstep(-0.5, 0.5, d);
    var border = smoothstep(-p.params.y - 0.5, -p.params.y + 0.5, d);
    if p.params.w > 0.0 {
        // Dashes run along each edge: x on the top and bottom, y on the sides.
        let rel = in.px - centre;
        let along = select(rel.x, rel.y, abs(rel.x) / half.x > abs(rel.y) / half.y);
        border = border * step(fract(along / p.params.w), 0.6);
    }
    let body = mix(p.fill, p.border, border * step(0.01, p.params.y));
    let a = inside * body.a;
    var out = vec4<f32>(body.rgb * a, a);
    if p.params.z > 0.0 {
        let off = vec2<f32>(p.params.z * 0.4, p.params.z);
        let ds = shape_sd(in.px - centre - off, half, r, p.shape);
        let sa = (1.0 - smoothstep(-p.params.z, p.params.z, ds)) * 0.45;
        out = out + vec4<f32>(0.0232, 0.0103, 0.0273, 1.0) * sa * (1.0 - out.a);
    }
    return out;
}
