//! Regressions for the PR #21 re-check, through the script host: beat predicates and
//! callbacks run in fresh Luau states (so a restored save replays exactly), a beat
//! transition the kernel would reject never unlocks its successor, and field views
//! pay for the rocks they scan.

use crate::common;

use std::path::Path;

use common::*;
use galaxy_contract::prelude::*;
use galaxy_core::{Save, Sim};
use galaxy_script::{HostEvent, ScriptHost};

const WHARF: StationId = StationId::from_static("station.lantern_wharf");
const HAUL: &str = "beat.c01_haul";
const AFTER: &str = "beat.c02_after";

/// Writes a one-script pack `gm.recheck` (script `recheck`, `on.dock`) with `tail`
/// appended to the manifest.
fn write_pack(dir: &Path, src: &str, tail: &str) {
    std::fs::create_dir_all(dir.join("scripts")).unwrap();
    std::fs::write(
        dir.join("manifest.toml"),
        format!(
            "id = \"gm.recheck\"\nname = \"Re-check probe\"\nversion = \"0.1.0\"\n\
             uses = [\"events.dock\", \"beats.read\", \"beats.write\", \"history.read\", \
             \"history.write\"]\n\n\
             [[script]]\nid = \"recheck\"\nfile = \"scripts/recheck.luau\"\nhandlers = [\"on.dock\"]\n\
             {tail}\n"
        ),
    )
    .unwrap();
    std::fs::write(dir.join("scripts/recheck.luau"), src).unwrap();
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

fn facts(sim: &Sim, kind: &str) -> Vec<Fact> {
    sim.history()
        .find(&HistoryQuery::kind(FactKind::new(kind).unwrap()))
        .into_iter()
        .cloned()
        .collect()
}

fn status(sim: &Sim, beat: &str) -> Option<BeatStatus> {
    sim.world().progress.beats.get(beat).map(|b| b.status)
}

fn aborted(host: &ScriptHost) -> Vec<String> {
    host.take_events()
        .into_iter()
        .filter_map(|e| match e {
            HostEvent::Aborted { diagnostic, .. } => Some(diagnostic.code),
            _ => None,
        })
        .collect()
}

// P1: beat calls share nothing, so save, restore and continue replays exactly.

/// Six beats, one per docking, whose `on_start` records how large a string fits in
/// the call's 1 MiB; and two predicates that share a module-level counter.
const MEMORY: &str = r#"--!strict
local G = require("@galaxy")

local calls = 0
local function second(): boolean
	calls += 1
	return calls >= 2
end

local function fit(ctx: G.Ctx, beat: G.BeatInfo)
	local k = 1024
	while k > 0 and not pcall(string.rep, "x", k * 1024) do
		k -= 4
	end
	ctx.history:record({ kind = "gm.recheck.fit", amount = k, note = beat.id }, beat.why)
end

local function docking(n: number)
	return G.beat({
		id = `beat.m0{n}`,
		act = 1,
		goal = "Dock again.",
		requires = G.counter("dockings", n),
		done = G.fact({ kind = "gm.recheck.never" }),
		on_start = fit,
	})
end

return G.script({
	id = "recheck",
	beats = {
		docking(1),
		docking(2),
		docking(3),
		docking(4),
		docking(5),
		docking(6),
		G.beat({
			id = "beat.u01",
			act = 1,
			goal = "Never.",
			requires = G.test(function(ctx: G.Ctx): boolean
				return second()
			end),
			done = G.fact({ kind = "gm.recheck.never" }),
		}),
		G.beat({
			id = "beat.u02",
			act = 1,
			goal = "Never.",
			requires = G.test(function(ctx: G.Ctx): boolean
				return second()
			end),
			done = G.fact({ kind = "gm.recheck.never" }),
		}),
	},
	on = {
		dock = function(ev: G.DockEvent, ctx: G.Ctx) end,
	},
})
"#;

fn dock_once(sim: &mut Sim) {
    dock(sim, &KETTLE);
    settle(sim);
}

fn fits(sim: &Sim) -> Vec<i64> {
    facts(sim, "gm.recheck.fit")
        .iter()
        .map(|f| f.amount.unwrap())
        .collect()
}

#[test]
fn beat_calls_run_in_fresh_states_so_save_restore_continue_matches() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("pack");
    write_pack(
        &dir,
        MEMORY,
        "[limits]\nmemory_bytes = 1048576\nsteps_per_call = 1000000",
    );

    // Uninterrupted.
    let mut a = new_sim(3);
    let _ha = hosted(&mut a, &dir);
    for _ in 0..6 {
        dock_once(&mut a);
    }
    let all = fits(&a);
    assert_eq!(all.len(), 6, "{all:?}");
    assert!(all[0] > 0, "{all:?}");
    assert!(
        all.iter().all(|k| *k == all[0]),
        "every callback fits the same: {all:?}"
    );
    for u in ["beat.u01", "beat.u02"] {
        assert_eq!(status(&a, u), None, "{u}: predicates share no upvalue");
    }

    // Saved halfway, loaded under a fresh host, continued.
    let path = tmp.path().join("r.sqlite");
    let mut b = new_sim(3);
    let hb = hosted(&mut b, &dir);
    for _ in 0..3 {
        dock_once(&mut b);
    }
    Save::create(&path, b.state()).unwrap();
    drop(hb);
    let mut c = Sim::new(Save::open(&path).unwrap().load().unwrap());
    let _hc = hosted(&mut c, &dir);
    for _ in 0..3 {
        dock_once(&mut c);
    }
    assert_eq!(fits(&c), all);
    assert_eq!(c.hash(), a.hash());
}

