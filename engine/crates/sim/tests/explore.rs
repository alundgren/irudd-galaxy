//! Explored memory (`galaxy_sim::explore`): recorded from movement, deterministic, saved,
//! replayed, set by edits, and guarded against authoring mistakes.

use std::path::Path;

use galaxy_api::engine::EXPLORED;
use galaxy_module::module::{Module, ModuleFiles};
use galaxy_sim::edit::Edit;
use galaxy_sim::{BuildStamp, Session, World, replay, save};

const MANIFEST: &str = "id = 'scout'\nversion = '0.1.0'\nmin_engine_version = '0.9.0'\n\
    writes = ['engine.position', 'engine.motion', 'engine.explorer', 'engine.explored', \
    'engine.place', 'engine.location']\n";

/// A scout flying +x at 30 units a second, remembering 16 units round it.
const START: &str = "[entities.scout]\n\
    \"engine.position\" = { x = 0.0, y = 0.0 }\n\
    \"engine.motion\" = { vx = 30.0 }\n\
    \"engine.explorer\" = { radius = 16.0 }\n";

fn module(start: &str) -> Module {
    let mut files = ModuleFiles::default();
    files.0.insert("manifest.toml".into(), MANIFEST.into());
    files.0.insert("start.toml".into(), start.into());
    Module::from_files(Path::new("scout"), files).unwrap()
}

fn cells(world: &World) -> String {
    world.component("scout.scout", EXPLORED).unwrap().fields["cells"]
        .as_str()
        .unwrap()
        .to_owned()
}

fn explored(world: &World, x: f64, y: f64) -> bool {
    galaxy_api::explored::explored_at(&cells(world), 8.0, x, y)
}

fn run(start: &str, ticks: u64) -> Session {
    let mut s = Session::new(vec![module(start)], 1).unwrap();
    for _ in 0..ticks {
        s.step(&[]).unwrap();
    }
    s
}

#[test]
fn memory_follows_movement_and_is_deterministic() {
    let s = run(START, 60);
    // Two seconds at 30 a second: x is 60. Behind and around is remembered; ahead isn't.
    assert!(explored(&s.world, 0.0, 0.0));
    assert!(explored(&s.world, 30.0, 10.0));
    assert!(explored(&s.world, 60.0, -10.0));
    assert!(!explored(&s.world, 100.0, 0.0));
    assert!(!explored(&s.world, 30.0, 40.0));
    let explore = s.world.services["explore"];
    let c = s.world.component("scout.scout", EXPLORED).unwrap();
    assert_eq!(c.cause, explore);
    assert_eq!(run(START, 60).world.hash(), s.world.hash());
}

#[test]
fn quiet_ticks_write_nothing() {
    let still = START.replace("vx = 30.0", "vx = 0.0");
    let mut s = run(&still, 2);
    let before = s.world.component("scout.scout", EXPLORED).unwrap().clone();
    for _ in 0..30 {
        s.step(&[]).unwrap();
    }
    let after = s.world.component("scout.scout", EXPLORED).unwrap();
    assert_eq!(after.tick, before.tick, "no new cell, no write");
}

#[test]
fn memory_survives_save_load_and_replay() {
    let mut s = Session::new(vec![module(START)], 1).unwrap();
    s.start_recording(BuildStamp::current());
    for _ in 0..40 {
        s.step(&[]).unwrap();
    }
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("save.db");
    save::write(&path, &s.world, s.modules(), &BuildStamp::current()).unwrap();
    let (_, world) = save::read(&path).unwrap();
    assert_eq!(cells(&world), cells(&s.world));
    let mut resumed = Session::resume(vec![module(START)], world).unwrap();
    for _ in 0..40 {
        s.step(&[]).unwrap();
        resumed.step(&[]).unwrap();
    }
    assert_eq!(cells(&resumed.world), cells(&s.world));
    assert_eq!(resumed.world.hash(), s.world.hash());
    let (again, report) = replay(&s.recording().unwrap()).unwrap();
    assert!(report.identical, "{report:?}");
    assert_eq!(cells(&again.world), cells(&s.world));
}

