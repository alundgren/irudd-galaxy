//! The place model through module scripts, scenarios, saves and replay, without a view.
use std::path::{Path, PathBuf};

use galaxy_api::engine::{CLOCK, LOCATION, PLACE, POSITION, SCRIPT_FAILED, WALK};
use galaxy_api::{Batch, Cause, Command};
use galaxy_module::module::Module;
use galaxy_sim::scenario::{InputsFile, run_module_tests};
use galaxy_sim::{BuildStamp, Session, replay, save};
use serde_json::{Value, json};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/modules/places")
}
fn module() -> Module {
    Module::read(&root().join("demo")).unwrap()
}
fn run(ticks: u64) -> Session {
    let inputs = InputsFile::read(&root().join("inputs.toml"))
        .unwrap()
        .by_tick();
    let mut s = Session::new(vec![module()], 7).unwrap();
    s.start_recording(BuildStamp::current());
    while s.world.tick < ticks {
        s.step(inputs.get(&(s.world.tick + 1)).map_or(&[], Vec::as_slice))
            .unwrap();
    }
    assert!(
        !s.world.facts.iter().any(|f| f.kind == SCRIPT_FAILED),
        "{:?}",
        s.log
    );
    s
}
fn patch(entity: &str, component: &str, value: Value) -> Command {
    Command::Patch {
        entity: format!("places.{entity}"),
        component: component.into(),
        fields: value.as_object().unwrap().clone(),
    }
}
fn apply(s: &mut Session, commands: Vec<Command>) -> Result<(), String> {
    let reg = s.registry().clone();
    s.world.apply(
        &Batch {
            cause: Cause::Script {
                module: "places".into(),
                script: "test".into(),
                handler: "test".into(),
                fact: None,
            },
            commands,
        },
        &reg,
    )
}
fn get(s: &Session, entity: &str, component: &str, field: &str) -> Value {
    s.world
        .component(&format!("places.{entity}"), component)
        .unwrap()
        .fields[field]
        .clone()
}

#[test]
fn scenarios_show_moves_walks_the_clock_and_separate_rooms() {
    let reports = run_module_tests(&[module()], "places").unwrap();
    assert_eq!(reports.len(), 1);
    assert!(reports[0].passed, "{:?}", reports[0].failures);
}

#[test]
fn a_saved_walk_and_recording_resume_without_any_presentation() {
    let mut middle = run(33);
    assert_eq!(get(&middle, "visitor", WALK, "active"), true);
    assert_eq!(get(&middle, "visitor", POSITION, "x"), 1.0);
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("walk.db");
    save::write(
        &path,
        &middle.world,
        middle.modules(),
        &BuildStamp::current(),
    )
    .unwrap();
    let (info, world) = save::read(&path).unwrap();
    assert_eq!(info.format, 1);
    assert_eq!(world, middle.world);
    let mut resumed = Session::resume(vec![module()], world).unwrap();
    resumed.start_recording(BuildStamp::current());
    for _ in 33..95 {
        resumed.step(&[]).unwrap();
    }
    let end = run(95);
    assert_eq!(resumed.world, end.world);
    for recording in [end.recording().unwrap(), resumed.recording().unwrap()] {
        let (played, report) = replay(&recording).unwrap();
        assert!(report.identical);
        assert_eq!(played.world, end.world);
    }
    // A script hot-load does not restart a walk or move the independent clock.
    middle.hot_load(module()).unwrap();
    for _ in 33..95 {
        middle.step(&[]).unwrap();
    }
    assert_eq!(get(&middle, "visitor", POSITION, "x"), 3.0);
    assert_eq!(get(&middle, "clock", CLOCK, "value"), 5);
}

#[test]
fn retargeting_starts_at_the_current_position_and_moves_cancel_walks() {
    let mut s = run(33);
    apply(
        &mut s,
        vec![Command::Walk {
            entity: "places.visitor".into(),
            spot: "places.b_door".into(),
            speed: 1.0,
            state: None,
        }],
    )
    .unwrap();
    assert_eq!(get(&s, "visitor", WALK, "from_x"), 1.0);
    for _ in 0..30 {
        s.step(&[]).unwrap();
    }
    assert_eq!(get(&s, "visitor", POSITION, "x"), 0.0);
    assert_eq!(get(&s, "visitor", WALK, "active"), false);
    apply(
        &mut s,
        vec![
            Command::Walk {
                entity: "places.visitor".into(),
                spot: "places.b_table".into(),
                speed: 1.0,
                state: None,
            },
            Command::Move {
                entity: "places.visitor".into(),
                place: "places.room_a".into(),
                spot: None,
                state: None,
            },
        ],
    )
    .unwrap();
    for _ in 0..100 {
        s.step(&[]).unwrap();
    }
    assert_eq!(get(&s, "visitor", LOCATION, "place"), "places.room_a");
    assert_eq!(get(&s, "visitor", WALK, "active"), false);
    assert_eq!(get(&s, "visitor", POSITION, "x"), 0.0);
}

