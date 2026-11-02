use galaxy_module::module::Module;
use galaxy_render::glam::Vec2;
use galaxy_sim::{
    Session,
    recording::{BuildStamp, Replay},
    scenario::{InputsFile, Pointer},
};
use galaxy_view::{View, screens::Boxed};
use std::path::Path;

fn module() -> Module {
    Module::read(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/play/rich-screens/rich"))
        .unwrap()
}
fn draw(v: &mut View, s: &Session) -> galaxy_render::Scene {
    let scene = v.scene(&s.world, 1.0, Vec2::new(1280.0, 720.0), 0.0);
    assert!(v.screens.problems.is_empty(), "{:?}", v.screens.problems);
    scene
}
fn find<'a>(b: &'a Boxed, id: &str) -> Option<&'a Boxed> {
    if b.widget.id.as_deref() == Some(id) {
        return Some(b);
    }
    b.children.iter().find_map(|b| find(b, id))
}
fn gesture(action: &str, widget: &str, delta: f32) -> Pointer {
    Pointer {
        tick: 1,
        action: action.into(),
        widget: widget.into(),
        delta,
    }
}

#[test]
fn rich_bot_replays_world_inputs_and_rebuilds_each_scrolled_and_dragging_note() {
    let mut s = Session::new(vec![module()], 1).unwrap();
    s.start_recording(BuildStamp::current());
    let mut v = View::detached(s.modules(), 1);
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/play/rich-screens-inputs.toml");
    let bot = InputsFile::read(&path).unwrap();
    for tick in 1..=70 {
        draw(&mut v, &s);
        let inputs: Vec<_> = bot
            .pointers
            .iter()
            .filter(|p| p.tick == tick)
            .filter_map(|p| v.screens.bot_pointer(p).unwrap())
            .collect();
        s.step(&inputs).unwrap();
        let scene = draw(&mut v, &s);
        s.record_presentation(serde_json::to_value(&v.screens.presentation).unwrap());
        if let Some((_, text)) = bot.notes.iter().find(|n| n.0 == tick) {
            let moment = v.moment(1.0, 0.0, (1280, 720), 1.0).unwrap();
            s.note(text, Some(serde_json::to_value(&moment).unwrap()), None);
            let recording = s.recording().unwrap();
            let mut replay = Replay::new(&recording).unwrap();
            while replay.step().unwrap() {}
            let mut rebuilt = View::detached(replay.session.modules(), 1);
            rebuilt.screens.presentation = moment.screens;
            assert_eq!(draw(&mut rebuilt, &replay.session).overlay, scene.overlay);
        }
    }
    assert_eq!(
        s.world
            .component("rich.state", "rich.state")
            .unwrap()
            .fields["drops"],
        1
    );
    assert_eq!(
        s.world
            .component("rich.state", "rich.state")
            .unwrap()
            .fields["offset"],
        260.0
    );
    let rec = s.recording().unwrap();
    let mut replay = Replay::new(&rec).unwrap();
    while replay.step().unwrap() {}
    assert_eq!(s.world.hash(), replay.session.world.hash());
    assert!(
        galaxy_view::film::recorded_zooms(&rec).iter().any(
            |(_, z)| matches!(z, galaxy_view::film::Zoom::Presentation(p) if p.drag.is_some())
        )
    );
}

#[test]
fn scroll_clips_hit_tests_and_drag_verdicts_do_not_call_luau() {
    let s = Session::new(vec![module()], 1).unwrap();
    let mut v = View::detached(s.modules(), 1);
    draw(&mut v, &s);
    let calls = v.screens.calls();
    let before = find(&v.screens.boxes[0], "wide").unwrap().rect;
    v.screens
        .bot_pointer(&gesture("scroll", "entries", 260.0))
        .unwrap();
    draw(&mut v, &s);
    let first = find(&v.screens.boxes[0], "entry-1").unwrap();
    assert!(first.rect.y < first.clip.unwrap().y);
    assert!(
        v.screens
            .bot_click(&galaxy_sim::scenario::Click {
                tick: 1,
                target: galaxy_sim::scenario::ClickTarget::Widget("entry-1".into())
            })
            .is_err()
    );
    v.screens
        .bot_pointer(&gesture("pickup", "large", 0.0))
        .unwrap();
    let dragging = draw(&mut v, &s);
    assert!(
        dragging
            .overlay
            .widgets
            .iter()
            .flat_map(|l| &l.texts)
            .any(|t| t.text.contains("Cannot drop: Needs a wider slot"))
    );
    assert_eq!(before, find(&v.screens.boxes[0], "wide").unwrap().rect);
    v.screens
        .bot_pointer(&gesture("move", "narrow", 0.0))
        .unwrap();
    let scene = draw(&mut v, &s);
    let preview = scene.overlay.widgets.last().unwrap().panels[0].rect;
    let target = find(&v.screens.boxes[0], "narrow").unwrap().rect;
    assert_eq!(
        preview.center(),
        target.center(),
        "the token stays under the pointer"
    );
    assert_eq!(
        scene
            .overlay
            .widgets
            .iter()
            .flat_map(|l| &l.texts)
            .filter(|t| t.text == "Large token")
            .count(),
        1,
        "only one token is drawn"
    );
    assert!(
        v.screens
            .bot_pointer(&gesture("drop", "narrow", 0.0))
            .unwrap()
            .is_none()
    );
    assert!(v.screens.presentation.drag.is_none());
    assert_eq!(v.screens.calls(), calls);
}

