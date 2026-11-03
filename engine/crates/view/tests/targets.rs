//! Clickable models, camera glides, bound Esc and right-click, and edge-placed widgets,
//! driven as `galaxy play` drives the bot in `tests/play/targets-inputs.toml`.
use galaxy_api::engine::{CAMERA_SETTLED, PLACE_ARRIVED, ROOM, TRAVEL};
use galaxy_module::module::Module;
use galaxy_render::glam::{Vec2, Vec3};
use galaxy_render::screen_style::ScreenStyle;
use galaxy_sim::scenario::InputsFile;
use galaxy_sim::{BuildStamp, Input, Replay, Session, travel};
use galaxy_view::{View, travel::Cut};
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/play")
}
fn module() -> Module {
    Module::read(&root().join("targets/demo")).unwrap()
}
/// The bot shrinks the window at tick 150.
fn size_at(tick: u64) -> Vec2 {
    if tick >= 150 {
        Vec2::new(1024.0, 576.0)
    } else {
        Vec2::new(1280.0, 720.0)
    }
}

/// `[[key]]` presses as play sends them through the module's bindings: down on the tick,
/// up on the next.
fn key_inputs(bot: &InputsFile) -> BTreeMap<u64, Vec<Input>> {
    let mut out: BTreeMap<u64, Vec<Input>> = BTreeMap::new();
    for (tick, key) in &bot.keys {
        for (at, down) in [(*tick, true), (tick + 1, false)] {
            let mut payload = Map::new();
            payload.insert("down".into(), Value::Bool(down));
            out.entry(at).or_default().push(Input {
                tick: at,
                source: if key == "mouse_right" {
                    "mouse"
                } else {
                    "keyboard"
                }
                .into(),
                name: "targets.back".into(),
                payload,
            });
        }
    }
    out
}

fn rect(v: &View, id: &str) -> galaxy_render::Rect {
    fn find<'a>(
        b: &'a galaxy_view::screens::Boxed,
        id: &str,
    ) -> Option<&'a galaxy_view::screens::Boxed> {
        if b.widget.id.as_deref() == Some(id) {
            return Some(b);
        }
        b.children.iter().find_map(|c| find(c, id))
    }
    v.screens
        .boxes
        .iter()
        .find_map(|b| find(b, id))
        .unwrap_or_else(|| panic!("no widget `{id}`"))
        .rect
}

struct Note {
    tick: u64,
    moment: galaxy_view::Moment,
    scene: galaxy_render::Scene,
}

