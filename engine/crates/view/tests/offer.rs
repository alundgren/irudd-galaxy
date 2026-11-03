//! A talk bubble's styled heading, subtitle and marker, answers in a line, and `check`
//! pressing buttons to reach screens that scripts open. Fixtures: `tests/play/offer`
//! and `tests/play/sign-panel`.
use galaxy_api::{
    command::{Batch, Cause, Command},
    engine::*,
};
use galaxy_module::module::Module;
use galaxy_render::glam::Vec2;
use galaxy_sim::scenario::{Click, ClickTarget};
use galaxy_sim::{BuildStamp, Input, Replay, Session, save};
use galaxy_view::{View, screens::Boxed};
use serde_json::json;
use std::path::Path;

fn fixture(name: &str) -> Module {
    Module::read(&Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../../tests/play/{name}")))
        .unwrap()
}
/// A copy of a fixture with text replaced in its files, for a variant of one module.
fn variant(name: &str, edits: &[(&str, &str, &str)]) -> (tempfile::TempDir, Module) {
    fn copy(from: &Path, to: &Path) {
        std::fs::create_dir_all(to).unwrap();
        for e in std::fs::read_dir(from).unwrap() {
            let e = e.unwrap();
            if e.file_type().unwrap().is_dir() {
                copy(&e.path(), &to.join(e.file_name()));
            } else {
                std::fs::copy(e.path(), to.join(e.file_name())).unwrap();
            }
        }
    }
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("demo");
    copy(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../../tests/play/{name}")),
        &dir,
    );
    for (file, old, new) in edits {
        let path = dir.join(file);
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains(old), "{file}: no `{old}`");
        std::fs::write(&path, text.replace(old, new)).unwrap();
    }
    let module = Module::read(&dir).unwrap();
    (tmp, module)
}
fn size() -> Vec2 {
    Vec2::new(1280.0, 720.0)
}
fn input(v: &mut View, s: &Session, name: &str) -> Input {
    v.scene(&s.world, 1.0, size(), 0.0);
    v.bot_talk_click(
        &s.world,
        &Click {
            tick: s.world.tick + 1,
            target: ClickTarget::Widget(name.into()),
        },
    )
    .unwrap_or_else(|e| panic!("{name}: {e}"))
}
fn click(s: &mut Session, v: &mut View, name: &str) {
    let i = input(v, s, name);
    v.before_step(&s.world);
    s.step(&[i]).unwrap();
}
fn bubble(v: &View) -> &Boxed {
    v.screens
        .boxes
        .iter()
        .find(|b| b.widget.kind == "bubble")
        .expect("a bubble on screen")
}
fn part<'a>(b: &'a Boxed, name: &str) -> &'a Boxed {
    let path = format!("{}/{name}", b.widget.path);
    b.children
        .iter()
        .find(|c| c.widget.path == path)
        .unwrap_or_else(|| panic!("no {path}"))
}
fn talk(s: &Session, id: &str) -> serde_json::Map<String, serde_json::Value> {
    s.world.component(id, TALK).unwrap().fields.clone()
}
fn answers(s: &Session) -> serde_json::Map<String, serde_json::Value> {
    s.world
        .component("offer.state", "offer.answers")
        .unwrap()
        .fields
        .clone()
}

