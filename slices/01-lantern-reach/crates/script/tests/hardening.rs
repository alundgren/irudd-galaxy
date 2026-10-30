//! The adversarial review of the script host (PR #18): each test reproduces one of the
//! reviewer's scenarios and checks the host now holds. Headless.

mod common;

use std::path::Path;

use common::*;
use galaxy_contract::history::HistoryQuery;
use galaxy_contract::pack::ValidationStep;
use galaxy_contract::prelude::*;
use galaxy_contract::talk::{Channel, MAX_LINE_LEN};
use galaxy_contract::world::WorldState;
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
				ctx.history:record({ kind = "gm.probe.bulk", subjects = {}, note = note }, why)
			end)))
			-- A note over 1 KiB: history keeps it for good, so it's refused.
			print("note", (pcall(function()
				ctx.history:record({ kind = "gm.probe.bulk", subjects = {}, note = string.rep("n", 1025) }, why)
			end)))
			-- Notes under the cap, three hundred times: over the call's quota.
			local chunk = string.rep("y", 1000)
			for _ = 1, 300 do
				pcall(function()
					ctx.history:record({ kind = "gm.probe.bulk", subjects = {}, note = chunk }, why)
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
    assert_eq!(log(&host), vec!["big\tfalse", "note\tfalse"]);
    let a = aborts(&host);
    assert_eq!(a.len(), 1, "{a:?}");
    assert_eq!(a[0].0, "over_quota", "{a:?}");
    assert_eq!(stage(&sim), 0, "nothing the call wrote applied");
    let bulk = HistoryQuery::kind("gm.probe.bulk".parse().unwrap());
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
    assert!(lines[0].ends_with(galaxy_script::vm::CUT_MARKER));
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
        .last(&HistoryQuery::kind(
            "gm.kettle_growth.stage_built".parse().unwrap(),
        ))
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
			ctx.history:record({ kind = "gm.probe.mark", subjects = { station = ev.station }, note = "mark" }, why)
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

// --- The re-check (round 3) ---

/// A strict probe script (`--!strict` first) whose dock handler is `body`.
fn strict_on_dock(body: &str) -> String {
    format!("--!strict\n{}", on_dock(body))
}

fn checked(dir: &Path) -> galaxy_contract::pack::ValidationReport {
    let world = new_sim(1).world().clone();
    check_dir(
        dir,
        CheckOptions {
            world: Some(&world),
            analyzer: galaxy_script::analyzer::find(None).as_deref(),
            ..CheckOptions::default()
        },
    )
    .report
}

/// `(code, file, line)` of every error in a report.
fn error_codes(r: &galaxy_contract::pack::ValidationReport) -> Vec<(String, String, Option<u32>)> {
    r.steps
        .iter()
        .flat_map(|s| &s.diagnostics)
        .filter(|d| d.severity == galaxy_contract::pack::Severity::Error)
        .map(|d| {
            let file = d
                .location
                .as_ref()
                .map(|l| l.file.clone())
                .unwrap_or_default();
            (
                d.code.clone(),
                file,
                d.location.as_ref().and_then(|l| l.line),
            )
        })
        .collect()
}

const UNCHECKED_HELPER: &str = "--!nocheck\nlocal t = { a = 1 }\nlocal M = {}\n\
     function M.first(): number\n\tfor _, v in t do\n\t\treturn v\n\tend\n\treturn 0\nend\n\
     local wrong: number = \"str\"\nreturn M\n";

// P1: every check covers exactly the files `require` can load.

#[test]
fn a_lib_module_is_checked_like_a_script() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("probe");
    probe_pack(
        &dir,
        &strict_on_dock(
            "\t\t\tlocal helper = require(\"../lib/helper\")\n\t\t\tprint(helper.first())",
        ),
        "",
    );
    // `lib = true` is a top-level key: put it before the first table.
    let manifest = std::fs::read_to_string(dir.join("manifest.toml")).unwrap();
    std::fs::write(dir.join("manifest.toml"), format!("lib = true\n{manifest}")).unwrap();
    std::fs::create_dir_all(dir.join("lib")).unwrap();
    std::fs::write(dir.join("lib/helper.luau"), UNCHECKED_HELPER).unwrap();

    let r = checked(&dir);
    assert!(!r.accepted());
    let codes = error_codes(&r);
    let at = |code: &str, line: u32| {
        codes
            .iter()
            .any(|(c, f, l)| c == code && f == "lib/helper.luau" && *l == Some(line))
    };
    assert!(at("typecheck_off", 1), "{codes:#?}");
    assert!(at("not_strict", 1), "{codes:#?}");
    assert!(at("unordered_iteration", 5), "{codes:#?}");
}

