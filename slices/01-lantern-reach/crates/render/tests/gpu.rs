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
fn a_sloped_face_stays_painted_at_every_zoom() {
    let mut b = render::MeshBuilder::new();
    b.paint(Color::hex(0xf4e2c4), 0.0);
    b.quad(
        Vec3::new(-15.0, -15.0, -30.0),
        Vec3::new(15.0, -15.0, -30.0),
        Vec3::new(15.0, 15.0, 30.0),
        Vec3::new(-15.0, 15.0, 30.0),
    );
    let mesh = b.build();
    for height in [40.0, 160.0, 640.0] {
        let mut r = renderer(400, 400);
        let handle = r.upload_mesh(&mesh);
        let mut scene = Scene::new(Camera::top_down(Vec2::ZERO, height));
        scene.clear_color = Color::hex(0xffffff);
        let mut inst = MeshInstance::at(handle, Vec2::ZERO, 0.0, 1.0);
        inst.material.hatch = 0.0;
        scene.meshes.push(inst);
        let img = r.render_to_rgba(&scene).unwrap();
        let centre = img.pixel(200, 200);
        let ink = |p: [u8; 4]| p[0] < 70 && p[1] < 50 && p[2] < 70;
        assert!(
            centre[0] > 80 && centre[1] > 60 && centre[0] > centre[2] && !ink(centre),
            "zoom {height} centre should be painted, got {centre:?}"
        );
        let mut dark = 0;
        for dy in -2i32..=2 {
            for dx in -2i32..=2 {
                if ink(img.pixel((200 + dx) as u32, (200 + dy) as u32)) {
                    dark += 1;
                }
            }
        }
        assert_eq!(dark, 0, "zoom {height} filled the slope with ink");
    }
}

