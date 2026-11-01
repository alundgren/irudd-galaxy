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
        reach: None,
        morph: None,
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
        builds: serde_json::Value::Null,
        changes: Vec::new(),
        orders: BTreeMap::new(),
        frames,
        outlines: BTreeMap::new(),
        parts: Default::default(),
    };
    manifest.write(dir).unwrap();
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

/// A tool's full report (`--detail`).
fn tool(name: &str, dir: &Path, args: &[&str]) -> Value {
    let mut args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
    args.push("--detail".into());
    compact_tool(
        name,
        dir,
        &args.iter().map(String::as_str).collect::<Vec<_>>(),
    )
}

/// A tool's report as the command line gives it.
fn compact_tool(name: &str, dir: &Path, args: &[&str]) -> Value {
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
    // The square on frame 10 sits across a cell edge: one event, not one a cell.
    let at_10: Vec<&Value> = events.iter().filter(|e| e["from"]["frame"] == 10).collect();
    assert_eq!(at_10.len(), 1, "{b}");
    // Strongest first.
    let changes: Vec<f64> = events
        .iter()
        .map(|e| e["change"].as_f64().unwrap())
        .collect();
    assert!(changes.windows(2).all(|w| w[0] >= w[1]), "{changes:?}");

    // --from/--to keep the events in the span and say how many they left out.
    let late = tool("flicker", &broken, &["--from", "0.3", "--to", "0.6"]);
    let late_events = late["events"].as_array().unwrap();
    assert!(!late_events.is_empty(), "{late}");
    assert!(
        late_events.iter().all(|e| {
            let t = e["from"]["t"].as_f64().unwrap();
            (0.3 - 1e-3..=0.6 + 1e-3).contains(&t)
        }),
        "{late}"
    );
    assert!(
        late["left_out"]
            .as_str()
            .unwrap()
            .contains("outside --from/--to"),
        "{late}"
    );

    // Compact by default: the strongest few, moments on one line, and what --detail adds.
    let c = compact_tool("flicker", &broken, &[]);
    assert!(c["reading"].is_string() && c["image"].is_string(), "{c}");
    assert!(c["events"].as_array().unwrap().len() <= 5, "{c}");
    assert!(
        c["events"][0]["from"].as_str().unwrap().starts_with("t "),
        "{c}"
    );
    assert!(c.get("changed_share").is_none(), "{c}");
    assert!(c["detail"].as_str().unwrap().contains("--detail"), "{c}");
    assert!(c["builds"]["note"].is_string(), "{c}");
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

/// A thing of four flat faces at the centre; the fourth's brightness on frame `i` is
/// `face(i)`.
fn faces(face: impl Fn(usize) -> u8) -> impl Fn(usize) -> Shot {
    move |i| {
        let mut image = Image::new(W, H, BG);
        let b = face(i);
        let colours: [Rgba; 4] = [
            [200, 120, 60, 255],
            [240, 220, 180, 255],
            [120, 90, 150, 255],
            [b, b, b, 255],
        ];
        for (k, c) in colours.iter().enumerate() {
            let (x, y) = (142 + (k as i64 % 2) * 20, 72 + (k as i64 / 2) * 20);
            image.fill_rect(x, y, 16, 16, *c);
        }
        let mut t = thing(160.0, 90.0);
        t.reach = Some(22.0);
        t.followed = true;
        Shot {
            image,
            things: BTreeMap::from([("m.ship".to_owned(), t)]),
            lines: Vec::new(),
        }
    }
}

#[test]
fn tone_finds_a_face_that_flips_and_not_one_that_eases() {
    let tmp = tempfile::tempdir().unwrap();
    // The fourth face goes from shadow to light and back: over eight frames, or at once.
    let eased = |i: usize| {
        let up =
            (i.saturating_sub(20).min(8) as f32 / 8.0) - (i.saturating_sub(50).min(8) as f32 / 8.0);
        (90.0 + 130.0 * up) as u8
    };
    let stepped = |i: usize| if (24..54).contains(&i) { 220 } else { 90 };
    let (smooth, hard) = (tmp.path().join("smooth"), tmp.path().join("hard"));
    film(&smooth, 80, faces(eased));
    film(&hard, 80, faces(stepped));
    let s = tool("tone", &smooth, &[]);
    assert_eq!(f(&s, "hard_steps"), 0.0, "{s}");
    let h = tool("tone", &hard, &[]);
    assert_eq!(f(&h, "hard_steps"), 2.0, "{h}");
    let steps: Vec<u64> = h["steps"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["frame"].as_u64().unwrap())
        .collect();
    assert_eq!(steps, [24, 54], "{h}");
    // The sky round the thing and the gaps between its faces are not the thing.
    assert!(f(&h, "change.max") > 0.1, "{h}");
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

#[test]
fn track_marks_zooms_and_does_not_flag_the_steps_they_make() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("zoom");
    // The camera follows the ship, 2 units a frame along x, while the player zooms in,
    // twice as close over frames 10 to 20; the ship stays at the centre throughout.
    film(&dir, 40, |i| {
        let mut image = Image::new(W, H, BG);
        image.dot(160.0, 90.0, 4.0, DOT);
        let mut things = BTreeMap::new();
        let mut ship = thing(160.0, 90.0);
        ship.x = 160.0 + 2.0 * i as f32;
        ship.followed = true;
        things.insert("t.ship".to_owned(), ship);
        Shot {
            image,
            things,
            lines: Vec::new(),
        }
    });
    let path = dir.join("frames.jsonl");
    let text = std::fs::read_to_string(&path).unwrap();
    let mut out = String::new();
    for (i, line) in text.lines().enumerate() {
        let mut v: Value = serde_json::from_str(line).unwrap();
        let k = 1.0 + (i.clamp(10, 20) - 10) as f32 / 10.0;
        let cx = 160.0 + 2.0 * i as f32;
        let m = &mut v["camera"]["view_proj"];
        m[0] = Value::from(2.0 * k / W as f32);
        m[5] = Value::from(2.0 * k / H as f32);
        m[12] = Value::from(-cx * 2.0 * k / W as f32);
        m[13] = Value::from(-90.0 * 2.0 * k / H as f32);
        v["camera"]["per_pixel"] = Value::from(1.0 / k);
        v["camera"]["target"] = serde_json::json!([cx, 90.0, 0.0]);
        out.push_str(&v.to_string());
        out.push('\n');
    }
    std::fs::write(&path, out).unwrap();
    let r = tool("track", &dir, &[]);
    let zooms = r["zooms"].as_array().unwrap();
    assert_eq!(zooms.len(), 1, "{r}");
    assert!(zooms[0].as_str().unwrap().starts_with("zoom in"), "{r}");
    assert_eq!(f(&r, "flagged"), 0.0, "{r}");
    assert!(r["reading"].as_str().unwrap().contains("zoomed"), "{r}");
}

/// A ship of radius 4 rams a fixed hub of radius 10 at x = 200: it sinks in by up to
/// `depth` on frames 20 to 25, rests touching from 26 to 31, then backs off.
fn ram(dir: &Path, depth: f32) {
    rammed(dir, 50, |i| match i {
        0..20 => 20.0 - i as f32,
        20..26 => -depth * (1.0 - ((i as f32 - 22.0) / 4.0).abs()).max(0.2),
        26..32 => 0.01,
        _ => 0.01 + (i - 31) as f32,
    });
}

/// A ship of radius 4 and a fixed hub of radius 10 at x = 200, `gap(i)` apart (surface to
/// surface) on frame `i` of `n`.
fn rammed(dir: &Path, n: usize, gap: impl Fn(usize) -> f32) {
    film(dir, n, |i| {
        let gap = gap(i);
        let mut image = Image::new(W, H, BG);
        image.dot(186.0 - gap, 90.0, 4.0, DOT);
        let mut things = BTreeMap::new();
        let mut ship = thing(186.0 - gap, 90.0);
        ship.followed = true;
        things.insert("t.ship".to_owned(), ship);
        things.insert("t.hub".to_owned(), thing(200.0, 90.0));
        Shot {
            image,
            things,
            lines: Vec::new(),
        }
    });
    let mut f = galaxy_motion::Film::read(dir).unwrap();
    let circle = |r: f32| galaxy_motion::film::Outline {
        base: vec![r; 128],
        morphed: Vec::new(),
    };
    f.manifest.outlines.insert("t.ship".into(), circle(4.0));
    f.manifest.outlines.insert("t.hub".into(), circle(10.0));
    f.manifest.write(dir).unwrap();
}

#[test]
fn track_tells_overlap_from_contact_and_keeps_a_shallow_one() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("ram");
    ram(&dir, 0.0123);
    let r = tool("track", &dir, &["--thing", "ship", "--to", "hub"]);
    let s = &r["surface"];
    assert_eq!(f(s, "overlap.intervals"), 1.0, "{r}");
    assert!((f(s, "overlap.deepest") + 0.0123).abs() < 2e-4, "{r}");
    assert_eq!(s["overlap"]["runs"][0]["from"]["frame"], 20, "{r}");
    assert_eq!(s["overlap"]["runs"][0]["to"]["frame"], 25, "{r}");
    assert!((f(s, "overlap.seconds") - 0.1).abs() < 1e-3, "{r}");
    // Contact range holds the overlap and the rest after it: one run.
    assert_eq!(f(s, "contact.intervals"), 1.0, "{r}");
    assert_eq!(s["contact"]["runs"][0]["to"]["frame"], 31, "{r}");
    assert!(
        s["frames_round_it"]
            .as_str()
            .unwrap()
            .contains("--gap t.hub")
    );
    let reading = r["reading"].as_str().unwrap();
    assert!(
        reading.contains("overlapping (gap below -0.0001) 1 time(s)"),
        "{reading}"
    );

    // Resting against it without sinking in: contact range, no overlap, said plainly.
    let rest = tmp.path().join("rest");
    ram(&rest, -0.02);
    let r = compact_tool("track", &rest, &["--thing", "ship", "--to", "hub"]);
    assert_eq!(f(&r, "surface.overlap.intervals"), 0.0, "{r}");
    assert!(r["reading"].as_str().unwrap().contains("no overlap"), "{r}");
}

