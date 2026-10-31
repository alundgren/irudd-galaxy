//! Re-check 2 of PR #31, finding 1: history-only packs forged protected canon facts
//! and advanced the core journey. A pack records facts only in its own namespace (its
//! whole ID); engine and journey events never come from a pack's hooks; a fact that
//! reports a checked change (`canon_opened`, `mechanic_taught`, `market_base_changed`)
//! only comes right after that change. No beat precondition can be met by a forgery.

use crate::common;

use std::collections::BTreeSet;
use std::path::Path;

use common::pilot::{GDO, KETTLE, PLAYER, Pilot};
use common::{dock, slice};
use galaxy_contract::canon::CanonStatus;
use galaxy_contract::history::FactDraft;
use galaxy_contract::needs::{Caller, Route, check};
use galaxy_contract::pack::PackLayer;
use galaxy_contract::prelude::*;
use galaxy_contract::progress::BeatState;
use galaxy_script::host::HostEvent;
use galaxy_script::{CheckOptions, check_dir};

const B15: &str = "beat.b15_fragment";
const SINGS: &str = "canon.fragments_sing";

/// Every capability a pack can declare.
fn all_uses() -> Vec<String> {
    galaxy_contract::api::uses()
        .iter()
        .filter(|u| u.available)
        .map(|u| u.id.to_owned())
        .collect()
}

fn fact(kind: &str, other: Option<&str>) -> Command {
    Command::RecordFact {
        fact: Box::new(FactDraft {
            kind: kind.parse().unwrap(),
            subjects: Subjects {
                other: other.map(|o| AnyId::new(o).unwrap()),
                ..Subjects::default()
            },
            amount: None,
            note: "forged".into(),
        }),
    }
}

fn opens(node: &str) -> Command {
    Command::SetCanonStatus {
        id: node.into(),
        status: CanonStatus::Open,
    }
}

/// `commands` from `pack` (running as `layer`, declaring everything) along `route`.
fn try_as(pack: &str, layer: PackLayer, route: Route, commands: &[Command]) -> Result<(), String> {
    let pack: PackId = pack.parse().unwrap();
    let uses = all_uses();
    check(
        commands,
        &Caller {
            pack: &pack,
            uses: &uses,
            layer,
        },
        route,
    )
}

fn hooks(pack: &str, commands: &[Command]) -> Result<(), String> {
    let layer = if pack == "core" {
        PackLayer::Core
    } else {
        PackLayer::GameMaster
    };
    try_as(pack, layer, Route::Hooks, commands)
}

fn refused(r: Result<(), String>, needle: &str) {
    let why = r.expect_err("refused");
    assert!(why.contains(needle), "{why}");
}

/// The re-check's probe, as the table sees it: `core.review_probe` with
/// `history.write` (here, every capability) writing `core.canon_opened`.
#[test]
fn a_fact_outside_the_packs_own_namespace_is_refused() {
    let forged = fact("core.canon_opened", Some(SINGS));
    refused(
        hooks("core.review_probe", std::slice::from_ref(&forged)),
        "own namespace (`core.review_probe.`)",
    );
    refused(
        hooks("gm", std::slice::from_ref(&forged)),
        "own namespace (`gm.`)",
    );
    // Even after a real opening: the namespace is the whole ID.
    refused(
        hooks("core.review_probe", &[opens(SINGS), forged]),
        "own namespace",
    );
    // `gm.a` writes `gm.a.*`, never `gm.b.*` or a bare engine kind.
    hooks("gm.a", &[fact("gm.a.noticed", None)]).unwrap();
    // A protected name can't hide deeper in the pack's own namespace either: lines
    // cite kinds by their last segment.
    refused(
        hooks(
            "gm.a",
            &[opens(SINGS), fact("gm.a.b.canon_opened", Some(SINGS))],
        ),
        "protected name",
    );
    refused(
        hooks("gm.a", &[fact("gm.a.x.mechanic_taught", None)]),
        "protected name",
    );
    refused(
        hooks("gm.a", &[fact("gm.b.noticed", None)]),
        "own namespace",
    );
    refused(hooks("gm.a", &[fact("gm.noticed", None)]), "own namespace");
    for engine in ["dock", "beat_completed", "quest_step", "reply", "loop_lap"] {
        refused(hooks("core", &[fact(engine, None)]), "own namespace");
    }
    // `history.write` is what an ordinary own-namespace fact needs.
    let pack: PackId = "gm.a".parse().unwrap();
    refused(
        check(
            &[fact("gm.a.noticed", None)],
            &Caller {
                pack: &pack,
                uses: &["events.dock".to_owned()],
                layer: PackLayer::GameMaster,
            },
            Route::Hooks,
        ),
        "`history.write`",
    );
}

