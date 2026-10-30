//! The adversarial review of the script host (PR #18): each test reproduces one of the
//! reviewer's scenarios and checks the host now holds. Headless.

mod common;

use std::path::Path;

use common::*;
use galaxy_contract::history::HistoryQuery;
use galaxy_contract::pack::ValidationStep;
use galaxy_contract::prelude::*;
use galaxy_contract::talk::{Channel, MAX_LINE_LEN};
use galaxy_core::Sim;
use galaxy_script::host::HostEvent;
use galaxy_script::rollback::causes_of;
use galaxy_script::{CheckOptions, ScriptHost, check_dir};

const PROBE: &str = "gm.probe";

/// Writes a one-script probe pack (`gm.probe`, script `probe`, `on.dock`). `tail` goes
/// right after the script's `handlers` line: more script keys, then `[limits]` and so on.
fn probe_pack(dir: &Path, src: &str, tail: &str) {
    std::fs::create_dir_all(dir.join("scripts")).unwrap();
    std::fs::write(
        dir.join("manifest.toml"),
        format!(
            "id = \"{PROBE}\"\nname = \"Probe\"\nversion = \"0.1.0\"\n\
             uses = [\"events.dock\", \"stations.read\", \"stations.write\", \"history.read\", \
             \"history.write\", \"factions.read\", \"factions.write\", \"npcs.write\", \
             \"talk.write\"]\n\n\
             [[script]]\nid = \"probe\"\nfile = \"scripts/probe.luau\"\nhandlers = [\"on.dock\"]\n\
             {tail}\n"
        ),
    )
    .unwrap();
    std::fs::write(dir.join("scripts/probe.luau"), src).unwrap();
}

/// A script whose dock handler is `body` (`ev` and `ctx` in scope).
fn on_dock(body: &str) -> String {
    format!(
        "local G = require(\"@galaxy\")\nreturn G.script({{\n\tid = \"probe\",\n\ton = {{\n\
         \t\tdock = function(ev, ctx)\n{body}\n\t\tend,\n\t}},\n}})\n"
    )
}

/// A sim running the probe pack unchecked, so even bad code runs.
fn probe_sim(dir: &Path) -> (Sim, ScriptHost) {
    let mut sim = new_sim(1);
    let host = ScriptHost::new(None);
    host.load_unchecked(dir).unwrap();
    host.attach(&mut sim);
    (sim, host)
}

fn dock_once(sim: &mut Sim) {
    dock(sim, &KETTLE);
    settle(sim);
}

/// `(code, message)` of every aborted call since the last look.
fn aborts(host: &ScriptHost) -> Vec<(String, String)> {
    host.take_events()
        .into_iter()
        .filter_map(|e| match e {
            HostEvent::Aborted { diagnostic, .. } => Some((diagnostic.code, diagnostic.message)),
            _ => None,
        })
        .collect()
}

fn log(host: &ScriptHost) -> Vec<String> {
    host.script_log(&pack_id(PROBE))
}

// 1. Native allocations count against the call.

#[test]
fn native_copies_are_measured_first_and_capped_per_call() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("probe");
    probe_pack(
        &dir,
        &on_dock(
            r#"
			local why = ctx:cause({ summary = "bulk", cites = { ev.fact } })
			-- The reviewer's 128 KiB note: refused before it is copied.
			local note = string.rep("x", 128 * 1024)
			print("big", (pcall(function()
				ctx.history:record({ kind = "gm.bulk", subjects = {}, note = note }, why)
			end)))
			-- Notes under the per-argument cap, a hundred times: over the call's quota.
			local chunk = string.rep("y", 15 * 1024)
			for _ = 1, 100 do
				pcall(function()
					ctx.history:record({ kind = "gm.bulk", subjects = {}, note = chunk }, why)
				end)
			end
			-- Catching the quota error doesn't save the call.
			pcall(function()
				ctx.stations:set_stage(ev.station, 7, why)
			end)
		"#,
        ),
        "[limits]\nmemory_bytes = 1048576",
    );
    let (mut sim, host) = probe_sim(&dir);
    dock_once(&mut sim);
    assert_eq!(log(&host), vec!["big\tfalse"]);
    let a = aborts(&host);
    assert_eq!(a.len(), 1, "{a:?}");
    assert_eq!(a[0].0, "over_quota", "{a:?}");
    assert_eq!(stage(&sim), 0, "nothing the call wrote applied");
    let bulk = HistoryQuery::kind("gm.bulk".parse().unwrap());
    assert_eq!(sim.history().count(&bulk), 0);
}