#[test]
#[ignore = "needs a GPU"]
fn a_foreground_beam_draws_over_a_roof_crease() {
    let mut b = render::MeshBuilder::new();
    b.paint(Color::hex(0xf4e2c4), 0.0);
    b.quad(
        Vec3::new(-10.0, -12.0, 0.0),
        Vec3::new(10.0, -12.0, 0.0),
        Vec3::new(10.0, 0.0, 8.0),
        Vec3::new(-10.0, 0.0, 8.0),
    );
    b.quad(
        Vec3::new(10.0, 12.0, 0.0),
        Vec3::new(-10.0, 12.0, 0.0),
        Vec3::new(-10.0, 0.0, 8.0),
        Vec3::new(10.0, 0.0, 8.0),
    );
    let mut r = renderer(200, 200);
    let handle = r.upload_mesh(&b.build());
    let mut scene = Scene::new(Camera::top_down(Vec2::ZERO, 36.0));
    scene.clear_color = Color::hex(0xffffff);
    let mut inst = MeshInstance::at(handle, Vec2::ZERO, 0.0, 1.0);
    inst.material.hatch = 0.0;
    scene.meshes.push(inst);
    let before = r.render_to_rgba(&scene).unwrap();
    scene.lines.push(Line::beam(
        Vec3::new(-20.0, 0.0, 40.0),
        Vec3::new(20.0, 0.0, 40.0),
        2.5,
        Color::hex(0x7ff0d8),
    ));
    let after = r.render_to_rgba(&scene).unwrap();
    let luma = |p: [u8; 4]| p[0] as i32 + p[1] as i32 + p[2] as i32;
    let over = after.pixel(100, 100);
    assert!(
        luma(over) > luma(before.pixel(100, 100)) + 40,
        "beam should cross the ridge, before {:?} after {over:?}",
        before.pixel(100, 100)
    );
    // The beam is a band, not a tint over the whole roof. Additive on white
    // clamps, so the check stays on the painted face, off the beam.
    let off = after.pixel(100, 140);
    assert!(
        luma(over) > luma(off) + 30,
        "beam {over:?} should be brighter than the roof beside it {off:?}"
    );
    assert_eq!(after.pixel(2, 2), [255, 255, 255, 255]);
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

/// Changed pixels at the centre of a changed 5x5 block: a patch changing tone by more
/// than a cel band's worth, not an edge or a thick outline moving by a pixel or two.
fn solid_change(a: &RgbaImage, b: &RgbaImage) -> usize {
    let (w, h) = (a.width as i32, a.height as i32);
    let changed = |x: i32, y: i32| {
        let (p, q) = (a.pixel(x as u32, y as u32), b.pixel(x as u32, y as u32));
        (0..3).any(|c| (p[c] as i32 - q[c] as i32).abs() > 48)
    };
    let mut n = 0;
    for y in 2..h - 2 {
        for x in 2..w - 2 {
            if (-2..=2).all(|dy| (-2..=2).all(|dx| changed(x + dx, y + dy))) {
                n += 1;
            }
        }
    }
    n
}

#[test]
#[ignore = "needs a GPU"]
fn a_slowly_mined_rock_never_pops_a_block() {
    // Mapped as the game does: meshed at every 1/20 of `remaining`, morphing towards
    // the next step in between, turning, and scaled with what is left. A facet
    // flipping cel band, or a crack or chip appearing at once, changes a solid patch
    // between two frames.
    let mut r = renderer(320, 320);
    let mut scene = Scene::new(Camera::top_down(Vec2::ZERO, 52.0).with_tilt(14.0));
    let mut worst = (0, 0);
    for kind in [RockKind::Ice, RockKind::Iron, RockKind::Glassite] {
        let mesh_at = |step: i32| {
            let params = RockParams::new(kind, 10.0, step as f32 / 20.0);
            kit::rock_morph(&params, (step - 1) as f32 / 20.0, 42)
        };
        let mut step = 20;
        let handle = r.upload_mesh(&mesh_at(step));
        let mut last: Option<RgbaImage> = None;
        for frame in 0..90 {
            let remaining = 1.0 - frame as f32 * 0.01;
            let s = (remaining * 20.0).ceil() as i32;
            if s != step {
                step = s;
                r.update_mesh(handle, &mesh_at(step));
            }
            let morph = (step as f32 - remaining * 20.0).clamp(0.0, 1.0);
            let inst = MeshInstance::at(
                handle,
                Vec2::ZERO,
                frame as f32 * 0.012,
                kit::rock_scale(remaining),
            )
            .with_morph(morph)
            .with_hatch_scale(1.0);
            scene.meshes = vec![inst];
            let img = r.render_to_rgba(&scene).unwrap();
            if let Some(prev) = &last {
                let n = solid_change(prev, &img);
                if n > worst.0 {
                    worst = (n, frame);
                }
            }
            last = Some(img);
        }
        r.remove_mesh(handle);
    }
    // The rock is about 110 pixels across. The flat-shaded rocks this replaced popped
    // over 2 000 pixels in a frame, and stepped re-meshes about 20; morphing between
    // steps changes no solid patch at all.
    assert!(
        worst.0 < 10,
        "{} pixels changed as a solid patch at frame {}",
        worst.0,
        worst.1
    );
}

#[test]
#[ignore = "needs a GPU"]
fn a_mining_beam_reaches_the_rock_and_its_glow_is_whole() {
    let mut r = renderer(300, 300);
    let data = kit::rock(&RockParams::new(RockKind::Iron, 10.0, 0.6), 3);
    let rock = r.upload_mesh(&data);
    let camera = Camera::top_down(Vec2::ZERO, 50.0).with_tilt(14.0);
    let mut scene = Scene::new(camera);
    scene.clear_color = Color::hex(0x202040);
    scene
        .meshes
        .push(MeshInstance::at(rock, Vec2::ZERO, 0.0, 1.0));
    let from = Vec3::new(-22.0, 4.0, 12.0);
    let t = data.raycast(from, -from).expect("the beam meets the rock");
    let hit = from - from * t;
    let before = r.render_to_rgba(&scene).unwrap();
    scene
        .lines
        .push(Line::beam(from, hit, 2.0, Color::hex(0x7ff0d8)));
    let lit = r.render_to_rgba(&scene).unwrap();
    scene.lines.clear();
    scene.glows.push(Glow {
        position: hit,
        radius: 4.0,
        color: Color::hex(0xbaffef),
    });
    let glow = r.render_to_rgba(&scene).unwrap();

    let view = r.logical_size();
    let px = |p: Vec3| {
        let s = camera.world_to_screen(p, view).unwrap();
        (s.x.round() as u32, s.y.round() as u32)
    };
    let luma = |img: &RgbaImage, (x, y): (u32, u32)| {
        let p = img.pixel(x, y);
        p[0] as i32 + p[1] as i32 + p[2] as i32
    };
    // The beam is drawn right up to where it lands, not chopped off by the rock.
    let dir = (hit - from).normalize();
    let near = px(hit - dir * 2.0);
    assert!(
        luma(&lit, near) > luma(&before, near) + 60,
        "beam at its landing point: {} -> {}",
        luma(&before, near),
        luma(&lit, near)
    );
    // The landing glow is a whole disc: as bright on the rock's side as on the ship's.
    let (inside, outside) = (px(hit + dir * 2.0), px(hit - dir * 2.0));
    let gain = |p| luma(&glow, p) - luma(&before, p);
    assert!(gain(inside) > 60, "glow over the rock: {}", gain(inside));
    assert!(
        gain(inside) * 2 > gain(outside),
        "glow cut by the rock: {} vs {}",
        gain(inside),
        gain(outside)
    );
}

/// Where a GPU test may save what it drew, for a person to look at: the folder in
/// `GALAXY_GPU_SHOTS`, if set.
fn keep_shot(img: &RgbaImage, name: &str) {
    if let Some(dir) = std::env::var_os("GALAXY_GPU_SHOTS") {
        img.save_png(std::path::Path::new(&dir).join(name)).unwrap();
    }
}

#[test]
#[ignore = "needs a GPU"]
fn every_fitted_part_reads_at_the_flight_zoom() {
    use kit::{HullParams, LookId, ShipFitLook};
    // As in flight: 550 u visible, tilted, a ship five times its kit size, hovering.
    let mut r = renderer(1280, 720);
    let camera = Camera::top_down(Vec2::ZERO, 550.0).with_tilt(14.0);
    let ship = |r: &mut Renderer, hull: &HullParams, look: ShipFitLook, at: Vec2| {
        let mesh = r.upload_mesh(&kit::fitted_hull(hull, 1, &look));
        let mut inst = MeshInstance::at(mesh, at, 0.6, 5.0);
        inst.transform = render::glam::Mat4::from_translation(Vec3::Z * 22.0) * inst.transform;
        inst
    };
    let mut scene = Scene::new(camera);
    scene.clear_color = Color::hex(0x2a1f48);
    let mut thin = Vec::new();
    for hull in [
        HullParams::wren(),
        HullParams::tandem(),
        HullParams::heron(),
    ] {
        let bare = {
            let mut s = scene.clone();
            s.meshes
                .push(ship(&mut r, &hull, ShipFitLook::default(), Vec2::ZERO));
            r.render_to_rgba(&s).unwrap()
        };
        for look in LookId::ALL {
            let fit = ShipFitLook::default().with(look.socket(), look);
            let mut s = scene.clone();
            s.meshes.push(ship(&mut r, &hull, fit, Vec2::ZERO));
            let worn = r.render_to_rgba(&s).unwrap();
            // Pixels the part visibly changes. A part a player can spot at a glance
            // changes a patch of at least 6 by 6 pixels.
            let changed = bare
                .pixels
                .as_chunks::<4>()
                .0
                .iter()
                .zip(worn.pixels.as_chunks::<4>().0)
                .filter(|(a, b)| (0..3).map(|i| a[i].abs_diff(b[i]) as u32).sum::<u32>() > 90)
                .count();
            eprintln!("{look:?} on {:?}: {changed} pixels", hull.class);
            if changed < 36 {
                thin.push(format!("{look:?} on {:?}: {changed} pixels", hull.class));
            }
        }
    }
    // A row of Wrens for the eye: bare, each part, then everything.
    let wren = HullParams::wren();
    let mut looks = vec![ShipFitLook::default()];
    looks.extend(
        LookId::ALL
            .iter()
            .map(|&l| ShipFitLook::default().with(l.socket(), l)),
    );
    looks.push(ShipFitLook::from_pairs([
        (kit::Socket::Nose, LookId::Lens),
        (kit::Socket::Utility, LookId::SeamLamp),
        (kit::Socket::Belly, LookId::Pods),
        (kit::Socket::Tail, LookId::Fins),
    ]));
    let mut row = scene.clone();
    for (i, look) in looks.into_iter().enumerate() {
        let at = Vec2::new(-390.0 + i as f32 * 130.0, 0.0);
        row.meshes.push(ship(&mut r, &wren, look, at));
    }
    keep_shot(&r.render_to_rgba(&row).unwrap(), "parts-at-flight-zoom.png");
    assert!(thin.is_empty(), "too small to spot: {thin:?}");
}

#[test]
#[ignore = "needs a GPU"]
fn the_cradle_draws_the_ship_large_with_socket_rings() {
    use kit::{Cradle, CradlePose, HullParams, LookId, ShipFitLook, Socket, SocketRing};
    let mut r = renderer(1280, 720);
    let view = r.logical_size();
    let hull = HullParams::wren();
    let look = ShipFitLook::default().with(Socket::Nose, LookId::Lens);
    let ship = r.upload_mesh(&kit::fitted_hull(&hull, 1, &look));
    let ghost = r.upload_mesh(&kit::part_ghost(LookId::Pods, &hull));
    let cradle = Cradle {
        ship,
        hull: hull.clone(),
        look,
        pose: CradlePose::fit(Vec2::ZERO, 80.0, &hull),
        rings: [
            SocketRing::Fitted,
            SocketRing::Empty,
            SocketRing::DropTarget,
            SocketRing::Empty,
        ],
        ghost: Some(ghost),
        shadow: kit::CRADLE_SHADOW,
    };
    let mut scene = Scene::new(Camera::top_down(Vec2::ZERO, 170.0).with_tilt(14.0));
    // A plank floor's brown, so the shadow has something to fall on.
    scene.clear_color = Color::hex(0x8a6440);
    let sockets = cradle.draw(&mut scene, view);
    let img = r.render_to_rgba(&scene).unwrap();
    keep_shot(&img, "cradle.png");

    // Count pixels of a colour around a socket's ring.
    let around = |s: &kit::CradleSocket, want: &dyn Fn([u8; 4]) -> bool| {
        let c = s.screen.unwrap();
        (0..72)
            .filter(|i| {
                let a = *i as f32 / 72.0 * std::f32::consts::TAU;
                let p = c + Vec2::new(a.cos(), a.sin()) * s.radius_px;
                want(img.pixel(p.x.round() as u32, p.y.round() as u32))
            })
            .count()
    };
    let sky = |p: [u8; 4]| p[2] > 170 && p[2] as i32 > p[0] as i32 + 60;
    let gold = |p: [u8; 4]| p[0] > 190 && p[1] > 130 && p[1] < 210 && p[2] < 130;
    let drop = &sockets[2];
    assert_eq!(drop.socket, Socket::Belly);
    assert!(
        around(drop, &sky) >= 20,
        "a dashed sky-blue ring on the hold"
    );
    assert!(
        around(&sockets[0], &gold) >= 30,
        "a gold ring on the drill mount"
    );
    assert!(around(&sockets[3], &sky) == 0, "no drop ring on the engine");

    // The shadow: without it, a patch to the bottom right of the ship is lighter.
    let mut lit = scene.clone();
    lit.meshes.remove(0);
    let bare = r.render_to_rgba(&lit).unwrap();
    let darker = img
        .pixels
        .as_chunks::<4>()
        .0
        .iter()
        .zip(bare.pixels.as_chunks::<4>().0)
        .filter(|(a, b)| {
            (a[0] as i32 + a[1] as i32 + a[2] as i32) + 120
                < b[0] as i32 + b[1] as i32 + b[2] as i32
        })
        .count();
    assert!(
        darker > 3000,
        "the lifted ship casts a shadow: {darker} pixels"
    );
}
