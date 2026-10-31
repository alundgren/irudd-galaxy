//! The sim on the generic test modules in `engine/tests/modules`.

use std::path::{Path, PathBuf};

use galaxy_api::engine::{HOT_LOADED, SCRIPT_FAILED};
use galaxy_module::module::{Module, ModuleFiles};
use galaxy_sim::scenario::{InputsFile, run_module_tests};
use galaxy_sim::{BuildStamp, Recording, Session, World, replay, save};
use serde_json::{Value, json};

fn tests_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests")
}

fn modules() -> Vec<Module> {
    ["stroll", "tally"]
        .iter()
        .map(|id| Module::read(&tests_dir().join("modules").join(id)).unwrap())
        .collect()
}

fn field(world: &World, entity: &str, component: &str, name: &str) -> Value {
    world.component(entity, component).unwrap().fields[name].clone()
}

/// Runs the scripted session in `tests/sessions/stroll.toml` to `ticks`, recording.
fn scripted(ticks: u64) -> Session {
    let script = InputsFile::read(&tests_dir().join("sessions/stroll.toml")).unwrap();
    let inputs = script.by_tick();
    let mut session = Session::new(modules(), 7).unwrap();
    session.start_recording(BuildStamp::current());
    while session.world.tick < ticks {
        let tick = session.world.tick + 1;
        for (at, dir) in &script.hot_loads {
            if *at == tick {
                session.hot_load(Module::read(dir).unwrap()).unwrap();
            }
        }
        session
            .step(inputs.get(&tick).map_or(&[][..], Vec::as_slice))
            .unwrap();
        for (at, text) in &script.notes {
            if *at == session.world.tick {
                session.note(text);
            }
        }
    }
    session
}

#[test]
fn the_scenario_tests_pass() {
    let reports = run_module_tests(&modules(), "stroll").unwrap();
    assert_eq!(reports.len(), 1);
    assert!(reports[0].passed, "{:?}", reports[0].failures);
}

#[test]
fn walkers_climb_onto_the_deck_and_every_change_has_a_cause() {
    let session = scripted(330);
    let world = &session.world;
    assert_eq!(field(world, "stroll.w1", "engine.position", "layer"), 1);
    assert_eq!(field(world, "stroll.w1", "engine.position", "height"), 2.0);
    assert_eq!(field(world, "stroll.w2", "engine.position", "layer"), 0);
    // w2 turned twice: back to walking forwards.
    assert_eq!(field(world, "stroll.w2", "engine.motion", "vx"), 1.5);
    assert_eq!(field(world, "stroll.w1", "stroll.walker", "steps"), 11);
    assert_eq!(field(world, "tally.board", "tally.count", "climbs"), 1);
    assert_eq!(field(world, "tally.board", "tally.count", "turns"), 2);
    assert_eq!(
        field(world, "tally.board", "tally.count", "last"),
        "stroll.w1"
    );

    let kinds: Vec<&str> = world.facts.iter().map(|f| f.kind.as_str()).collect();
    assert_eq!(kinds.iter().filter(|k| **k == "stroll.climbed").count(), 1);
    assert!(kinds.contains(&"stroll.started") && kinds.contains(&"stroll.paced"));
    assert!(!kinds.contains(&SCRIPT_FAILED), "{kinds:?}");

    // Every component and fact points at a cause that exists.
    for components in world.entities.values() {
        for c in components.values() {
            assert!((c.cause as usize) < world.causes.len());
        }
    }
    for f in &world.facts {
        assert!((f.cause as usize) < world.causes.len());
    }
    // The deck step was the stroll script's, on the cadence.
    let cause = &world.causes[world
        .component("stroll.w1", "engine.position")
        .unwrap()
        .cause as usize];
    let cause = serde_json::to_value(&cause.cause).unwrap();
    assert!(
        cause["kind"] == "script" || cause["service"] == "ground" || cause["service"] == "motion",
        "{cause}"
    );
    let climbed = world
        .facts
        .iter()
        .find(|f| f.kind == "stroll.climbed")
        .unwrap();
    let cause = serde_json::to_value(&world.causes[climbed.cause as usize].cause).unwrap();
    assert_eq!(
        cause,
        json!({ "kind": "script", "module": "stroll", "script": "scripts/stroll.luau", "handler": "cadence" })
    );
}

