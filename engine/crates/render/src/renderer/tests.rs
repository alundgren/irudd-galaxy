use super::*;
use crate::{Camera, Color, Glow, MeshBuilder, MeshInstance, Rect};
use glam::Vec3;

#[test]
#[ignore = "requires a GPU"]
fn widget_scissor_clips_panels_and_glyphs_at_two_scales() {
    use crate::{Text, UiLayer};
    for scale in [1.0, 2.0] {
        let mut renderer = Renderer::new_headless(128, 128).unwrap();
        renderer.set_scale_factor(scale);
        let mut scene = Scene::new(Camera::top_down(Vec2::ZERO, 64.0));
        scene.overlay.panels.push(Panel {
            fill: Color::WHITE,
            border_px: 0.0,
            radius: 0.0,
            shadow: false,
            ..Panel::new(Rect::new(0.0, 0.0, 128.0, 128.0))
        });
        let before = renderer.render_to_rgba(&scene).unwrap();
        scene.overlay.widgets.push(UiLayer {
            panels: vec![Panel {
                fill: Color::hex(0xe00000),
                ..Panel::new(Rect::new(0.0, 0.0, 100.0, 100.0))
            }],
            texts: vec![Text::new("WIDE TEXT", Vec2::new(0.0, 10.0), 30.0)],
            clip: Some(Rect::new(16.0, 16.0, 20.0, 20.0)),
        });
        let after = renderer.render_to_rgba(&scene).unwrap();
        let mut changed = false;
        for y in 0..128 {
            for x in 0..128 {
                let inside = x as f32 >= 16.0 * scale
                    && (x as f32) < 36.0 * scale
                    && y as f32 >= 16.0 * scale
                    && (y as f32) < 36.0 * scale;
                if !inside {
                    assert_eq!(before.pixel(x, y), after.pixel(x, y));
                } else {
                    changed |= before.pixel(x, y) != after.pixel(x, y);
                }
            }
        }
        assert!(changed);
    }
}

#[test]
fn world_raster_respects_texture_limits_without_changing_aspect() {
    assert_eq!(world_size((1280, 720), 8192), (2560, 1440));
    assert_eq!(world_size((1279, 719), 8192), (2558, 1438));
    assert_eq!(world_size((4096, 4096), 8192), (8192, 8192));
    assert_eq!(world_size((4097, 720), 8192), (4097, 720));
    assert_eq!(world_size((720, 4097), 8192), (720, 4097));
}

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
            assert_eq!(image.pixel(16, 32), [255, 255, 255, 255]);
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
        // World antialiasing must not wash out the flat cel faces or filter UI.
        assert_eq!(clear.pixel(0, 0), clear.pixel(32, 32));
        empty.clear_color = Color::hex(0x000000);
        let mut panel = Panel::new(Rect::new(16.0, 16.0, 32.0, 32.0));
        panel.fill = Color::WHITE;
        panel.border_px = 0.0;
        panel.radius = 0.0;
        panel.shadow = false;
        empty.overlay.panels.push(panel);
        let ui = renderer.render_to_rgba(&empty).unwrap();
        assert_eq!(ui.pixel(15, 32), [0, 0, 0, 255]);
        assert_eq!(ui.pixel(16, 32), [255, 255, 255, 255]);
    }
}

#[test]
#[ignore = "requires a GPU"]
fn widget_layers_cover_earlier_text_beyond_the_old_four_layer_limit() {
    use crate::{Text, UiLayer};
    let mut renderer = Renderer::new_headless(96, 96).unwrap();
    let mut scene = Scene::new(Camera::top_down(Vec2::ZERO, 96.0));
    for _ in 0..12 {
        scene.overlay.widgets.push(UiLayer {
            texts: vec![Text::new("INK", Vec2::new(10.0, 10.0), 30.0)],
            ..Default::default()
        });
    }
    scene.overlay.widgets.push(UiLayer {
        panels: vec![Panel {
            fill: Color::WHITE,
            border_px: 0.0,
            radius: 0.0,
            shadow: false,
            ..Panel::new(Rect::new(0.0, 0.0, 96.0, 96.0))
        }],
        ..Default::default()
    });
    let covered = renderer.render_to_rgba(&scene).unwrap();
    assert_eq!(covered.pixel(20, 20), [255, 255, 255, 255]);
    for layer in &mut scene.overlay.widgets {
        layer.texts.clear();
    }
    let plain = renderer.render_to_rgba(&scene).unwrap();
    for y in 4..90 {
        for x in 4..90 {
            assert_eq!(covered.pixel(x, y), plain.pixel(x, y));
        }
    }
}