#[test]
fn the_heading_stands_out_and_subtitle_and_marker_are_quieter_in_one_target() {
    let mut s = Session::new(vec![fixture("offer/demo")], 1).unwrap();
    let mut v = View::detached(s.modules(), 1);
    click(&mut s, &mut v, "speaker.offer.host");
    assert_eq!(talk(&s, "offer.gift")["current"], "offer.ask");
    v.scene(&s.world, 1.0, size(), 0.0);
    assert!(v.screens.problems.is_empty(), "{:#?}", v.screens.problems);
    let b = bubble(&v);
    let text = |c: &Boxed| c.text.clone().unwrap();
    // The life sign's name moves into the bubble as its heading; its sign is hidden.
    let title = text(part(b, "title"));
    let subtitle = text(part(b, "subtitle"));
    let line = text(part(b, "line"));
    assert_eq!(title.text, "Host");
    assert_eq!(subtitle.text, "In the hall");
    assert!(title.size > line.size && title.weight > line.weight);
    assert!(subtitle.size < line.size && subtitle.weight < line.weight);
    assert!(!v.screens.boxes.iter().any(|b| b.widget.kind == "sign"));
    // A line with answers has no Continue marker and the bubble itself does not advance.
    assert!(b.widget.on.is_none());
    assert!(
        b.children
            .iter()
            .all(|c| !c.widget.path.ends_with("/marker"))
    );

    // Decline, then the repeat talk: Continue, then Done, both small and trailing.
    click(&mut s, &mut v, "choice.offer.not_now");
    click(&mut s, &mut v, "speaker.offer.host");
    assert_eq!(talk(&s, "offer.chat")["active"], true);
    v.scene(&s.world, 1.0, size(), 0.0);
    let b = bubble(&v).clone();
    let marker = part(&b, "marker");
    let (m, l) = (text(marker), text(part(&b, "line")));
    assert_eq!(m.text, "Continue");
    assert!(m.size < l.size && m.weight < l.weight);
    // Against the bubble's end edge.
    assert!(m.pos.x > b.rect.x + b.rect.w * 0.8);
    // The heading, line, marker and the paper between them all click the one bubble.
    let gap = Vec2::new(b.rect.center().x, part(&b, "line").rect.y - 2.0);
    for p in b.children.iter().map(|c| c.rect.center()).chain([gap]) {
        let i = v.screens.click(p, 1).expect("the bubble takes the click");
        assert_eq!(i.name, TALK_NEXT);
        assert_eq!(i.payload["line"], "offer.chat_1");
    }
    click(&mut s, &mut v, "talk.offer.chat");
    v.scene(&s.world, 1.0, size(), 0.0);
    assert_eq!(text(part(bubble(&v), "marker")).text, "Done");
}

#[test]
fn an_answer_records_once_shows_its_branch_and_replays() {
    let mut s = Session::new(vec![fixture("offer/demo")], 1).unwrap();
    s.start_recording(BuildStamp::current());
    let mut v = View::detached(s.modules(), 1);
    click(&mut s, &mut v, "speaker.offer.host");
    v.scene(&s.world, 1.0, size(), 0.0);
    let take = bubble(&v)
        .children
        .iter()
        .find(|c| c.widget.kind == "row")
        .unwrap()
        .children
        .iter()
        .map(|c| c.widget.text.clone())
        .collect::<Vec<_>>();
    assert_eq!(take, ["Take it", "Not now"], "answers in `order`");
    // A stale double click: the same drawn answer twice on one tick.
    let chosen = input(&mut v, &s, "choice.offer.take");
    assert_eq!(chosen.name, TALK_CHOSEN);
    assert_eq!(chosen.payload["line"], "offer.ask");
    v.before_step(&s.world);
    s.step(&[chosen.clone(), chosen.clone()]).unwrap();
    let events = |s: &Session| {
        s.world
            .facts
            .iter()
            .filter(|f| f.kind == TALK_CHOSEN)
            .count()
    };
    assert_eq!(events(&s), 1, "the stale second answer is no event");
    assert_eq!(answers(&s)["last"], "Take it");
    assert_eq!(answers(&s)["taken"], true);
    assert_eq!(talk(&s, "offer.gift")["current"], "offer.taken");
    // The chosen branch shows, with Done, as one click target again.
    v.scene(&s.world, 1.0, size(), 0.0);
    let b = bubble(&v);
    assert_eq!(
        part(b, "line").text.as_ref().unwrap().text,
        "It's yours. Mind the handle."
    );
    assert!(b.widget.on.is_some());
    // An old answer queued later still does nothing.
    v.before_step(&s.world);
    s.step(std::slice::from_ref(&chosen)).unwrap();
    assert_eq!(events(&s), 1);
    let drawn = v.scene(&s.world, 0.5, size(), 2.0);

    let rec = s.recording().unwrap();
    let mut replay = Replay::new(&rec).unwrap();
    let mut rebuilt = View::detached(replay.session.modules(), 1);
    while replay.session.world.tick < s.world.tick {
        rebuilt.before_step(&replay.session.world);
        assert!(replay.step().unwrap());
    }
    assert_eq!(s.world.hash(), replay.session.world.hash());
    assert_eq!(
        drawn.overlay,
        rebuilt
            .scene(&replay.session.world, 0.5, size(), 2.0)
            .overlay,
        "a note mid-talk redraws identically"
    );
    let tmp = tempfile::tempdir().unwrap();
    let file = tmp.path().join("offer.db");
    save::write(&file, &s.world, s.modules(), &BuildStamp::current()).unwrap();
    let (_, w) = save::read(&file).unwrap();
    let resumed = Session::resume(vec![fixture("offer/demo")], w).unwrap();
    assert_eq!(resumed.world.hash(), s.world.hash());

    click(&mut s, &mut v, "talk.offer.gift");
    assert_eq!(talk(&s, "offer.gift")["seen"], true);
}

