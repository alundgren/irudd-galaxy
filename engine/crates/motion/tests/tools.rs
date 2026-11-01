//! Every tool on small films made here, each with a known defect and without it: the
//! tool must flag the one and not the other. No GPU: the frames are drawn in memory.

use std::collections::BTreeMap;
use std::path::Path;

use galaxy_motion::film::{Cam, FORMAT, Frame, LineProbe, Manifest, Thing};
use galaxy_motion::image::{Image, Rgba};
use galaxy_motion::{Opts, run};
use serde_json::Value;

const W: u32 = 320;
const H: u32 = 180;
const BG: Rgba = [30, 24, 48, 255];
const DOT: Rgba = [255, 210, 110, 255];

/// A camera that shows world units as pixels, +y up, origin at the bottom left.
fn camera() -> Cam {
    let mut m = [0.0; 16];
    m[0] = 2.0 / W as f32;
    m[5] = 2.0 / H as f32;
    m[10] = 0.001;
    m[12] = -1.0;
    m[13] = -1.0;
    m[15] = 1.0;
    Cam {
        eye: [0.0, 0.0, 100.0],
        target: [0.0; 3],
        view_proj: m,
        per_pixel: 1.0,
    }
}

/// What a test film shows on frame `i`.
struct Shot {
    image: Image,
    things: BTreeMap<String, Thing>,
    lines: Vec<LineProbe>,
}

fn thing(x: f32, y: f32) -> Thing {
    Thing {
        x,
        y,
        height: 0.0,
        angle: 0.0,
        screen: Some([x, H as f32 - y]),
        on_screen: true,
        followed: false,
    }
}

/// Writes a film of `n` frames at 60 fps into `dir`.
fn film(dir: &Path, n: usize, shot: impl Fn(usize) -> Shot) {
    std::fs::create_dir_all(dir.join("frames")).unwrap();
    let mut frames = Vec::new();
    for i in 0..n {
        let s = shot(i);
        let file = format!("frames/{i:05}.png");
        s.image.save_png(&dir.join(&file)).unwrap();
        frames.push(Frame {
            index: i,
            file: Some(file),
            time: i as f32 / 60.0,
            tick: i as u64 / 2,
            alpha: if i % 2 == 0 { 0.0 } else { 0.5 },
            hash: "same".into(),
            camera: camera(),
            zoom: None,
            things: s.things,
            lines: s.lines,
        });
    }
    let manifest = Manifest {
        format: FORMAT,
        fps: 60,
        ticks_per_second: 30,
        width: W,
        height: H,
        scale: 1.0,
        drawn: true,
        adapter: None,
        source: Value::Null,
        frames,
    };
    std::fs::write(
        dir.join("film.json"),
        serde_json::to_string(&manifest).unwrap(),
    )
    .unwrap();
}

/// Dots 16 px apart along y = 90 (screen y 90), the pattern `offset` px along.
fn dotted(offset: f32) -> Shot {
    let mut image = Image::new(W, H, BG);
    let mut dots = Vec::new();
    let mut x = 8.0 + offset.rem_euclid(16.0) - 16.0;
    while x < W as f32 + 16.0 {
        image.dot(x, 90.0, 2.5, DOT);
        dots.push([x, 90.0, 0.0, 1.0]);
        x += 16.0;
    }
    let line = LineProbe {
        kind: "dotted".into(),
        closed: false,
        alpha: 1.0,
        width: 5.0,
        points: vec![[0.0, 90.0, 0.0], [W as f32, 90.0, 0.0]],
        phase: 0.0,
        spacing: Some(16.0),
        dots: dots
            .into_iter()
            .filter(|d| d[0] >= 0.0 && d[0] <= W as f32)
            .collect(),
    };
    Shot {
        image,
        things: BTreeMap::new(),
        lines: vec![line],
    }
}

fn tool(name: &str, dir: &Path, args: &[&str]) -> Value {
    let opts = Opts::parse(&args.iter().map(|a| a.to_string()).collect::<Vec<_>>()).unwrap();
    run(name, dir, &opts).unwrap_or_else(|e| panic!("{name}: {e}"))
}

fn f(v: &Value, path: &str) -> f64 {
    galaxy_motion::check::lookup(v, path)
        .and_then(Value::as_f64)
        .unwrap_or_else(|| panic!("no {path} in {v}"))
}

