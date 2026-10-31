//! Contention is not a fault (PR #21 re-check 2). A write that is valid on the world a
//! call read but loses to a change made earlier in the tick (an undock, cargo moved)
//! never counts toward quarantine: a beat transition tries again next tick, and an
//! event handler runs again for the same event, up to `limits.contention_retries`
//! times, before it is dropped with a `script_contention_dropped` fact. Script faults
//! still quarantine.

use crate::common;

use std::path::Path;

use common::*;
use galaxy_contract::pack::ScriptHealth;
use galaxy_contract::prelude::*;
use galaxy_core::{Save, Sim};
use galaxy_progress::ProgressSystem;
use galaxy_script::{HostEvent, ScriptHost};

const WHARF: StationId = StationId::from_static("station.lantern_wharf");
const B03: &str = "beat.b03_first_delivery";

/// A one-script pack `gm.contend` (script `contend`, `on.dock`, plus `on.reply` if the
/// script handles it), `tail` appended to the manifest.
fn write_pack(dir: &Path, src: &str, tail: &str) {
    std::fs::create_dir_all(dir.join("scripts")).unwrap();
    let handlers = if src.contains("reply = function") {
        "\"on.dock\", \"on.reply\""
    } else {
        "\"on.dock\""
    };
    std::fs::write(
        dir.join("manifest.toml"),
        format!(
            "id = \"gm.contend\"\nname = \"Contention probe\"\nversion = \"0.1.0\"\n\
             uses = [\"events.dock\", \"events.reply\", \"beats.read\", \"beats.write\", \
             \"history.read\", \"history.write\", \"npcs.write\"]\n\n\
             [[script]]\nid = \"contend\"\nfile = \"scripts/contend.luau\"\n\
             handlers = [{handlers}]\n\
             {tail}\n"
        ),
    )
    .unwrap();
    std::fs::write(dir.join("scripts/contend.luau"), src).unwrap();
}

fn hosted(sim: &mut Sim, dir: &Path) -> ScriptHost {
    let host = ScriptHost::new(None);
    host.load_unchecked(dir).unwrap();
    host.attach(sim);
    host
}

fn act(sim: &mut Sim, commands: Vec<Command>) {
    sim.submit(CommandBatch {
        cause: Cause::system("test_player", "the player acts"),
        commands,
    });
    step(sim, 1);
}

/// Queued ahead of the script host in the next tick, like a player's input.
fn queue(sim: &mut Sim, commands: Vec<Command>) {
    sim.submit(CommandBatch {
        cause: Cause::system("test_player", "the player acts first"),
        commands,
    });
}

fn undock() -> Vec<Command> {
    vec![Command::SetShipDocked {
        ship: PLAYER,
        station: None,
    }]
}

fn take_ice(n: i64) -> Vec<Command> {
    vec![Command::AdjustCargo {
        ship: PLAYER,
        ware: ICE,
        delta: -n,
    }]
}

fn dock_at(sim: &mut Sim, station: &StationId) {
    let region = sim.world().stations.get(station).unwrap().region.clone();
    act(
        sim,
        vec![
            Command::SetShipRegion {
                ship: PLAYER,
                region,
            },
            Command::SetShipDocked {
                ship: PLAYER,
                station: Some(station.clone()),
            },
            Command::Emit {
                event: Event::Dock {
                    ship: PLAYER,
                    station: station.clone(),
                },
            },
        ],
    );
}

fn give_ice(sim: &mut Sim, n: i64) {
    act(
        sim,
        vec![Command::AdjustCargo {
            ship: PLAYER,
            ware: ICE,
            delta: n,
        }],
    );
}

fn ice(sim: &Sim) -> u32 {
    sim.world().ships.get(&PLAYER).unwrap().hold.count(&ICE)
}

fn facts(sim: &Sim, kind: &str) -> Vec<Fact> {
    sim.history()
        .find(&HistoryQuery::kind(FactKind::new(kind).unwrap()))
        .into_iter()
        .cloned()
        .collect()
}

