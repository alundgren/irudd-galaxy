//! Editing the world from outside its rules (`galaxy_sim::edit`), on the generic modules
//! `tests/edits/purse` and `tests/modules/places/demo`.

use std::path::{Path, PathBuf};

use galaxy_api::command::Cause;
use galaxy_api::engine::EDITED;
use galaxy_module::module::Module;
use galaxy_sim::edit::Edit;
use galaxy_sim::recording::RecEvent;
use galaxy_sim::scenario::{DueEdits, InputsFile, run_module_tests, run_scenario};
use galaxy_sim::{BuildStamp, Input, Recording, Session, World, replay, save};
use serde_json::{Value, json};

fn tests_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests")
}

fn purse() -> Vec<Module> {
    vec![Module::read(&tests_dir().join("edits/purse")).unwrap()]
}

fn places() -> Vec<Module> {
    vec![Module::read(&tests_dir().join("modules/places/demo")).unwrap()]
}

fn edit(line: &str) -> Edit {
    Edit::parse_text(line).unwrap()
}

fn field(world: &World, entity: &str, component: &str, name: &str) -> Value {
    world.component(entity, component).unwrap().fields[name].clone()
}

fn buy(tick: u64) -> Input {
    Input {
        tick,
        source: "script".into(),
        name: "purse.buy".into(),
        payload: Default::default(),
        via: None,
    }
}

#[test]
fn set_changes_only_the_fields_named() {
    let mut s = Session::new(purse(), 1).unwrap();
    s.edit("test", &[edit("set purse.buyer purse.wallet coins=15000")])
        .unwrap();
    assert_eq!(
        field(&s.world, "purse.buyer", "purse.wallet", "coins"),
        15000
    );
    assert_eq!(field(&s.world, "purse.buyer", "purse.wallet", "work"), 0);
    // A whole number into a number field is coerced, as for scripts.
    s.edit("test", &[edit("set purse.buyer engine.position x=7")])
        .unwrap();
    assert_eq!(field(&s.world, "purse.buyer", "engine.position", "x"), 7.0);
}

#[test]
fn put_adds_components_and_entities_and_remove_takes_them_away() {
    let mut s = Session::new(purse(), 1).unwrap();
    s.edit(
        "test",
        &[
            // A second tool, a new entity, with its fields' defaults but the price.
            edit("put purse.tool2 purse.goods price=40"),
            // A component the buyer didn't have.
            edit("put purse.buyer purse.ledger edits=3"),
            edit("remove purse.buyer engine.motion"),
            edit("remove purse.book"),
        ],
    )
    .unwrap();
    let w = &s.world;
    assert_eq!(field(w, "purse.tool2", "purse.goods", "price"), 40);
    assert_eq!(field(w, "purse.tool2", "purse.goods", "work"), 10);
    assert_eq!(field(w, "purse.buyer", "purse.ledger", "edits"), 3);
    assert!(w.component("purse.buyer", "engine.motion").is_none());
    assert!(!w.entities.contains_key("purse.book"));
    // The buyer stops moving: motion was the only thing moving it.
    s.step(&[]).unwrap();
    assert_eq!(field(&s.world, "purse.buyer", "engine.position", "x"), 0.0);
}

#[test]
fn place_teleports_a_located_entity() {
    let mut s = Session::new(places(), 1).unwrap();
    s.edit(
        "test",
        &[edit("place places.visitor places.room_b places.b_table")],
    )
    .unwrap();
    let w = &s.world;
    assert_eq!(
        field(w, "places.visitor", "engine.location", "place"),
        "places.room_b"
    );
    assert_eq!(
        field(w, "places.visitor", "engine.location", "spot"),
        "places.b_table"
    );
    assert_eq!(field(w, "places.visitor", "engine.position", "x"), 3.0);
    // Without a spot: the place's arrival.
    s.edit("test", &[edit("place places.visitor places.room_a")])
        .unwrap();
    assert_eq!(
        field(&s.world, "places.visitor", "engine.location", "spot"),
        "places.a_door"
    );
}

