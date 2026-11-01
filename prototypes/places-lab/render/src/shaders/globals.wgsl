// Shared per-frame uniforms. Prepended to mesh.wgsl and fx.wgsl.

struct PointLight {
    pos_radius: vec4<f32>,
    color: vec4<f32>,
};

struct Globals {
    view_proj: mat4x4<f32>,
    // xyz: camera eye, w: presentation time in seconds.
    camera_pos: vec4<f32>,
    // xyz: camera forward, w: 1 for orthographic.
    camera_fwd: vec4<f32>,
    // x, y: viewport in physical pixels, z: UI scale factor.
    viewport: vec4<f32>,
    sun_dir: vec4<f32>,
    sun_color: vec4<f32>,
    ambient: vec4<f32>,
    rim: vec4<f32>,
    // x: point light count.
    counts: vec4<f32>,
    points: array<PointLight, 8>,
};

@group(0) @binding(0) var<uniform> g: Globals;

const INK: vec3<f32> = vec3<f32>(0.0232, 0.0103, 0.0273);