fn health(sim: &Sim, pack: &str, script: &str) -> ScriptHealth {
    sim.world()
        .packs
        .get(pack)
        .and_then(|p| p.health.get(script))
        .cloned()
        .unwrap_or_default()
}

fn status(sim: &Sim, beat: &str) -> Option<BeatStatus> {
    sim.world().progress.beats.get(beat).map(|b| b.status)
}

/// (aborted codes, contended handlers, dropped handlers) since the last call.
fn outcomes(host: &ScriptHost) -> (Vec<String>, Vec<String>, Vec<String>) {
    let mut out = (Vec::new(), Vec::new(), Vec::new());
    for e in host.take_events() {
        match e {
            HostEvent::Aborted { diagnostic, .. } => out.0.push(diagnostic.code),
            HostEvent::Contended { handler, .. } => out.1.push(handler),
            HostEvent::ContentionDropped { handler, .. } => out.2.push(handler),
            _ => {}
        }
    }
    out
}

// The reviewer's scenario: the shipped journey's b03 handover, raced by undocks.

fn core_sim(seed: u64) -> (Sim, ScriptHost) {
    let mut sim = new_sim(seed);
    let host = ScriptHost::new(None);
    let report = host.load(&slice().join("packs/core"), sim.world(), sim.history());
    assert!(report.accepted(), "{report:#?}");
    host.attach(&mut sim);
    sim.add_system(Box::new(ProgressSystem::default()));
    (sim, host)
}

/// From a new game to b03 under way, with 20 ice aboard.
fn to_b03(sim: &mut Sim) {
    step(sim, 1);
    act(
        sim,
        vec![
            Command::SetShipDocked {
                ship: PLAYER,
                station: None,
            },
            Command::Emit {
                event: Event::Undock {
                    ship: PLAYER,
                    station: WHARF,
                },
            },
        ],
    );
    step(sim, 2);
    give_ice(sim, 20);
    step(sim, 3);
    assert_eq!(status(sim, B03), Some(BeatStatus::Active));
}

/// The player answers Odo at Kettle: Deliver (a `reply` event, through the progress
/// system). Act 1's reply handler runs in the next tick.
fn pick_deliver(sim: &mut Sim) {
    let input = PlayerInput {
        replies: vec![Reply {
            npc: "npc.odo_fenn".parse().unwrap(),
            choice: "choice.deliver_ice".parse().unwrap(),
        }],
        ..PlayerInput::default()
    };
    let r = sim.step(&input).expect("not paused");
    assert!(r.rejected.is_empty(), "{:#?}", r.rejected);
}

#[test]
fn b03_handover_races_never_quarantine_and_it_completes_once_docked() {
    let (mut sim, host) = core_sim(21);
    to_b03(&mut sim);
    host.take_events();
    for race in 1..=4 {
        dock_at(&mut sim, &KETTLE);
        step(&mut sim, 1);
        assert_eq!(ice(&sim), 20, "race {race}: docking hands nothing over");
        pick_deliver(&mut sim);
        // The handover runs this tick, but the player undocks ahead of it.
        queue(&mut sim, undock());
        step(&mut sim, 1);
        let (aborted, contended, _) = outcomes(&host);
        assert!(aborted.is_empty(), "race {race}: {aborted:?}");
        assert_eq!(contended, vec!["on.reply".to_owned()], "race {race}");
        let h = health(&sim, "core", "act1");
        assert_eq!((h.aborts, h.quarantined), (0, false), "race {race}");
        assert_eq!(h.retries.len(), 1, "race {race}: the handover waits");
        // Run again next tick: the player has left, so it hands nothing over.
        step(&mut sim, 1);
        assert!(health(&sim, "core", "act1").retries.is_empty());
        assert_eq!(ice(&sim), 20, "race {race}");
        assert_eq!(outcomes(&host), Default::default(), "race {race}");
    }
    assert_eq!(status(&sim, B03), Some(BeatStatus::Active));

    // Docked for good, the handover goes through and b03 completes.
    dock_at(&mut sim, &KETTLE);
    step(&mut sim, 1);
    pick_deliver(&mut sim);
    step(&mut sim, 3);
    assert_eq!(ice(&sim), 0);
    assert_eq!(status(&sim, B03), Some(BeatStatus::Completed));
    let h = health(&sim, "core", "act1");
    assert_eq!((h.aborts, h.quarantined), (0, false));
    assert!(facts(&sim, "script_quarantined").is_empty());
}