#[test]
fn strip_flow_and_dots_see_a_crawl_and_not_a_still_pattern() {
    let tmp = tempfile::tempdir().unwrap();
    let (still, crawl) = (tmp.path().join("still"), tmp.path().join("crawl"));
    film(&still, 30, |_| dotted(0.0));
    film(&crawl, 30, |i| dotted(i as f32 * 0.8));
    let along = ["--along", "20,90;300,90"];
    let s = tool("strip", &still, &along);
    let c = tool("strip", &crawl, &along);
    assert!(f(&s, "drift_px_per_frame").abs() < 0.05, "{s}");
    assert!((f(&c, "drift_px_per_frame") - 0.8).abs() < 0.15, "{c}");
    assert!(c["reading"].as_str().unwrap().contains("slides"), "{c}");
    assert!(Path::new(c["image"].as_str().unwrap()).is_file());

    let s = tool("flow", &still, &along);
    let c = tool("flow", &crawl, &along);
    assert!(f(&s, "along.px_per_frame").abs() < 0.1, "{s}");
    assert!((f(&c, "along.px_per_frame") - 0.8).abs() < 0.2, "{c}");

    let s = tool("dots", &still, &[]);
    let c = tool("dots", &crawl, &[]);
    assert!(f(&s, "drift_px_per_frame").abs() < 0.01, "{s}");
    assert!((f(&c, "drift_px_per_frame") - 0.8).abs() < 0.01, "{c}");
}

#[test]
fn flicker_flags_a_blip_and_a_flicker_and_leaves_steady_motion_alone() {
    let tmp = tempfile::tempdir().unwrap();
    let moving = |i: usize| {
        let mut image = Image::new(W, H, BG);
        image.fill_rect(20 + i as i64 * 3, 120, 12, 12, DOT);
        image
    };
    let (calm, broken) = (tmp.path().join("calm"), tmp.path().join("broken"));
    film(&calm, 40, |i| Shot {
        image: moving(i),
        things: BTreeMap::new(),
        lines: Vec::new(),
    });
    film(&broken, 40, |i| {
        let mut image = moving(i);
        // Shown on frame 10 only; and on every other frame from 20 to 31.
        if i == 10 {
            image.fill_rect(200, 30, 10, 10, [255, 255, 255, 255]);
        }
        if (20..32).contains(&i) && i % 2 == 0 {
            image.fill_rect(260, 40, 10, 10, [255, 255, 255, 255]);
        }
        Shot {
            image,
            things: BTreeMap::new(),
            lines: Vec::new(),
        }
    });
    let c = tool("flicker", &calm, &[]);
    assert_eq!(
        (f(&c, "pops"), f(&c, "blips"), f(&c, "flicker_runs")),
        (0.0, 0.0, 0.0),
        "{c}"
    );
    let b = tool("flicker", &broken, &[]);
    assert!(f(&b, "blips") >= 1.0, "{b}");
    assert!(f(&b, "flicker_runs") >= 1.0, "{b}");
    let events = b["events"].as_array().unwrap();
    assert!(
        events
            .iter()
            .any(|e| e["kind"] == "blip" && e["from"]["frame"] == 10),
        "{b}"
    );
    assert!(
        events
            .iter()
            .any(|e| e["kind"] == "flicker" && e["box_px"][0].as_u64().unwrap() >= 240),
        "{b}"
    );
}

#[test]
fn compare_finds_the_frames_and_place_that_differ() {
    let tmp = tempfile::tempdir().unwrap();
    let (a, b) = (tmp.path().join("a"), tmp.path().join("b"));
    film(&a, 12, |i| dotted(i as f32));
    film(&b, 12, |i| {
        let mut s = dotted(i as f32);
        if (5..9).contains(&i) {
            s.image.fill_rect(250, 20, 20, 20, [255, 255, 255, 255]);
        }
        s
    });
    let same = tool("compare", &a, &["--with", a.to_str().unwrap()]);
    assert_eq!(f(&same, "flip.visibly_different_frames"), 0.0, "{same}");
    let diff = tool("compare", &a, &["--with", b.to_str().unwrap()]);
    assert_eq!(f(&diff, "flip.visibly_different_frames"), 4.0, "{diff}");
    let worst = &diff["flip"]["worst_frames"][0];
    let frame = worst["frame"].as_u64().unwrap();
    assert!((5..9).contains(&frame), "{diff}");
    let b = worst["box_px"].as_array().unwrap();
    assert!(
        b[0].as_u64().unwrap() >= 240 && b[1].as_u64().unwrap() <= 40,
        "{diff}"
    );
}