#[test]
fn targets_glides_keys_and_edges_play_replay_and_redraw() {
    let mut s = Session::new(vec![module()], 3).unwrap();
    s.start_recording(BuildStamp::current());
    let mut v = View::detached(s.modules(), 3);
    let bot = InputsFile::read(&root().join("targets-inputs.toml")).unwrap();
    let mut keys = key_inputs(&bot);
    let highlight = ScreenStyle::default().highlight;
    let mut notes = Vec::new();
    let mut eyes = Vec::new();
    let mut landed = None;
    for tick in 1..=210u64 {
        let size = size_at(tick);
        v.before_step(&s.world);
        v.scene(&s.world, 1.0, size, 0.0);
        if tick == 33 {
            // The painted label over the second figure's face blocks clicks there; the
            // rest of the figure still takes them.
            let cover = rect(&v, "cover").center();
            assert!(v.target_click(&s.world, cover, tick).is_none());
            let input = v
                .bot_target_click(&s.world, "second-target", tick)
                .unwrap()
                .unwrap();
            assert_eq!(input.payload["who"], "second");
            // Over empty floor, nothing is clicked and the pointer is an arrow.
            assert!(
                v.target_click(&s.world, Vec2::new(30.0, 360.0), tick)
                    .is_none()
            );
        }
        let mut inputs = keys.remove(&tick).unwrap_or_default();
        for p in bot.pointers.iter().filter(|p| p.tick == tick) {
            inputs.extend(v.bot_pointer(&s.world, p).unwrap());
        }
        for c in bot.clicks.iter().filter(|c| c.tick == tick) {
            inputs.push(
                travel::arrival(&s.world, true)
                    .map(Ok)
                    .unwrap_or_else(|| v.bot_talk_click(&s.world, c))
                    .unwrap(),
            );
        }
        if !inputs.iter().any(|i| travel::is_landing(&i.name))
            && let Some(input) = travel::arrival(&s.world, false)
        {
            inputs.push(input);
        }
        if tick == 50 {
            assert_eq!(inputs.len(), 1);
            assert_eq!(inputs[0].name, CAMERA_SETTLED, "a mid-glide click lands it");
        }
        s.step(&inputs).unwrap();
        let scene = v.scene(&s.world, 0.5, size, tick as f32 / 30.0);
        s.record_presentation(serde_json::to_value(&v.screens.presented).unwrap());
        eyes.push((tick, scene.camera.eye));
        let glide = Cut::step(&s.world, 0.5).filter(|c| c.busy());
        if let Some(c) = &glide {
            assert!(c.glide > 0.0 || c.to == "targets.yard", "tick {tick}");
            if c.glide > 0.0 {
                // A glide draws no fade or title card.
                assert!(scene.overlay.upper[1].panels.is_empty(), "tick {tick}");
            }
        }
        match tick {
            15 | 48 => {
                let c = glide.expect("mid-glide");
                let t = c.glide_progress().unwrap();
                assert!(t > 0.0 && t < 1.0, "tick {tick}: {t}");
            }
            37 => {
                // Hovering the first figure: lifted, outlined, hand pointer.
                assert_eq!(v.targeted.as_deref(), Some("targets.first"));
                assert!(v.hand);
                let outlined: Vec<_> = scene
                    .meshes
                    .iter()
                    .filter(|m| m.material.outline == highlight)
                    .collect();
                assert_eq!(outlined.len(), 1);
                let lifted = outlined[0].transform.w_axis.z;
                let resting = s
                    .world
                    .component("targets.first", galaxy_api::engine::POSITION)
                    .unwrap()
                    .fields["height"]
                    .as_f64()
                    .unwrap() as f32;
                assert!(lifted > resting + 0.05, "{lifted} over {resting}");
            }
            40 => {
                let room = &s.world.component("targets.hall", ROOM).unwrap().fields;
                assert_eq!(room["camera"], "camera_first");
                let trip = &s.world.component("targets.player", TRAVEL).unwrap().fields;
                assert_eq!(trip["from"], trip["to"]);
                assert_eq!(trip["from_camera"], "camera");
            }
            50 => {
                landed = Some(scene.camera);
                let room_camera = v.room_camera(&s.world, "targets.hall").unwrap();
                assert_eq!(scene.camera, room_camera, "landed on the next frame");
            }
            70 => {
                assert!(v.targeted.is_none());
                assert!(!v.hand);
            }
            _ => {}
        }
        for id in ["status", "keys", "log"] {
            let r = rect(&v, id);
            let edge = match id {
                "status" => (r.x + r.w, r.y),
                "keys" => (r.x + r.w * 0.5, r.y + r.h),
                _ => (r.x, r.y + r.h),
            };
            let want = match id {
                "status" => (size.x - 16.0, 16.0),
                "keys" => (size.x * 0.5, size.y - 16.0),
                _ => (16.0, size.y - 16.0),
            };
            assert!(
                (edge.0 - want.0).abs() < 0.01 && (edge.1 - want.1).abs() < 0.01,
                "tick {tick}: {id} at {edge:?}, want {want:?}"
            );
        }
        assert!(
            !v.problems().iter().any(|p| p.contains("viewport")),
            "tick {tick}: {:?}",
            v.problems()
        );
        if let Some((_, text)) = bot.notes.iter().find(|n| n.0 == tick) {
            let moment = v
                .moment(0.5, tick as f32 / 30.0, (size.x as u32, size.y as u32), 1.0)
                .unwrap();
            s.note(text, Some(serde_json::to_value(&moment).unwrap()), None);
            notes.push(Note {
                tick,
                moment,
                scene,
            });
        }
    }
    assert!(landed.is_some());
    // The glide into the hall closes in on the room camera, frame after frame.
    let hall = {
        let mut staged = s.world.clone();
        staged
            .entities
            .get_mut("targets.hall")
            .unwrap()
            .get_mut(ROOM)
            .unwrap()
            .fields
            .insert("camera".into(), "camera".into());
        v.room_camera(&staged, "targets.hall").unwrap().eye
    };
    let distances: Vec<f32> = eyes
        .iter()
        .filter(|(t, _)| (5..=29).contains(t))
        .map(|(_, e)| (*e - hall).length())
        .collect();
    assert!(
        distances.windows(2).all(|d| d[1] <= d[0] + 1e-3),
        "{distances:?}"
    );
    assert!(distances.last().unwrap() < &1e-3);
    let choice = &s
        .world
        .component("targets.state", "targets.choice")
        .unwrap()
        .fields;
    assert_eq!(choice["backs"], 3, "Esc, right-click and Esc again");
    let facts: Vec<_> = s.world.facts.iter().map(|f| f.kind.as_str()).collect();
    assert_eq!(facts.iter().filter(|k| **k == PLACE_ARRIVED).count(), 2);
    assert!(facts.contains(&CAMERA_SETTLED));
    assert_eq!(
        s.world
            .component("targets.player", "engine.location")
            .unwrap()
            .fields["place"],
        "targets.yard"
    );

    // Replay is identical, and a fresh view redraws every note: mid-glide, mid-hover,
    // landed and at the smaller size.
    let rec = s.recording().unwrap();
    assert!(rec.events.iter().any(|e| matches!(e,
        galaxy_sim::recording::RecEvent::Input(i) if i.source == "mouse")));
    let mut replay = Replay::new(&rec).unwrap();
    let mut rebuilt = View::detached(replay.session.modules(), 3);
    for note in &notes {
        while replay.session.world.tick < note.tick {
            rebuilt.before_step(&replay.session.world);
            replay.step().unwrap();
        }
        let redrawn = rebuilt.note_scene(&replay.session.world, &note.moment);
        assert_eq!(
            redrawn.overlay, note.scene.overlay,
            "note tick {}",
            note.tick
        );
        assert_eq!(redrawn.camera, note.scene.camera, "note tick {}", note.tick);
        assert_eq!(redrawn.meshes, note.scene.meshes, "note tick {}", note.tick);
    }
    while replay.step().unwrap() {}
    assert_eq!(replay.session.world.hash(), s.world.hash());
    assert_eq!(notes.len(), 6);
    let _ = Vec3::ZERO;
}

