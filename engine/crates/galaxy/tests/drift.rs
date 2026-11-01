//! The `drift` test module (`engine/tests/play/drift`): its models read, a shot draws it,
//! and the program that made its models. Flight on it is tested in
//! `crates/sim/tests/flight.rs`.

use std::path::{Path, PathBuf};
use std::process::Command;

use galaxy_mesh::gltf::{Part, read, write};
use galaxy_mesh::{Color, MeshBuilder, MeshData};
use galaxy_render::glam::{Mat4, Quat, Vec2, Vec3};
use serde_json::Value;

fn drift() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/play/drift")
}

const MODELS: [&str; 5] = ["ship", "rock", "beacon", "hub", "pad"];

#[test]
fn the_committed_models_read_and_are_well_formed() {
    for name in MODELS {
        let path = drift().join("models").join(format!("{name}.gltf"));
        let text = std::fs::read_to_string(&path).unwrap();
        let mesh = read(&text).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert!(mesh.is_well_formed(), "{name}");
        assert!(mesh.triangle_count() >= 12, "{name}");
    }
    let rock = read(&std::fs::read_to_string(drift().join("models/rock.gltf")).unwrap()).unwrap();
    let (lo, hi) = rock.bounds().unwrap();
    let (mlo, mhi) = rock.morphed(1.0).bounds().unwrap();
    assert!(
        (mhi - mlo).length() < (hi - lo).length() * 0.8,
        "the rock morphs smaller"
    );
    let beacon =
        read(&std::fs::read_to_string(drift().join("models/beacon.gltf")).unwrap()).unwrap();
    assert!(
        beacon.vertices.iter().any(|v| v.color[3] == 1.0),
        "the lamp glows"
    );
}

#[test]
#[ignore = "needs a GPU: galaxy shot draws the test module"]
fn a_shot_draws_the_module() {
    let tmp = tempfile::tempdir().unwrap();
    let png = tmp.path().join("drift.png");
    let out = Command::new(env!("CARGO_BIN_EXE_engine-galaxy"))
        .args(["shot", "--modules"])
        .arg(drift())
        .arg("--out")
        .arg(&png)
        .args(["--size", "640x360"])
        .env_remove("GALAXY_INSTALL_ROOT")
        .output()
        .unwrap();
    let json: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(json["ok"], true, "{json}");
    assert_eq!(json["models_drawn"], 11, "{json}");
    assert_eq!(json["problems"], serde_json::json!([]), "{json}");
    let bytes = std::fs::read(&png).unwrap();
    assert_eq!(&bytes[1..4], b"PNG");
}

fn film(args: &[&str], out: &Path) -> Value {
    let o = Command::new(env!("CARGO_BIN_EXE_engine-galaxy"))
        .args(["film", "--modules"])
        .arg(drift())
        .args(args)
        .arg("--out")
        .arg(out)
        .env_remove("GALAXY_INSTALL_ROOT")
        .output()
        .unwrap();
    serde_json::from_slice(&o.stdout).unwrap()
}

fn inputs(name: &str) -> String {
    drift()
        .join("../films/inputs")
        .join(name)
        .display()
        .to_string()
}

#[test]
fn a_probes_only_film_needs_no_gpu_and_the_course_stays_still() {
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path().join("leg");
    let json = film(
        &[
            "--inputs",
            &inputs("route.toml"),
            "--from",
            "4",
            "--to",
            "34",
            "--probes-only",
        ],
        &out,
    );
    assert_eq!(json["ok"], true, "{json}");
    assert_eq!(
        (json["frames"].as_u64(), json["drawn"].as_bool()),
        (Some(61), Some(false))
    );
    let film = galaxy_motion::Film::read(&out).unwrap();
    // Frames land on ticks and half way between, as the window draws them at 60 fps.
    let f = film.frames();
    assert_eq!(
        (f[0].tick, f[0].alpha, f[1].tick, f[1].alpha),
        (4, 0.0, 4, 0.5)
    );
    assert!(f[0].things["drift.ship"].followed);
    let dots = galaxy_motion::run("dots", &out, &galaxy_motion::Opts::new()).unwrap();
    assert!(
        dots["drift_px_per_frame"].as_f64().unwrap().abs() < 0.01,
        "{dots}"
    );
    assert_eq!(dots["steps"], 60, "{dots}");
}

#[test]
fn the_wheel_in_an_inputs_file_zooms_the_film() {
    let tmp = tempfile::tempdir().unwrap();
    let script = tmp.path().join("zoom.toml");
    std::fs::write(
        &script,
        "[[zoom]]
tick = 10
steps = 3.0
",
    )
    .unwrap();
    let out = tmp.path().join("f");
    let json = film(
        &[
            "--inputs",
            script.to_str().unwrap(),
            "--to",
            "20",
            "--probes-only",
        ],
        &out,
    );
    assert_eq!(json["ok"], true, "{json}");
    let film = galaxy_motion::Film::read(&out).unwrap();
    let at = |tick: u64| film.frames().iter().find(|f| f.tick == tick).unwrap();
    assert_eq!(at(9).zoom, None);
    let closer = at(10).camera.per_pixel / at(9).camera.per_pixel;
    assert!((closer - 0.88f32.powi(3)).abs() < 1e-3, "{closer}");
}