#[test]
fn track_names_an_overlap_with_its_own_target_and_the_bounce_at_first_contact() {
    let tmp = tempfile::tempdir().unwrap();
    // Ordered to dock at the hub from the start: the overlap is with its own target.
    let dir = tmp.path().join("dock");
    ram(&dir, 0.5);
    let mut m = galaxy_motion::Film::read(&dir).unwrap().manifest;
    m.orders.insert("t.ship".into(), "dock t.hub".into());
    m.write(&dir).unwrap();
    let r = compact_tool("track", &dir, &["--thing", "ship", "--to", "hub"]);
    assert_eq!(r["surface"]["order_target"], "dock t.hub", "{r}");
    assert_eq!(f(&r, "surface.overlap.with_order_target"), 1.0, "{r}");
    let reading = r["reading"].as_str().unwrap();
    assert!(
        reading.contains("target of t.ship's own order"),
        "{reading}"
    );
    // An order given during the film to go elsewhere: not its target any more.
    m.changes.push(galaxy_motion::film::Change {
        tick: 2,
        t: 4.0 / 60.0,
        kind: "order".into(),
        what: "t.ship: move 0.0,0.0".into(),
    });
    m.write(&dir).unwrap();
    let r = tool("track", &dir, &["--thing", "ship", "--to", "hub"]);
    assert_eq!(f(&r, "surface.overlap.with_order_target"), 0.0, "{r}");

    // Touches on frame 20, bounces 0.15 off by frame 23, touches again from 26, then
    // flies off 30 units under held thrust: the bounce is 0.15, the rebound far more.
    let bump = tmp.path().join("bump");
    rammed(&bump, 60, |i| match i {
        0..20 => 20.0 - i as f32,
        20..=23 => 0.05 * (i - 20) as f32,
        24..=25 => 0.15 - 0.04 * (i - 23) as f32,
        26..=35 => 0.0,
        _ => (i - 35) as f32 * 1.25,
    });
    let r = compact_tool("track", &bump, &["--thing", "ship", "--to", "hub"]);
    let fc = &r["surface"]["first_contact"];
    assert!((f(fc, "bounce") - 0.15).abs() < 1e-3, "{r}");
    assert!(
        fc["contact_again_at"]
            .as_str()
            .unwrap()
            .contains("frame 26"),
        "{r}"
    );
    assert!(f(&r, "gap.rebound") > 20.0, "{r}");
    let reading = r["reading"].as_str().unwrap();
    assert!(reading.contains("bounced off to 0.15"), "{reading}");
    assert!(
        reading.contains("surface.first_contact.bounce"),
        "{reading}"
    );
}