#[test]
fn an_edit_has_its_own_cause_and_scripts_hear_it_on_the_next_tick() {
    let mut s = Session::new(purse(), 1).unwrap();
    s.edit("live", &[edit("set purse.buyer purse.wallet coins=5")])
        .unwrap();
    let c = s.world.component("purse.buyer", "purse.wallet").unwrap();
    assert_eq!(
        s.world.causes[c.cause as usize].cause,
        Cause::Edit {
            source: "live".into()
        }
    );
    let fact = s.world.facts.iter().find(|f| f.kind == EDITED).unwrap();
    assert_eq!(fact.tick, 0);
    assert_eq!(
        Value::Object(fact.payload.clone()),
        json!({"source": "live", "entities": ["purse.buyer"], "components": ["purse.wallet"]})
    );
    assert_eq!(field(&s.world, "purse.book", "purse.ledger", "edits"), 0);
    s.step(&[]).unwrap();
    assert_eq!(field(&s.world, "purse.book", "purse.ledger", "edits"), 1);
    assert_eq!(
        field(&s.world, "purse.book", "purse.ledger", "last"),
        "live"
    );
}

#[test]
fn mistakes_are_refused_by_name_and_change_nothing() {
    let mut s = Session::new(purse(), 1).unwrap();
    let hash = s.world.hash();
    for (line, says) in [
        (
            "set purse.nobody purse.wallet coins=1",
            "no entity `purse.nobody`",
        ),
        (
            "set purse.buyer purse.pocket coins=1",
            "no component type `purse.pocket`",
        ),
        ("set purse.buyer purse.wallet gold=1", "has no field `gold`"),
        (
            "set purse.buyer purse.wallet coins=\"lots\"",
            "`purse.wallet.coins`",
        ),
        (
            "set purse.tool purse.wallet coins=1",
            "has no `purse.wallet` to set",
        ),
        (
            "remove purse.tool purse.wallet",
            "has no `purse.wallet` to remove",
        ),
        ("remove purse.nobody", "no entity `purse.nobody`"),
        (
            "put elsewhere.thing purse.goods",
            "no loaded module `elsewhere`",
        ),
        ("set buyer purse.wallet coins=1", "not a full entity ID"),
        (
            "place purse.buyer purse.nowhere",
            "no entity `purse.nowhere`",
        ),
    ] {
        let e = s
            .edit(
                "test",
                &[edit("set purse.buyer purse.wallet coins=9"), edit(line)],
            )
            .unwrap_err();
        assert!(e.contains(says), "{line}: {e}");
        assert!(e.contains("edit 2"), "{line}: {e}");
    }
    // The first edit was fine, but the batch is whole or nothing.
    assert_eq!(s.world.hash(), hash);
    // A move of something with no location is the place service's own refusal.
    let e = s
        .edit("test", &[edit("place purse.buyer purse.tool")])
        .unwrap_err();
    assert!(e.contains("nothing changed"), "{e}");
    assert_eq!(s.world.hash(), hash);
}

/// A session with edits before the first tick, part way and on the last tick, recorded.
fn edited_session() -> Session {
    let mut s = Session::new(purse(), 3).unwrap();
    s.start_recording(BuildStamp::current());
    s.edit(
        "inputs",
        &[edit("set purse.buyer purse.wallet coins=150 work=10")],
    )
    .unwrap();
    for tick in 1..=40 {
        let inputs = if tick == 20 { vec![buy(tick)] } else { vec![] };
        s.step(&inputs).unwrap();
        if tick == 10 {
            s.edit(
                "live",
                &[
                    edit("put purse.tool2 purse.goods price=1"),
                    edit("set purse.buyer engine.motion vx=-2"),
                ],
            )
            .unwrap();
        }
    }
    s.edit("live", &[edit("remove purse.tool2")]).unwrap();
    s
}

#[test]
fn an_edited_session_replays_to_the_same_world() {
    let s = edited_session();
    let rec = s.recording().unwrap();
    let edits: Vec<u64> = rec
        .events
        .iter()
        .filter_map(|e| match e {
            RecEvent::Edit { tick, .. } => Some(*tick),
            _ => None,
        })
        .collect();
    assert_eq!(edits, [0, 10, 40]);
    // Through the file, as a replay reads it.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("rec.json");
    rec.write(&path).unwrap();
    let rec = Recording::read(&path).unwrap();
    let (again, report) = replay(&rec).unwrap();
    assert!(report.identical, "{report:?}");
    assert_eq!(again.world, s.world);
    assert_eq!(
        field(&again.world, "purse.tool", "purse.goods", "owned"),
        true
    );
    assert!(!again.world.entities.contains_key("purse.tool2"));
}