#[test]
fn a_hot_load_changes_the_running_scripts_and_schemas() {
    let before = scripted(359);
    let steps = |s: &Session| {
        field(&s.world, "stroll.w1", "stroll.walker", "steps")
            .as_i64()
            .unwrap()
    };
    assert_eq!(steps(&before), 11);
    let after = scripted(390);
    // The changed rules count ten steps a second from tick 360 on (cadence at 360 and 390).
    assert_eq!(steps(&after), 11 + 10 + 10);
    assert_eq!(
        field(&after.world, "stroll.w1", "stroll.walker", "boosts"),
        7
    );
    let loaded: Vec<_> = after
        .world
        .facts
        .iter()
        .filter(|f| f.kind == HOT_LOADED)
        .collect();
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].payload["version"], "0.1.1");

    // A change that would lose data is refused, and nothing changes.
    let mut session = scripted(10);
    let hash = session.world.hash();
    let mut files = modules()[0].files.clone();
    let manifest = files.0["manifest.toml"].replace("climbs = \"int\"\n", "");
    files.0.insert("manifest.toml".into(), manifest);
    let changed = Module::from_files(Path::new("stroll"), files).unwrap();
    let error = session.hot_load(changed).unwrap_err();
    assert!(error.contains("climbs"), "{error}");
    assert_eq!(session.world.hash(), hash);
}

#[test]
fn a_recording_replays_to_the_same_world_twice() {
    let session = scripted(450);
    let rec = session.recording().unwrap();
    assert_eq!(rec.end.hash, session.world.hash());
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("session.json");
    rec.write(&path).unwrap();
    let read = Recording::read(&path).unwrap();
    assert_eq!(read, rec);
    for _ in 0..2 {
        let (replayed, report) = replay(&read).unwrap();
        assert!(report.identical, "{report:?}");
        assert_eq!(replayed.world, session.world);
        assert_eq!(report.notes.len(), 1);
        assert_eq!(report.notes[0].tick, 150);
    }
    // A recording whose end doesn't match is found out.
    let mut wrong = rec.clone();
    wrong.end.hash = "0".repeat(64);
    assert!(!replay(&wrong).unwrap().1.identical);
}

#[test]
fn a_save_loads_back_exactly_and_carries_on_the_same() {
    let straight = scripted(330);
    let half = scripted(200);
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("half.db");
    save::write(&path, &half.world, half.modules(), &BuildStamp::current()).unwrap();
    // Writing again replaces the file.
    save::write(&path, &half.world, half.modules(), &BuildStamp::current()).unwrap();
    let (info, world) = save::read(&path).unwrap();
    assert_eq!(world, half.world);
    let listed: Vec<(&str, &str)> = info
        .modules
        .iter()
        .map(|m| (m.id.as_str(), m.version.as_str()))
        .collect();
    assert_eq!(listed, [("stroll", "0.1.0"), ("tally", "0.1.0")]);

    let script = InputsFile::read(&tests_dir().join("sessions/stroll.toml")).unwrap();
    let inputs = script.by_tick();
    let mut resumed = Session::resume(modules(), world).unwrap();
    while resumed.world.tick < 330 {
        let tick = resumed.world.tick + 1;
        resumed
            .step(inputs.get(&tick).map_or(&[][..], Vec::as_slice))
            .unwrap();
    }
    assert_eq!(resumed.world.hash(), straight.world.hash());
}

#[test]
fn a_damaged_or_foreign_save_is_refused() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("x.db");
    std::fs::write(&path, "not a database").unwrap();
    assert!(save::read(&path).is_err());
    assert!(save::read(&tmp.path().join("none.db")).is_err());
}

#[test]
fn a_module_may_not_write_what_it_was_not_granted() {
    let mut files = ModuleFiles::default();
    files.0.insert(
        "manifest.toml".into(),
        "id = \"rogue\"\nversion = \"0.1.0\"\nmin_engine_version = \"0.2.0\"\n\
         scripts = [\"main.luau\"]\n"
            .into(),
    );
    files.0.insert(
        "main.luau".into(),
        "return { on = { ['engine.start'] = function(event, ctx)
            ctx.patch('stroll.w1', 'stroll.walker', { steps = 99 })
        end } }"
            .into(),
    );
    let rogue = Module::from_files(Path::new("rogue"), files).unwrap();
    let mut list = modules();
    list.push(rogue);
    let mut session = Session::new(list, 1).unwrap();
    session.step(&[]).unwrap();
    let failed = session
        .world
        .facts
        .iter()
        .find(|f| f.kind == SCRIPT_FAILED)
        .expect("the write was refused");
    assert_eq!(failed.payload["module"], "rogue");
    assert_eq!(failed.payload["code"], "bad_command");
    assert_eq!(
        field(&session.world, "stroll.w1", "stroll.walker", "steps"),
        0
    );
}