#[test]
fn a_room_without_glide_keeps_the_fade_cut_and_camera_changes_snap() {
    let mut files = module().files.clone();
    let manifest = files
        .0
        .get("start.toml")
        .unwrap()
        .replace("glide = 0.8", "glide = 0.0");
    files.0.insert("start.toml".into(), manifest);
    let module = Module::from_files(Path::new("targets"), files).unwrap();
    let mut s = Session::new(vec![module], 3).unwrap();
    let go = Input {
        tick: 1,
        source: "ui".into(),
        name: "targets.go".into(),
        payload: Map::new(),
    };
    s.step(&[go]).unwrap();
    let trip = &s.world.component("targets.player", TRAVEL).unwrap().fields;
    assert_eq!(trip["glide"], 0.0);
    let cut = Cut::step(&s.world, 0.5).unwrap();
    assert!(cut.glide_progress().is_none());
    assert_eq!(cut.place(), "targets.yard", "fades out of the source first");
    for _ in 0..40 {
        let inputs: Vec<_> = travel::arrival(&s.world, false).into_iter().collect();
        s.step(&inputs).unwrap();
    }
    let mut payload = Map::new();
    payload.insert("who".into(), "first".into());
    s.step(&[Input {
        tick: 0,
        source: "ui".into(),
        name: "targets.pick".into(),
        payload,
    }])
    .unwrap();
    let trip = &s.world.component("targets.player", TRAVEL).unwrap().fields;
    assert_eq!(trip["active"], false, "without glide a camera change snaps");
}

/// One journey through the hall: `events` are (tick, input name, source, payload `who`).
/// Returns, per tick, the eye and the screen point of a fixed hall point at the same
/// presentation time on both sides of the step (tick - 1 at alpha 1, tick at alpha 0),
/// for ticks where the hall is shown on both sides.
struct Journey {
    s: Session,
    v: View,
    jumps: Vec<(u64, f32, f32)>,
    eyes: BTreeMap<u64, Vec3>,
    note: Option<Note>,
}

fn journey(events: &[(u64, &str, &str, &str)], ticks: u64, note_at: u64) -> Journey {
    let mut s = Session::new(vec![module()], 3).unwrap();
    s.start_recording(BuildStamp::current());
    let mut v = View::detached(s.modules(), 3);
    let size = Vec2::new(1280.0, 720.0);
    let point = Vec3::new(-2.0, 0.0, 1.0);
    let hall =
        |w: &galaxy_sim::World, alpha| galaxy_view::travel::place(w, alpha) == Some("targets.hall");
    let (mut jumps, mut eyes, mut note) = (Vec::new(), BTreeMap::new(), None);
    for tick in 1..=ticks {
        v.before_step(&s.world);
        let before = v.scene(&s.world, 1.0, size, 0.0).camera;
        let shown_before = hall(&s.world, 1.0);
        let mut inputs = Vec::new();
        for (at, name, source, who) in events {
            for (t, down) in [(*at, true), (at + 1, false)] {
                if t != tick || (*name != "back" && !down) {
                    continue;
                }
                let mut payload = Map::new();
                match *name {
                    "back" => drop(payload.insert("down".into(), down.into())),
                    "pick" => drop(payload.insert("who".into(), (*who).into())),
                    _ => {}
                }
                inputs.push(Input {
                    tick,
                    source: (*source).into(),
                    name: format!("targets.{name}"),
                    payload,
                });
            }
        }
        if let Some(input) = travel::arrival(&s.world, false) {
            inputs.push(input);
        }
        s.step(&inputs).unwrap();
        let after = v.scene(&s.world, 0.0, size, 0.0).camera;
        if shown_before && hall(&s.world, 0.0) {
            let px = (after.world_to_screen(point, size).unwrap()
                - before.world_to_screen(point, size).unwrap())
            .length();
            jumps.push((tick, (after.eye - before.eye).length(), px));
        }
        let scene = v.scene(&s.world, 0.5, size, tick as f32 / 30.0);
        s.record_presentation(serde_json::to_value(&v.screens.presented).unwrap());
        eyes.insert(tick, scene.camera.eye);
        if tick == note_at {
            let moment = v.moment(0.5, tick as f32 / 30.0, (1280, 720), 1.0).unwrap();
            s.note(
                "mid-glide",
                Some(serde_json::to_value(&moment).unwrap()),
                None,
            );
            note = Some(Note {
                tick,
                moment,
                scene,
            });
        }
    }
    Journey {
        s,
        v,
        jumps,
        eyes,
        note,
    }
}