// A beat transition that loses to contention tries again next tick.

/// c01 completes at Kettle, noting the haul there (`ExpectDocked`); c02 needs c01 done.
const CHAIN: &str = r#"--!strict
local G = require("@galaxy")

local KETTLE = "station.kettle_dock"

return G.script({
	id = "contend",
	beats = {
		G.beat({
			id = "beat.c01_haul",
			act = 1,
			goal = "Bring five ice to Kettle.",
			requires = G.cargo("ware.ice", 5),
			done = G.docked_at(KETTLE),
			on_complete = function(ctx: G.Ctx, beat: G.BeatInfo)
				ctx.beats:expect_docked(KETTLE, beat.why)
				ctx.history:record({ kind = "gm.contend.hauled", subjects = { station = KETTLE }, note = "hauled" }, beat.why)
			end,
		}),
		G.beat({
			id = "beat.c02_after",
			act = 1,
			goal = "After the haul.",
			requires = G.done("beat.c01_haul"),
			done = G.fact({ kind = "gm.contend.never" }),
		}),
	},
	on = {
		dock = function(ev: G.DockEvent, ctx: G.Ctx) end,
	},
})
"#;

#[test]
fn a_contested_beat_completion_retries_and_never_quarantines() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("pack");
    write_pack(&dir, CHAIN, "");
    let mut sim = new_sim(4);
    let host = hosted(&mut sim, &dir);
    give_ice(&mut sim, 5);
    step(&mut sim, 1);
    assert_eq!(status(&sim, "beat.c01_haul"), Some(BeatStatus::Active));
    host.take_events();

    // Three (and more) completion ticks lost to an undock: more than
    // `quarantine_after` would allow if they counted.
    for race in 1..=4 {
        // Docked at the end of this tick; c01 completes in the next, which an undock
        // queued ahead of the host contests.
        act(
            &mut sim,
            vec![Command::SetShipDocked {
                ship: PLAYER,
                station: Some(KETTLE),
            }],
        );
        queue(&mut sim, undock());
        step(&mut sim, 1);
        let (aborted, contended, _) = outcomes(&host);
        assert!(aborted.is_empty(), "race {race}: {aborted:?}");
        assert_eq!(contended, vec!["beat beat.c01_haul".to_owned()]);
        assert_eq!(status(&sim, "beat.c01_haul"), Some(BeatStatus::Active));
        assert_eq!(status(&sim, "beat.c02_after"), None);
        let h = health(&sim, "gm.contend", "contend");
        assert_eq!((h.aborts, h.quarantined), (0, false), "race {race}");
    }
    assert!(facts(&sim, "gm.contend.hauled").is_empty());

    // The world settles: c01 completes and c02 starts in the same tick.
    act(
        &mut sim,
        vec![Command::SetShipDocked {
            ship: PLAYER,
            station: Some(KETTLE),
        }],
    );
    step(&mut sim, 1);
    assert_eq!(status(&sim, "beat.c01_haul"), Some(BeatStatus::Completed));
    assert_eq!(status(&sim, "beat.c02_after"), Some(BeatStatus::Active));
    assert_eq!(facts(&sim, "gm.contend.hauled").len(), 1);
    assert!(outcomes(&host).0.is_empty());
}

// An event handler that loses to contention runs again for the same event.

/// The player's answer at Kettle hands over all the ice they carry, as read.
const HANDOVER: &str = r#"--!strict
local G = require("@galaxy")

local KETTLE = "station.kettle_dock"

