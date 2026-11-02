use galaxy_api::{Batch, Cause, Command, engine::*};
use galaxy_module::module::Module;
use galaxy_render::glam::Vec2;
use galaxy_sim::{BuildStamp, Input, Replay, Session, life, save};
use galaxy_view::View;
use serde_json::{Value, json};
use std::path::Path;

fn module() -> Module {
    Module::read(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/play/life/demo")).unwrap()
}
fn session() -> Session {
    Session::new(vec![module()], 7).unwrap()
}
fn get(s: &Session, id: &str, c: &str, k: &str) -> Value {
    s.world.component(&format!("life.{id}"), c).unwrap().fields[k].clone()
}
fn input(s: &mut Session, name: &str) {
    s.step(&[Input {
        tick: 0,
        source: "test".into(),
        name: format!("life.{name}"),
        payload: Default::default(),
    }])
    .unwrap();
    assert!(
        !s.world.facts.iter().any(|f| f.kind == SCRIPT_FAILED),
        "{:?}",
        s.log
    );
}
fn ticks(s: &mut Session, n: usize) {
    for _ in 0..n {
        s.step(&[]).unwrap();
    }
}
fn patch(s: &mut Session, id: &str, c: &str, f: Value) -> Result<(), String> {
    s.world.apply(
        &Batch {
            cause: Cause::Engine {
                service: "test".into(),
            },
            commands: vec![Command::Patch {
                entity: format!("life.{id}"),
                component: c.into(),
                fields: f.as_object().unwrap().clone(),
            }],
        },
        &s.registry().clone(),
    )
}
fn size() -> Vec2 {
    Vec2::new(1280.0, 720.0)
}

#[test]
fn day_rises_walks_settles_only_on_script_clock_and_leaving_finishes() {
    let mut s = session();
    ticks(&mut s, 35);
    assert_eq!(get(&s, "clock", CLOCK, "value"), 0);
    assert_eq!(get(&s, "walker", BEING, "state"), "Resting");
    input(&mut s, "advance");
    assert_eq!(get(&s, "walker", LIFE, "phase"), "rise");
    assert_eq!(get(&s, "walker", LOCATION, "spot"), "life.right");
    ticks(&mut s, life::POSE_TICKS as usize);
    assert_eq!(get(&s, "walker", LIFE, "phase"), "walk");
    ticks(&mut s, 20);
    let x = get(&s, "walker", POSITION, "x").as_f64().unwrap();
    assert!(x < 1.0 && x > -2.0);
    input(&mut s, "go");
    assert_eq!(get(&s, "walker", LIFE, "phase"), "");
    assert_eq!(get(&s, "walker", WALK, "active"), false);
    assert_eq!(get(&s, "walker", LOCATION, "spot"), "life.left");
    assert_eq!(get(&s, "walker", BEING, "state"), "Observing");
    assert_eq!(
        s.world
            .facts
            .iter()
            .filter(|f| f.kind == WALK_FINISHED)
            .count(),
        1
    );
    // Both ends of this change are outside the player's hall: no animation is pending.
    input(&mut s, "advance"); // arrives in the hall, hence visible
    ticks(&mut s, 160);
    assert_eq!(get(&s, "walker", LOCATION, "place"), "life.hall");
    input(&mut s, "go"); // return to room
    input(&mut s, "advance"); // arrives into room
    ticks(&mut s, 200);
    assert_eq!(get(&s, "walker", LOCATION, "spot"), "life.right");
    input(&mut s, "go");
    input(&mut s, "advance"); // same room, out of sight
    assert_eq!(get(&s, "walker", LIFE, "phase"), "");
    assert_eq!(get(&s, "walker", BEING, "state"), "Resting");
    assert_eq!(
        s.world
            .facts
            .iter()
            .filter(|f| f.kind == CLOCK_ADVANCED)
            .count(),
        4
    );
}

#[test]
fn switching_presentation_and_clock_skips_finish_the_world() {
    let mut s = session();
    input(&mut s, "advance");
    ticks(&mut s, 25);
    input(&mut s, "presentation");
    assert_eq!(get(&s, "walker", LOCATION, "spot"), "life.left");
    assert_eq!(get(&s, "walker", LIFE, "phase"), "");
    input(&mut s, "advance");
    assert_eq!(get(&s, "walker", LOCATION, "spot"), "life.hall_left");
    input(&mut s, "presentation");
    input(&mut s, "advance");
    ticks(&mut s, 25);
    input(&mut s, "advance");
    ticks(&mut s, 200);
    assert_eq!(get(&s, "walker", BEING, "state"), "Resting");
    assert_eq!(get(&s, "walker", LOCATION, "spot"), "life.right");
}

#[test]
fn another_clock_click_retargets_without_snapping_to_the_old_endpoint() {
    let mut s = session();
    input(&mut s, "advance");
    ticks(&mut s, 30);
    let x = get(&s, "walker", POSITION, "x").as_f64().unwrap();
    input(&mut s, "advance");
    let next = get(&s, "walker", POSITION, "x").as_f64().unwrap();
    assert!(
        (next - x).abs() < 0.04,
        "retarget jumped to the old endpoint"
    );
    assert_eq!(get(&s, "walker", LIFE, "current"), "life.visit");
    assert_eq!(get(&s, "walker", WALK, "active"), false);
    ticks(&mut s, 200);
    assert_eq!(get(&s, "walker", LOCATION, "spot"), "life.hall_left");
}

#[test]
fn departure_fade_keeps_the_mid_walk_pose_while_the_world_finishes() {
    let mut s = session();
    input(&mut s, "advance");
    ticks(&mut s, 30);
    let v = View::detached(s.modules(), 7);
    let before = v.pose(&s.world, "life.walker", 1.0).unwrap();
    input(&mut s, "go");
    assert_eq!(get(&s, "walker", LOCATION, "spot"), "life.left");
    let fade = v.pose(&s.world, "life.walker", 1.0).unwrap();
    assert_eq!(
        fade.x, before.x,
        "leaving keeps the displayed boundary pose"
    );
    ticks(&mut s, 3);
    assert_eq!(fade, v.pose(&s.world, "life.walker", 1.0).unwrap());
    let fresh = View::detached(s.modules(), 7);
    assert_eq!(fade, fresh.pose(&s.world, "life.walker", 1.0).unwrap());
}

#[test]
fn direct_clock_writes_record_each_increase_and_rollback_the_facts() {
    let mut s = session();
    patch(&mut s, "clock", CLOCK, json!({"value":2})).unwrap();
    let fact = s.world.facts.last().unwrap();
    assert_eq!(fact.kind, CLOCK_ADVANCED);
    assert_eq!(fact.payload["amount"], 2);
    let before = s.world.clone();
    let result = s.world.apply(
        &Batch {
            cause: Cause::Engine {
                service: "test".into(),
            },
            commands: vec![
                Command::AdvanceClock {
                    entity: "life.clock".into(),
                    amount: 2,
                },
                Command::Patch {
                    entity: "life.rest".into(),
                    component: ACT.into(),
                    fields: json!({"duration":0}).as_object().unwrap().clone(),
                },
            ],
        },
        &s.registry().clone(),
    );
    assert!(result.is_err());
    assert_eq!(s.world, before);
}

#[test]
fn saved_and_replayed_mid_walk_rebuild_the_morph_bubble_sign_and_hash() {
    let mut s = session();
    s.start_recording(BuildStamp::current());
    s.step(&[Input {
        tick: 0,
        source: "talk".into(),
        name: TALK_START.into(),
        payload: json!({"talk":"life.speech"}).as_object().unwrap().clone(),
    }])
    .unwrap();
    input(&mut s, "advance");
    let mut v = View::detached(s.modules(), 7);
    let mut last: Option<Vec2> = None;
    for _ in 0..40 {
        v.before_step(&s.world);
        s.step(&[]).unwrap();
        for alpha in [0.0, 0.5, 1.0] {
            v.scene(&s.world, alpha, size(), 0.0);
            let b = v
                .screens
                .boxes
                .iter()
                .find(|b| b.widget.id.as_deref() == Some("talk.life.speech"))
                .unwrap();
            let at = Vec2::new(b.rect.x, b.rect.y);
            if let Some(old) = last {
                assert!((at - old).length() < 5.0, "bubble jumped");
            }
            last = Some(at);
        }
    }
    let rec = s.recording().unwrap();
    let mut replay = Replay::new(&rec).unwrap();
    let mut rebuilt = View::detached(replay.session.modules(), 7);
    while replay.session.world.tick < s.world.tick {
        rebuilt.before_step(&replay.session.world);
        replay.step().unwrap();
    }
    assert_eq!(s.world.hash(), replay.session.world.hash());
    assert_eq!(
        v.scene(&s.world, 0.37, size(), 1.0).overlay,
        rebuilt
            .scene(&replay.session.world, 0.37, size(), 1.0)
            .overlay
    );
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("life.db");
    save::write(&file, &s.world, s.modules(), &BuildStamp::current()).unwrap();
    let (_, w) = save::read(&file).unwrap();
    let mut loaded = Session::resume(vec![module()], w).unwrap();
    ticks(&mut loaded, 150);
    ticks(&mut s, 150);
    assert_eq!(s.world.hash(), loaded.world.hash());
    assert_eq!(get(&s, "walker", BEING, "state"), "Observing");
}

#[test]
fn invalid_days_roll_back_instead_of_failing_during_play() {
    let mut s = session();
    let before = s.world.hash();
    for (id, c, f) in [
        ("rest", ACT, json!({"duration":0})),
        ("return", ACT, json!({"next":"life.rest"})),
        ("rest", ACT, json!({"spot":"life.hall_left"})),
        ("walker", LIFE, json!({"speed":0})),
        ("room", PLACE, json!({"arrival":null})),
    ] {
        assert!(patch(&mut s, id, c, f).is_err());
        assert_eq!(s.world.hash(), before);
    }
}

#[test]
fn talk_check_covers_resting_morphs_and_the_whole_day_path() {
    let mut s = session();
    let mut v = View::detached(s.modules(), 7);
    assert!(v.check_life(&s.world, s.registry(), size()).is_empty());
    // This opaque figure sits between the two clear endpoints, nearer the camera.
    s.world
        .apply(
            &Batch {
                cause: Cause::Engine {
                    service: "test".into(),
                },
                commands: vec![
                    Command::Put {
                        entity: "life.blocker".into(),
                        component: MODEL.into(),
                        fields: json!({"model":"life.being"}).as_object().unwrap().clone(),
                    },
                    Command::Put {
                        entity: "life.blocker".into(),
                        component: POSITION.into(),
                        fields: json!({"x":-0.5,"y":-0.4}).as_object().unwrap().clone(),
                    },
                    Command::Put {
                        entity: "life.blocker".into(),
                        component: LOCATION.into(),
                        fields: json!({"place":"life.room"}).as_object().unwrap().clone(),
                    },
                ],
            },
            &s.registry().clone(),
        )
        .unwrap();
    assert!(v.check_talk(&s.world, size()).is_empty());
    let found = v.check_life(&s.world, s.registry(), size());
    assert!(
        found
            .iter()
            .any(|f| f.contains("life.blocker") && f.contains("covered by geometry")),
        "{found:#?}"
    );
}
