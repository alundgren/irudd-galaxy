//! A resting button: drawn in place with its own fill and a dashed frame, its reason on
//! hover, and a click on it records nothing, from the pointer or a bot.
use galaxy_module::module::Module;
use galaxy_render::{Overlay, Rect, glam::Vec2, screen_style::ScreenStyle};
use galaxy_sim::{
    BuildStamp, Replay, Session,
    scenario::{Click, ClickTarget, InputsFile},
};
use galaxy_view::View;
use std::path::Path;

fn size() -> Vec2 {
    Vec2::new(1280.0, 720.0)
}

fn root() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/play")
}

fn rect_of(v: &View, id: &str) -> Rect {
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
}

/// The surface painted exactly over `rect`.
fn panel_at(overlay: &Overlay, rect: Rect) -> galaxy_render::Panel {
    overlay
        .widgets
        .iter()
        .flat_map(|l| &l.panels)
        .find(|p| p.rect == rect)
        .copied()
        .expect("a surface over the button")
}

fn named(id: &str, tick: u64) -> Click {
    Click {
        tick,
        target: ClickTarget::Widget(id.into()),
    }
}

#[test]
fn a_resting_button_is_drawn_resting_explains_itself_and_records_nothing() {
    let module = Module::read(&root().join("resting/demo")).unwrap();
    let mut s = Session::new(vec![module], 1).unwrap();
    s.start_recording(BuildStamp::current());
    let mut v = View::detached(s.modules(), 1);
    let scene = v.scene(&s.world, 1.0, size(), 0.0);
    assert!(v.screens.problems.is_empty(), "{:?}", v.screens.problems);
    let style = ScreenStyle::default();
    style.check().unwrap();

    // Ready: the button fill and a solid frame. Resting: its own fill, dashed.
    let (add, empty) = (rect_of(&v, "add"), rect_of(&v, "empty"));
    let ready = panel_at(&scene.overlay, add);
    assert_eq!((ready.fill, ready.dash), (style.button, 0.0));
    let resting = panel_at(&scene.overlay, empty);
    assert_eq!(resting.fill, style.resting_fill());
    assert_eq!(resting.dash, style.resting_dash);
    assert!(resting.dash > 0.0 && resting.border_px >= 1.0);
    assert_ne!(resting.fill, ready.fill);
    // Its label keeps full-contrast text.
    let label = scene
        .overlay
        .widgets
        .iter()
        .flat_map(|l| &l.texts)
        .find(|t| t.text == "Empty")
        .unwrap();
    assert_eq!(label.color, style.text);

    // Hovering shows the reason, readably.
    let look = v.look(&s.world, 1.0, size(), empty.center());
    assert_eq!(look["hover"]["text"], "Nothing to empty yet.", "{look}");
    assert_eq!(look["hover"]["readable"], true, "{look}");

    // A pointer click lands on it and records nothing; a bot reports why.
    assert!(v.screens.click(empty.center(), 1).is_none());
    assert!(v.screens.pointer_down(empty.center(), 1).is_none());
    assert!(v.target_click(&s.world, empty.center(), 1).is_none());
    assert!(v.talk_click(&s.world, empty.center(), 1).is_none());
    let reason = Some((
        "demo:scripts/screen.luau/screen/row[2]/empty[2]".to_string(),
        "Nothing to empty yet.".to_string(),
    ));
    assert_eq!(v.screens.bot_resting(&named("empty", 1)), reason);
    let point = Click {
        tick: 1,
        target: ClickTarget::Point {
            x: empty.center().x,
            y: empty.center().y,
        },
    };
    assert_eq!(v.screens.bot_resting(&point), reason);
    for click in [named("empty", 1), point] {
        let e = v.bot_talk_click(&s.world, &click).unwrap_err();
        assert!(e.ends_with("resting: Nothing to empty yet."), "{e}");
    }
    assert_eq!(v.screens.bot_resting(&named("add", 1)), None);
    assert!(v.bot_talk_click(&s.world, &named("add", 1)).is_ok());
}