fn assert_no_jump(j: &Journey, what: &str) {
    for (tick, eye, px) in &j.jumps {
        assert!(
            *eye < 0.01 && *px < 0.5,
            "{what}: tick {tick} jumps {eye} world units, {px} px"
        );
    }
}

#[test]
fn going_back_during_a_glide_glides_on_from_where_the_camera_is() {
    // The review's journey: frame the first figure, then go back while still gliding,
    // with Esc and with the right button.
    for source in ["keyboard", "mouse"] {
        let j = journey(
            &[
                (5, "go", "ui", ""),
                (40, "pick", "ui", "first"),
                (52, "back", source, ""),
            ],
            90,
            55,
        );
        assert_no_jump(&j, source);
        assert!(j.jumps.iter().any(|(t, ..)| *t == 52));
        let trip =
            &j.s.world
                .component("targets.player", TRAVEL)
                .unwrap()
                .fields;
        assert_eq!(trip["start_tick"], 52);
        let taken = galaxy_view::travel::interrupted(trip).unwrap();
        assert_eq!(
            (taken["start_tick"].as_u64(), taken["camera"].as_str()),
            (Some(40), Some("camera_first"))
        );
        // It glides back rather than arriving at once: the eye still moves after the press,
        // and comes to rest on the hall's camera.
        let home = j.v.room_camera(&j.s.world, "targets.hall").unwrap().eye;
        assert!((j.eyes[&53] - home).length() > 0.1);
        assert!((j.eyes[&53] - j.eyes[&60]).length() > 0.1);
        assert!((j.eyes[&90] - home).length() < 1e-3);

        // Replay reaches the same world, and a fresh view redraws the note taken while
        // the camera glided back.
        let rec = j.s.recording().unwrap();
        let mut replay = Replay::new(&rec).unwrap();
        let mut rebuilt = View::detached(replay.session.modules(), 3);
        let note = j.note.as_ref().unwrap();
        while replay.session.world.tick < note.tick {
            rebuilt.before_step(&replay.session.world);
            replay.step().unwrap();
        }
        let redrawn = rebuilt.note_scene(&replay.session.world, &note.moment);
        assert_eq!(redrawn.camera, note.scene.camera);
        assert_eq!(redrawn.meshes, note.scene.meshes);
        while replay.step().unwrap() {}
        assert_eq!(replay.session.world.hash(), j.s.world.hash());
    }
}

#[test]
fn camera_changes_during_any_glide_continue_from_the_current_view() {
    // Framing a figure while still gliding in, then going back, still arrives at the
    // place once.
    let j = journey(
        &[
            (5, "go", "ui", ""),
            (15, "pick", "ui", "first"),
            (20, "back", "keyboard", ""),
            (24, "pick", "ui", "second"),
        ],
        90,
        26,
    );
    assert_no_jump(&j, "while arriving");
    assert!(j.jumps.iter().any(|(t, ..)| *t == 15));
    let facts: Vec<_> = j.s.world.facts.iter().map(|f| f.kind.as_str()).collect();
    assert_eq!(facts.iter().filter(|k| **k == PLACE_ARRIVED).count(), 1);
    assert!(!facts.contains(&CAMERA_SETTLED));
    let second = j.v.room_camera(&j.s.world, "targets.hall").unwrap().eye;
    assert!((j.eyes[&90] - second).length() < 1e-3);

    // Leaving the hall mid-glide fades out of the glide as it moves, not from a snap.
    let j = journey(
        &[
            (5, "go", "ui", ""),
            (40, "pick", "ui", "first"),
            (45, "back", "keyboard", ""),
            (48, "back", "keyboard", ""),
        ],
        60,
        49,
    );
    assert_no_jump(&j, "fading out");
    assert!(j.jumps.iter().any(|(t, ..)| *t == 48));
    assert_eq!(
        j.s.world
            .component("targets.player", "engine.location")
            .unwrap()
            .fields["place"],
        "targets.yard"
    );
}