#[test]
fn require_loads_only_from_the_script_roots() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("probe");
    probe_pack(
        &dir,
        &on_dock(
            r#"
			for _, name in ipairs({ "../lib/helper", "../tests/helper", "../lib/" .. "helper", "./../tests/helper" }) do
				print(name, (pcall(function() return require(name) end)))
			end
			require("../tests/helper")
		"#,
        ),
        "",
    );
    for folder in ["lib", "tests"] {
        std::fs::create_dir_all(dir.join(folder)).unwrap();
        std::fs::write(dir.join(folder).join("helper.luau"), UNCHECKED_HELPER).unwrap();
    }
    let (mut sim, host) = probe_sim(&dir);
    dock_once(&mut sim);
    assert_eq!(
        log(&host),
        vec![
            "../lib/helper\tfalse",
            "../tests/helper\tfalse",
            "../lib/helper\tfalse",
            "./../tests/helper\tfalse",
        ]
    );
    let a = aborts(&host);
    assert!(
        a.len() == 1
            && a[0]
                .1
                .contains("isn't a module in this pack's script roots"),
        "{a:?}"
    );
    // The check says nothing can load `lib/` here, and doesn't scan it.
    let r = checked(&dir);
    let warned = r
        .steps
        .iter()
        .flat_map(|s| &s.diagnostics)
        .any(|d| d.code == "unloadable_file");
    assert!(warned, "{r:#?}");
    assert!(
        !error_codes(&r)
            .iter()
            .any(|(_, f, _)| f.starts_with("lib/") || f.starts_with("tests/")),
        "{:#?}",
        error_codes(&r)
    );
}

#[test]
fn a_declared_lib_root_loads_at_runtime() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("probe");
    probe_pack(
        &dir,
        &strict_on_dock("\t\t\tprint(require(\"../lib/helper\").answer)"),
        "",
    );
    let manifest = std::fs::read_to_string(dir.join("manifest.toml")).unwrap();
    std::fs::write(dir.join("manifest.toml"), format!("lib = true\n{manifest}")).unwrap();
    std::fs::create_dir_all(dir.join("lib")).unwrap();
    std::fs::write(
        dir.join("lib/helper.luau"),
        "--!strict\nreturn { answer = 42 }\n",
    )
    .unwrap();
    let (mut sim, host) = probe_sim(&dir);
    dock_once(&mut sim);
    assert_eq!(log(&host), vec!["42"]);
}

// P2: no source can turn the strict typecheck off.

#[test]
fn a_mode_directive_anywhere_is_an_error() {
    let variants = [
        "--[[x]] --!nocheck",
        "--[==[ x ]==]--!nonstrict",
        "--[[\nnote\n]] --!nocheck",
        "--!NoCheck",
        "local s = \"--!nonstrict\"",
    ];
    for (n, first) in variants.iter().enumerate() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("probe");
        let body = "\t\t\tlocal value: number = \"this is a string\"\n\t\t\tprint(value)";
        probe_pack(&dir, &format!("--!strict\n{first}\n{}", on_dock(body)), "");
        let codes = error_codes(&checked(&dir));
        assert!(
            codes.iter().any(|(c, _, _)| c == "typecheck_off"),
            "variant {n} `{first}`: {codes:#?}"
        );
    }
}