#[test]
fn glyph_catalogue_and_unreadable_grids_are_checked_headlessly() {
    for (file, replacement, expected) in [
        ("icons.toml", "[icons]\nother='X'", "missing module icon"),
        ("icons.toml", "[icons]\ntoken='\u{10ffff}'", "glyph"),
        (
            "screen.luau",
            "return {screen=function(_,ui) return ui.grid{width=10,columns=2,children={ui.text{text='Wide'},ui.text{text='W'}}} end}",
            "clipped",
        ),
    ] {
        let mut m = module();
        m.files.0.insert(file.into(), replacement.into());
        let s = Session::new(vec![m], 1).unwrap();
        let mut v = View::detached(s.modules(), 1);
        v.scene(&s.world, 1.0, Vec2::new(1280.0, 720.0), 0.0);
        assert!(
            v.screens.problems.iter().any(|p| p.contains(expected)),
            "{:?}",
            v.screens.problems
        );
    }
}

#[test]
fn thumb_drag_and_token_drop_motion_survive_state_changes_and_notes() {
    let mut s = Session::new(vec![module()], 1).unwrap();
    s.start_recording(BuildStamp::current());
    let mut v = View::detached(s.modules(), 1);
    let bot = InputsFile::read(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/play/rich-screens-motion.toml"),
    )
    .unwrap();
    let mut held = None;
    let mut returning = None;
    for tick in 1..=132 {
        draw(&mut v, &s);
        let inputs: Vec<_> = bot
            .pointers
            .iter()
            .filter(|p| p.tick == tick)
            .filter_map(|p| v.screens.bot_pointer(p).unwrap())
            .collect();
        s.step(&inputs).unwrap();
        let scene = draw(&mut v, &s);
        s.record_presentation(serde_json::to_value(&v.screens.presentation).unwrap());
        if tick == 1 {
            let list = find(&v.screens.boxes[0], "entries").unwrap();
            let first = find(list, "entry-1").unwrap();
            assert!(first.rect.x >= list.rect.x + 12.0);
            assert!(first.rect.y >= list.rect.y + 12.0);
            let panels: Vec<_> = scene
                .overlay
                .widgets
                .iter()
                .flat_map(|l| &l.panels)
                .collect();
            let thumb = panels
                .iter()
                .find(|p| p.rect.w == 32.0 && p.rect.h >= 44.0)
                .unwrap();
            assert!(thumb.rect.x >= first.rect.x + first.rect.w + 8.0);
        }
        if tick == 20 {
            let list = find(&v.screens.boxes[0], "entries").unwrap();
            assert!(
                (v.screens.presentation.scroll[&list.widget.path]
                    - (list.content_height - list.rect.h))
                    .abs()
                    < 0.001
            );
            assert!(
                (s.world
                    .component("rich.state", "rich.state")
                    .unwrap()
                    .fields["offset"]
                    .as_f64()
                    .unwrap()
                    - f64::from(list.content_height - list.rect.h))
                .abs()
                    < 0.001
            );
        }
        if v.screens.presentation.drag.is_some() || v.screens.presentation.settle.is_some() {
            assert_eq!(
                scene
                    .overlay
                    .widgets
                    .iter()
                    .flat_map(|l| &l.texts)
                    .filter(|t| t.text == "Large token")
                    .count(),
                1
            );
        }
        if tick == 46 {
            held = Some(scene.overlay.widgets.last().unwrap().panels[0].rect);
        }
        if tick == 53 {
            let r = scene.overlay.widgets.last().unwrap().panels[0].rect;
            assert_ne!(Some(r), held);
            returning = Some(r);
        }
        if tick == 62 {
            assert!(v.screens.presentation.settle.is_none());
            assert_ne!(
                Some(find(&v.screens.boxes[0], "large").unwrap().rect),
                returning
            );
        }
        if [98, 130].contains(&tick) {
            assert!(v.screens.presentation.settle.is_none());
            let slot = find(&v.screens.boxes[0], "wide").unwrap();
            assert!(
                find(slot, "large").is_some(),
                "the one token is now in its slot"
            );
            assert_eq!(
                s.world
                    .component("rich.state", "rich.state")
                    .unwrap()
                    .fields["drops"],
                1
            );
        }
        if let Some((_, text)) = bot.notes.iter().find(|n| n.0 == tick) {
            let moment = v.moment(1.0, 0.0, (1280, 720), 1.0).unwrap();
            s.note(text, Some(serde_json::to_value(&moment).unwrap()), None);
            let rec = s.recording().unwrap();
            let mut replay = Replay::new(&rec).unwrap();
            while replay.step().unwrap() {}
            let mut rebuilt = View::detached(replay.session.modules(), 1);
            rebuilt.screens.presentation = moment.screens;
            assert_eq!(draw(&mut rebuilt, &replay.session).overlay, scene.overlay);
        }
    }
    let rec = s.recording().unwrap();
    let mut replay = Replay::new(&rec).unwrap();
    while replay.step().unwrap() {}
    assert_eq!(s.world.hash(), replay.session.world.hash());
}

