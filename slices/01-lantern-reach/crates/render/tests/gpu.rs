//! GPU tests. They need a real adapter, so they are ignored by default:
//!
//! ```text
//! cargo test -p render -- --ignored
//! ```

use render::demo::DemoWorld;
use render::glam::{Vec2, Vec3};
use render::kit::{self, RockKind, RockParams};
use render::{Camera, Color, Glow, Line, MeshInstance, PausedCard, Renderer, RgbaImage, Scene};

fn renderer(w: u32, h: u32) -> Renderer {
    Renderer::new_headless(w, h).expect("a GPU adapter (these tests are #[ignore]d for a reason)")
}

fn distinct_colors(img: &RgbaImage) -> usize {
    let mut seen = std::collections::HashSet::new();
    for px in img.pixels.as_chunks::<4>().0.iter().step_by(7) {
        seen.insert([px[0] >> 3, px[1] >> 3, px[2] >> 3]);
    }
    seen.len()
}

#[test]
#[ignore = "needs a GPU"]
fn demo_world_renders_something_rich() {
    let mut r = renderer(640, 360);
    let world = DemoWorld::new(&mut r);
    let img = r.render_to_rgba(&world.scene(r.logical_size())).unwrap();
    assert_eq!((img.width, img.height), (640, 360));
    assert_eq!(img.pixels.len(), 640 * 360 * 4);
    assert!(
        img.pixels.as_chunks::<4>().0.iter().all(|p| p[3] == 255),
        "opaque output"
    );
    assert!(distinct_colors(&img) > 200, "{}", distinct_colors(&img));
    let png = img.encode_png().unwrap();
    assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
}

#[test]
#[ignore = "needs a GPU"]
fn rendering_is_repeatable() {
    let mut r = renderer(320, 200);
    let world = DemoWorld::new(&mut r);
    let scene = world.scene(r.logical_size());
    let a = r.render_to_rgba(&scene).unwrap();
    let b = r.render_to_rgba(&scene).unwrap();
    assert!(a == b, "the same scene must give the same pixels");
}

#[test]
#[ignore = "needs a GPU"]
fn mesh_is_drawn_with_an_ink_outline() {
    let mut r = renderer(200, 200);
    let rock = r.upload_mesh(&kit::rock(&RockParams::new(RockKind::Iron, 10.0, 1.0), 1));
    let mut scene = Scene::new(Camera::top_down(Vec2::ZERO, 40.0));
    scene.clear_color = Color::hex(0xffffff);
    scene
        .meshes
        .push(MeshInstance::at(rock, Vec2::ZERO, 0.0, 1.0));
    let img = r.render_to_rgba(&scene).unwrap();
    // The centre is rock, the corner is the white clear colour.
    let centre = img.pixel(100, 100);
    assert!(
        centre[0] > centre[2],
        "rust-coloured centre, got {centre:?}"
    );
    assert_eq!(img.pixel(2, 2), [255, 255, 255, 255]);
    // Walking out from the centre we cross dark ink before reaching the background.
    let row: Vec<[u8; 4]> = (100..200).map(|x| img.pixel(x, 100)).collect();
    let ink = row.iter().position(|p| p[0] < 70 && p[1] < 50 && p[2] < 70);
    let white = row.iter().position(|p| p[0] > 250 && p[1] > 250);
    assert!(
        matches!((ink, white), (Some(i), Some(w)) if i < w),
        "{ink:?} {white:?}"
    );
}