#[test]
fn print_lines_and_luau_allocations_are_capped() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("probe");
    probe_pack(
        &dir,
        &on_dock(
            r#"
			print(string.rep("z", 512 * 1024))
			print("buffer", (pcall(buffer.create, 2 * 1024 * 1024)))
			print("rep", (pcall(string.rep, "x", 2 * 1024 * 1024)))
		"#,
        ),
        "[limits]\nmemory_bytes = 1048576",
    );
    let (mut sim, host) = probe_sim(&dir);
    dock_once(&mut sim);
    let lines = log(&host);
    assert_eq!(lines.len(), 3, "{lines:?}");
    assert_eq!(lines[0].len(), 1024, "a print line is cut at 1 KiB");
    assert_eq!(&lines[1..], ["buffer\tfalse", "rep\tfalse"]);
}

// 2. Numbers from scripts never panic or wrap in Rust.

#[test]
fn script_numbers_are_checked_not_cast() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("probe");
    probe_pack(
        &dir,
        &on_dock(
            r#"
			local lo, hi = -4611686018427387904, 4611686018427387904
			local n = ctx.rng:int(lo, hi)
			print("rng", n >= lo and n <= hi)
			print("rng_min", (pcall(function() return ctx.rng:int(-9223372036854775808, 0) end)))
			print("rng_2^63", (pcall(function() return ctx.rng:int(0, 9223372036854775807) end)))
			local why = ctx:cause({ summary = "numbers", cites = { ev.fact } })
			print("handle", (pcall(function()
				ctx.stations:set_stage(ev.station, 2, { handle = -9223372036854775808 } :: any)
			end)))
			print("fraction", (pcall(function() ctx.stations:set_stage(ev.station, 2.5, why) end)))
			print("negative", (pcall(function() ctx.stations:set_stage(ev.station, -1, why) end)))
			print("huge", (pcall(function()
				ctx.factions:adjust_standing("faction.wharf_cooperative", 1e12, why)
			end)))
			ctx.stations:set_stage(ev.station, 1, why)
		"#,
        ),
        "",
    );
    // The validator's smoke run takes the same paths and must not panic either.
    let state = galaxy_core::new_game(&core_data(), 1).unwrap();
    let report = check_dir(
        &dir,
        CheckOptions {
            world: Some(&state.world),
            history: Some(&state.history),
            ..CheckOptions::default()
        },
    )
    .report;
    let smoke = report
        .steps
        .iter()
        .find(|s| s.step == ValidationStep::Smoke)
        .unwrap();
    assert!(smoke.diagnostics.is_empty(), "{smoke:#?}");

    let (mut sim, host) = probe_sim(&dir);
    dock_once(&mut sim);
    assert_eq!(
        log(&host),
        vec![
            "rng\ttrue",
            "rng_min\ttrue",
            "rng_2^63\tfalse",
            "handle\tfalse",
            "fraction\tfalse",
            "negative\tfalse",
            "huge\tfalse",
        ]
    );
    assert!(aborts(&host).is_empty());
    assert_eq!(stage(&sim), 1);
}

// 3. A handler's writes are one transaction.

#[test]
fn a_handler_that_would_be_half_applied_applies_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("probe");
    probe_pack(
        &dir,
        &on_dock(
            r#"
			local why = ctx:cause({ summary = "grow and invent", cites = { ev.fact } })
			ctx.stations:set_stage(ev.station, 7, why)
			ctx.npcs:put({ id = "gm.ghost", name = "Ghost", role = "drifter", location = "station." .. "missing" }, why)
		"#,
        ),
        "",
    );
    let (mut sim, host) = probe_sim(&dir);
    dock_once(&mut sim); // `step` asserts the kernel rejected nothing
    assert_eq!(stage(&sim), 0, "the stage didn't go up without the npc");
    assert!(!sim.world().npcs.contains("gm.ghost"));
    let a = aborts(&host);
    assert_eq!(a.len(), 1, "{a:?}");
    assert_eq!(a[0].0, "rejected_write");
    assert!(a[0].1.contains("references_resolve"), "{}", a[0].1);
}

#[test]
fn a_second_cause_aborts_the_call() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("probe");
    probe_pack(
        &dir,
        &on_dock(
            r#"
			local a = ctx:cause({ summary = "first", cites = { ev.fact } })
			ctx.stations:set_stage(ev.station, 7, a)
			local b = ctx:cause({ summary = "second", cites = { ev.fact } })
			ctx.stations:set_stage(ev.station, 8, b)
		"#,
        ),
        "",
    );
    let (mut sim, host) = probe_sim(&dir);
    dock_once(&mut sim);
    assert_eq!(stage(&sim), 0);
    assert_eq!(aborts(&host)[0].0, "second_cause");
}