#[test]
#[ignore = "needs a GPU: films the course and reads its pixels"]
fn a_film_draws_every_frame_and_the_course_stays_still_in_its_pixels() {
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path().join("leg");
    let json = film(
        &[
            "--inputs",
            &inputs("route.toml"),
            "--from",
            "4",
            "--to",
            "34",
            "--size",
            "640x360",
        ],
        &out,
    );
    assert_eq!(json["ok"], true, "{json}");
    assert!(out.join("frames/00060.png").is_file());
    let strip = galaxy_motion::run("strip", &out, &galaxy_motion::Opts::new()).unwrap();
    assert!(
        strip["drift_px_per_frame"].as_f64().unwrap().abs() < 0.1,
        "{strip}"
    );
    assert!(strip["confidence"].as_f64().unwrap() > 0.8, "{strip}");
}

// --- The models' maker. Run it on purpose, then commit what it wrote:
//     cargo xtask cargo test -p galaxy --test drift -- --ignored make_the_models

fn paint(b: &mut MeshBuilder, hex: u32, glow: f32) {
    b.paint(Color::hex(hex), glow);
}

/// A small ship, nose along +x: a wedge hull, two wings, a canopy and an engine glow.
fn ship() -> Vec<(&'static str, MeshData)> {
    let mut hull = MeshBuilder::new();
    paint(&mut hull, 0xefdcb4, 0.0);
    let outline = [
        Vec2::new(2.4, 0.0),
        Vec2::new(0.6, 0.7),
        Vec2::new(-1.6, 0.8),
        Vec2::new(-1.8, 0.0),
        Vec2::new(-1.6, -0.8),
        Vec2::new(0.6, -0.7),
    ];
    hull.with(Mat4::from_translation(Vec3::new(0.0, 0.0, -0.35)), |b| {
        b.extrude(&outline, 0.7, 0.7);
    });
    let mut wings = MeshBuilder::new();
    paint(&mut wings, 0xd9774b, 0.0);
    for side in [1.0, -1.0] {
        let wing = [
            Vec2::new(0.2, 0.6 * side),
            Vec2::new(-1.4, 2.0 * side),
            Vec2::new(-1.7, 2.0 * side),
            Vec2::new(-1.3, 0.6 * side),
        ];
        let wing: Vec<Vec2> = if side > 0.0 {
            wing.to_vec()
        } else {
            wing.iter().rev().copied().collect()
        };
        wings.with(Mat4::from_translation(Vec3::new(0.0, 0.0, -0.12)), |b| {
            b.extrude(&wing, 0.24, 0.9);
        });
    }
    let mut canopy = MeshBuilder::new();
    paint(&mut canopy, 0x7fd6e0, 0.0);
    canopy.with(
        Mat4::from_scale_rotation_translation(
            Vec3::new(1.0, 0.7, 0.6),
            Quat::IDENTITY,
            Vec3::new(0.3, 0.0, 0.3),
        ),
        |b| b.lathe(&[(0.0, 0.0), (0.55, 0.05), (0.4, 0.35), (0.0, 0.5)], 8, 0.0),
    );
    let mut glow = MeshBuilder::new();
    paint(&mut glow, 0xffcf6b, 1.0);
    glow.with(Mat4::from_translation(Vec3::new(-1.85, 0.0, 0.0)), |b| {
        b.cuboid(Vec3::new(0.2, 0.9, 0.35));
    });
    vec![
        ("hull", hull.build()),
        ("wings", wings.build()),
        ("canopy", canopy.build()),
        ("engine", glow.build()),
    ]
}