#[test]
fn track_sees_judder_and_overshoot() {
    let tmp = tempfile::tempdir().unwrap();
    let shot = |x: f32| {
        let mut image = Image::new(W, H, BG);
        image.dot(x, 90.0, 4.0, DOT);
        let mut things = BTreeMap::new();
        things.insert("t.ship".to_owned(), thing(x, 90.0));
        things.insert("t.pad".to_owned(), thing(200.0, 90.0));
        Shot {
            image,
            things,
            lines: Vec::new(),
        }
    };
    let (smooth, judder, over) = (
        tmp.path().join("s"),
        tmp.path().join("j"),
        tmp.path().join("o"),
    );
    // Easing in to the pad at x = 200.
    let ease = |i: usize| 20.0 + 180.0 * (1.0 - (1.0 - (i as f32 / 40.0).min(1.0)).powi(2));
    film(&smooth, 50, |i| shot(ease(i)));
    // Drawn only at tick positions: the same place on two frames running.
    film(&judder, 50, |i| shot(ease(i - i % 2)));
    // Past the pad by 15, then back.
    film(&over, 50, |i| {
        let t = i as f32 / 30.0;
        shot(if t < 1.0 {
            20.0 + 195.0 * t
        } else {
            215.0 - 15.0 * ((t - 1.0) * 2.0).min(1.0)
        })
    });
    let s = tool("track", &smooth, &["--thing", "ship", "--to", "pad"]);
    let j = tool("track", &judder, &["--thing", "ship", "--to", "pad"]);
    let o = tool("track", &over, &["--thing", "ship", "--to", "pad"]);
    assert!(f(&s, "screen.judder") < 0.1, "{s}");
    assert!(f(&j, "screen.judder") > 0.5, "{j}");
    assert!(f(&s, "gap.overshoot") < 0.01, "{s}");
    assert!((f(&o, "gap.overshoot") - 15.0).abs() < 0.5, "{o}");
    assert!(
        o["reading"].as_str().unwrap().contains("overshooting"),
        "{o}"
    );
}

#[test]
fn sheet_lays_out_frames_and_an_onion_skin() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("f");
    film(&dir, 30, |i| {
        let x = 30.0 + i as f32 * 8.0;
        let mut image = Image::new(W, H, BG);
        image.fill_rect(x as i64 - 5, 85, 10, 10, DOT);
        let mut things = BTreeMap::new();
        things.insert("t.box".to_owned(), thing(x, 90.0));
        Shot {
            image,
            things,
            lines: Vec::new(),
        }
    });
    let s = tool("sheet", &dir, &["--count", "4", "--at", "0.25"]);
    assert_eq!(s["frames"].as_array().unwrap().len(), 5, "{s}");
    let img = Image::load_png(Path::new(s["image"].as_str().unwrap())).unwrap();
    assert!(img.width >= 4 * 100);
    let o = tool("sheet", &dir, &["--onion", "box", "--poses", "3"]);
    let gaps: Vec<f64> = o["gaps_between_poses"]
        .as_array()
        .unwrap()
        .iter()
        .map(|g| g.as_f64().unwrap())
        .collect();
    assert_eq!(gaps.len(), 2);
    assert!(
        (gaps[0] - gaps[1]).abs() < 9.0,
        "steady speed spaces poses evenly: {gaps:?}"
    );
}

#[test]
fn a_tool_that_needs_pixels_says_so_on_a_probes_only_film() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("f");
    film(&dir, 5, |_| dotted(0.0));
    let path = dir.join("film.json");
    let mut m: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    m["drawn"] = Value::Bool(false);
    std::fs::write(&path, m.to_string()).unwrap();
    let err = run("strip", &dir, &Opts::new()).unwrap_err();
    assert!(err.contains("probes only"), "{err}");
    assert!(run("dots", &dir, &Opts::new()).is_ok());
    assert!(
        run("nope", &dir, &Opts::new())
            .unwrap_err()
            .contains("track, dots")
    );
}