/// `history.write` never stands in for opening canon or teaching: the protected names
/// only come right after the change they report, whatever the pack declares.
#[test]
fn protected_facts_only_follow_the_change_they_report() {
    // canon_opened: right after the `set_canon_status` that opens the node it names.
    refused(
        hooks("core", &[fact("core.canon_opened", Some(SINGS))]),
        "only by `ctx.canon:open`",
    );
    refused(
        hooks(
            "core",
            &[
                opens("canon.fragment_is_lens_shard"),
                fact("core.canon_opened", Some(SINGS)),
            ],
        ),
        "only by `ctx.canon:open`",
    );
    hooks(
        "core",
        &[opens(SINGS), fact("core.canon_opened", Some(SINGS))],
    )
    .unwrap();
    hooks(
        "gm.ancients_bearing",
        &[
            opens("canon.second_fragment"),
            fact(
                "gm.ancients_bearing.canon_opened",
                Some("canon.second_fragment"),
            ),
        ],
    )
    .unwrap();
    // ... and it needs `canon.write`, not `history.write`.
    let core: PackId = "core".parse().unwrap();
    let history = vec!["history.write".to_owned()];
    let only_history = Caller {
        pack: &core,
        uses: &history,
        layer: PackLayer::Core,
    };
    refused(
        check(
            &[opens(SINGS), fact("core.canon_opened", Some(SINGS))],
            &only_history,
            Route::Hooks,
        ),
        "`canon.write`",
    );

    // mechanic_taught: right after its `mech.*` flag (and slots).
    let flag = Command::SetFlag {
        flag: "mech.tender".parse().unwrap(),
        on: true,
    };
    let slot = Command::BindSlot {
        slot: "slot.input.t".parse().unwrap(),
        mechanic: Some("mech.tender".parse().unwrap()),
    };
    refused(
        hooks("core", &[fact("core.mechanic_taught", Some("mech.tender"))]),
        "only by `ctx.mechanics:teach`",
    );
    refused(
        hooks(
            "core",
            &[
                Command::SetFlag {
                    flag: "mech.other".parse().unwrap(),
                    on: true,
                },
                fact("core.mechanic_taught", Some("mech.tender")),
            ],
        ),
        "only by `ctx.mechanics:teach`",
    );
    hooks(
        "core",
        &[
            flag,
            slot,
            fact("core.mechanic_taught", Some("mech.tender")),
        ],
    )
    .unwrap();
    refused(
        check(
            &[
                Command::SetFlag {
                    flag: "mech.tender".parse().unwrap(),
                    on: true,
                },
                fact("core.mechanic_taught", Some("mech.tender")),
            ],
            &only_history,
            Route::Hooks,
        ),
        "`mechanics.write`",
    );

    // market_base_changed: right after the `set_market_base` it reports.
    let mut base = fact("market_base_changed", None);
    if let Command::RecordFact { fact } = &mut base {
        fact.subjects.station = Some(KETTLE);
        fact.subjects.ware = Some("ware.ice".parse().unwrap());
        fact.amount = Some(1500);
    }
    refused(hooks("core", &[base]), "right after the `set_market_base`");
}