/// A lumpy rock about two units across, with a morph target a third smaller and flatter
/// (as if mined), and a fresh cut on the target.
fn rock() -> Vec<(&'static str, MeshData)> {
    // An octahedron's corners pushed about, then each face split in four.
    let corners = [
        Vec3::new(1.1, 0.1, 0.0),
        Vec3::new(-0.9, -0.2, 0.1),
        Vec3::new(0.1, 1.0, -0.1),
        Vec3::new(-0.2, -1.05, 0.0),
        Vec3::new(0.15, 0.1, 0.9),
        Vec3::new(-0.1, 0.0, -0.8),
    ];
    let faces = [
        [0, 2, 4],
        [2, 1, 4],
        [1, 3, 4],
        [3, 0, 4],
        [2, 0, 5],
        [1, 2, 5],
        [3, 1, 5],
        [0, 3, 5],
    ];
    let bumps = [
        1.12, 0.94, 1.05, 0.9, 1.08, 0.97, 1.1, 0.92, 1.0, 1.06, 0.95, 1.03,
    ];
    let build = |scale: Vec3, cut: f32| {
        let mut b = MeshBuilder::new();
        paint(&mut b, 0xb98a6a, 0.0);
        b.bite(cut);
        b.part(|b| {
            for (i, f) in faces.iter().enumerate() {
                let [a, c, d] = f.map(|k| corners[k] * scale);
                let mid = |p: Vec3, q: Vec3, k: usize| (p + q) * 0.5 * bumps[(i + k) % bumps.len()];
                let (ab, bc, ca) = (mid(a, c, 0), mid(c, d, 1), mid(d, a, 2));
                b.tri(a, ab, ca);
                b.tri(ab, c, bc);
                b.tri(ca, bc, d);
                b.tri(ab, bc, ca);
            }
        });
        b.build()
    };
    let mut full = build(Vec3::ONE, 0.0);
    let mined = build(Vec3::new(0.62, 0.62, 0.5), 0.8);
    assert!(full.morph_to(&mined), "same topology");
    vec![("rock", full)]
}

/// A post with a glowing lamp on top.
fn beacon() -> Vec<(&'static str, MeshData)> {
    let mut post = MeshBuilder::new();
    paint(&mut post, 0x3fa7a0, 0.0);
    post.frustum(0.35, 0.2, 2.4, 6);
    let mut lamp = MeshBuilder::new();
    paint(&mut lamp, 0xffcf6b, 1.0);
    lamp.with(Mat4::from_translation(Vec3::new(0.0, 0.0, 2.4)), |b| {
        b.lathe(&[(0.0, 0.0), (0.5, 0.15), (0.45, 0.6), (0.0, 0.8)], 8, 0.0);
    });
    vec![("post", post.build()), ("lamp", lamp.build())]
}

/// A round hub about 14 across: a squat drum with a ring round it and three towers.
fn hub() -> Vec<(&'static str, MeshData)> {
    let mut drum = MeshBuilder::new();
    paint(&mut drum, 0xefdcb4, 0.0);
    drum.with(Mat4::from_translation(Vec3::new(0.0, 0.0, -1.5)), |b| {
        b.lathe(
            &[(0.0, 0.0), (6.0, 0.0), (6.4, 1.2), (5.6, 3.0), (0.0, 3.4)],
            16,
            0.0,
        );
    });
    let mut ring = MeshBuilder::new();
    paint(&mut ring, 0xd9774b, 0.0);
    ring.torus(7.0, 0.7, 0.5, 20, 6);
    let mut towers = MeshBuilder::new();
    paint(&mut towers, 0x3fa7a0, 0.0);
    let mut lamps = MeshBuilder::new();
    paint(&mut lamps, 0xffe29a, 1.0);
    for (x, y) in [(2.5, 1.0), (-2.0, 2.2), (-0.5, -2.6)] {
        towers.with(Mat4::from_translation(Vec3::new(x, y, 1.5)), |b| {
            b.frustum(0.9, 0.6, 3.0, 6);
        });
        lamps.with(Mat4::from_translation(Vec3::new(x, y, 4.75)), |b| {
            b.cuboid(Vec3::new(0.7, 0.7, 0.5));
        });
    }
    vec![
        ("drum", drum.build()),
        ("ring", ring.build()),
        ("towers", towers.build()),
        ("lamps", lamps.build()),
    ]
}

/// A flat landing pad with a glowing rim, three units across.
fn pad() -> Vec<(&'static str, MeshData)> {
    let mut plate = MeshBuilder::new();
    paint(&mut plate, 0xc98f4a, 0.0);
    plate.frustum(1.5, 1.3, 0.15, 6);
    let mut rim = MeshBuilder::new();
    paint(&mut rim, 0xffcf6b, 1.0);
    rim.with(Mat4::from_translation(Vec3::new(0.0, 0.0, 0.15)), |b| {
        b.frustum(1.3, 1.1, 0.08, 6);
    });
    vec![("plate", plate.build()), ("rim", rim.build())]
}

/// Makes a model's parts, each with its node name.
type Maker = fn() -> Vec<(&'static str, MeshData)>;

#[test]
#[ignore = "writes the drift models; run on purpose and commit the result"]
fn make_the_models() {
    let makers: [(&str, Maker); 5] = [
        ("ship", ship),
        ("rock", rock),
        ("beacon", beacon),
        ("hub", hub),
        ("pad", pad),
    ];
    for (name, make) in makers {
        let parts = make();
        let parts: Vec<Part> = parts
            .iter()
            .map(|(n, m)| Part { name: n, mesh: m })
            .collect();
        let path = drift().join("models").join(format!("{name}.gltf"));
        std::fs::write(&path, write(&parts)).unwrap();
    }
}