// P1: a successor sees its predecessor done only once the completion will commit.

/// c01 completes at Kettle, noting the haul there (`ExpectDocked`); c02 needs c01 done.
/// The dock handler at the Wharf reads the Shallows ice field (for the P2 test).
const CHAIN: &str = r#"--!strict
local G = require("@galaxy")

local KETTLE = "station.kettle_dock"
local WHARF = "station.lantern_wharf"

return G.script({
	id = "recheck",
	beats = {
		G.beat({
			id = "beat.c01_haul",
			act = 1,
			goal = "Bring five ice to Kettle.",
			requires = { G.docked_at(KETTLE), G.cargo("ware.ice", 5) },
			done = { G.fact({ kind = "gm.recheck.go" }), G.docked_at(KETTLE) },
			on_complete = function(ctx: G.Ctx, beat: G.BeatInfo)
				ctx.beats:expect_docked(KETTLE, beat.why)
				ctx.history:record({ kind = "gm.recheck.hauled", note = "hauled" }, beat.why)
			end,
		}),
		G.beat({
			id = "beat.c02_after",
			act = 1,
			goal = "After the haul.",
			requires = G.done("beat.c01_haul"),
			done = G.fact({ kind = "gm.recheck.never" }),
			on_start = function(ctx: G.Ctx, beat: G.BeatInfo)
				ctx.history:record({ kind = "gm.recheck.after", note = "c02 started" }, beat.why)
			end,
		}),
	},
	on = {
		dock = function(ev: G.DockEvent, ctx: G.Ctx)
			if ev.station ~= WHARF then
				return
			end
			local f = ctx.beats:field("field.shallows.ice")
			local why = ctx:cause({ summary = "the probe reads the field", cites = { ev.fact } })
			ctx.history:record({ kind = "gm.recheck.field", amount = if f then f.rocks else -1, note = "field" }, why)
		end,
	},
})
"#;

fn chain_sim(dir: &Path) -> (Sim, ScriptHost) {
    write_pack(dir, CHAIN, "[limits]\nsteps_per_call = 1000");
    let mut sim = new_sim(4);
    let host = hosted(&mut sim, dir);
    (sim, host)
}