/// Engine and journey events never come from a pack's hooks; the beat runtime writes
/// the beat events for a core-layer pack only; the few a hook may emit come right
/// after the change they report.
#[test]
fn journey_and_engine_events_come_only_from_their_writers() {
    let beat: BeatId = B15.parse().unwrap();
    let completed = Command::Emit {
        event: Event::BeatCompleted { beat: beat.clone() },
    };
    let started = Command::Emit {
        event: Event::BeatStarted {
            beat: beat.clone(),
            act: 4,
        },
    };
    let put = Command::PutBeat {
        id: beat,
        value: None,
    };
    for c in [&completed, &started, &put] {
        refused(
            hooks("core", std::slice::from_ref(c)),
            "only the beat runtime",
        );
        try_as(
            "core",
            PackLayer::Core,
            Route::Beat,
            std::slice::from_ref(c),
        )
        .unwrap();
        refused(
            try_as(
                "gm",
                PackLayer::GameMaster,
                Route::Beat,
                std::slice::from_ref(c),
            ),
            "game-master layer never drives journey beats",
        );
        refused(
            try_as(
                "core",
                PackLayer::Core,
                Route::Data,
                std::slice::from_ref(c),
            ),
            "only the beat runtime",
        );
    }
    // A flag other than a mechanic is a beat reward, never a hook's.
    refused(
        hooks(
            "core",
            &[Command::SetFlag {
                flag: "cap.automation".parse().unwrap(),
                on: true,
            }],
        ),
        "only the beat runtime",
    );

    // Docking, trades and mining are the engine's.
    for event in [
        Event::Dock {
            ship: PLAYER,
            station: KETTLE,
        },
        Event::Undock {
            ship: PLAYER,
            station: KETTLE,
        },
        Event::Sell {
            ship: PLAYER,
            station: KETTLE,
            ware: "ware.ice".parse().unwrap(),
            quantity: 1,
            unit_price: 10,
        },
    ] {
        let e = Command::Emit { event };
        refused(hooks("core", std::slice::from_ref(&e)), "never emits");
        refused(
            try_as("core", PackLayer::Core, Route::Beat, &[e]),
            "never emits",
        );
    }

    // `deliver`: right after the cargo it hands over.
    let ice: WareId = "ware.ice".parse().unwrap();
    let deliver = Command::Emit {
        event: Event::Deliver {
            ship: PLAYER,
            station: KETTLE,
            ware: ice.clone(),
            quantity: 20,
            ledger: None,
        },
    };
    let cargo = |delta| Command::AdjustCargo {
        ship: PLAYER,
        ware: ice.clone(),
        delta,
    };
    refused(
        hooks("core", std::slice::from_ref(&deliver)),
        "follows the `adjust_cargo`",
    );
    refused(
        hooks("core", &[cargo(-5), deliver.clone()]),
        "follows the `adjust_cargo`",
    );
    hooks("core", &[cargo(-20), deliver]).unwrap();

    // `standing_changed` / `regard_changed`: right after the deed's change.
    let coop: FactionId = "faction.wharf_cooperative".parse().unwrap();
    let changed = |delta| Command::Emit {
        event: Event::StandingChanged {
            faction: coop.clone(),
            deed: AnyId::new("deed.forged").unwrap(),
            delta,
        },
    };
    refused(hooks("core", &[changed(80)]), "standing_changed");
    hooks(
        "core",
        &[
            Command::AdjustStanding {
                faction: coop.clone(),
                delta: 80,
            },
            changed(80),
        ],
    )
    .unwrap();
}