#[test]
fn strict_must_be_the_first_line() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("probe");
    probe_pack(
        &dir,
        &format!("\n-- a note\n--!strict\n{}", on_dock("")),
        "",
    );
    let codes = error_codes(&checked(&dir));
    assert!(
        codes
            .iter()
            .any(|(c, f, l)| c == "not_strict" && f == "scripts/probe.luau" && *l == Some(2)),
        "{codes:#?}"
    );
    // Leading blank lines are fine.
    probe_pack(&dir, &format!("\n\n--!strict\n{}", on_dock("")), "");
    let codes = error_codes(&checked(&dir));
    assert!(
        !codes.iter().any(|(c, _, _)| c == "not_strict"),
        "{codes:#?}"
    );
}

// P2: the reviewer's iterator bypass, end to end.

#[test]
fn an_iterator_expression_that_may_yield_the_table_is_rejected() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("probe");
    probe_pack(
        &dir,
        &strict_on_dock(
            "\t\t\tlocal t = { a = 1 }\n\t\t\tfor _, v in pairs({}) and t do\n\t\t\t\tprint(v)\n\t\t\tend",
        ),
        "",
    );
    let codes = error_codes(&checked(&dir));
    assert!(
        codes
            .iter()
            .any(|(c, _, l)| c == "unordered_iteration" && *l == Some(8)),
        "{codes:#?}"
    );
}

// P2: script text is capped before it reaches the log, history or a save.

#[test]
fn abort_messages_and_error_objects_are_cut_to_1_kib() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("probe");
    probe_pack(
        &dir,
        &on_dock(
            r#"
			if ctx.stations:stage(ev.station) == 0 then
				error(string.rep("x", 1024 * 1024))
			end
			error(setmetatable({}, { __tostring = function() return string.rep("y", 100000) end }))
		"#,
        ),
        "[limits]\nquarantine_after = 2",
    );
    let (mut sim, host) = probe_sim(&dir);
    dock_once(&mut sim);
    sim.submit(CommandBatch::new(Cause::system("test", "stage 1")).with(
        Command::SetStationStage {
            station: KETTLE,
            stage: 1,
        },
    ));
    step(&mut sim, 1);
    dock_once(&mut sim);
    let a = aborts(&host);
    assert_eq!(a.len(), 2, "{a:?}");
    for (_, message) in &a {
        assert!(message.len() <= 1024, "{} bytes", message.len());
        assert!(
            message.ends_with(galaxy_script::vm::CUT_MARKER),
            "{message}"
        );
    }
    let note = sim
        .history()
        .last(&HistoryQuery::kind(galaxy_script::host::QUARANTINED))
        .map(|f| match &f.payload {
            galaxy_contract::history::FactPayload::Note(n) => n.clone(),
            other => panic!("{other:?}"),
        })
        .expect("quarantined");
    assert!(note.len() < 1024 + 200, "{} bytes", note.len());
    assert!(note.contains(galaxy_script::vm::CUT_MARKER));
}

// P2: native helpers charge the step budget for their work.

/// Runs `body` on one dock and returns the abort codes.
fn abort_codes(body: &str, tail: &str) -> Vec<String> {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("probe");
    probe_pack(&dir, &on_dock(body), tail);
    let (mut sim, host) = probe_sim(&dir);
    dock_once(&mut sim);
    assert_eq!(stage(&sim), 0, "an aborted call writes nothing");
    aborts(&host).into_iter().map(|(c, _)| c).collect()
}