#[test]
fn a_save_made_after_edits_loads_and_runs_on_the_same() {
    let mut s = edited_session();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("save.db");
    save::write(&path, &s.world, s.modules(), &BuildStamp::current()).unwrap();
    let (_, world) = save::read(&path).unwrap();
    assert_eq!(world.hash(), s.world.hash());
    let mut resumed = Session::resume(purse(), world).unwrap();
    // A save carries on as the edited world, edits included.
    for _ in 0..30 {
        s.step(&[]).unwrap();
        resumed.step(&[]).unwrap();
    }
    assert_eq!(resumed.world.hash(), s.world.hash());
    // And the resumed world takes edits too.
    resumed
        .edit("test", &[edit("set purse.buyer purse.wallet coins=1")])
        .unwrap();
}

#[test]
fn inputs_file_edits_wait_for_their_tick_and_mistyped_ones_are_refused_first() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("inputs.toml");
    std::fs::write(
        &path,
        "[[edit]]\nentity = 'purse.buyer'\ncomponent = 'purse.wallet'\nset = { coins = 100, work = 10 }\n\
         [[input]]\ntick = 2\nname = 'purse.buy'\n\
         [[edit]]\ntick = 5\nentity = 'purse.tool'\nremove = true\n",
    )
    .unwrap();
    let script = InputsFile::read(&path).unwrap();
    assert_eq!(script.edits.len(), 2);
    let mut s = Session::new(purse(), 1).unwrap();
    s.start_recording(BuildStamp::current());
    let mut due = DueEdits::new(&script.edits, 0, "inputs", &s).unwrap();
    due.apply(&mut s).unwrap();
    let inputs = script.by_tick();
    while s.world.tick < 6 {
        let tick = s.world.tick + 1;
        s.step(inputs.get(&tick).map_or(&[][..], Vec::as_slice))
            .unwrap();
        if tick == 4 {
            assert_eq!(field(&s.world, "purse.tool", "purse.goods", "owned"), true);
        }
        due.apply(&mut s).unwrap();
    }
    assert!(due.is_empty());
    assert!(!s.world.entities.contains_key("purse.tool"));
    assert!(replay(&s.recording().unwrap()).unwrap().1.identical);
    // A resumed save at tick 3 skips the edit before it and keeps the one after.
    let later = DueEdits::new(&script.edits, 3, "inputs", &s).unwrap();
    assert!(!later.is_empty());

    for (text, says) in [
        (
            "entity = 'purse.buyer'\ncomponent = 'purse.wallet'\nset = { gold = 1 }",
            "gold",
        ),
        (
            "entity = 'purse.buyer'\ncomponent = 'purse.purse'\nremove = true",
            "purse.purse",
        ),
        (
            "entity = 'purse.buyer'\ncomponent = 'purse.wallet'\nset = { coins = true }",
            "coins",
        ),
    ] {
        std::fs::write(&path, format!("[[edit]]\n{text}\n")).unwrap();
        let script = InputsFile::read(&path).unwrap();
        let e = DueEdits::new(&script.edits, 0, "inputs", &s).unwrap_err();
        assert!(e.contains(says), "{e}");
    }
    for text in [
        "entity = 'purse.buyer'\nset = { coins = 1 }",
        "entity = 'purse.buyer'\nremove = true\nplace = 'purse.tool'",
        "entity = 'purse.buyer'\nremove = true\ncolour = 'red'",
        "tick = -1\nentity = 'purse.buyer'\nremove = true",
    ] {
        std::fs::write(&path, format!("[[edit]]\n{text}\n")).unwrap();
        assert!(InputsFile::read(&path).is_err(), "{text}");
    }
}