#[test]
fn a_declined_offer_ends_the_talk_and_module_logic_keeps_it_away() {
    let mut s = Session::new(vec![fixture("offer/demo")], 1).unwrap();
    let mut v = View::detached(s.modules(), 1);
    click(&mut s, &mut v, "speaker.offer.host");
    let advance = Input {
        tick: 0,
        source: "test".into(),
        name: TALK_NEXT.into(),
        payload: json!({"talk": "offer.gift", "line": "offer.ask"})
            .as_object()
            .unwrap()
            .clone(),
    };
    s.step(&[advance]).unwrap();
    assert_eq!(
        talk(&s, "offer.gift")["current"],
        "offer.ask",
        "answers only"
    );
    click(&mut s, &mut v, "choice.offer.not_now");
    let gift = talk(&s, "offer.gift");
    assert_eq!(gift["active"], false);
    assert_eq!(gift["seen"], true);
    assert_eq!(gift["enabled"], false);
    assert_eq!(answers(&s)["declined"], true);
    click(&mut s, &mut v, "speaker.offer.host");
    assert_eq!(talk(&s, "offer.chat")["active"], true);
    assert_eq!(talk(&s, "offer.gift")["active"], false);
}

#[test]
fn an_answer_queued_after_its_talk_is_removed_does_nothing_and_replays() {
    // A handler may remove a finished one-shot talk; a second click is still queued.
    let (_tmp, module) = variant(
        "offer/demo",
        &[(
            "scripts/main.luau",
            r#"ctx.patch("gift", "engine.talk", { enabled = false })"#,
            r#"ctx.remove("gift", "engine.talk")"#,
        )],
    );
    let mut s = Session::new(vec![module.clone()], 1).unwrap();
    s.start_recording(BuildStamp::current());
    let mut v = View::detached(s.modules(), 1);
    click(&mut s, &mut v, "speaker.offer.host");
    let decline = input(&mut v, &s, "choice.offer.not_now");
    let advance = Input {
        name: TALK_NEXT.into(),
        payload: json!({"talk": "offer.gift", "line": "offer.ask"})
            .as_object()
            .unwrap()
            .clone(),
        ..decline.clone()
    };
    v.before_step(&s.world);
    s.step(std::slice::from_ref(&decline)).unwrap();
    assert!(s.world.component("offer.gift", TALK).is_none(), "removed");
    assert_eq!(answers(&s)["declined"], true);
    let before = s.world.facts.len();
    v.before_step(&s.world);
    s.step(&[decline.clone(), advance.clone()]).unwrap();
    s.step(&[]).unwrap();
    let chosen = s
        .world
        .facts
        .iter()
        .filter(|f| f.kind == TALK_CHOSEN)
        .count();
    assert_eq!(chosen, 1, "the late answer is no event");
    assert!(
        s.world.facts[before..]
            .iter()
            .all(|f| f.kind != SCRIPT_FAILED),
        "no handler failed"
    );
    // The host falls back to its repeat talk.
    click(&mut s, &mut v, "speaker.offer.host");
    assert_eq!(talk(&s, "offer.chat")["active"], true);

    let rec = s.recording().unwrap();
    let mut replay = Replay::new(&rec).unwrap();
    while replay.session.world.tick < s.world.tick {
        assert!(replay.step().unwrap());
    }
    assert_eq!(s.world.hash(), replay.session.world.hash());
}

