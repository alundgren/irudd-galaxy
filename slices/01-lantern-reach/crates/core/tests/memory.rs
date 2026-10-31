//! World memory (issue #47, 4.4): summarised history per station, person and faction
//! in the save, with an FTS5 index, synced on write and built on open for older saves.

use std::path::{Path, PathBuf};

use galaxy_contract::pack::{PackLayer, PackState};
use galaxy_contract::prelude::*;
use galaxy_contract::talk::{Channel, TalkLine};
use galaxy_core::memory::{self, Filter, Source, SubjectKind};
use galaxy_core::{Save, Sim, new_game};

const WHARF: StationId = StationId::from_static("station.lantern_wharf");
const KETTLE: StationId = StationId::from_static("station.kettle_dock");
const PLAYER: ShipId = ShipId::from_static("ship.player");
const ICE: WareId = WareId::from_static("ware.ice");
const COOP: FactionId = FactionId::from_static("faction.wharf_cooperative");
const ODO: NpcId = NpcId::from_static("npc.odo_fenn");
const GM: &str = "gm.chronicle";

fn data_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packs/core/data")
}

fn emit(sim: &mut Sim, cause: Cause, events: Vec<Event>) {
    let mut batch = CommandBatch::new(cause);
    for event in events {
        batch = batch.with(Command::Emit { event });
    }
    sim.submit(batch);
    sim.step(&PlayerInput::default()).unwrap();
}

fn verb(summary: &str) -> Cause {
    Cause::verb(Actor::Player, VerbKind::Wait, summary)
}

fn sell(station: &StationId, quantity: u32) -> Event {
    Event::Sell {
        ship: PLAYER,
        station: station.clone(),
        ware: ICE,
        quantity,
        unit_price: 1200,
    }
}

fn last_fact(sim: &Sim) -> FactId {
    sim.history().last_fact_id().unwrap()
}

/// A short game: two sales and a delivery at Kettle in one visit, a standing change,
/// Odo's line, a game-master note and a live-layer note.
fn played() -> Sim {
    let mut state = new_game(&data_dir(), 1).unwrap();
    state.world.packs.insert(
        GM.parse().unwrap(),
        PackState {
            layer: Some(PackLayer::GameMaster),
            ..PackState::default()
        },
    );
    let mut sim = galaxy_core::game(state);
    emit(
        &mut sim,
        verb("dock"),
        vec![Event::Dock {
            ship: PLAYER,
            station: KETTLE,
        }],
    );
    emit(
        &mut sim,
        verb("sell"),
        vec![sell(&KETTLE, 10), sell(&KETTLE, 20)],
    );
    emit(
        &mut sim,
        verb("deliver"),
        vec![Event::Deliver {
            ship: PLAYER,
            station: KETTLE,
            ware: ICE,
            quantity: 20,
            ledger: None,
        }],
    );
    emit(
        &mut sim,
        Cause::system("progress", "deeds"),
        vec![Event::StandingChanged {
            faction: COOP,
            deed: "deed.delivered".parse().unwrap(),
            delta: 5,
        }],
    );
    let cite = last_fact(&sim);
    sim.submit(CommandBatch::new(verb("talk")).with(Command::Say {
        line: Box::new(TalkLine {
            channel: Channel::DockTalk,
            speaker: Some(ODO),
            place: Some(KETTLE),
            text: "The beans will hear about this.".into(),
            cites: vec![cite],
            about: Vec::new(),
            choices: Vec::new(),
        }),
    }));
    sim.step(&PlayerInput::default()).unwrap();
    let note = |kind: &str, note: &str| Command::RecordFact {
        fact: Box::new(FactDraft {
            kind: kind.parse().unwrap(),
            subjects: Subjects {
                station: Some(KETTLE),
                ..Subjects::default()
            },
            amount: None,
            note: note.into(),
        }),
    };
    sim.submit(
        CommandBatch::new(Cause {
            kind: CauseKind::Script {
                pack: GM.parse().unwrap(),
                revision: "r1".into(),
            },
            summary: "the chronicle".into(),
            cites: Vec::new(),
        })
        .with(note(
            "gm.chronicle.festival",
            "A lantern festival at Kettle Dock",
        )),
    );
    sim.submit(
        CommandBatch::new(Cause {
            kind: CauseKind::Mind {
                npc: Some(ODO),
                decision: "banter".into(),
            },
            summary: "the live layer".into(),
            cites: Vec::new(),
        })
        .with(note(
            "mind.banter",
            "A barge captain brags about a full hold of ice",
        )),
    );
    sim.step(&PlayerInput::default()).unwrap();
    emit(
        &mut sim,
        verb("undock"),
        vec![Event::Undock {
            ship: PLAYER,
            station: KETTLE,
        }],
    );
    sim
}