#[test]
fn a_scenario_sets_its_start_state_and_a_wrong_edit_fails_it() {
    let reports = run_module_tests(&purse(), "purse").unwrap();
    assert_eq!(reports.len(), 1);
    assert!(reports[0].passed, "{:?}", reports[0].failures);
    assert_eq!(reports[0].ticks, 4);

    let modules = purse();
    let run = |text: &str| run_scenario(&modules, "purse", "tests/x.toml", text);
    let wrong = [
        (
            "ticks = 2\n[[edit]]\nentity = 'buyer'\ncomponent = 'wallet'\nset = { gold = 1 }",
            "gold",
        ),
        (
            "ticks = 2\n[[edit]]\nentity = 'nobody'\ncomponent = 'wallet'\nset = { coins = 1 }",
            "purse.nobody",
        ),
        (
            "ticks = 2\n[[edit]]\ntick = 1\nentity = 'tool'\ncomponent = 'wallet'\nset = { coins = 1 }",
            "has no `purse.wallet`",
        ),
        (
            "ticks = 2\n[[edit]]\ntick = 3\nentity = 'tool'\nremove = true",
            "after the last tick",
        ),
    ];
    for (text, says) in wrong {
        let e = run(text).unwrap_err();
        assert!(e.contains(says) && e.contains("tests/x.toml"), "{e}");
    }
    // An edit whose outcome the expectations don't see fails as a test, not silently.
    let report = run(
        "ticks = 1\n[[edit]]\nentity = 'buyer'\ncomponent = 'wallet'\nset = { coins = 5 }\n\
         [[expect]]\nentity = 'buyer'\ncomponent = 'wallet'\nfield = 'coins'\nequals = 6\n",
    )
    .unwrap();
    assert!(!report.passed);
    assert!(
        report.failures[0].contains("is 5, expected 6"),
        "{:?}",
        report.failures
    );
}

#[test]
fn edits_and_hot_loads_between_the_same_ticks_replay_in_the_order_made() {
    let modules = purse();
    let mut changed = modules[0].clone();
    // A hot-load that changes the goods' default price: an edit putting goods after it
    // gets the new default, one before it the old.
    let manifest = changed.files.get("manifest.toml").unwrap().replace(
        "price = { type = \"int\", default = 100 }",
        "price = { type = \"int\", default = 70 }",
    );
    changed.files.0.insert("manifest.toml".into(), manifest);
    let changed = Module::from_files(&modules[0].manifest.dir, changed.files).unwrap();
    let mut s = Session::new(modules, 1).unwrap();
    s.start_recording(BuildStamp::current());
    s.step(&[]).unwrap();
    s.edit("live", &[edit("put purse.a purse.goods")]).unwrap();
    s.hot_load(changed).unwrap();
    s.edit("live", &[edit("put purse.b purse.goods")]).unwrap();
    s.step(&[]).unwrap();
    assert_eq!(field(&s.world, "purse.a", "purse.goods", "price"), 100);
    assert_eq!(field(&s.world, "purse.b", "purse.goods", "price"), 70);
    let (again, report) = replay(&s.recording().unwrap()).unwrap();
    assert!(report.identical, "{report:?}");
    assert_eq!(again.world, s.world);
}

fn life() -> Vec<Module> {
    vec![Module::read(&tests_dir().join("play/life/demo")).unwrap()]
}

#[test]
fn state_the_engine_keeps_is_refused_with_what_to_use_instead() {
    let mut s = Session::new(life(), 1).unwrap();
    let hash = s.world.hash();
    for (line, says) in [
        (
            "set life.speech engine.talk current=life.line",
            "`current` of `engine.talk` is kept by the engine",
        ),
        (
            "put life.speech engine.talk speaker=life.walker",
            "set the fields you mean",
        ),
        (
            "set life.walker engine.life phase=walk",
            "`phase` of `engine.life`",
        ),
        (
            "set life.walker engine.location place=life.room",
            "teleport with `place`",
        ),
        ("put life.walker engine.walk active=false", "start a walk"),
    ] {
        let e = s.edit("test", &[edit(line)]).unwrap_err();
        assert!(e.contains(says), "{line}: {e}");
        assert!(e.contains("kept by the engine"), "{line}: {e}");
    }
    assert_eq!(s.world.hash(), hash);
    // What the module authors is still the edit's to set.
    s.edit(
        "test",
        &[
            edit("set life.speech engine.talk width=280"),
            edit("set life.walker engine.life speed=2"),
        ],
    )
    .unwrap();
    assert_eq!(field(&s.world, "life.walker", "engine.life", "speed"), 2.0);
}