#[test]
fn answers_are_authored_checked_and_laid_out() {
    let mut s = Session::new(vec![fixture("offer/demo")], 1).unwrap();
    let r = s.registry().clone();
    let mut try_batch = |commands: Vec<Command>| {
        let before = s.world.hash();
        let e = s
            .world
            .apply(
                &Batch {
                    cause: Cause::Engine {
                        service: "test".into(),
                    },
                    commands,
                },
                &r,
            )
            .unwrap_err();
        assert_eq!(s.world.hash(), before, "rolled back");
        e
    };
    let patch = |id: &str, component: &str, fields: serde_json::Value| Command::Patch {
        entity: id.into(),
        component: component.into(),
        fields: fields.as_object().unwrap().clone(),
    };
    let put = |id: &str, fields: serde_json::Value| Command::Put {
        entity: id.into(),
        component: TALK_CHOICE.into(),
        fields: fields.as_object().unwrap().clone(),
    };
    assert!(
        try_batch(vec![patch(
            "offer.ask",
            TALK_LINE,
            json!({"next": "offer.taken"})
        )])
        .contains("has choices")
    );
    assert!(
        try_batch(vec![patch(
            "offer.take",
            TALK_CHOICE,
            json!({"next": "offer.ask"})
        )])
        .contains("cycle")
    );
    assert!(
        try_batch(vec![put(
            "offer.stray",
            json!({"line": "offer.nowhere", "text": "Hm"})
        )])
        .contains("missing talk line")
    );
    assert!(
        try_batch(vec![put(
            "offer.blank",
            json!({"line": "offer.ask", "text": " "})
        )])
        .contains("empty text")
    );
    let mut v = View::detached(s.modules(), 1);
    let found = v.check_talk(&s.world, size());
    assert!(found.is_empty(), "{found:#?}");
    // A bubble too narrow for its answers fails the check's layout.
    s.world
        .apply(
            &Batch {
                cause: Cause::Engine {
                    service: "test".into(),
                },
                commands: vec![patch("offer.gift", TALK, json!({"width": 120.0}))],
            },
            &r,
        )
        .unwrap();
    let found = v.check_talk(&s.world, size());
    assert!(
        found
            .iter()
            .any(|f| f.contains("line `offer.ask`") && f.contains("choices/")),
        "{found:#?}"
    );
}

#[test]
fn check_presses_buttons_to_reach_panels_over_name_signs() {
    let mut s = Session::new(vec![fixture("sign-panel/demo")], 1).unwrap();
    s.step(&[]).unwrap();
    let before = s.world.hash();
    let mut v = View::detached(s.modules(), 1);
    // The first screen is clear: only the press opens the list over the sign.
    v.scene(&s.world, 1.0, size(), 0.0);
    assert!(v.screens.problems.is_empty(), "{:#?}", v.screens.problems);
    let (found, states) = v.check_presses(&mut s, "signpanel", size());
    assert!(
        found.iter().any(|f| f.contains("after pressing `toggle`")
            && f.contains("signpanel.host/sign: overlaps")),
        "{found:#?}"
    );
    assert_eq!(s.world.hash(), before, "check leaves the world as it was");
    assert_eq!(states[0]["states"], 2);

    let mut s = Session::new(vec![fixture("offer/demo")], 1).unwrap();
    s.step(&[]).unwrap();
    let mut v = View::detached(s.modules(), 1);
    let (found, states) = v.check_presses(&mut s, "offer", size());
    assert!(found.is_empty(), "{found:#?}");
    assert_eq!(
        states[0]["states"], 2,
        "first screen and answers shown; hiding them again is the first state"
    );
}

#[test]
fn check_presses_again_after_a_press_that_only_changes_script_state() {
    // The first press moves a page counter and the screen looks the same; the second
    // opens the list over the name sign.
    let (_tmp, module) = variant(
        "sign-panel/demo",
        &[
            (
                "manifest.toml",
                "open = \"bool\"",
                "open = \"bool\"
page = \"int\"",
            ),
            (
                "scripts/main.luau",
                r#"ctx.patch("state", "ui_state", { open = not ctx.get("state", "ui_state").open })"#,
                r#"local page = ctx.get("state", "ui_state").page + 1
            ctx.patch("state", "ui_state", { page = page, open = page >= 2 })"#,
            ),
        ],
    );
    let mut s = Session::new(vec![module], 1).unwrap();
    s.step(&[]).unwrap();
    let before = s.world.hash();
    let mut v = View::detached(s.modules(), 1);
    let (found, states) = v.check_presses(&mut s, "signpanel", size());
    assert!(
        found
            .iter()
            .any(|f| f.contains("after pressing `toggle` > `toggle`")
                && f.contains("signpanel.host/sign: overlaps")),
        "{found:#?}"
    );
    assert_eq!(s.world.hash(), before, "check leaves the world as it was");
    assert_eq!(
        states[0]["states"], 3,
        "first, page 1, page 2 with the list"
    );
    assert_eq!(states[0]["truncated"], false);
}
