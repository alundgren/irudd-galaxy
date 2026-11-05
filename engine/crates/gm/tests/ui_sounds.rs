//! UI sounds, headless: every screen action leaves its cue the moment the view sees it,
//! the module's `[ui]` table picks the sound (look over kind over base), hovers are spaced
//! by `hover_gap`, and the cues a played session recorded mix offline through
//! `galaxy-gm audio mix` with no window or device. The world never hears any of it.
use galaxy_audio::{Bank, Scene};
use galaxy_module::module::Module;
use galaxy_render::{Rect, glam::Vec2};
use galaxy_sim::{BuildStamp, Input, Session, recording::RecEvent};
use galaxy_view::View;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command;

fn engine() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn size() -> Vec2 {
    Vec2::new(1280.0, 720.0)
}

/// A played session in miniature: the session, its view listening for UI cues, and the
/// audio scene that turns them into sounds.
struct Desk {
    s: Session,
    v: View,
    audio: Scene,
    /// Every input stepped, by tick, to run the world again without a view.
    inputs: Vec<Vec<Input>>,
    /// The sound each cue played live, in order (none when silent).
    played: Vec<Option<String>>,
}

impl Desk {
    fn new() -> Desk {
        let module = Module::read(&engine().join("tests/play/ui-sounds/demo")).unwrap();
        let mut s = Session::new(vec![module], 1).unwrap();
        s.start_recording(BuildStamp::current());
        let bank = Bank::load(s.modules());
        assert!(bank.problems.is_empty(), "{:?}", bank.problems);
        let mut v = View::detached(s.modules(), 1);
        v.screens.listen(true);
        Desk {
            s,
            v,
            audio: Scene::new(bank),
            inputs: Vec::new(),
            played: Vec::new(),
        }
    }

    /// The cues waiting since the last call, as `(action, sound)`; recorded like play does.
    fn hear(&mut self) -> Vec<(&'static str, Option<String>)> {
        let mut out = Vec::new();
        for cue in self.v.screens.take_cues() {
            let sound = self.audio.ui(&cue).map(|(_, name)| name.to_owned());
            out.push((cue.action.name(), sound.clone()));
            self.played.push(sound);
            self.s.record_ui_sound(cue);
        }
        out
    }

    /// One drawn frame of the current tick, and what it heard.
    fn frame(&mut self) -> Vec<(&'static str, Option<String>)> {
        self.v.scene(&self.s.world, 1.0, size(), 0.0);
        self.hear()
    }

    /// Runs one tick with `inputs`, then draws it.
    fn step(&mut self, inputs: Vec<Input>) -> Vec<(&'static str, Option<String>)> {
        self.s.step(&inputs).unwrap();
        self.inputs.push(inputs);
        self.frame()
    }

    /// Quiet ticks, so the next hover is past the gap and a released token has settled.
    fn idle(&mut self, ticks: usize) -> Vec<(&'static str, Option<String>)> {
        (0..ticks).flat_map(|_| self.step(Vec::new())).collect()
    }

    fn at(&self, id: &str) -> Vec2 {
        centre(&self.v, id)
    }

    fn next(&self) -> u64 {
        self.s.world.tick + 1
    }

    fn move_to(&mut self, id: &str) -> Vec<(&'static str, Option<String>)> {
        let (p, t) = (self.at(id), self.next());
        assert!(self.v.screens.pointer_move(p, t).is_none());
        self.frame()
    }

    fn down(&mut self, id: &str) -> Option<Input> {
        let (p, t) = (self.at(id), self.next());
        self.v.screens.pointer_down(p, t)
    }

    fn up(&mut self, id: &str) -> Option<Input> {
        let (p, t) = (self.at(id), self.next());
        self.v.screens.pointer_up(p, t)
    }
}