#[test]
fn zero_distance_walk_finishes_once_and_clock_never_ticks_itself() {
    let mut s = run(2);
    apply(
        &mut s,
        vec![Command::Walk {
            entity: "places.visitor".into(),
            spot: "places.b_door".into(),
            speed: 1.0,
            state: None,
        }],
    )
    .unwrap();
    for _ in 0..60 {
        s.step(&[]).unwrap();
    }
    assert_eq!(get(&s, "visitor", WALK, "active"), false);
    assert_eq!(
        s.world
            .facts
            .iter()
            .filter(|f| f.kind == "engine.walk_finished")
            .count(),
        1
    );
    assert_eq!(get(&s, "clock", CLOCK, "value"), 5);
}

#[test]
fn authored_cycles_wrong_spots_and_dangling_references_are_atomic_errors() {
    let mut s = run(33);
    let before = s.world.clone();
    let bad = [
        patch("station", PLACE, json!({"parent": "places.room_a"})),
        patch("room_a", PLACE, json!({"enter": "places.block_a"})),
        patch("room_a", PLACE, json!({"arrival": "places.b_door"})),
        patch("room_a", PLACE, json!({"width": -1})),
        patch("b_table", "engine.spot", json!({"name": "door"})),
        patch("visitor", LOCATION, json!({"place": "places.room_a"})),
        Command::Remove {
            entity: "places.room_b".into(),
            component: None,
        },
        Command::Move {
            entity: "places.player".into(),
            place: "places.room_a".into(),
            spot: Some("places.b_door".into()),
            state: None,
        },
        Command::Walk {
            entity: "places.player".into(),
            spot: "places.b_table".into(),
            speed: 1.0,
            state: None,
        },
        Command::Walk {
            entity: "places.visitor".into(),
            spot: "places.b_table".into(),
            speed: 0.0,
            state: None,
        },
        Command::AdvanceClock {
            entity: "places.clock".into(),
            amount: -1,
        },
        Command::AdvanceClock {
            entity: "places.clock".into(),
            amount: i64::MAX,
        },
    ];
    for command in bad {
        assert!(
            apply(
                &mut s,
                vec![patch("clock", CLOCK, json!({"value": 6})), command.clone()]
            )
            .is_err(),
            "{command:?}"
        );
        assert_eq!(s.world, before, "rollback after {command:?}");
    }
}

#[test]
fn bad_start_data_and_place_changes_on_resume_are_refused() {
    let mut m = module();
    m.start
        .get_mut("places.station")
        .unwrap()
        .get_mut(PLACE)
        .unwrap()
        .insert("parent".into(), json!("places.room_a"));
    assert!(Session::new(vec![m], 1).err().unwrap().contains("cycle"));
    let mut w = run(33).world;
    w.entities.remove("places.room_b");
    assert!(Session::resume(vec![module()], w).is_err());
}

#[test]
fn high_level_commands_keep_component_capabilities_and_call_order() {
    let mut s = run(2);
    let before = s.world.clone();
    let mut reg = s.registry().clone();
    reg.writes.get_mut("places").unwrap().remove(POSITION);
    let batch = Batch {
        cause: Cause::Start {
            module: "places".into(),
        },
        commands: vec![Command::Move {
            entity: "places.player".into(),
            place: "places.room_b".into(),
            spot: None,
            state: None,
        }],
    };
    assert!(
        s.world
            .apply(&batch, &reg)
            .unwrap_err()
            .contains("engine.position")
    );
    assert_eq!(s.world, before);
    apply(
        &mut s,
        vec![
            Command::Move {
                entity: "places.visitor".into(),
                place: "places.room_a".into(),
                spot: None,
                state: None,
            },
            Command::Walk {
                entity: "places.visitor".into(),
                spot: "places.a_table".into(),
                speed: 1.0,
                state: None,
            },
        ],
    )
    .unwrap();
    assert_eq!(get(&s, "visitor", WALK, "place"), "places.room_a");
}

#[test]
fn same_place_bodies_still_collide() {
    let mut s = run(1);
    apply(
        &mut s,
        vec![patch(
            "probe_b",
            LOCATION,
            json!({"place": "places.room_a"}),
        )],
    )
    .unwrap();
    s.step(&[]).unwrap();
    let a = get(&s, "probe_a", POSITION, "x").as_f64().unwrap();
    let b = get(&s, "probe_b", POSITION, "x").as_f64().unwrap();
    assert!((a - b).abs() >= 2.0);
}
