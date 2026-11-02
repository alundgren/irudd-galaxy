use galaxy_api::{
    command::{Batch, Cause, Command},
    engine::*,
};
use galaxy_module::module::Module;
use galaxy_render::glam::Vec2;
use galaxy_sim::scenario::{Click, ClickTarget};
use galaxy_sim::{BuildStamp, Input, Replay, Session, save, travel};
use galaxy_view::View;
use serde_json::json;
use std::path::Path;
fn module() -> Module {
    Module::read(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/play/talk/demo")).unwrap()
}
fn size() -> Vec2 {
    Vec2::new(1280.0, 720.0)
}
fn click(s: &mut Session, v: &mut View, name: &str) {
    v.scene(&s.world, 1.0, size(), 0.0);
    let input = travel::arrival(&s.world, true).unwrap_or_else(|| {
        v.bot_talk_click(
            &s.world,
            &Click {
                tick: s.world.tick + 1,
                target: ClickTarget::Widget(name.into()),
            },
        )
        .unwrap()
    });
    v.before_step(&s.world);
    s.step(&[input]).unwrap();
}
fn patch(
    s: &mut Session,
    id: &str,
    component: &str,
    fields: serde_json::Value,
) -> Result<(), String> {
    s.world.apply(
        &Batch {
            cause: Cause::Engine {
                service: "test".into(),
            },
            commands: vec![Command::Patch {
                entity: id.into(),
                component: component.into(),
                fields: fields.as_object().unwrap().clone(),
            }],
        },
        &s.registry().clone(),
    )
}
#[test]
fn stories_wait_repeat_and_reconstruct_mid_line_after_room_changes() {
    let mut s = Session::new(vec![module()], 7).unwrap();
    s.start_recording(BuildStamp::current());
    let mut v = View::detached(s.modules(), 7);
    click(&mut s, &mut v, "speaker.talk.marker");
    assert_eq!(
        s.world.component("talk.marker_talk", TALK).unwrap().fields["active"],
        true
    );
    click(&mut s, &mut v, "talk.talk.marker_talk");
    click(&mut s, &mut v, "enter_a");
    click(&mut s, &mut v, "land");
    click(&mut s, &mut v, "speaker.talk.first");
    click(&mut s, &mut v, "talk.talk.welcome");
    assert_eq!(
        s.world.component("talk.welcome", TALK).unwrap().fields["current"],
        "talk.welcome_2"
    );
    let drawn = v.scene(&s.world, 0.4, size(), 3.0);
    let rec = s.recording().unwrap();
    let mut replay = Replay::new(&rec).unwrap();
    let mut rebuilt = View::detached(replay.session.modules(), 7);
    while replay.session.world.tick < s.world.tick {
        rebuilt.before_step(&replay.session.world);
        assert!(replay.step().unwrap());
    }
    assert_eq!(s.world.hash(), replay.session.world.hash());
    assert_eq!(
        drawn.overlay,
        rebuilt
            .scene(&replay.session.world, 0.4, size(), 3.0)
            .overlay
    );
    let tmp = tempfile::tempdir().unwrap();
    let file = tmp.path().join("talk.db");
    save::write(&file, &s.world, s.modules(), &BuildStamp::current()).unwrap();
    let (_, w) = save::read(&file).unwrap();
    assert_eq!(
        Session::resume(vec![module()], w).unwrap().world.hash(),
        s.world.hash()
    );
    click(&mut s, &mut v, "leave");
    assert_eq!(
        s.world.component("talk.welcome", TALK).unwrap().fields["seen"],
        false
    );
    assert_eq!(
        s.world.component("talk.welcome", TALK).unwrap().fields["active"],
        false
    );
    click(&mut s, &mut v, "land");
    click(&mut s, &mut v, "enter_a");
    click(&mut s, &mut v, "land");
    click(&mut s, &mut v, "speaker.talk.first");
    assert_eq!(
        s.world.component("talk.welcome", TALK).unwrap().fields["current"],
        "talk.welcome_1"
    );
    click(&mut s, &mut v, "talk.talk.welcome");
    click(&mut s, &mut v, "talk.talk.welcome");
    assert_eq!(
        s.world.component("talk.welcome", TALK).unwrap().fields["seen"],
        true
    );
    click(&mut s, &mut v, "speaker.talk.first");
    assert_eq!(
        s.world.component("talk.repeat", TALK).unwrap().fields["active"],
        true
    );
    click(&mut s, &mut v, "talk.talk.repeat");
    click(&mut s, &mut v, "speaker.talk.first");
    assert_eq!(
        s.world.component("talk.repeat", TALK).unwrap().fields["active"],
        true
    );
    let rec = s.recording().unwrap();
    let mut replay = Replay::new(&rec).unwrap();
    while replay.step().unwrap() {}
    assert_eq!(s.world.hash(), replay.session.world.hash());
    assert!(
        !s.world.facts.iter().any(|f| f.kind == SCRIPT_FAILED),
        "{:?}",
        s.log
    );
}
#[test]
fn checks_all_places_and_lines_against_geometry_and_screens() {
    let mut s = Session::new(vec![module()], 1).unwrap();
    let mut v = View::detached(s.modules(), 1);
    let findings = v.check_talk(&s.world, size());
    assert!(findings.is_empty(), "{findings:#?}");
    patch(&mut s, "talk.third", MODEL, json!({"model":"talk.covered"})).unwrap();
    let findings = v.check_talk(&s.world, size());
    assert!(
        findings.iter().any(|f| f.contains("talk.room_b")
            && f.contains("focus `face`")
            && f.contains("covered by geometry")),
        "{findings:#?}"
    );
    patch(&mut s, "talk.third", MODEL, json!({"model":"talk.being"})).unwrap();
    patch(&mut s, "talk.welcome", TALK, json!({"y":120})).unwrap();
    let findings = v.check_talk(&s.world, size());
    assert!(
        findings
            .iter()
            .any(|f| f.contains("focus `face`") && f.contains("covered by talk.welcome/talk")),
        "{findings:#?}"
    );
    patch(&mut s, "talk.welcome", TALK, json!({"y":-12})).unwrap();
    patch(&mut s, "talk.third", POSITION, json!({"x":1000})).unwrap();
    let findings = v.check_talk(&s.world, size());
    assert!(
        findings
            .iter()
            .any(|f| f.contains("talk.third") && f.contains("off screen")),
        "{findings:#?}"
    );
}
#[test]
fn focus_can_name_another_part_or_explicitly_have_none() {
    let mut m = module();
    let text = m.files.0.get_mut("models/being.gltf").unwrap();
    *text = text.replace("\"name\":\"face\"", "\"name\":\"front\"");
    let mut s = Session::new(vec![m], 1).unwrap();
    let mut v = View::detached(s.modules(), 1);
    assert!(
        v.check_talk(&s.world, size())
            .iter()
            .any(|f| f.contains("no focus mesh part `face`"))
    );
    for (id, part) in [
        ("talk.first", "front"),
        ("talk.second", "front"),
        ("talk.third", ""),
    ] {
        let r = s.registry().clone();
        s.world
            .apply(
                &Batch {
                    cause: Cause::Engine {
                        service: "test".into(),
                    },
                    commands: vec![Command::Put {
                        entity: id.into(),
                        component: FOCUS.into(),
                        fields: json!({"part":part}).as_object().unwrap().clone(),
                    }],
                },
                &r,
            )
            .unwrap();
    }
    let findings = v.check_talk(&s.world, size());
    assert!(findings.is_empty(), "{findings:#?}");
}
#[test]
fn authoring_errors_roll_back_and_stale_advances_do_nothing() {
    let mut s = Session::new(vec![module()], 1).unwrap();
    let before = s.world.hash();
    assert!(
        patch(
            &mut s,
            "talk.welcome_2",
            TALK_LINE,
            json!({"next":"talk.welcome_1"})
        )
        .unwrap_err()
        .contains("cycle")
    );
    assert_eq!(s.world.hash(), before);
    let i = Input {
        tick: 1,
        source: "test".into(),
        name: TALK_NEXT.into(),
        payload: json!({"talk":"talk.welcome","line":"talk.welcome_1"})
            .as_object()
            .unwrap()
            .clone(),
    };
    s.step(&[i]).unwrap();
    assert_eq!(
        s.world.component("talk.welcome", TALK).unwrap().fields["active"],
        false
    );
}

#[test]
fn moving_speakers_keep_their_bubble_on_the_interpolated_anchor() {
    let mut s = Session::new(vec![module()], 7).unwrap();
    let mut v = View::detached(s.modules(), 7);
    click(&mut s, &mut v, "enter_a");
    click(&mut s, &mut v, "land");
    click(&mut s, &mut v, "speaker.talk.second");
    click(&mut s, &mut v, "move");
    let mut last: Option<Vec2> = None;
    for _ in 0..42 {
        v.before_step(&s.world);
        s.step(&[]).unwrap();
        for alpha in [0.0, 0.5, 1.0] {
            let scene = v.scene(&s.world, alpha, size(), 0.0);
            let mut anchors = std::collections::BTreeMap::new();
            v.place_anchors(&s.world, alpha, size(), scene.camera, &mut anchors);
            let b = v
                .screens
                .boxes
                .iter()
                .find(|b| b.widget.kind == "bubble")
                .unwrap();
            let a = anchors["being.talk.second"];
            assert!((b.rect.center().x - a.x).abs() < 0.001);
            assert!((b.rect.y + b.rect.h + 12.0 - a.y).abs() < 0.001);
            if let Some(previous) = last {
                assert!(
                    (a - previous).length() < 1.2,
                    "bubble must not jump between frames"
                );
            }
            last = Some(a);
        }
    }
    assert!(!s.world.facts.iter().any(|f| f.kind == SCRIPT_FAILED));
}

#[test]
fn check_finds_a_walk_that_moves_focus_under_a_room_panel() {
    let mut s = Session::new(vec![module()], 1).unwrap();
    let r = s.registry().clone();
    // Starts in clear space; its destination is beyond the panel and camera edge.
    patch(
        &mut s,
        "talk.room_a_middle",
        SPOT,
        json!({"x":12.0,"y":1.0}),
    )
    .unwrap();
    s.world
        .apply(
            &Batch {
                cause: Cause::Engine {
                    service: "test".into(),
                },
                commands: vec![Command::Walk {
                    entity: "talk.second".into(),
                    spot: "talk.room_a_middle".into(),
                    speed: 20.0,
                    state: None,
                }],
            },
            &r,
        )
        .unwrap();
    let mut v = View::detached(s.modules(), 1);
    let found = v.check_talk(&s.world, size());
    assert!(
        found
            .iter()
            .any(|f| f.contains("talk.second")
                && (f.contains("off screen") || f.contains("covered by"))),
        "{found:#?}"
    );
}

#[test]
fn double_click_on_one_drawn_line_advances_only_once() {
    let mut s = Session::new(vec![module()], 1).unwrap();
    s.start_recording(BuildStamp::current());
    let mut v = View::detached(s.modules(), 1);
    click(&mut s, &mut v, "enter_a");
    click(&mut s, &mut v, "land");
    click(&mut s, &mut v, "speaker.talk.first");
    v.scene(&s.world, 1.0, size(), 0.0);
    let next = v
        .screens
        .bot_click(&Click {
            tick: s.world.tick + 1,
            target: ClickTarget::Widget("talk.talk.welcome".into()),
        })
        .unwrap();
    s.step(&[next.clone(), next]).unwrap();
    let f = &s.world.component("talk.welcome", TALK).unwrap().fields;
    assert_eq!(f["current"], "talk.welcome_2");
    assert_eq!(f["seen"], false);
    let rec = s.recording().unwrap();
    let mut replay = Replay::new(&rec).unwrap();
    while replay.step().unwrap() {}
    assert_eq!(s.world.hash(), replay.session.world.hash());
}