/// The bot file the window run plays: resting clicks are skipped and the rest replays.
#[test]
fn the_bot_run_skips_resting_clicks_and_replays_identically() {
    let module = Module::read(&root().join("resting/demo")).unwrap();
    let mut s = Session::new(vec![module], 1).unwrap();
    s.start_recording(BuildStamp::current());
    let mut v = View::detached(s.modules(), 1);
    let bot = InputsFile::read(&root().join("resting-inputs.toml")).unwrap();
    let mut resting = Vec::new();
    let mut notes = Vec::new();
    for tick in 1..=75 {
        v.before_step(&s.world);
        v.scene(&s.world, 1.0, size(), 0.0);
        for p in bot.pointers.iter().filter(|p| p.tick == tick) {
            assert!(v.bot_pointer(&s.world, p).unwrap().is_none());
        }
        let mut inputs = Vec::new();
        for c in bot.clicks.iter().filter(|c| c.tick == tick) {
            match v.screens.bot_resting(c) {
                Some((_, reason)) => resting.push((tick, reason)),
                None => inputs.push(v.bot_talk_click(&s.world, c).unwrap()),
            }
        }
        s.step(&inputs).unwrap();
        let scene = v.scene(&s.world, 1.0, size(), 0.0);
        assert!(v.screens.problems.is_empty(), "{:?}", v.screens.problems);
        s.record_presentation(serde_json::to_value(&v.screens.presented).unwrap());
        if let Some((_, text)) = bot.notes.iter().find(|n| n.0 == tick) {
            let moment = v.moment(1.0, 0.0, (1280, 720), 1.0).unwrap();
            s.note(text, Some(serde_json::to_value(&moment).unwrap()), None);
            notes.push((tick, moment, scene.overlay));
        }
    }
    assert_eq!(
        resting,
        [
            (20, "Nothing to empty yet.".to_string()),
            (55, "The tray is full: empty it first.".to_string())
        ]
    );
    let rec = s.recording().unwrap();
    let names: Vec<_> = rec
        .events
        .iter()
        .filter_map(|e| match e {
            galaxy_sim::recording::RecEvent::Input(i) => Some(i.name.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(names, ["demo.add", "demo.add", "demo.empty"]);
    assert_eq!(
        s.world.component("demo.state", "demo.tray").unwrap().fields["count"],
        0
    );
    let mut replay = Replay::new(&rec).unwrap();
    let mut rebuilt = View::detached(replay.session.modules(), 1);
    for (tick, moment, overlay) in &notes {
        while replay.session.world.tick < *tick {
            rebuilt.before_step(&replay.session.world);
            replay.step().unwrap();
        }
        assert_eq!(
            rebuilt.note_scene(&replay.session.world, moment).overlay,
            *overlay,
            "note tick {tick}"
        );
    }
    while replay.step().unwrap() {}
    assert_eq!(replay.session.world.hash(), s.world.hash());
}

fn screen(code: &str) -> (Session, View) {
    let mut files = galaxy_module::module::ModuleFiles::default();
    files.0.insert(
        "manifest.toml".into(),
        "id = 'test'\nversion = '0.1.0'\nmin_engine_version = '0.7.0'\nscripts = ['screen.luau']\nevents = ['click']\n".into(),
    );
    files.0.insert("screen.luau".into(), code.into());
    let module = Module::from_files(Path::new("test"), files).unwrap();
    let session = Session::new(vec![module], 1).unwrap();
    let view = View::detached(session.modules(), 1);
    (session, view)
}

#[test]
fn only_buttons_rest_with_a_reason_after_their_own_label() {
    for (widget, problem) in [
        (
            r#"ui.text { text = "Note", resting = "Why" }"#,
            "only buttons rest",
        ),
        (
            r#"ui.button { text = "Go", on = "click", resting = " " }"#,
            "resting needs a short reason",
        ),
    ] {
        let (s, mut v) = screen(&format!(
            "return {{ screen = function(_, ui) return {widget} end }}"
        ));
        v.scene(&s.world, 1.0, size(), 0.0);
        assert!(
            v.screens.problems.iter().any(|p| p.contains(problem)),
            "{widget}: {:?}",
            v.screens.problems
        );
    }
    let (s, mut v) = screen(
        r#"return { screen = function(_, ui)
            return ui.button { id = "go", text = "Go", on = "click", width = 200,
                hover = "Starts the run", resting = "Not yet." }
        end }"#,
    );
    v.scene(&s.world, 1.0, size(), 0.0);
    assert!(v.screens.problems.is_empty(), "{:?}", v.screens.problems);
    let look = v.look(&s.world, 1.0, size(), rect_of(&v, "go").center());
    assert_eq!(look["hover"]["text"], "Starts the run\nNot yet.", "{look}");
}