/// The centre of the widget with this id, as laid out in the last frame.
fn centre(v: &View, id: &str) -> Vec2 {
    fn find(b: &galaxy_view::screens::Boxed, id: &str) -> Option<Rect> {
        if b.widget.id.as_deref() == Some(id) {
            return Some(b.rect);
        }
        b.children.iter().find_map(|c| find(c, id))
    }
    v.screens
        .boxes
        .iter()
        .find_map(|b| find(b, id))
        .unwrap_or_else(|| panic!("no widget {id}"))
        .center()
}

fn one(action: &'static str, sound: &str) -> Vec<(&'static str, Option<String>)> {
    vec![(action, Some(format!("demo.{sound}")))]
}

#[test]
fn every_ui_action_sounds_at_once_and_hovers_are_spaced() {
    let mut d = Desk::new();
    assert_eq!(
        d.frame(),
        vec![],
        "what the first frame shows is already there"
    );

    // Hover: one sound per thing entered, none while the pointer stays on it.
    assert_eq!(d.move_to("shelf"), one("hover", "tick"));
    assert_eq!(d.frame(), vec![]);
    let p = d.at("shelf") + Vec2::new(4.0, 0.0);
    d.v.screens.pointer_move(p, d.next());
    assert_eq!(
        d.frame(),
        vec![],
        "moving within a button is not a new hover"
    );

    // Press: the cue is there as the click is handled, before any frame or tick.
    let toggle = d.down("shelf").expect("the shelf button sends toggle");
    assert_eq!(d.hear(), one("press", "click"));
    assert!(d.up("shelf").is_none());
    assert_eq!(d.hear(), vec![]);
    // Open: the shelf panel appears on the tick the toggle runs.
    assert_eq!(d.step(vec![toggle]), one("open", "open"));

    // A look's own press sound wins over the base one.
    d.idle(5);
    assert_eq!(d.move_to("chip"), one("hover", "tick"));
    let pick = d.down("chip").expect("chip sends pick");
    assert_eq!(d.hear(), one("press", "coin"));
    d.up("chip");
    d.step(vec![pick]);

    // Deny: a resting button takes the click, records nothing and says no.
    d.idle(5);
    assert_eq!(d.move_to("locked"), one("hover", "tick"));
    assert!(
        d.down("locked").is_none(),
        "a resting button records nothing"
    );
    assert_eq!(d.hear(), one("deny", "thud"));
    d.up("locked");

    // Drag: the source's kind has its own hover; a refusing slot denies the drop.
    d.idle(5);
    assert_eq!(d.move_to("token"), one("hover", "lift"));
    assert!(d.down("token").is_none());
    assert_eq!(d.hear(), one("drag", "lift"));
    assert_eq!(
        d.move_to("narrow"),
        vec![],
        "no hovers while a value is held"
    );
    assert!(d.up("narrow").is_none());
    assert_eq!(d.hear(), one("deny", "thud"));
    // Settled home, picked up again and let go on the slot that takes it.
    d.idle(10);
    d.move_to("token");
    d.down("token");
    assert_eq!(d.hear(), one("drag", "lift"));
    d.move_to("wide");
    let place = d.up("wide").expect("the wide slot takes the token");
    assert_eq!(d.hear(), one("drop", "place"));
    d.step(vec![place]);
    d.idle(10);

    // Close: the shelf goes away on the tick the second toggle runs.
    d.move_to("shelf");
    let toggle = d.down("shelf").unwrap();
    assert_eq!(d.hear(), one("press", "click"));
    d.up("shelf");
    assert_eq!(d.step(vec![toggle]), one("close", "close"));

    // A fast sweep down the list: a cue for every row entered, a sound at most every
    // hover_gap (0.08 s). One row a tick (1/30 s) sounds on rows 1 and 4.
    d.idle(5);
    let rows = [
        "row-ash",
        "row-birch",
        "row-cedar",
        "row-elm",
        "row-fir",
        "row-oak",
    ];
    let mut swept = Vec::new();
    for row in rows {
        let (p, t) = (d.at(row), d.next());
        d.v.screens.pointer_move(p, t);
        swept.extend(d.step(Vec::new()));
    }
    assert_eq!(swept.len(), 6, "one hover per row entered: {swept:?}");
    let sounded: Vec<bool> = swept.iter().map(|(_, s)| s.is_some()).collect();
    assert_eq!(sounded, [true, false, false, true, false, false]);
    assert_eq!(d.idle(3), vec![], "resting on a row repeats nothing");
    // A slow sweep back, five ticks a row: every row sounds.
    for row in rows.iter().rev().skip(1) {
        let (p, t) = (d.at(row), d.next());
        d.v.screens.pointer_move(p, t);
        let heard = d.step(Vec::new());
        assert_eq!(heard, one("hover", "tick"), "{row}");
        d.idle(4);
    }

    // The world never heard a cue: the same inputs without a view end on the same hash,
    // and the recording replays to it with its UI sounds in it.
    let mut quiet = Session::new(d.s.modules().cloned().collect(), 1).unwrap();
    for inputs in &d.inputs {
        quiet.step(inputs).unwrap();
    }
    assert_eq!(quiet.world.hash(), d.s.world.hash());
    let rec = d.s.recording().unwrap();
    let recorded = rec
        .events
        .iter()
        .filter(|e| matches!(e, RecEvent::UiSound(_)))
        .count();
    assert_eq!(recorded, d.played.len());
    let (_, replayed) = galaxy_sim::replay(&rec).unwrap();
    assert!(replayed.identical, "{replayed:?}");

    // Offline: the recording's UI sounds mix through the same scene, with no device.
    let dir = tempfile::tempdir().unwrap();
    rec.write(&dir.path().join("recording.json")).unwrap();
    let modules = engine().join("tests/play/ui-sounds");
    std::fs::write(
        dir.path().join("mix.toml"),
        format!(
            "modules = {:?}\nrecording = 'recording.json'\n",
            modules.to_str().unwrap()
        ),
    )
    .unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_engine-galaxy-gm"))
        .args(["audio", "mix"])
        .arg(dir.path().join("mix.toml"))
        .arg("--out")
        .arg(dir.path().join("mix.wav"))
        .current_dir(engine())
        .env_remove("GALAXY_INSTALL_ROOT")
        .output()
        .unwrap();
    let mix: Value = serde_json::from_slice(&out.stdout)
        .unwrap_or_else(|_| panic!("{}", String::from_utf8_lossy(&out.stderr)));
    assert_eq!(out.status.code(), Some(0), "{mix}");
    assert_eq!(mix["world_hash"], d.s.world.hash());
    let heard = mix["ui_sounds"].as_array().unwrap();
    // Offline, every cue plays exactly what it played live: the same gap, the same draw.
    let offline: Vec<Option<String>> = heard
        .iter()
        .map(|c| c["sound"].as_str().map(str::to_owned))
        .collect();
    assert_eq!(offline, d.played);
    let sound_of = |action: &str| -> Vec<Value> {
        heard
            .iter()
            .filter(|c| c["action"] == action)
            .map(|c| c["sound"].clone())
            .collect()
    };
    for (action, sound) in [
        ("open", "demo.open"),
        ("close", "demo.close"),
        ("drop", "demo.place"),
    ] {
        assert_eq!(sound_of(action), [Value::from(sound)], "{action}");
    }
    assert_eq!(
        sound_of("press"),
        ["demo.click", "demo.coin", "demo.click"].map(Value::from)
    );
    assert_eq!(
        sound_of("deny"),
        ["demo.thud", "demo.thud"].map(Value::from)
    );
    assert_eq!(
        sound_of("drag"),
        ["demo.lift", "demo.lift"].map(Value::from)
    );
    // Each one starts on the frame its screen clock names: the sound bus carries it.
    assert!(
        mix["buses"]["sound"]["sample_peak_dbfs"].as_f64().unwrap() > -20.0,
        "{mix}"
    );
}