#[test]
fn track_click_pages_to_pointer_and_release_over_nothing_returns_home() {
    let s = Session::new(vec![module()], 1).unwrap();
    let mut v = View::detached(s.modules(), 1);
    draw(&mut v, &s);
    let input = v
        .screens
        .bot_pointer(&gesture("pickup", "entries::track-end", 0.0))
        .unwrap()
        .unwrap();
    assert!(input.payload["offset"].as_f64().unwrap() > 500.0);
    v.screens
        .bot_pointer(&gesture("drop", "entries::track-end", 0.0))
        .unwrap();
    v.screens
        .bot_pointer(&gesture("pickup", "large", 0.0))
        .unwrap();
    let p = Vec2::new(1250.0, 680.0);
    v.screens.pointer_move(p, 1);
    assert!(v.screens.pointer_up(p, 1).is_none());
    assert!(v.screens.presentation.settle.is_some());
    let state = v.screens.presentation.clone();
    let mut rebuilt = View::detached(s.modules(), 1);
    rebuilt.screens.presentation =
        serde_json::from_value(serde_json::to_value(state).unwrap()).unwrap();
    assert_eq!(draw(&mut v, &s).overlay, draw(&mut rebuilt, &s).overlay);
}

#[test]
fn lost_focus_returns_the_token_and_releases_the_scroll_thumb() {
    let s = Session::new(vec![module()], 1).unwrap();
    let mut v = View::detached(s.modules(), 1);
    draw(&mut v, &s);
    v.screens
        .bot_pointer(&gesture("pickup", "entries::thumb", 0.0))
        .unwrap();
    v.screens.cancel_pointer(1);
    assert!(v.screens.presentation.scroll_grab.is_none());
    v.screens
        .bot_pointer(&gesture("pickup", "large", 0.0))
        .unwrap();
    v.screens.pointer_move(Vec2::new(500.0, 600.0), 1);
    v.screens.cancel_pointer(1);
    assert!(v.screens.presentation.drag.is_none());
    assert!(v.screens.presentation.settle.is_some());
}

#[test]
fn earlier_screen_recordings_are_refused_clearly() {
    use galaxy_sim::recording::Recording;
    let mut s = Session::new(vec![module()], 1).unwrap();
    s.start_recording(BuildStamp::current());
    s.step(&[]).unwrap();
    s.record_presentation(
        serde_json::to_value(galaxy_view::screens::Presentation::default()).unwrap(),
    );
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("recording.json");
    s.recording().unwrap().write(&path).unwrap();
    Recording::read(&path).unwrap();
    let mut raw: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    for event in raw["events"].as_array_mut().unwrap() {
        if event["type"] == "presentation" {
            event["state"].as_object_mut().unwrap().remove("version");
        }
    }
    std::fs::write(&path, serde_json::to_vec(&raw).unwrap()).unwrap();
    let message = Recording::read(&path).err().unwrap();
    assert!(message.contains("screen presentation format"));
    assert!(message.contains("build that made it"));
}