// 4. A revision stops in the tick its rollback is queued.

#[test]
fn a_rolled_back_revision_cant_regrow_kettle_in_the_rollback_tick() {
    let (mut sim, host) = kettle_sim(&kettle_dir());
    let before = stage(&sim);
    grow_to_stage_2(&mut sim);
    let change = sim
        .history()
        .last(&HistoryQuery::kind("gm.stage_built".parse().unwrap()))
        .unwrap()
        .cause;
    // Enough for stage 3, committed: the handlers would run on them next tick.
    deliver(&mut sim, &ICE, 100);
    deliver(&mut sim, &PLATE, 100);
    step(&mut sim, 1);
    assert_eq!(stage(&sim), 2);

    let id = pack_id(KETTLE_PACK);
    let revision = host.revision(&id).unwrap();
    galaxy_script::rollback(&mut sim, &[change], host.rollback_line(&id).as_ref());
    step(&mut sim, 1);
    assert_eq!(stage(&sim), before, "rolled back, and not regrown");
    let pack = sim.world().packs.get(KETTLE_PACK).unwrap();
    assert!(pack.retired.contains(&revision));
    step(&mut sim, 3);
    assert_eq!(stage(&sim), before);
}

// 5. No state survives a call outside the declared stores.

#[test]
fn locals_and_modules_start_over_every_call_so_restores_replay_exactly() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("probe");
    probe_pack(
        &dir,
        r#"
local G = require("@galaxy")
local memo = require("./memo")
local calls = 0
return G.script({
	id = "probe",
	on = {
		dock = function(ev, ctx)
			calls += 1
			memo.n += 1
			local why = ctx:cause({ summary = "count", cites = { ev.fact } })
			ctx.stations:set_stage(ev.station, calls * 10 + memo.n, why)
		end,
	},
})
"#,
        "",
    );
    std::fs::write(dir.join("scripts/memo.luau"), "return { n = 0 }\n").unwrap();

    let (mut sim, _host) = probe_sim(&dir);
    dock_once(&mut sim);
    assert_eq!(stage(&sim), 11);
    // Restore: a copy of the state under a fresh host.
    let mut restored = Sim::new(sim.state().clone());
    let fresh = ScriptHost::new(None);
    fresh.load_unchecked(&dir).unwrap();
    restored.add_system(fresh.system());

    dock_once(&mut sim);
    dock_once(&mut restored);
    assert_eq!(stage(&sim), 11, "the upvalue and module table started over");
    assert_eq!(stage(&restored), 11);
    assert_eq!(sim.hash(), restored.hash());
}

// 6. Rollback retires the revision even when no line can be said as declared.

#[test]
fn rolling_back_everything_a_revision_did_still_retires_it() {
    let (mut sim, host) = kettle_sim(&kettle_dir());
    grow_to_stage_2(&mut sim);
    let id = pack_id(KETTLE_PACK);
    let revision = host.revision(&id).unwrap();
    // Every cause the revision made. The core pack has Odo now, so the install brought
    // nobody and rolling everything back leaves him (the rollback's speaker) in place.
    let causes = causes_of(&sim, &id, &revision);
    let out = galaxy_script::rollback(&mut sim, &causes, host.rollback_line(&id).as_ref());
    step(&mut sim, 1); // nothing rejected
    assert!(sim.world().npcs.contains(&ODO), "core's Odo stays");
    assert_eq!(stage(&sim), 0);
    assert!(
        sim.world()
            .packs
            .get(KETTLE_PACK)
            .unwrap()
            .retired
            .contains(&revision)
    );
    let line = out.line.expect("Odo says it, at Kettle");
    assert_eq!((line.speaker, line.place), (Some(ODO), Some(KETTLE)));
    assert_eq!(lines(&sim, Channel::DockTalk).len(), 1);
    assert_eq!(lines(&sim, Channel::Chronicle).len(), 1);
}