#[test]
fn pairs_and_next_charge_for_the_table_they_walk() {
    // The reviewer's probe, scaled down: 200 walks of a 20,000-entry table.
    let walk = r#"
			local why = ctx:cause({ summary = "after the walks", cites = { ev.fact } })
			local t = table.create(20000, 1)
			for _ = 1, 200 do
				pcall(function() local _ = WALK end)
			end
			ctx.stations:set_stage(ev.station, 1, why)
		"#;
    for call in ["pairs(t)", "next(t)"] {
        let codes = abort_codes(&walk.replace("WALK", call), "");
        assert_eq!(codes, vec!["over_budget"], "{call}");
    }
    // An emptied table keeps its slots, and a walk still pays for them.
    let codes = abort_codes(
        r#"
			local t = table.create(60000, 1)
			for i = 1, 60000 do t[i] = nil end
			for _ = 1, 3 do
				local _ = next(t)
			end
		"#,
        "[limits]\nsteps_per_call = 250000",
    );
    assert_eq!(codes, vec!["over_budget"]);
    // Calling `next` over and over with the same key is charged too.
    let codes = abort_codes(
        r#"
			local t = table.create(60000, 1)
			for i = 2, 60000 do t[i] = nil end
			for _ = 1, 100 do
				local _ = next(t, 1)
			end
		"#,
        "[limits]\nsteps_per_call = 200000",
    );
    assert_eq!(codes, vec!["over_budget"]);
}

#[test]
fn a_pairs_loop_over_a_small_table_is_cheap() {
    // 500 loops over 50 entries: well within the default budget.
    let codes = abort_codes(
        r#"
			local t = {}
			for i = 1, 50 do t["k" .. i] = i end
			local sum = 0
			for _ = 1, 500 do
				for _, v in pairs(t) do sum += v end
			end
			print(sum)
		"#,
        "",
    );
    assert!(codes.is_empty(), "{codes:?}");
}

#[test]
fn sort_rep_and_other_native_helpers_charge_the_budget() {
    // Each sets up within the budget of 200,000 steps; only the helper's own charge
    // takes it over.
    let probes = [
        "local t = table.create(50000, 1) table.sort(t)",
        "local _ = string.rep(\"x\", 16 * 1024 * 1024)",
        "local t = table.create(50000, 1) for _ = 1, 20 do local _ = table.find(t, 2) end",
        "local t = table.create(50000, 1) for _ = 1, 20 do table.insert(t, 1, 2) end",
        "local s = string.rep(\"a\", 100000) for _ = 1, 300 do local _ = s:sub(1, -1) end",
        "local s = string.rep(\"a\", 100000) for _ = 1, 100 do local _ = s:find(\"aaaaaaab\", 1, true) end",
        "local t = table.create(30000, 1) for _ = 1, 20 do table.clear(t) end",
        "local b = buffer.create(100000) for _ = 1, 300 do buffer.fill(b, 0, 1) end",
    ];
    for probe in probes {
        let body = format!(
            "\t\t\tlocal why = ctx:cause({{ summary = \"after\", cites = {{ ev.fact }} }})\n\
             \t\t\tpcall(function() {probe} end)\n\
             \t\t\tctx.stations:set_stage(ev.station, 1, why)"
        );
        let codes = abort_codes(
            &body,
            "[limits]\nmemory_bytes = 33554432\nsteps_per_call = 200000",
        );
        assert_eq!(codes, vec!["over_budget"], "{probe}");
    }
}

// Non-blocking: the same call runs out of memory the same way, whatever ran before.

#[test]
fn memory_limits_dont_depend_on_earlier_calls() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("probe");
    probe_pack(
        &dir,
        &on_dock(
            r#"
			local fit = 0
			for k = 1024, 1, -4 do
				if pcall(string.rep, "x", k * 1024) then
					fit = k
					break
				end
			end
			local why = ctx:cause({ summary = "fit", cites = { ev.fact } })
			ctx.stations:set_stage(ev.station, fit, why)
		"#,
        ),
        "[limits]\nmemory_bytes = 1048576\nsteps_per_call = 1000000",
    );
    let (mut sim, _host) = probe_sim(&dir);
    let mut stages = Vec::new();
    for _ in 0..4 {
        dock_once(&mut sim);
        stages.push(stage(&sim));
    }
    // A fresh host on a copy of the state gets the same.
    let mut restored = Sim::new(sim.state().clone());
    let fresh = ScriptHost::new(None);
    fresh.load_unchecked(&dir).unwrap();
    restored.add_system(fresh.system());
    dock_once(&mut sim);
    dock_once(&mut restored);
    stages.push(stage(&sim));
    stages.push(stage(&restored));
    assert!(stages[0] > 0, "{stages:?}");
    assert!(stages.iter().all(|s| *s == stages[0]), "{stages:?}");
    assert_eq!(sim.hash(), restored.hash());
}