#[test]
fn edits_set_and_clear_memory_and_movement_adds_to_it() {
    let still = START.replace("vx = 30.0", "vx = 0.0");
    let mut s = run(&still, 1);
    s.edit(
        "test",
        &[Edit::parse_text("set scout.scout engine.explored cells=\"10:10+3\"").unwrap()],
    )
    .unwrap();
    assert!(explored(&s.world, 85.0, 85.0));
    // The next tick marks round the scout again, onto the edited memory.
    s.step(&[]).unwrap();
    assert!(explored(&s.world, 85.0, 85.0) && explored(&s.world, 0.0, 0.0));
    // Clearing: the scout's own surroundings come back next tick, nothing else.
    s.edit(
        "test",
        &[Edit::parse_text("put scout.scout engine.explored").unwrap()],
    )
    .unwrap();
    assert_eq!(cells(&s.world), "");
    s.step(&[]).unwrap();
    assert!(explored(&s.world, 0.0, 0.0) && !explored(&s.world, 85.0, 85.0));
    // A malformed memory is refused with the reason and changes nothing.
    let before = s.world.hash();
    let e = s
        .edit(
            "test",
            &[Edit::parse_text("set scout.scout engine.explored cells=\"0:0+2,2+1\"").unwrap()],
        )
        .unwrap_err();
    assert!(e.contains("touches the run before it"), "{e}");
    assert_eq!(s.world.hash(), before);
}

#[test]
fn a_runaway_radius_or_cell_is_refused_at_start() {
    for (extra, needle) in [
        ("radius = 2000.0", "spans more than 64 cells"),
        ("radius = -1.0", "radius must be a finite distance"),
    ] {
        let start = START.replace("radius = 16.0", extra);
        let e = Session::new(vec![module(&start)], 1).err().unwrap();
        assert!(e.contains(needle), "{e}");
    }
    let start = format!("{START}\"engine.explored\" = {{ cell = 0.0 }}\n");
    let e = Session::new(vec![module(&start)], 1).err().unwrap();
    assert!(e.contains("cell must be"), "{e}");
}

#[test]
fn memory_is_kept_in_the_place_it_was_made_in() {
    let start = format!(
        "{START}\"engine.location\" = {{ place = \"scout.reach\" }}
         [entities.reach]
\"engine.place\" = {{ width = 2000, depth = 1500 }}
         [entities.dock]
\"engine.place\" = {{}}
"
    );
    let mut s = run(&start, 30);
    // Flying inside the reach records in the reach's coordinates.
    let memory = s.world.component("scout.scout", EXPLORED).unwrap();
    assert_eq!(memory.fields["place"], "scout.reach");
    assert!(explored(&s.world, 0.0, 0.0) && explored(&s.world, 25.0, 0.0));
    // In another place it records nothing: the reach's cells are not the dock's.
    let before = cells(&s.world);
    s.edit(
        "test",
        &[Edit::parse_text("place scout.scout scout.dock").unwrap()],
    )
    .unwrap();
    s.edit(
        "test",
        &[Edit::parse_text("set scout.scout engine.position x=500 y=500").unwrap()],
    )
    .unwrap();
    for _ in 0..10 {
        s.step(&[]).unwrap();
    }
    assert_eq!(cells(&s.world), before);
    assert_eq!(
        s.world.component("scout.scout", EXPLORED).unwrap().fields["place"],
        "scout.reach"
    );
    // Cleared, the memory starts again where its explorer is.
    s.edit(
        "test",
        &[Edit::parse_text("put scout.scout engine.explored").unwrap()],
    )
    .unwrap();
    s.step(&[]).unwrap();
    let memory = s.world.component("scout.scout", EXPLORED).unwrap();
    assert_eq!(memory.fields["place"], "scout.dock");
    assert!(explored(&s.world, 500.0, 500.0) && !explored(&s.world, 0.0, 0.0));
    // A memory of something that isn't a place is refused.
    let e = s
        .edit(
            "test",
            &[Edit::parse_text("set scout.scout engine.explored place=scout.scout").unwrap()],
        )
        .unwrap_err();
    assert!(e.contains("is not an engine.place"), "{e}");
    // Nor can the place a memory is of be removed from under it, with nothing else in it.
    s.edit(
        "test",
        &[Edit::parse_text("place scout.scout scout.reach").unwrap()],
    )
    .unwrap();
    for text in ["remove scout.dock engine.place", "remove scout.dock"] {
        let e = s
            .edit("test", &[Edit::parse_text(text).unwrap()])
            .unwrap_err();
        assert!(e.contains("is not an engine.place"), "{text}: {e}");
    }
}