#[test]
fn flicker_tells_events_on_a_things_model_from_those_only_near_it() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("near");
    // A hub wearing a 24-unit square at (100, 90); a small patch on it shows on frame 8,
    // and a longer bar beside it, off its model, on frame 16.
    film(&dir, 26, |i| {
        let mut image = Image::new(W, H, BG);
        if i == 8 {
            image.fill_rect(95, 85, 10, 10, [255, 255, 255, 255]);
        }
        if i == 16 {
            image.fill_rect(124, 88, 40, 4, [255, 255, 255, 255]);
        }
        let mut hub = thing(100.0, 90.0);
        hub.reach = Some(17.0);
        Shot {
            image,
            things: [("t.hub".to_owned(), hub)].into(),
            lines: Vec::new(),
        }
    });
    let mut m = galaxy_motion::Film::read(&dir).unwrap().manifest;
    let s = 12.0;
    m.parts.models.insert(
        "square".into(),
        vec![galaxy_motion::film::PartMesh {
            name: "deck".into(),
            triangles: vec![
                -s, -s, 0.0, s, -s, 0.0, s, s, 0.0, -s, -s, 0.0, s, s, 0.0, -s, s, 0.0,
            ],
        }],
    );
    m.parts.things.insert(
        "t.hub".into(),
        galaxy_motion::film::Worn {
            model: "square".into(),
            scale: 1.0,
        },
    );
    m.write(&dir).unwrap();
    let r = tool("flicker", &dir, &["--near", "hub"]);
    let events = r["events"].as_array().unwrap();
    assert_eq!(events.len(), 2, "{r}");
    assert_eq!(
        events[0]["from"]["frame"], 8,
        "the one on the model first: {r}"
    );
    assert!(
        events[0]["on"].as_str().unwrap().starts_with("on t.hub"),
        "{r}"
    );
    assert!(
        events[1]["on"].as_str().unwrap().starts_with("only near"),
        "{r}"
    );
    assert_eq!(r["near_split"]["on_model"], 1, "{r}");
    assert_eq!(r["near_split"]["only_near"], 1, "{r}");
    // --event 2 is the bar alone, without the list or the other close-ups.
    let one = compact_tool("flicker", &dir, &["--near", "hub", "--event", "2"]);
    assert!(one.get("events").is_none(), "{one}");
    assert_eq!(one["chosen"]["n"], 2, "{one}");
    let reading = one["reading"].as_str().unwrap();
    assert!(reading.starts_with("event 2: "), "{reading}");
    assert!(reading.contains("only near t.hub"), "{reading}");
}