/// Every fact a core beat's preconditions read, collected from the beats as
/// registered: none can be written by another pack with every capability, and core
/// itself writes the protected ones only through their hooks.
#[test]
fn no_journey_precondition_can_be_met_by_a_forged_fact() {
    let sim = common::new_sim(1);
    let checked = check_dir(
        &slice().join("packs/core"),
        CheckOptions {
            world: Some(sim.world()),
            history: Some(sim.history()),
            ..Default::default()
        },
    );
    assert!(checked.report.accepted(), "{:#?}", checked.report);
    let vm = checked.vm.expect("core loads");
    let mut kinds = BTreeSet::new();
    for s in &vm.scripts {
        for b in &s.beats {
            collect_kinds(&serde_json::to_value(&b.def).unwrap(), &mut kinds);
        }
    }
    for k in [
        "core.canon_opened",
        "core.mechanic_taught",
        "undock",
        "reply",
    ] {
        assert!(kinds.contains(k), "{k} not among {kinds:?}");
    }
    let others = [
        ("core.review_probe", PackLayer::GameMaster),
        ("gm", PackLayer::GameMaster),
        ("gm.chronicle", PackLayer::GameMaster),
        ("coreish", PackLayer::Core),
    ];
    for kind in &kinds {
        let lone = [fact(kind, Some(SINGS))];
        for (pack, layer) in others {
            for route in [Route::Hooks, Route::Beat, Route::Data] {
                assert!(
                    try_as(pack, layer, route, &lone).is_err(),
                    "{pack} could write `{kind}` along {route:?}"
                );
            }
        }
        let name = kind.strip_prefix("core.");
        let core_may = name.is_some_and(|n| !galaxy_contract::needs::PROTECTED_FACTS.contains(&n));
        assert_eq!(
            hooks("core", &lone).is_ok(),
            core_may,
            "core recording `{kind}` on its own"
        );
    }
}

fn collect_kinds(v: &serde_json::Value, out: &mut BTreeSet<String>) {
    match v {
        serde_json::Value::Object(m) => {
            if let Some(k) = m
                .get("query")
                .and_then(|q| q.get("kind"))
                .and_then(|k| k.as_str())
            {
                out.insert(k.to_owned());
            }
            m.values().for_each(|v| collect_kinds(v, out));
        }
        serde_json::Value::Array(a) => a.iter().for_each(|v| collect_kinds(v, out)),
        _ => {}
    }
}

/// The re-check's probe at runtime, against the real core pack with b15 at its carry
/// step: a layer with `id = "core.review_probe"` and only `events.dock` and
/// `history.write` records `core.canon_opened` naming `canon.fragments_sing` on
/// docking. The game master's check refuses it, and run anyway it records nothing:
/// b15 stays at the carry step and the node stays sealed.
#[test]
fn a_history_only_layer_cant_open_canon_or_move_the_fragment_beat() {
    let dir = tempfile::tempdir().unwrap();
    write_probe(dir.path());
    let mut p = Pilot::with(15, |s| {
        s.world().progress.beats.insert(
            B15.parse().unwrap(),
            BeatState {
                pack: "core".parse().unwrap(),
                act: 4,
                kind: BeatKind::Directed,
                status: BeatStatus::Active,
                step: 1,
                step_id: Some("carry".into()),
                goal: "Nell asks you to carry the fragment through a lane, and listen.".into(),
                giver: None,
                since: None,
                step_since: None,
                done_since: None,
                started: Tick(0),
            },
        );
    });
    p.idle(2);
    let status = |p: &Pilot| p.world().canon.nodes[SINGS].status;
    let sealed = status(&p);
    assert_ne!(sealed, CanonStatus::Open);

    // The game master's checks refuse it: core's namespace isn't a layer's.
    let r = p
        .host
        .load_layer(dir.path(), p.sim.world(), p.sim.history());
    assert!(!r.accepted());
    let codes: Vec<_> = r.errors().map(|d| d.code.as_str()).collect();
    assert!(codes.contains(&"gm_core_id"), "{r:#?}");

    // Past every check, as a layer: the write is refused and nothing moves.
    p.host.load_unchecked(dir.path()).unwrap();
    p.sim.submit(
        CommandBatch::new(Cause::system("test", "layer")).with(Command::SetPackLayer {
            pack: "core.review_probe".parse().unwrap(),
            layer: Some(PackLayer::GameMaster),
        }),
    );
    // Stepped directly: the pilot treats any abort as a core failure, and the probe's
    // is expected.
    let mut aborts: Vec<String> = Vec::new();
    for station in [&GDO, &KETTLE, &GDO] {
        dock(&mut p.sim, station);
        for _ in 0..3 {
            p.sim.step(&PlayerInput::default());
        }
        for e in p.host.take_events() {
            if let HostEvent::Aborted {
                pack, diagnostic, ..
            } = e
            {
                assert_eq!(pack.as_str(), "core.review_probe", "{diagnostic:?}");
                aborts.push(diagnostic.code);
            }
        }
    }
    assert_eq!(p.step_of(B15), Some(1), "b15 stays at the carry step");
    assert_eq!(status(&p), sealed);
    assert!(p.facts("core.canon_opened").is_empty());
    assert!(
        aborts.iter().any(|c| c == "unprefixed_fact_kind"),
        "{aborts:?}"
    );
}