// Non-blocking: the host checks a call's writes against the world the kernel will
// apply them to, with every invariant of the sim.

/// Rejects a world where more than one station is at stage 5 or above.
struct OneBigStation;

impl galaxy_core::invariants::Invariant for OneBigStation {
    fn name(&self) -> &'static str {
        "one_big_station"
    }
    fn check(&self, world: &WorldState) -> Result<(), String> {
        let big = world
            .stations
            .values()
            .filter(|s| s.build.stage >= 5)
            .count();
        if big > 1 {
            Err(format!("{big} stations are at stage 5"))
        } else {
            Ok(())
        }
    }
}

#[test]
fn preview_sees_queued_batches_and_added_invariants() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("probe");
    probe_pack(
        &dir,
        &on_dock(
            r#"
			local why = ctx:cause({ summary = "grow elsewhere", cites = { ev.fact } })
			for _, id in ipairs(ctx.stations:list()) do
				if id ~= ev.station then
					ctx.stations:set_stage(id, 5, why)
					break
				end
			end
		"#,
        ),
        "",
    );
    let (mut sim, host) = probe_sim(&dir);
    sim.add_invariant(Box::new(OneBigStation));
    dock(&mut sim, &KETTLE);
    step(&mut sim, 1);
    // Queued ahead of the script's batch, in the tick the handler runs.
    sim.submit(
        CommandBatch::new(Cause::system("test", "kettle grows")).with(Command::SetStationStage {
            station: KETTLE,
            stage: 5,
        }),
    );
    // `step` asserts the kernel rejects nothing: the host caught it first. Valid on
    // the world the handler read, it lost to the queued batch: contention, retried.
    step(&mut sim, 1);
    assert_eq!(stage(&sim), 5);
    assert!(aborts(&host).is_empty());
    // Run again on the world with Kettle grown, the write breaks the invariant there
    // too: a fault.
    step(&mut sim, 1);
    let a = aborts(&host);
    assert_eq!(a.len(), 1, "{a:?}");
    assert_eq!(a[0].0, "rejected_write");
    assert!(a[0].1.contains("one_big_station"), "{}", a[0].1);
}

// Non-blocking: history sums can't overflow.

#[test]
fn a_history_sum_that_overflows_is_an_error() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("probe");
    probe_pack(
        &dir,
        &on_dock(
            r#"
			if ctx.stations:stage(ev.station) == 0 then
				local why = ctx:cause({ summary = "big amounts", cites = { ev.fact } })
				for _ = 1, 3 do
					ctx.history:record({ kind = "gm.probe.big", subjects = {}, amount = 4611686018427387904, note = "" }, why)
				end
				ctx.stations:set_stage(ev.station, 1, why)
			else
				print("sum", (pcall(function() return ctx.history:sum({ kind = "gm.probe.big" }) end)))
			end
		"#,
        ),
        "",
    );
    let (mut sim, host) = probe_sim(&dir);
    dock_once(&mut sim);
    assert_eq!(stage(&sim), 1);
    dock_once(&mut sim);
    assert_eq!(log(&host), vec!["sum\tfalse"]);
    let q = HistoryQuery::kind("gm.probe.big".parse().unwrap());
    assert_eq!(sim.history().sum(&q), None);
}