#[test]
fn compare_puts_how_the_followed_thing_moved_side_by_side() {
    let tmp = tempfile::tempdir().unwrap();
    let (a, b) = (tmp.path().join("a"), tmp.path().join("b"));
    ram(&a, 0.0123);
    ram(&b, 0.0123);
    let same = tool("compare", &a, &["--with", b.to_str().unwrap()]);
    assert_eq!(same["behaviour"]["same"], true, "{same}");
    assert_eq!(same["behaviour"]["films"].as_array().unwrap().len(), 2);
    let c = tmp.path().join("c");
    ram(&c, 2.0);
    let diff = compact_tool("compare", &a, &["--with", c.to_str().unwrap()]);
    assert_eq!(diff["behaviour"]["same"], false, "{diff}");
    assert!(
        diff["reading"]
            .as_str()
            .unwrap()
            .contains("moves differently"),
        "{diff}"
    );
    // Probes-only films (a module before and after a change): behaviour, no pixels.
    let path = c.join("film.json");
    let mut m: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    m["drawn"] = Value::Bool(false);
    std::fs::write(&path, m.to_string()).unwrap();
    let probes = compact_tool("compare", &a, &["--with", c.to_str().unwrap()]);
    assert_eq!(probes["behaviour"]["same"], false, "{probes}");
    assert!(
        probes["pixels"].as_str().unwrap().contains("probes only"),
        "{probes}"
    );
    assert!(probes.get("image").is_none(), "{probes}");
}