return G.script({
	id = "contend",
	on = {
		dock = function(ev: G.DockEvent, ctx: G.Ctx) end,
		reply = function(ev: G.ReplyEvent, ctx: G.Ctx)
			if ev.station ~= KETTLE then
				return
			end
			local carried = ctx.beats:pilot().cargo["ware.ice"] or 0
			if carried < 1 then
				return
			end
			ctx.beats:deliver(
				{ station = KETTLE, ware = "ware.ice", quantity = carried },
				ctx:cause({ summary = "hand over the ice", cites = { ev.fact } })
			)
		end,
	},
})
"#;

/// Docked at Kettle, the player answers: the handover runs in the next tick.
fn answer_at_kettle(sim: &mut Sim) {
    dock_at(sim, &KETTLE);
    act(
        sim,
        vec![Command::Emit {
            event: Event::Reply {
                ship: PLAYER,
                npc: "npc.odo_fenn".parse().unwrap(),
                station: KETTLE,
                choice: "choice.hand_over".parse().unwrap(),
            },
        }],
    );
}

fn handover_sim(dir: &Path, tail: &str) -> (Sim, ScriptHost) {
    write_pack(dir, HANDOVER, tail);
    let mut sim = new_sim(5);
    let host = hosted(&mut sim, dir);
    give_ice(&mut sim, 12);
    host.take_events();
    (sim, host)
}

#[test]
fn a_handler_that_loses_to_contention_runs_again_and_succeeds() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut sim, host) = handover_sim(&tmp.path().join("pack"), "");
    answer_at_kettle(&mut sim);
    // The handler reads 12 ice, but 5 are sold ahead of it: it would hand over more
    // than is aboard.
    queue(&mut sim, take_ice(5));
    step(&mut sim, 1);
    let (aborted, contended, _) = outcomes(&host);
    assert!(aborted.is_empty(), "{aborted:?}");
    assert_eq!(contended, vec!["on.reply".to_owned()]);
    assert_eq!(ice(&sim), 7);
    assert!(facts(&sim, "deliver").is_empty());
    let h = health(&sim, "gm.contend", "contend");
    assert_eq!(h.aborts, 0);
    assert_eq!(h.retries.len(), 1);
    assert_eq!(h.retries[0].lost, 1);
    assert_eq!(h.retries[0].event.event.kind(), EventKind::Reply);

    // Next tick, the same event again: it reads 7 and hands them over.
    step(&mut sim, 1);
    assert_eq!(ice(&sim), 0);
    let delivered = facts(&sim, "deliver");
    assert_eq!(delivered.len(), 1);
    assert_eq!(delivered[0].amount, Some(7));
    let h = health(&sim, "gm.contend", "contend");
    assert_eq!(h.aborts, 0);
    assert!(h.retries.is_empty());
    assert_eq!(outcomes(&host), Default::default());
}

#[test]
fn a_handler_that_keeps_losing_is_dropped_after_the_bound_with_a_fact() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut sim, host) =
        handover_sim(&tmp.path().join("pack"), "[limits]\ncontention_retries = 2");
    answer_at_kettle(&mut sim);
    // One ice sold ahead of every run: the first run and both retries lose.
    for run in 1..=3u32 {
        queue(&mut sim, take_ice(1));
        step(&mut sim, 1);
        let (aborted, contended, dropped) = outcomes(&host);
        assert!(aborted.is_empty(), "run {run}: {aborted:?}");
        let h = health(&sim, "gm.contend", "contend");
        assert_eq!(h.aborts, 0);
        if run < 3 {
            assert_eq!(contended, vec!["on.reply".to_owned()], "run {run}");
            assert_eq!(h.retries.len(), 1);
            assert_eq!(h.retries[0].lost, run);
        } else {
            assert!(contended.is_empty());
            assert_eq!(dropped, vec!["on.reply".to_owned()]);
            assert!(h.retries.is_empty(), "dropped: {h:?}");
        }
    }
    let dropped = facts(&sim, "script_contention_dropped");
    assert_eq!(dropped.len(), 1);
    assert_eq!(dropped[0].amount, Some(3));
    assert_eq!(dropped[0].subjects.station, Some(KETTLE));
    let FactPayload::Note(note) = &dropped[0].payload else {
        panic!("{:?}", dropped[0].payload)
    };
    assert!(note.contains("gm.contend/contend"), "{note}");
    assert!(note.len() <= 1024);
    assert!(facts(&sim, "deliver").is_empty());
    assert_eq!(ice(&sim), 9);

    // Nothing runs again for that event, and the script is healthy.
    step(&mut sim, 2);
    assert_eq!(outcomes(&host), Default::default());
    assert_eq!(ice(&sim), 9);
    assert!(!health(&sim, "gm.contend", "contend").quarantined);
}

