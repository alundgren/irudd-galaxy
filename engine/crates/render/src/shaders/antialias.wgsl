// Resolve the denser world raster into output pixels in linear colour. UI is
// drawn afterwards at native resolution. At the texture-size limit the raster
// falls back to native size, making this an exact copy rather than an upscale.
struct Screen {
    viewport: vec4<f32>,
};
@group(0) @binding(0) var world: texture_2d<f32>;
@group(0) @binding(1) var<uniform> screen: Screen;

@vertex
fn vs_antialias(@builtin(vertex_index) vi: u32) -> @builtin(position) vec4<f32> {
    let uv = vec2<f32>(f32((vi << 1u) & 2u), f32(vi & 2u));
    return vec4<f32>(uv * 2.0 - 1.0, 0.0, 1.0);
}

@fragment
fn fs_antialias(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let scale = i32(textureDimensions(world).x) / i32(screen.viewport.x);
    let p = vec2<i32>(pos.xy) * scale;
    var sum = vec3<f32>(0.0);
    for (var y = 0; y < scale; y++) {
        for (var x = 0; x < scale; x++) {
            sum += textureLoad(world, p + vec2<i32>(x, y), 0).rgb;
        }
    }
    return vec4<f32>(sum / f32(scale * scale), 1.0);
}
