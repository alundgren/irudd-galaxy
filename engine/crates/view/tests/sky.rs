//! Depth in the sky on the generic `tests/play/depth/sky` module, drawn only: its layers and
//! landmarks reach the scene far to near, slide at their parallax as the ship flies,
//! hold still with reduced motion, scatter the same way from the same seed, and leave
//! the world exactly as it would be without them.
use galaxy_module::module::Module;
use galaxy_render::far::{Far, Mark, Outline};
use galaxy_render::glam::Vec2;
use galaxy_sim::{Input, Session};
use galaxy_view::View;
use serde_json::{Map, Value};
use std::path::{Path, PathBuf};

const SIZE: Vec2 = Vec2::new(1280.0, 720.0);

fn folder() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/play/depth/sky")
}

fn module() -> Module {
    Module::read(&folder()).unwrap()
}

fn key(tick: u64, name: &str, down: bool) -> Input {
    let mut payload = Map::new();
    payload.insert("down".into(), Value::Bool(down));
    Input {
        tick,
        source: "keyboard".into(),
        name: format!("sky.{name}"),
        payload,
    }
}

/// Flies ahead with boost for `ticks`, keeping the view's history.
fn fly(s: &mut Session, v: &mut View, ticks: u64) {
    for _ in 0..ticks {
        let next = s.world.tick + 1;
        let inputs = if next == 2 {
            vec![key(next, "thrust_ahead", true), key(next, "boost", true)]
        } else {
            Vec::new()
        };
        v.before_step(&s.world);
        s.step(&inputs).unwrap();
    }
}

fn kind(f: &Far) -> &'static str {
    match f {
        Far::Field(f) => match f.mark {
            Mark::Dust => "dust",
            Mark::Speck => "speck",
            Mark::Hatch => "hatch",
        },
        Far::Shape(s) => match s.outline {
            Outline::Disc => "disc",
            Outline::Wash => "wash",
        },
        Far::Meshes(_) => "meshes",
    }
}

/// Where the disc and the wash are on the plane, from the camera's target.
fn shapes_from_camera(v: &mut View, s: &Session) -> Vec<Vec2> {
    let scene = v.scene(&s.world, 1.0, SIZE, 0.0);
    let target = scene.camera.target.truncate();
    scene
        .far
        .iter()
        .filter_map(|f| match f {
            Far::Shape(s) => Some(s.center.truncate() - target),
            _ => None,
        })
        .collect()
}

#[test]
fn layers_and_landmarks_draw_far_to_near_behind_the_play_layer() {
    let mut s = Session::new(vec![module()], 1).unwrap();
    let mut v = View::detached(s.modules(), 1);
    fly(&mut s, &mut v, 5);
    let scene = v.scene(&s.world, 1.0, SIZE, 0.0);
    let kinds: Vec<&str> = scene.far.iter().map(kind).collect();
    assert_eq!(
        kinds,
        ["hatch", "disc", "wash", "dust", "speck", "meshes", "meshes"]
    );
    // The far hub and the small rocks wear the far look and fade into the haze; the
    // play layer's own meshes don't.
    for f in &scene.far {
        if let Far::Meshes(m) = f {
            assert!(m.iter().all(|i| i.haze.a > 0.2 && i.haze.a < 0.6));
        }
    }
    assert!(scene.meshes.iter().all(|m| m.haze.a == 0.0));
    assert!(v.problems().is_empty(), "{:?}", v.problems());
}

#[test]
fn landmarks_slide_at_their_parallax_and_hold_still_with_reduced_motion() {
    let mut s = Session::new(vec![module()], 1).unwrap();
    let mut v = View::detached(s.modules(), 1);
    fly(&mut s, &mut v, 2);
    let start = s.world.tick;
    let target_at = |v: &mut View, s: &Session| v.scene(&s.world, 1.0, SIZE, 0.0).camera.target;
    let t0 = target_at(&mut v, &s);
    let before = shapes_from_camera(&mut v, &s);
    fly(&mut s, &mut v, 90);
    let t1 = target_at(&mut v, &s);
    let after = shapes_from_camera(&mut v, &s);
    let moved = (t1 - t0).truncate();
    assert!(moved.length() > 20.0, "the ship flew {moved}");
    // The disc (parallax 0.03) and the wash (0.05) fall back on screen by that share
    // of the camera's travel: far less than the play layer, which falls back by all of it.
    for (i, parallax) in [0.03, 0.05].into_iter().enumerate() {
        let slid = after[i] - before[i];
        assert!(
            (slid + moved * parallax).length() < 1e-3,
            "{i}: {slid} vs {moved}"
        );
    }
    // With reduced motion nothing in the sky moves on screen.
    let mut still = Session::new(vec![module()], 1).unwrap();
    let mut v = View::detached(still.modules(), 1);
    v.set_reduced_motion(true);
    fly(&mut still, &mut v, 2);
    let before = shapes_from_camera(&mut v, &still);
    fly(&mut still, &mut v, s.world.tick - start);
    let after = shapes_from_camera(&mut v, &still);
    for (a, b) in before.iter().zip(&after) {
        assert!((*a - *b).length() < 1e-3, "{a} {b}");
    }
    let scene = v.scene(&still.world, 1.0, SIZE, 0.0);
    assert!(scene.still_sky);
}

#[test]
fn the_sky_is_seeded_and_never_touches_the_world() {
    let scatter = |seed: u64| {
        let mut s = Session::new(vec![module()], seed).unwrap();
        let mut v = View::detached(s.modules(), seed);
        fly(&mut s, &mut v, 60);
        let scene = v.scene(&s.world, 1.0, SIZE, 0.0);
        (scene.far, s.world.hash())
    };
    let (a, hash) = scatter(3);
    assert_eq!(a, scatter(3).0, "the same seed scatters the same sky");
    assert_ne!(a, scatter(4).0, "another seed scatters it differently");
    // The same flight without the sky's layers and landmarks ends on the same world.
    let dir = tempfile::tempdir().unwrap();
    for entry in std::fs::read_dir(folder()).unwrap() {
        let entry = entry.unwrap();
        if entry.file_type().unwrap().is_dir() {
            continue;
        }
        std::fs::copy(entry.path(), dir.path().join(entry.file_name())).unwrap();
    }
    for sub in ["models", "scripts"] {
        std::fs::create_dir_all(dir.path().join(sub)).unwrap();
        for entry in std::fs::read_dir(folder().join(sub)).unwrap() {
            let entry = entry.unwrap();
            std::fs::copy(entry.path(), dir.path().join(sub).join(entry.file_name())).unwrap();
        }
    }
    std::fs::write(dir.path().join("style.toml"), "").unwrap();
    let plain = Module::read(dir.path()).unwrap();
    let mut s = Session::new(vec![plain], 3).unwrap();
    let mut v = View::detached(s.modules(), 3);
    fly(&mut s, &mut v, 60);
    assert!(v.scene(&s.world, 1.0, SIZE, 0.0).far.is_empty());
    assert_eq!(s.world.hash(), hash);
}