// Script faults still count.

#[test]
fn a_script_fault_still_quarantines_after_three_aborts() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("pack");
    write_pack(
        &dir,
        r#"--!strict
local G = require("@galaxy")

return G.script({
	id = "contend",
	on = {
		dock = function(ev: G.DockEvent, ctx: G.Ctx)
			-- A write invalid on the world it read: there's no such station.
			local why = ctx:cause({ summary = "a drifter turns up", cites = { ev.fact } })
			ctx.npcs:put({ id = "gm.ghost", name = "Ghost", role = "drifter", location = "station." .. "missing" }, why)
		end,
	},
})
"#,
        "",
    );
    let mut sim = new_sim(6);
    let host = hosted(&mut sim, &dir);
    for n in 1..=3u32 {
        dock_at(&mut sim, &KETTLE);
        step(&mut sim, 1);
        let (aborted, contended, _) = outcomes(&host);
        assert_eq!(aborted, vec!["rejected_write".to_owned()], "dock {n}");
        assert!(contended.is_empty(), "a fault is never retried");
        let h = health(&sim, "gm.contend", "contend");
        assert_eq!(h.aborts, n);
        assert!(h.retries.is_empty());
        assert_eq!(h.quarantined, n == 3);
    }
    assert_eq!(facts(&sim, "script_quarantined").len(), 1);

    // A Lua error counts the same way.
    let dir = tmp.path().join("erroring");
    write_pack(
        &dir,
        r#"--!strict
local G = require("@galaxy")

return G.script({
	id = "contend",
	on = {
		dock = function(ev: G.DockEvent, ctx: G.Ctx)
			error("boom")
		end,
	},
})
"#,
        "",
    );
    let mut sim = new_sim(6);
    let host = hosted(&mut sim, &dir);
    for _ in 0..3 {
        dock_at(&mut sim, &KETTLE);
        step(&mut sim, 1);
    }
    assert_eq!(outcomes(&host).0.len(), 3);
    assert!(health(&sim, "gm.contend", "contend").quarantined);
}

// Save and load while a retry waits.

#[test]
fn save_and_load_mid_retry_continue_like_an_uninterrupted_run() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("pack");
    // Contested once, then it goes through.
    let lose_once = |sim: &mut Sim| {
        answer_at_kettle(sim);
        queue(sim, take_ice(5));
        step(sim, 1);
    };

    let (mut a, _ha) = handover_sim(&dir, "");
    lose_once(&mut a);
    step(&mut a, 3);
    assert_eq!(facts(&a, "deliver").len(), 1);

    let (mut b, hb) = handover_sim(&dir, "");
    lose_once(&mut b);
    assert_eq!(health(&b, "gm.contend", "contend").retries.len(), 1);
    let path = tmp.path().join("mid.sqlite");
    Save::create(&path, b.state()).unwrap();
    drop(hb);
    let mut c = Sim::new(Save::open(&path).unwrap().load().unwrap());
    let _hc = hosted(&mut c, &dir);
    assert_eq!(health(&c, "gm.contend", "contend").retries.len(), 1);
    step(&mut c, 3);
    assert_eq!(facts(&c, "deliver").len(), 1);
    assert_eq!(c.hash(), a.hash());
}