#[test]
fn a_long_rollback_chronicle_is_cut_to_fit() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("probe");
    probe_pack(
        &dir,
        &on_dock(
            r#"
			local why = ctx:cause({ summary = string.rep("s", 180), cites = { ev.fact } })
			ctx.history:record({ kind = "gm.probe_mark", subjects = { station = ev.station }, note = "mark" }, why)
		"#,
        ),
        "\n[rollback]\nplace = \"station.kettle_dock\"\ntext = \"We took that back.\"",
    );
    let (mut sim, host) = probe_sim(&dir);
    for _ in 0..3 {
        dock_once(&mut sim);
    }
    let id = pack_id(PROBE);
    let revision = host.revision(&id).unwrap();
    let causes = causes_of(&sim, &id, &revision);
    assert_eq!(causes.len(), 3);
    let out = galaxy_script::rollback(&mut sim, &causes, host.rollback_line(&id).as_ref());
    assert!(out.line.is_some());
    step(&mut sim, 1);
    let chronicle = lines(&sim, Channel::Chronicle);
    assert_eq!(chronicle.len(), 1);
    assert!(chronicle[0].line.text.len() <= MAX_LINE_LEN);
    assert!(
        sim.world()
            .packs
            .get(PROBE)
            .unwrap()
            .retired
            .contains(&revision)
    );
}

// 7. Nothing a script can see depends on heap addresses.

#[test]
fn no_addresses_or_address_order_reach_a_script() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("probe");
    probe_pack(
        &dir,
        &on_dock(
            r#"
			local summary = tostring({}) .. " " .. string.format("%s %*", {}, print) .. ` {buffer.create(1)}`
			local why = ctx:cause({ summary = summary, cites = { ev.fact } })
			local keyed = {}
			keyed[{}] = 1
			keyed.x = 2
			print("pairs", (pcall(function()
				for _ in pairs(keyed) do
				end
			end)))
			print("next", (pcall(next, keyed)))
			local named = { b = 2, a = 1 }
			local seen = 0
			for _ in pairs(named) do
				seen += 1
			end
			print("plain", seen)
			-- The collector doesn't run during a call: a weak entry stays.
			local weak = setmetatable({}, { __mode = "v" })
			weak.a = {}
			for i = 1, 5000 do
				local _ = string.rep("g", 64) .. i
			end
			print("weak", weak.a ~= nil)
			ctx.stations:set_stage(ev.station, 1, why)
		"#,
        ),
        "",
    );
    let (mut sim, host) = probe_sim(&dir);
    dock_once(&mut sim);
    assert_eq!(
        log(&host),
        vec!["pairs\tfalse", "next\tfalse", "plain\t2", "weak\ttrue"]
    );
    let cause = sim
        .history()
        .causes()
        .values()
        .find(|c| matches!(c.cause.kind, CauseKind::Script { .. }))
        .unwrap();
    assert_eq!(cause.cause.summary, "table table function buffer");
}

// 9. A quarantined revision stays quarantined after another revision runs.

#[test]
fn reloading_a_quarantined_revision_keeps_it_quarantined() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("probe");
    let a = on_dock(
        r#"
			if ctx.stations:stage(ev.station) == 0 then
				error("not yet")
			end
			local why = ctx:cause({ summary = "seven", cites = { ev.fact } })
			ctx.stations:set_stage(ev.station, 7, why)
		"#,
    );
    let limits = "[limits]\nquarantine_after = 1";
    probe_pack(&dir, &a, limits);
    let (mut sim, host) = probe_sim(&dir);
    let id = pack_id(PROBE);
    let rev_a = host.revision(&id).unwrap();
    dock_once(&mut sim);
    assert_eq!(aborts(&host).len(), 1, "A is quarantined");

    // A would succeed now, if it ran.
    sim.submit(CommandBatch::new(Cause::system("test", "stage 1")).with(
        Command::SetStationStage {
            station: KETTLE,
            stage: 1,
        },
    ));
    step(&mut sim, 1);

    // Revision B aborts too: its health replaces A's.
    probe_pack(&dir, &on_dock("\t\t\terror(\"always\")"), limits);
    host.load_unchecked(&dir).unwrap();
    dock_once(&mut sim);
    assert_eq!(aborts(&host).len(), 1, "B is quarantined");

    // The exact A again: still quarantined.
    probe_pack(&dir, &a, limits);
    host.load_unchecked(&dir).unwrap();
    assert_eq!(host.revision(&id).unwrap(), rev_a);
    dock_once(&mut sim);
    assert!(aborts(&host).is_empty(), "A didn't even run");
    assert_eq!(stage(&sim), 1);
    let health = &sim.world().packs.get(PROBE).unwrap().health["probe"];
    assert!(health.quarantined_before.contains(&rev_a));
}