/// A layer recording its own `<id>.mechanic_taught` or `<id>.canon_opened` with
/// `ctx.history:record` is refused by the hook as well as by the table.
#[test]
fn history_record_refuses_the_protected_names() {
    let dir = tempfile::tempdir().unwrap();
    write_pack(
        dir.path(),
        "gm.teach_probe",
        &[
            "events.dock",
            "history.write",
            "mechanics.write",
            "canon.write",
        ],
        "ctx.history:record({ kind = \"gm.teach_probe.mechanic_taught\", subjects = { npc = \"npc.odo_fenn\", other = \"mech.tender\" }, note = \"forged\" }, why)",
    );
    let mut sim = common::new_sim(1);
    let host = galaxy_script::ScriptHost::new(None);
    host.load_unchecked(dir.path()).unwrap();
    host.attach(&mut sim);
    dock(&mut sim, &KETTLE);
    common::settle(&mut sim);
    let codes: Vec<String> = host
        .take_events()
        .into_iter()
        .filter_map(|e| match e {
            HostEvent::Aborted { diagnostic, .. } => Some(diagnostic.code),
            _ => None,
        })
        .collect();
    assert!(
        codes.iter().any(|c| c == "protected_fact_kind"),
        "{codes:?}"
    );
    assert!(
        !sim.history()
            .facts()
            .iter()
            .any(|f| f.kind.as_str().ends_with("mechanic_taught"))
    );
}

fn write_probe(dir: &Path) {
    write_pack(
        dir,
        "core.review_probe",
        &["events.dock", "history.write"],
        "ctx.history:record({ kind = \"core.canon_opened\", subjects = { other = \"canon.fragments_sing\" }, note = \"forged\" }, why)",
    );
}

fn write_pack(dir: &Path, id: &str, uses: &[&str], body: &str) {
    let uses = uses
        .iter()
        .map(|u| format!("\"{u}\""))
        .collect::<Vec<_>>()
        .join(", ");
    std::fs::create_dir_all(dir.join("scripts")).unwrap();
    std::fs::write(
        dir.join("manifest.toml"),
        format!(
            "id = \"{id}\"\nname = \"Probe\"\nversion = \"0.1.0\"\nuses = [{uses}]\n\
             [[script]]\nid = \"main\"\nfile = \"scripts/main.luau\"\nhandlers = [\"on.dock\"]\n"
        ),
    )
    .unwrap();
    std::fs::write(
        dir.join("scripts/main.luau"),
        format!(
            "--!strict\nlocal G = require(\"@galaxy\")\n\
             return G.script({{ id = \"main\", on = {{ dock = function(ev: G.DockEvent, ctx: G.Ctx)\n\
             \tlocal why = ctx:cause({{ summary = \"probe\", cites = {{ ev.fact }} }})\n\
             \t{body}\nend }} }})\n"
        ),
    )
    .unwrap();
}