#[test]
#[ignore = "needs a GPU"]
fn inner_ink_marks_a_roof_ridge_and_spares_a_flat_slab() {
    fn painted(quads: &[[Vec3; 4]]) -> render::MeshData {
        let mut b = render::MeshBuilder::new();
        b.paint(Color::hex(0xf4e2c4), 0.0);
        for q in quads {
            b.quad(q[0], q[1], q[2], q[3]);
        }
        b.build()
    }
    let roof = painted(&[
        [
            Vec3::new(-10.0, -12.0, 0.0),
            Vec3::new(10.0, -12.0, 0.0),
            Vec3::new(10.0, 0.0, 8.0),
            Vec3::new(-10.0, 0.0, 8.0),
        ],
        [
            Vec3::new(10.0, 12.0, 0.0),
            Vec3::new(-10.0, 12.0, 0.0),
            Vec3::new(-10.0, 0.0, 8.0),
            Vec3::new(10.0, 0.0, 8.0),
        ],
    ]);
    let slab = painted(&[[
        Vec3::new(-10.0, -12.0, 1.0),
        Vec3::new(10.0, -12.0, 1.0),
        Vec3::new(10.0, 12.0, 1.0),
        Vec3::new(-10.0, 12.0, 1.0),
    ]]);
    let interior_ink = |mesh: render::MeshData| {
        let mut r = renderer(200, 200);
        let handle = r.upload_mesh(&mesh);
        let mut scene = Scene::new(Camera::top_down(Vec2::ZERO, 36.0).with_tilt(14.0));
        scene.clear_color = Color::hex(0xffffff);
        let mut inst = MeshInstance::at(handle, Vec2::ZERO, 0.0, 1.0);
        inst.material.hatch = 0.0;
        scene.meshes.push(inst);
        let img = r.render_to_rgba(&scene).unwrap();
        let dark = |p: [u8; 4]| p[0] < 70 && p[1] < 50 && p[2] < 70;
        let white = |p: [u8; 4]| p[0] > 250 && p[1] > 250 && p[2] > 250;
        let mut n = 0;
        for y in 0..img.height {
            for x in 0..img.width {
                if !dark(img.pixel(x, y)) {
                    continue;
                }
                let mut near_bg = false;
                for dy in -10i32..=10 {
                    for dx in -10i32..=10 {
                        let xx = x as i32 + dx;
                        let yy = y as i32 + dy;
                        if !(0..img.width as i32).contains(&xx)
                            || !(0..img.height as i32).contains(&yy)
                            || white(img.pixel(xx as u32, yy as u32))
                        {
                            near_bg = true;
                        }
                    }
                }
                if !near_bg {
                    n += 1;
                }
            }
        }
        n
    };
    let ridge = interior_ink(roof);
    let flat = interior_ink(slab);
    assert!(ridge > 12, "roof ridge should carry inner ink, got {ridge}");
    assert!(flat < 6, "a flat slab has no interior crease, got {flat}");
}

#[test]
#[ignore = "needs a GPU"]
fn paused_card_covers_the_centre() {
    let mut r = renderer(640, 360);
    let mut scene = Scene::new(Camera::top_down(Vec2::ZERO, 50.0));
    scene.clear_color = Color::hex(0x000000);
    let before = r.render_to_rgba(&scene).unwrap();
    scene.overlay.paused = Some(PausedCard::default());
    let after = r.render_to_rgba(&scene).unwrap();
    // The card is parchment just inside its left edge, and the text adds ink and detail.
    let card = after.pixel(320 - 230, 180);
    assert!(card[0] > 200 && card[1] > 180, "{card:?}");
    assert_eq!(before.pixel(320, 180), [0, 0, 0, 255]);
    assert!(distinct_colors(&after) > distinct_colors(&before) + 5);
}

#[test]
#[ignore = "needs a GPU"]
fn removed_meshes_and_odd_input_are_survivable() {
    let mut r = renderer(128, 128);
    let m = r.upload_mesh(&kit::hull(&kit::HullParams::wren(), 1));
    let empty = r.upload_mesh(&render::MeshData::default());
    r.update_mesh(m, &kit::hull(&kit::HullParams::heron(), 1));
    let mut scene = Scene::new(Camera::perspective(
        Vec3::new(0.0, -30.0, 20.0),
        Vec3::ZERO,
        50.0,
    ));
    scene.meshes.push(MeshInstance::at(m, Vec2::ZERO, 0.0, 1.0));
    scene
        .meshes
        .push(MeshInstance::at(empty, Vec2::ZERO, 0.0, 1.0));
    scene
        .lines
        .push(Line::path(vec![Vec3::ZERO], 1.0, Color::WHITE));
    scene.glows.push(Glow {
        position: Vec3::NAN,
        radius: 1.0,
        color: Color::WHITE,
    });
    r.render_to_rgba(&scene).unwrap();
    r.remove_mesh(m);
    r.render_to_rgba(&scene).unwrap();
    r.resize(64, 32);
    let img = r.render_to_rgba(&scene).unwrap();
    assert_eq!((img.width, img.height), (64, 32));
}

#[test]
#[ignore = "needs a GPU"]
fn stale_mesh_handle_never_touches_a_reused_slot() {
    let mut r = renderer(200, 200);
    let iron = kit::rock(&RockParams::new(RockKind::Iron, 10.0, 1.0), 1);
    let old = r.upload_mesh(&iron);
    r.remove_mesh(old);
    // The freed slot is reused, but the new handle is distinct from the stale one.
    let new = r.upload_mesh(&iron);
    assert_ne!(old, new);
    // A stale remove must not free the live mesh.
    r.remove_mesh(old);
    let mut scene = Scene::new(Camera::top_down(Vec2::ZERO, 40.0));
    scene.clear_color = Color::hex(0xffffff);
    scene
        .meshes
        .push(MeshInstance::at(new, Vec2::ZERO, 0.0, 1.0));
    let img = r.render_to_rgba(&scene).unwrap();
    assert_ne!(
        img.pixel(100, 100),
        [255, 255, 255, 255],
        "live mesh still drawn"
    );
}