#[test]
fn a_rejected_completion_never_unlocks_its_successor() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut sim, host) = chain_sim(&tmp.path().join("pack"));
    act(
        &mut sim,
        vec![Command::AdjustCargo {
            ship: PLAYER,
            ware: ICE,
            delta: 5,
        }],
    );
    dock_at(&mut sim, &KETTLE);
    step(&mut sim, 1);
    assert_eq!(status(&sim, HAUL), Some(BeatStatus::Active));
    act(
        &mut sim,
        vec![Command::RecordFact {
            fact: Box::new(FactDraft {
                kind: FactKind::from_static("gm.recheck.go"),
                subjects: Default::default(),
                amount: None,
                note: "go".into(),
            }),
        }],
    );
    host.take_events();

    // c01 completes this tick, but the player undocks first: the kernel would reject
    // the handover, so the host drops it and c02 doesn't see c01 done. It lost to
    // contention, so it tries again next tick and nothing counts against the script.
    sim.submit(CommandBatch {
        cause: Cause::system("test_player", "undock"),
        commands: vec![Command::SetShipDocked {
            ship: PLAYER,
            station: None,
        }],
    });
    let r = sim.step(&PlayerInput::default()).unwrap();
    assert!(r.rejected.is_empty(), "{:#?}", r.rejected);
    let contended: Vec<String> = host
        .take_events()
        .into_iter()
        .filter_map(|e| match e {
            HostEvent::Aborted { diagnostic, .. } => Some(diagnostic.code),
            HostEvent::Contended { handler, .. } => Some(handler),
            _ => None,
        })
        .collect();
    assert_eq!(contended, vec![format!("beat {HAUL}")]);
    assert_eq!(status(&sim, HAUL), Some(BeatStatus::Active));
    assert_eq!(status(&sim, AFTER), None, "the successor stays locked");
    assert!(facts(&sim, "gm.recheck.after").is_empty());
    assert!(facts(&sim, "gm.recheck.hauled").is_empty());

    // Docked for the whole tick, c01 completes and c02 follows in the same tick.
    dock_at(&mut sim, &KETTLE);
    step(&mut sim, 1);
    assert_eq!(status(&sim, HAUL), Some(BeatStatus::Completed));
    assert_eq!(status(&sim, AFTER), Some(BeatStatus::Active));
    assert_eq!(facts(&sim, "gm.recheck.after").len(), 1);
    assert_eq!(facts(&sim, "gm.recheck.hauled").len(), 1);
    assert!(aborted(&host).is_empty());
}

// P2: a field view pays for every rock it scans.

#[test]
fn a_field_view_charges_the_step_budget_for_the_rocks_it_scans() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut sim, host) = chain_sim(&tmp.path().join("pack"));
    dock_at(&mut sim, &WHARF);
    step(&mut sim, 1);
    let read = facts(&sim, "gm.recheck.field");
    assert_eq!(read.len(), 1, "a small world reads within 1,000 steps");
    let field = read[0].amount.unwrap();
    assert!(field > 0);
    assert!(aborted(&host).is_empty());

    // 20,000 more rocks elsewhere: the same read is over a 1,000-step budget.
    let template = sim
        .world()
        .rocks
        .values()
        .find(|r| r.field.as_str() != "field.shallows.ice")
        .unwrap()
        .clone();
    let more: Vec<Command> = (0..20_000)
        .map(|i| {
            let id = RockId::new(format!("rock.recheck.extra.{i:05}")).unwrap();
            Command::PutRock {
                id: id.clone(),
                value: Some(Box::new(Rock {
                    id,
                    ..template.clone()
                })),
            }
        })
        .collect();
    act(&mut sim, more);
    dock_at(&mut sim, &WHARF);
    step(&mut sim, 1);
    assert_eq!(aborted(&host), vec!["over_budget".to_owned()]);
    assert_eq!(facts(&sim, "gm.recheck.field").len(), 1);
}