#[test]
fn modules_without_ui_sounds_stay_silent_and_views_not_listening_leave_no_cues() {
    let module = Module::read(&engine().join("tests/play/resting/demo")).unwrap();
    let s = Session::new(vec![module], 1).unwrap();
    let mut audio = Scene::new(Bank::load(s.modules()));
    let mut v = View::detached(s.modules(), 1);
    v.scene(&s.world, 1.0, size(), 0.0);
    // A check or film view never listens: a click there is no cue.
    let add = centre(&v, "add");
    v.screens.pointer_down(add, 1);
    assert!(v.screens.take_cues().is_empty());
    v.screens.listen(true);
    v.screens.pointer_down(add, 1);
    let cues = v.screens.take_cues();
    assert_eq!(cues.len(), 1);
    assert!(audio.ui(&cues[0]).is_none(), "no [ui]: the press is silent");
}

/// A copy of the demo module in `dir`, with `edit` applied to its files.
fn demo_copy(dir: &Path, edit: impl FnOnce(&Path)) -> PathBuf {
    let from = engine().join("tests/play/ui-sounds/demo");
    let to = dir.join("demo");
    for sub in ["", "scripts", "sounds"] {
        std::fs::create_dir_all(to.join(sub)).unwrap();
        for entry in std::fs::read_dir(from.join(sub)).unwrap() {
            let path = entry.unwrap().path();
            if path.is_file() {
                std::fs::copy(&path, to.join(sub).join(path.file_name().unwrap())).unwrap();
            }
        }
    }
    edit(&to);
    to
}

