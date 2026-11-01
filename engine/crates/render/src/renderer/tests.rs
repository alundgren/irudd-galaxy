use super::*;
use crate::{Camera, Color, Glow, MeshBuilder, MeshInstance};
use glam::Vec3;

#[test]
#[ignore = "requires a GPU with 4x MSAA"]
fn inner_ink_keeps_fractional_coverage_and_single_sample_fallback() {
    let mut renderer = Renderer::new_headless(64, 64).unwrap();
    assert_eq!(renderer.samples, 4);
    let mut mesh = MeshBuilder::new();
    for (left, right, paint) in [
        (-24.0, 0.0, Color::WHITE),
        (0.0, 24.0, Color::hex(0x777777)),
    ] {
        mesh.paint(paint, 1.0);
        mesh.quad(
            Vec3::new(left, -24.0, 0.0),
            Vec3::new(right, -24.0, 0.0),
            Vec3::new(right, 24.0, 0.0),
            Vec3::new(left, 24.0, 0.0),
        );
    }
    let handle = renderer.upload_mesh(&mesh.build());
    let mut scene = Scene::new(Camera::top_down(Vec2::ZERO, 64.0));
    scene.ink.inner_px = 2.0;
    scene.ink.color = Color::hex(0x000000);
    scene.meshes.push(MeshInstance::new(handle, Mat4::IDENTITY));

    for samples in [4, 1] {
        if samples == 1 {
            renderer.samples = 1;
            renderer.layouts.ink =
                renderer
                    .device
                    .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                        label: Some("single-sample ink test"),
                        entries: &[
                            uniform_entry(0, wgpu::ShaderStages::FRAGMENT),
                            texture_entry(1, 1),
                            texture_entry(2, 1),
                        ],
                    });
            renderer.pipelines =
                create_pipelines(&renderer.device, &renderer.layouts, renderer.format, 1);
        }
        // Rebuild targets and bindings, including the fallback, through real resize.
        renderer.resize(80, 80);
        renderer.resize(64, 64);
        let mut reds = Vec::new();
        for shift in [0.0, 0.5, 1.0] {
            scene.meshes[0].transform = Mat4::from_translation(Vec3::new(shift, 0.0, 0.0));
            let image = renderer.render_to_rgba(&scene).unwrap();
            // Inside the white face: only ink changes here, never paint.
            reds.push(u32::from(image.pixel(31, 32)[0]));
        }
        assert!(reds[2] > reds[0] + 60, "ink must remain visible: {reds:?}");
        if samples == 4 {
            assert!(
                reds[1] > reds[0] + 20 && reds[1] < reds[2] - 20,
                "half-pixel motion must partially cover ink: {reds:?}"
            );
        }
        // Exercise effects-only and empty scenes, including their final resolve.
        let mut empty = Scene::new(scene.camera);
        let clear = renderer.render_to_rgba(&empty).unwrap();
        empty.glows.push(Glow {
            position: Vec3::ZERO,
            radius: 8.0,
            color: Color::WHITE,
        });
        let glow = renderer.render_to_rgba(&empty).unwrap();
        assert_ne!(clear.pixels, glow.pixels);
        empty.glows.clear();
        assert_eq!(
            clear.pixels,
            renderer.render_to_rgba(&empty).unwrap().pixels
        );
    }
}