#[test]
fn a_walk_is_moved_with_place_and_a_dangling_reference_is_refused() {
    let mut s = Session::new(places(), 1).unwrap();
    s.edit(
        "test",
        &[edit("place places.visitor places.room_b places.b_door")],
    )
    .unwrap();
    let walk = Input {
        name: "places.walk".into(),
        ..buy(1)
    };
    s.step(&[walk]).unwrap();
    assert_eq!(
        field(&s.world, "places.visitor", "engine.walk", "active"),
        true
    );
    let hash = s.world.hash();
    for (line, says) in [
        ("remove places.visitor", "is walking"),
        (
            "put places.visitor engine.walk active=false",
            "kept by the engine",
        ),
        // Its spots and doors would name a place that is gone: what resume checks.
        ("remove places.room_b", "nothing changed"),
    ] {
        let e = s.edit("test", &[edit(line)]).unwrap_err();
        assert!(e.contains(says), "{line}: {e}");
    }
    assert_eq!(s.world.hash(), hash);
    // `place` stops the walk with the move, so the thing can then go.
    s.edit("test", &[edit("place places.visitor places.room_a")])
        .unwrap();
    assert_eq!(
        field(&s.world, "places.visitor", "engine.walk", "active"),
        false
    );
    s.edit(
        "test",
        &[
            edit("remove places.visitor"),
            // A new thing may be given a location, then teleported like any other.
            edit("put places.newcomer engine.location place=places.room_a"),
            edit("place places.newcomer places.room_b places.b_table"),
        ],
    )
    .unwrap();
    assert_eq!(
        field(&s.world, "places.newcomer", "engine.location", "spot"),
        "places.b_table"
    );
}

fn grant(tick: u64, source: &str, coins: i64) -> Input {
    Input {
        tick,
        source: source.into(),
        name: "purse.grant".into(),
        payload: json!({ "coins": coins }).as_object().unwrap().clone(),
        via: None,
    }
}

#[test]
fn a_debug_event_goes_through_the_module_rules_only_from_the_debug_source() {
    let mut s = Session::new(purse(), 1).unwrap();
    let e = s.check_input(&grant(1, "keyboard", 5)).unwrap_err();
    assert!(e.contains("is a debug event"), "{e}");
    let e = s.check_input(&buy(1).clone_as("debug")).unwrap_err();
    assert!(e.contains("only debug events"), "{e}");
    s.check_input(&grant(1, "debug", 5)).unwrap();
    // A screen button naming one does nothing.
    s.step(&[grant(1, "ui", 700)]).unwrap();
    assert_eq!(field(&s.world, "purse.buyer", "purse.wallet", "coins"), 0);
    assert_eq!(galaxy_sim::edit::edits(&s.world), 0);

    s.start_recording(BuildStamp::current());
    s.step(&[grant(2, "debug", 700)]).unwrap();
    s.step(&[grant(3, "debug", 700)]).unwrap();
    // The script's own cap holds: a debug event keeps the module's rules.
    assert_eq!(
        field(&s.world, "purse.buyer", "purse.wallet", "coins"),
        1000
    );
    assert_eq!(galaxy_sim::edit::edits(&s.world), 2);
    s.edit("live", &[edit("set purse.buyer purse.wallet work=10")])
        .unwrap();
    assert_eq!(galaxy_sim::edit::edits(&s.world), 3);
    s.step(&[]).unwrap();
    let (again, report) = replay(&s.recording().unwrap()).unwrap();
    assert!(report.identical, "{report:?}");
    assert_eq!(galaxy_sim::edit::edits(&again.world), 3);

    // The count is the world's, so a save keeps it.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("save.db");
    save::write(&path, &s.world, s.modules(), &BuildStamp::current()).unwrap();
    let (_, world) = save::read(&path).unwrap();
    assert_eq!(galaxy_sim::edit::edits(&world), 3);
}

trait CloneAs {
    fn clone_as(&self, source: &str) -> Input;
}

impl CloneAs for Input {
    fn clone_as(&self, source: &str) -> Input {
        Input {
            source: source.into(),
            ..self.clone()
        }
    }
}