#[test]
fn the_bundled_sqlite_has_fts5() {
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    conn.execute_batch("CREATE VIRTUAL TABLE t USING fts5(text)")
        .expect("rusqlite's bundled SQLite is built with FTS5");
}

#[test]
fn entries_summarise_history_per_station_person_and_faction() {
    let sim = played();
    let entries = memory::summarize(sim.state());
    // The visit's two sales are one entry, written when the visit ends.
    let trades: Vec<_> = entries.iter().filter(|e| e.kind == "trade").collect();
    assert_eq!(trades.len(), 1, "{entries:#?}");
    assert_eq!(trades[0].text, "Sold 30 ice at Kettle Dock for 360 cr");
    assert_eq!(trades[0].facts.len(), 2);
    assert!(trades[0].id.starts_with("mem.fact."));
    let delivery = entries.iter().find(|e| e.kind == "delivery").unwrap();
    assert_eq!(delivery.text, "Delivered 20 ice at Kettle Dock");
    let standing = entries.iter().find(|e| e.kind == "standing").unwrap();
    assert_eq!(standing.subjects[0].kind, SubjectKind::Faction);
    assert!(standing.text.contains("rose by 5"), "{}", standing.text);
    let talk = entries.iter().find(|e| e.kind == "talk").unwrap();
    assert_eq!(talk.text, "Odo Fenn: The beans will hear about this.");
    assert!(talk.id.starts_with("mem.talk."));
    // Sources come from the causes.
    let source = |needle: &str| {
        entries
            .iter()
            .find(|e| e.text.contains(needle))
            .unwrap_or_else(|| panic!("{needle}: {entries:#?}"))
            .source
    };
    assert_eq!(source("lantern festival"), Source::GameMaster);
    assert_eq!(source("barge captain"), Source::LiveLayer);
    assert_eq!(source("Sold 30 ice"), Source::Authored);
    // Every entry counts the docking it happened in.
    assert!(entries.iter().all(|e| e.docking >= 1), "{entries:#?}");
    // Deterministic.
    assert_eq!(entries, memory::summarize(sim.state()));
}

#[test]
fn the_save_searches_its_memory_with_fts5_and_lists_by_subject() {
    let sim = played();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("memory.sqlite");
    let save = Save::create(&path, sim.state()).unwrap();
    let all = memory::summarize(sim.state());
    assert_eq!(memory::count(&save).unwrap(), all.len() as u64);

    let kettle = Filter {
        station: Some(KETTLE.to_string()),
        ..Filter::default()
    };
    let found = memory::search(&save, "sold ice", &kettle).unwrap();
    assert_eq!(found[0].text, "Sold 30 ice at Kettle Dock for 360 cr");
    // Stemmed: "deliver" finds "Delivered".
    let found = memory::search(&save, "deliver", &kettle).unwrap();
    assert!(found.iter().any(|e| e.kind == "delivery"), "{found:#?}");
    // Punctuation and FTS operators in the query are plain words.
    assert!(memory::search(&save, "beans\" OR (", &Filter::default()).is_ok());
    // Nothing at the Wharf.
    let wharf = Filter {
        station: Some(WHARF.to_string()),
        ..Filter::default()
    };
    assert!(memory::search(&save, "ice", &wharf).unwrap().is_empty());

    // By person and by faction.
    let odo = memory::list(
        &save,
        &Filter {
            person: Some(ODO.to_string()),
            ..Filter::default()
        },
    )
    .unwrap();
    assert_eq!(odo.len(), 1);
    assert_eq!(odo[0].kind, "talk");
    let coop = memory::list(
        &save,
        &Filter {
            faction: Some(COOP.to_string()),
            ..Filter::default()
        },
    )
    .unwrap();
    assert_eq!(coop.len(), 1);
    // By source.
    let live = memory::list(
        &save,
        &Filter {
            source: Some(Source::LiveLayer),
            ..Filter::default()
        },
    )
    .unwrap();
    assert_eq!(live.len(), 1);
    assert!(live[0].text.contains("barge captain"));
    // A read-back entry is the entry.
    let stored = memory::list(&save, &Filter::default()).unwrap();
    let newest = all.last().unwrap();
    assert_eq!(&stored[0], newest);
}

