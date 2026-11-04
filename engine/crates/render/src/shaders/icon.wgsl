// Module icons: a texture rasterised at the exact physical size of its rectangle, read
// texel for pixel. Tinted icons use only its coverage; painted ones keep their colours.

struct Screen {
    viewport: vec4<f32>,
};

struct Icon {
    // x, y, w, h in physical pixels, whole numbers.
    rect: vec4<f32>,
    // Linear tint (rgb) and its alpha.
    tint: vec4<f32>,
    // x: 1 tinted, 0 painted; y: opacity.
    params: vec4<f32>,
};

@group(0) @binding(0) var<uniform> screen: Screen;
@group(0) @binding(1) var<storage, read> icons: array<Icon>;
@group(0) @binding(2) var pixels: texture_2d<f32>;

struct VOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) px: vec2<f32>,
    @location(1) @interpolate(flat) ii: u32,
};

@vertex
fn vs_icon(@builtin(vertex_index) vi: u32, @builtin(instance_index) ii: u32) -> VOut {
    let p = icons[ii];
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(0.0, 0.0), vec2<f32>(1.0, 0.0), vec2<f32>(1.0, 1.0),
        vec2<f32>(0.0, 0.0), vec2<f32>(1.0, 1.0), vec2<f32>(0.0, 1.0),
    );
    let px = p.rect.xy + corners[vi] * p.rect.zw;
    var o: VOut;
    o.clip = vec4<f32>(px.x / screen.viewport.x * 2.0 - 1.0, 1.0 - px.y / screen.viewport.y * 2.0, 0.0, 1.0);
    o.px = px;
    o.ii = ii;
    return o;
}

@fragment
fn fs_icon(in: VOut) -> @location(0) vec4<f32> {
    let p = icons[in.ii];
    let size = vec2<i32>(p.rect.zw);
    let at = clamp(vec2<i32>(floor(in.px - p.rect.xy)), vec2<i32>(0), size - vec2<i32>(1));
    let texel = textureLoad(pixels, at, 0);
    var rgb = texel.rgb;
    var a = texel.a;
    if p.params.x > 0.5 {
        rgb = p.tint.rgb;
        a = a * p.tint.a;
    }
    a = a * p.params.y;
    return vec4<f32>(rgb * a, a);
}
