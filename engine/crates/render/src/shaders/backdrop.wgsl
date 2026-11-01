// The backdrop: a vertical gradient with a soft vignette, then repeating tiles of nebula
// puffs and stars that scroll with the camera at a small parallax.

struct Sky {
    top: vec4<f32>,
    bottom: vec4<f32>,
    // xy: cloud scroll offset (logical px), z: tile size (logical px), w: UI scale.
    clouds: vec4<f32>,
    // xy: star scroll offset (logical px).
    stars: vec4<f32>,
    // x: tiles across, y: tiles down, z: cloud count, w: star count.
    counts: vec4<f32>,
    // xy: viewport (physical px), z: time.
    viewport: vec4<f32>,
};

struct Blob {
    // xy: position in the tile, z: radius.
    pos: vec4<f32>,
    color: vec4<f32>,
};

@group(0) @binding(0) var<uniform> sky: Sky;
@group(0) @binding(1) var<storage, read> blobs: array<Blob>;

struct GOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_gradient(@builtin(vertex_index) vi: u32) -> GOut {
    let uv = vec2<f32>(f32((vi << 1u) & 2u), f32(vi & 2u));
    var o: GOut;
    o.clip = vec4<f32>(uv * 2.0 - 1.0, 0.0, 1.0);
    o.uv = vec2<f32>(uv.x, 1.0 - uv.y);
    return o;
}

@fragment
fn fs_gradient(in: GOut) -> @location(0) vec4<f32> {
    var col = mix(sky.top.rgb, sky.bottom.rgb, smoothstep(0.0, 1.0, in.uv.y));
    let d = in.uv - 0.5;
    col *= 1.0 - dot(d, d) * 0.55;
    return vec4<f32>(col, 1.0);
}

struct BOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) @interpolate(flat) star: u32,
};

@vertex
fn vs_blob(@builtin(vertex_index) vi: u32, @builtin(instance_index) ii: u32) -> BOut {
    let clouds = u32(sky.counts.z);
    let n = clouds + u32(sky.counts.w);
    let b = blobs[ii % n];
    let tile_i = ii / n;
    let tx = f32(tile_i % u32(sky.counts.x)) - 1.0;
    let ty = f32(tile_i / u32(sky.counts.x)) - 1.0;
    let is_star = ii % n >= clouds;
    let tile = sky.clouds.z;
    var offset = sky.clouds.xy;
    if is_star {
        offset = sky.stars.xy;
    }
    let p = b.pos.xy - offset;
    let base = (p - floor(p / tile) * tile + vec2<f32>(tx, ty) * tile) * sky.clouds.w;
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, -1.0), vec2<f32>(1.0, 1.0),
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, 1.0), vec2<f32>(-1.0, 1.0),
    );
    let corner = corners[vi];
    var r = b.pos.z * sky.clouds.w;
    if is_star {
        // Room for the soft halo around a star.
        r *= 3.0;
    }
    let px = base + corner * r;
    var o: BOut;
    o.clip = vec4<f32>(px.x / sky.viewport.x * 2.0 - 1.0, 1.0 - px.y / sky.viewport.y * 2.0, 0.0, 1.0);
    o.uv = corner;
    o.color = b.color;
    o.star = select(0u, 1u, is_star);
    return o;
}

@fragment
fn fs_blob(in: BOut) -> @location(0) vec4<f32> {
    let d = length(in.uv);
    var a: f32;
    if in.star == 1u {
        let core = 1.0 - smoothstep(0.18, 0.36, d);
        let halo = exp(-d * d * 9.0) * 0.35;
        a = (core + halo) * in.color.a;
    } else {
        a = exp(-d * d * 3.0) * (1.0 - smoothstep(0.75, 1.0, d)) * in.color.a;
    }
    return vec4<f32>(in.color.rgb * a, a);
}