/// `galaxy-gm audio mix` of `rec` with the sounds in `modules`: its report.
fn mix(rec: &galaxy_sim::Recording, modules: &Path) -> Value {
    let dir = tempfile::tempdir().unwrap();
    rec.write(&dir.path().join("recording.json")).unwrap();
    std::fs::write(
        dir.path().join("mix.toml"),
        format!(
            "modules = {:?}\nrecording = 'recording.json'\n",
            modules.to_str().unwrap()
        ),
    )
    .unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_engine-galaxy-gm"))
        .args(["audio", "mix"])
        .arg(dir.path().join("mix.toml"))
        .current_dir(engine())
        .env_remove("GALAXY_INSTALL_ROOT")
        .output()
        .unwrap();
    let mix: Value = serde_json::from_slice(&out.stdout)
        .unwrap_or_else(|_| panic!("{}", String::from_utf8_lossy(&out.stderr)));
    assert_eq!(out.status.code(), Some(0), "{mix}");
    mix
}

#[test]
fn nested_panels_and_lists_open_and_close_inside_one_that_stays() {
    let dir = tempfile::tempdir().unwrap();
    let demo = demo_copy(dir.path(), |to| {
        std::fs::write(
            to.join("scripts/screen.luau"),
            r#"return {
    on = {
        toggle = function(_, ctx)
            local desk = ctx.get("state", "desk")
            ctx.patch("state", "desk", { shelf = not desk.shelf })
        end,
        pick = function() end,
        place = function() end,
        locked = function() end,
    },
    screen = function(ctx, ui)
        local desk = ctx.get("state", "desk")
        local inner = {}
        if desk.shelf then
            inner = {
                ui.panel { id = "nested", children = {
                    ui.text { text = "Now open" },
                    ui.scroll { id = "deep", height = 60, children = { ui.text { text = "Deep" } } },
                } },
                ui.scroll { id = "list", height = 60, children = { ui.text { text = "Row" } } },
            }
        end
        return ui.panel { id = "outer", width = 400, children = inner }
    end,
}
"#,
        )
        .unwrap();
    });
    let mut s = Session::new(vec![Module::read(&demo).unwrap()], 1).unwrap();
    let mut v = View::detached(s.modules(), 1);
    v.screens.listen(true);
    let frame = |s: &Session, v: &mut View| -> Vec<(&'static str, String)> {
        v.scene(&s.world, 1.0, size(), 0.0);
        let mut heard: Vec<_> = v
            .screens
            .take_cues()
            .into_iter()
            .map(|c| (c.action.name(), c.widget))
            .collect();
        heard.sort();
        heard
    };
    assert_eq!(frame(&s, &mut v), vec![]);
    let toggle = || Input {
        tick: 0,
        source: "ui".into(),
        name: "demo.toggle".into(),
        payload: Default::default(),
    };
    let step = |s: &mut Session| {
        let mut input = toggle();
        input.tick = s.world.tick + 1;
        s.step(&[input]).unwrap();
    };
    step(&mut s);
    // The panel and the list in the outer panel open; the list inside the new panel is
    // part of that panel's sound.
    let heard = frame(&s, &mut v);
    let widgets = |heard: &[(&str, String)]| -> Vec<String> {
        heard
            .iter()
            .map(|(_, w)| w.rsplit('/').next().unwrap().to_owned())
            .collect()
    };
    assert_eq!(widgets(&heard), ["list[2]", "nested[1]"], "{heard:?}");
    assert!(heard.iter().all(|(a, _)| *a == "open"), "{heard:?}");
    assert_eq!(
        frame(&s, &mut v),
        vec![],
        "a panel that stays is heard once"
    );
    step(&mut s);
    let heard = frame(&s, &mut v);
    assert_eq!(widgets(&heard), ["list[2]", "nested[1]"], "{heard:?}");
    assert!(heard.iter().all(|(a, _)| *a == "close"), "{heard:?}");
}