#[test]
fn flicker_makes_a_close_up_of_any_event_or_moment_asked_for() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("f");
    film(&dir, 30, |i| {
        let mut image = Image::new(W, H, BG);
        // A square shown on frame 8 only, and a bar on frames 18 to 29.
        if i == 8 {
            image.fill_rect(40, 40, 16, 16, [255, 255, 255, 255]);
        }
        if i >= 18 {
            image.fill_rect(200, 100, 30, 12, [255, 255, 255, 255]);
        }
        Shot {
            image,
            things: BTreeMap::new(),
            lines: Vec::new(),
        }
    });
    let all = tool("flicker", &dir, &[]);
    let events = all["events"].as_array().unwrap();
    assert!(events.len() >= 2, "{all}");
    for (k, e) in events.iter().enumerate() {
        assert_eq!(e["n"].as_u64().unwrap() as usize, k + 1, "{all}");
        assert!(e["looks_like"].is_string(), "{all}");
    }
    let last = events.len().to_string();
    let one = tool("flicker", &dir, &["--event", &last]);
    assert!(one["events"].as_array().unwrap().is_empty(), "{one}");
    let chosen = &one["chosen"];
    assert_eq!(chosen["n"].as_u64().unwrap().to_string(), last, "{one}");
    assert!(
        Path::new(chosen["closeup"].as_str().unwrap()).is_file(),
        "{one}"
    );
    assert!(chosen["changed"]["looks_like"]["kind"].is_string(), "{one}");
    // The square appearing is an area, not edges stepping.
    let square = events
        .iter()
        .find(|e| e["from"]["frame"] == 8)
        .unwrap_or_else(|| panic!("{all}"));
    assert!(
        square["looks_like"].as_str().unwrap().contains("area"),
        "{square}"
    );
    // --at picks the event starting there; with nothing there it needs --near.
    let at = tool("flicker", &dir, &["--at", "0.3"]);
    assert_eq!(at["chosen"]["from"]["frame"], 18, "{at}");
    let opts = Opts::parse(&["--at".to_owned(), "0.2".to_owned()]).unwrap();
    let err = run("flicker", &dir, &opts).unwrap_err();
    assert!(err.contains("--near"), "{err}");
    let opts = Opts::parse(&["--event".to_owned(), "99".to_owned()]).unwrap();
    assert!(run("flicker", &dir, &opts).unwrap_err().contains("--event"));
}