#[test]
fn writes_append_new_entries_and_a_diverged_timeline_rebuilds() {
    let mut sim = played();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("memory.sqlite");
    let mut save = Save::create(&path, sim.state()).unwrap();
    let before = memory::list(&save, &Filter::default()).unwrap();
    let earlier = sim.state().clone();

    // A second visit: its sale is written once the visit ends.
    emit(
        &mut sim,
        verb("dock"),
        vec![Event::Dock {
            ship: PLAYER,
            station: WHARF,
        }],
    );
    emit(&mut sim, verb("sell"), vec![sell(&WHARF, 5)]);
    save.write(sim.state()).unwrap();
    assert_eq!(
        memory::count(&save).unwrap(),
        before.len() as u64,
        "an open visit's trades wait for the undock"
    );
    emit(
        &mut sim,
        verb("undock"),
        vec![Event::Undock {
            ship: PLAYER,
            station: WHARF,
        }],
    );
    save.write(sim.state()).unwrap();
    let after = memory::list(
        &save,
        &Filter {
            limit: Some(1000),
            ..Filter::default()
        },
    )
    .unwrap();
    assert_eq!(after.len(), before.len() + 1);
    assert_eq!(after[0].text, "Sold 5 ice at Lantern Wharf for 60 cr");
    // The earlier entries are untouched.
    assert_eq!(&after[1..=before.len().min(after.len() - 1)], &before[..]);

    // Writing the earlier state back (a hard restore) rebuilds to match it.
    save.write(&earlier).unwrap();
    let back = memory::list(
        &save,
        &Filter {
            limit: Some(1000),
            ..Filter::default()
        },
    )
    .unwrap();
    let mut want = memory::summarize(&earlier);
    want.reverse();
    assert_eq!(back, want);
}

#[test]
fn a_save_made_before_world_memory_gets_it_on_open() {
    let sim = played();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("old.sqlite");
    drop(Save::create(&path, sim.state()).unwrap());
    {
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute_batch(
            "DROP TABLE memory_meta; DROP TABLE memory_entry; DROP TABLE memory_subject;
             DROP TABLE memory_fts; DROP TABLE memory_embedding;",
        )
        .unwrap();
    }
    let save = Save::open(&path).unwrap();
    let mut want = memory::summarize(sim.state());
    want.reverse();
    let got = memory::list(
        &save,
        &Filter {
            limit: Some(1000),
            ..Filter::default()
        },
    )
    .unwrap();
    assert_eq!(got, want);
    // The state is untouched: memory is never part of it.
    assert_eq!(
        galaxy_core::state_hash(&save.load().unwrap()),
        galaxy_core::state_hash(sim.state())
    );
    // The embedding table is there for later, and empty.
    drop(save);
    let conn = rusqlite::Connection::open(&path).unwrap();
    let n: i64 = conn
        .query_row("SELECT COUNT(*) FROM memory_embedding", [], |r| r.get(0))
        .unwrap();
    assert_eq!(n, 0);
}