#[test]
fn a_recorded_fresh_start_mixes_its_start_music_and_a_resumed_save_does_not() {
    let dir = tempfile::tempdir().unwrap();
    let demo = demo_copy(dir.path(), |to| {
        let mut audio = std::fs::read_to_string(to.join("audio.toml")).unwrap();
        audio = audio.replace(
            "\n[ui]\n",
            "\n[[events]]\nevent = \"engine.start\"\nsound = \"open\"\nbus = \"music\"\nfade = 0\n\n[ui]\n",
        );
        std::fs::write(to.join("audio.toml"), audio).unwrap();
    });
    let module = Module::read(&demo).unwrap();
    let mut fresh = Session::new(vec![module.clone()], 1).unwrap();
    fresh.start_recording(BuildStamp::current());
    for _ in 0..3 {
        fresh.step(&[]).unwrap();
    }
    let heard = mix(&fresh.recording().unwrap(), &demo);
    assert!(
        heard["buses"]["music"]["sample_peak_dbfs"].is_number(),
        "a fresh start plays engine.start, as play does: {heard}"
    );
    // Resumed from a save: engine.start sounded in the session that made it.
    let mut resumed = Session::resume(vec![module], fresh.world.clone()).unwrap();
    resumed.start_recording(BuildStamp::current());
    for _ in 0..3 {
        resumed.step(&[]).unwrap();
    }
    let heard = mix(&resumed.recording().unwrap(), &demo);
    assert!(
        heard["buses"]["music"]["sample_peak_dbfs"].is_null(),
        "a resumed save's old facts stay quiet: {heard}"
    );
}

#[test]
fn a_recording_mix_ends_on_a_hot_load_made_after_its_last_tick() {
    let dir = tempfile::tempdir().unwrap();
    let demo = demo_copy(dir.path(), |_| {});
    let mut s = Session::new(vec![Module::read(&demo).unwrap()], 1).unwrap();
    s.start_recording(BuildStamp::current());
    for _ in 0..3 {
        s.step(&[]).unwrap();
    }
    let script = demo.join("scripts/screen.luau");
    let mut text = std::fs::read_to_string(&script).unwrap();
    text.push_str("-- edited after the last tick\n");
    std::fs::write(&script, text).unwrap();
    s.hot_load(Module::read(&demo).unwrap()).unwrap();
    let rec = s.recording().unwrap();
    let (_, replayed) = galaxy_sim::replay(&rec).unwrap();
    assert!(replayed.identical, "{replayed:?}");
    let heard = mix(&rec, &demo);
    assert_eq!(heard["world_hash"], rec.end.hash, "{heard}");
}
