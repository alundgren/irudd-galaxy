//! Happenings (issue #47, 4.1), played headless through the game's real systems and
//! the core pack: a happening waits for its cause facts, starts when the player docks
//! at its station, its speaker says its line, it runs for its number of counted
//! dockings there (a dock-tap doesn't count) and then ends. One at a time per station.
//! Kettle's first harvest pays off only on the player's Accept, with a receipt.

use crate::common;

use common::pilot::*;
use galaxy_contract::happening::HappeningRun;
use galaxy_contract::needs::{Caller, Route, check};
use galaxy_contract::pack::PackLayer;
use galaxy_contract::prelude::*;

const LAMP_NIGHT: &str = "happening.kettle.lamp_night";
const HARVEST: &str = "happening.kettle.first_harvest";
const WHARF_NIGHT: &str = "happening.wharf.lamp_night";
const ODO: &str = "npc.odo_fenn";
const RATIONS: WareId = WareId::from_static("ware.rations");

/// The gm-examples greenhouse building a stage at Kettle, as its fact.
fn built(p: &mut Pilot, stage: i64) {
    p.sim.submit(
        CommandBatch::new(Cause::system("test", "Kettle's greenhouse grows")).with(
            Command::RecordFact {
                fact: Box::new(FactDraft {
                    kind: "gm.kettle_growth.stage_built".parse().unwrap(),
                    subjects: Subjects {
                        station: Some(KETTLE),
                        ..Subjects::default()
                    },
                    amount: Some(stage),
                    note: format!("Kettle Dock stage {stage}"),
                }),
            },
        ),
    );
    p.idle(2);
}

fn running(p: &Pilot, station: &StationId) -> Option<String> {
    let active = galaxy_progress::happenings::active(p.world(), station);
    assert!(active.len() <= 1, "one at a time: {active:?}");
    active.first().map(|h| h.id.to_string())
}

fn run_of(p: &Pilot, id: &str) -> HappeningRun {
    p.world().happenings.get(id).unwrap().run.clone()
}

fn dockings(p: &Pilot, station: &StationId) -> u64 {
    galaxy_core::happenings::dockings_at(p.world(), station)
}

/// Docks at the Wharf, then at Kettle: one more counted docking at Kettle.
fn round_trip(p: &mut Pilot) {
    p.dock_by_hand(&WHARF);
    p.dock_by_hand(&KETTLE);
}

#[test]
fn a_happening_runs_for_its_counted_dockings_and_then_ends() {
    let mut p = Pilot::new(7);
    built(&mut p, 2);
    // Its cause exists, but it starts only where the player is docked.
    assert!(run_of(&p, LAMP_NIGHT).is_upcoming());
    assert_eq!(running(&p, &KETTLE), None);

    p.dock_by_hand(&KETTLE);
    assert_eq!(running(&p, &KETTLE).as_deref(), Some(LAMP_NIGHT));
    assert!(p.heard(ODO, "strung lamps along the second ring"));
    let started = p.facts("happening_started");
    assert_eq!(started.len(), 1);
    assert_eq!(started[0].subjects.station, Some(KETTLE));
    assert_eq!(
        started[0].subjects.other.as_ref().map(|o| o.as_str()),
        Some(LAMP_NIGHT)
    );
    // The line cites the fact that caused it.
    let line = p
        .talk()
        .into_iter()
        .find(|l| l.line.text.contains("strung lamps"))
        .unwrap()
        .line
        .clone();
    let cited = p.sim.history().fact(line.cites[0]).unwrap();
    assert_eq!(cited.kind.as_str(), "gm.kettle_growth.stage_built");

    // A dock-tap is a docking, but not a counted one: it doesn't burn the happening.
    let before = dockings(&p, &KETTLE);
    p.dock_by_hand(&KETTLE);
    assert_eq!(
        dockings(&p, &KETTLE),
        before,
        "a re-dock in place doesn't count"
    );
    assert_eq!(running(&p, &KETTLE).as_deref(), Some(LAMP_NIGHT));

    // The second and third counted dockings still show it; the fourth ends it.
    round_trip(&mut p);
    assert_eq!(running(&p, &KETTLE).as_deref(), Some(LAMP_NIGHT));
    round_trip(&mut p);
    assert_eq!(running(&p, &KETTLE).as_deref(), Some(LAMP_NIGHT));
    assert!(!p.has_fact("happening_ended"));
    round_trip(&mut p);
    assert_eq!(running(&p, &KETTLE), None);
    assert!(matches!(run_of(&p, LAMP_NIGHT), HappeningRun::Ended { .. }));
    let ended = p.facts("happening_ended");
    assert_eq!(ended.len(), 1);
    assert_eq!(
        galaxy_progress::happenings::ended(p.world(), &KETTLE)
            .first()
            .map(|h| h.id.as_str()),
        Some(LAMP_NIGHT)
    );
    // Silent: Odo said nothing more about it.
    let lamp_lines = p
        .talk()
        .iter()
        .filter(|l| l.line.text.contains("strung lamps"))
        .count();
    assert_eq!(lamp_lines, 1);
    p.assert_healthy();
}

#[test]
fn one_happening_at_a_time_per_station_and_the_next_waits() {
    let mut p = Pilot::new(8);
    built(&mut p, 2);
    built(&mut p, 3);
    p.dock_by_hand(&KETTLE);
    // Both are due at Kettle; the first in ID order runs, the other waits.
    assert_eq!(running(&p, &KETTLE).as_deref(), Some(HARVEST));
    assert!(run_of(&p, LAMP_NIGHT).is_upcoming());
    assert_eq!(
        galaxy_progress::happenings::upcoming(p.world(), &KETTLE)
            .iter()
            .map(|h| h.id.as_str())
            .collect::<Vec<_>>(),
        [LAMP_NIGHT]
    );
    // The Wharf is its own station: its lamp night runs alongside.
    p.dock_by_hand(&WHARF);
    assert_eq!(running(&p, &WHARF).as_deref(), Some(WHARF_NIGHT));
    p.dock_by_hand(&KETTLE);
    round_trip(&mut p);
    assert_eq!(running(&p, &KETTLE).as_deref(), Some(HARVEST));
    // The harvest's fourth docking ends it, and the lamp night starts in its place.
    round_trip(&mut p);
    assert!(matches!(run_of(&p, HARVEST), HappeningRun::Ended { .. }));
    assert_eq!(running(&p, &KETTLE).as_deref(), Some(LAMP_NIGHT));
    assert!(p.heard(ODO, "strung lamps"));
    // The Wharf's ran its two dockings meanwhile.
    assert_eq!(running(&p, &WHARF), None);
    assert!(matches!(
        run_of(&p, WHARF_NIGHT),
        HappeningRun::Ended { .. }
    ));
    p.assert_healthy();
}

#[test]
fn the_first_harvest_gives_a_crate_only_on_accept_with_a_receipt() {
    let mut p = Pilot::new(9);
    built(&mut p, 3);
    let credits = p.credits();
    p.dock_by_hand(&KETTLE);
    assert_eq!(running(&p, &KETTLE).as_deref(), Some(HARVEST));
    // Odo asks; nothing moves until the player answers.
    let ask = p
        .talk()
        .into_iter()
        .find(|l| l.line.text.contains("First harvest"))
        .unwrap()
        .line
        .clone();
    let choices: Vec<_> = ask.choices.iter().map(|c| c.id.as_str()).collect();
    assert_eq!(choices, ["choice.harvest_take", "choice.harvest_later"]);
    assert_eq!(p.cargo(&RATIONS), 0);

    // Not now: nothing changes, and Odo asks again at the next docking.
    p.reply(ODO, "choice.harvest_later");
    assert_eq!(p.cargo(&RATIONS), 0);
    assert!(p.heard(ODO, "It'll keep"));
    round_trip(&mut p);
    assert!(p.heard(ODO, "still put by for you"));
    assert_eq!(p.cargo(&RATIONS), 0);

    // Accept: the crate comes aboard as the pilot's own, with a receipt.
    p.reply(ODO, "choice.harvest_take");
    assert_eq!(p.cargo(&RATIONS), 4);
    assert_eq!(p.credits(), credits, "a gift: no credits move");
    assert!(p.has_fact("core.harvest_taken"));
    assert!(
        p.talk().iter().any(|l| l.line.speaker.is_none()
            && l.line.text == "Odo gave you 4 rations from the first harvest · a gift"),
        "the receipt says what came aboard"
    );
    // Once per save.
    p.reply(ODO, "choice.harvest_take");
    assert_eq!(p.cargo(&RATIONS), 4);
    round_trip(&mut p);
    assert!(
        p.talk()
            .iter()
            .filter(|l| l.line.text.contains("still put by"))
            .count()
            == 1,
        "no more asking once it's taken"
    );
    p.assert_healthy();
}

#[test]
fn a_game_master_pack_writes_a_happening_that_cites_an_existing_fact() {
    let mut p = Pilot::new(10);
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/happening_probe");
    let report = p.host.load_layer(&dir, p.sim.world(), p.sim.history());
    assert!(report.accepted(), "{report:#?}");
    p.dock_by_hand(&KETTLE);
    p.dock_by_hand(&WHARF);
    let h = p
        .world()
        .happenings
        .get("gm.happening_probe.crowd")
        .expect("the pack wrote it")
        .clone();
    assert_eq!(
        h.by.as_ref().map(|b| b.as_str()),
        Some("gm.happening_probe")
    );
    let cited = h.cause.facts[0].id.unwrap();
    assert_eq!(p.sim.history().fact(cited).unwrap().kind.as_str(), "dock");
    // Its cause exists and the player is docked there with nothing running, so it
    // starts in this docking, and runs for its one docking.
    assert_eq!(
        running(&p, &WHARF).as_deref(),
        Some("gm.happening_probe.crowd")
    );
    assert!(p.heard("npc.ilse_marrow", "a crowd on the quay"));
    p.dock_by_hand(&KETTLE);
    p.dock_by_hand(&WHARF);
    assert_eq!(running(&p, &WHARF), None);
    assert!(matches!(
        run_of(&p, "gm.happening_probe.crowd"),
        HappeningRun::Ended { .. }
    ));
}

#[test]
fn no_pack_records_the_runtimes_facts_or_writes_happenings_without_the_capability() {
    let pack: PackId = "gm.a".parse().unwrap();
    let all: Vec<String> = galaxy_contract::api::USES
        .iter()
        .map(|u| u.id.to_owned())
        .collect();
    let caller = Caller {
        pack: &pack,
        uses: &all,
        layer: PackLayer::GameMaster,
    };
    for kind in ["gm.a.happening_started", "gm.a.b.happening_ended"] {
        let forged = Command::RecordFact {
            fact: Box::new(FactDraft {
                kind: kind.parse().unwrap(),
                subjects: Subjects::default(),
                amount: None,
                note: String::new(),
            }),
        };
        let why = check(&[forged], &caller, Route::Hooks).unwrap_err();
        assert!(why.contains("protected name"), "{why}");
    }
    let put = Command::PutHappening {
        id: "gm.a.crowd".parse().unwrap(),
        value: None,
    };
    let none: Vec<String> = Vec::new();
    let bare = Caller {
        pack: &pack,
        uses: &none,
        layer: PackLayer::GameMaster,
    };
    let why = check(std::slice::from_ref(&put), &bare, Route::Hooks).unwrap_err();
    assert!(why.contains("happenings.write"), "{why}");
    check(&[put], &caller, Route::Hooks).unwrap();
}

// --- Review of the happenings runtime (e0f16dc): starts without a fact, and races ---

const CROWD: &str = "gm.happening_edit.crowd";

/// A new game with the flagship docked at the Wharf, and `crowd` waiting there if given.
fn docked_at_wharf(crowd: Option<galaxy_contract::happening::Happening>) -> galaxy_core::Sim {
    let mut state = galaxy_core::new_game(&common::core_data(), 10).unwrap();
    state.world.ships.get_mut(&PLAYER).unwrap().docked_at = Some(WHARF);
    if let Some(h) = crowd {
        state.world.happenings.insert(h.id.clone(), h);
    }
    galaxy_core::game(state)
}

/// An upcoming happening at the Wharf, caused by any docking there.
fn crowd() -> galaxy_contract::happening::Happening {
    use galaxy_contract::happening::{CauseFact, Happening, HappeningCause, HappeningLine, Lasts};
    Happening {
        id: CROWD.parse().unwrap(),
        station: WHARF,
        show: vec![],
        lasts: Lasts { dockings: 1 },
        cause: HappeningCause {
            facts: vec![CauseFact {
                kind: Some("dock".into()),
                station: Some(WHARF),
                ..CauseFact::default()
            }],
        },
        line: HappeningLine {
            id: "line.ilse_marrow.edit_crowd".into(),
            speaker: "npc.ilse_marrow".parse().unwrap(),
            text: "I've kept the old line.".into(),
            choices: vec![],
        },
        by: None,
        run: HappeningRun::Upcoming,
    }
}

/// A pack that, on a docking at the Wharf, rewrites the crowd's line while it waits.
fn edit_pack(dir: &std::path::Path) {
    std::fs::create_dir_all(dir.join("scripts")).unwrap();
    std::fs::write(
        dir.join("manifest.toml"),
        "id = \"gm.happening_edit\"\nname = \"Happening edit\"\nversion = \"0.1.0\"\n\
         uses = [\"events.dock\", \"happenings.read\", \"happenings.write\", \"talk.write\"]\n\n\
         [[script]]\nid = \"edit\"\nfile = \"scripts/edit.luau\"\nhandlers = [\"on.dock\"]\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("scripts/edit.luau"),
        r#"--!strict
local G = require("@galaxy")
return G.script({ id = "edit", on = { dock = function(ev: G.DockEvent, ctx: G.Ctx)
    if ev.station ~= "station.lantern_wharf" then return end
    local h = ctx.happenings:get("gm.happening_edit.crowd")
    if h == nil or (h.run ~= nil and h.run.status ~= "upcoming") then return end
    h.by = nil
    h.run = nil
    h.line.text = "I've updated the line."
    ctx.happenings:put(h, ctx:cause({ summary = "the crowd's news changes", cites = { ev.fact } }))
end } })
"#,
    )
    .unwrap();
}

/// The docking at the Wharf, committed this tick.
fn wharf_docking(sim: &mut galaxy_core::Sim) {
    common::dock(sim, &WHARF);
    common::step(sim, 1);
}

fn crowd_of(sim: &galaxy_core::Sim, id: &str) -> galaxy_contract::happening::Happening {
    sim.world().happenings.get(id).unwrap().clone()
}

#[test]
fn a_happening_a_hook_writes_starts_the_next_tick_with_nothing_else_happening() {
    let mut sim = docked_at_wharf(None);
    let host = galaxy_script::ScriptHost::new(None);
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/happening_probe");
    let report = host.load_layer(&dir, sim.world(), sim.history());
    assert!(report.accepted(), "{report:#?}");
    // The game's order: the script host, then progress.
    host.attach(&mut sim);
    sim.add_system(Box::new(galaxy_progress::ProgressSystem::default()));
    common::step(&mut sim, 1);
    wharf_docking(&mut sim);
    // The hook writes it, citing the docking: no fact comes with that.
    common::step(&mut sim, 1);
    let id = "gm.happening_probe.crowd";
    assert!(crowd_of(&sim, id).run.is_upcoming());
    let facts = sim.history().len();
    // Its cause exists and the player is docked there, so it starts the next tick,
    // although nothing else happened.
    common::step(&mut sim, 1);
    assert!(
        crowd_of(&sim, id).is_active(),
        "{:?}",
        crowd_of(&sim, id).run
    );
    let started = &sim.history().facts()[facts..];
    assert_eq!(
        started.iter().map(|f| f.kind.as_str()).collect::<Vec<_>>(),
        ["happening_started"]
    );
    assert!(
        common::lines(&sim, galaxy_contract::talk::Channel::DockTalk)
            .iter()
            .any(|l| l.line.text.contains("a crowd on the quay"))
    );
}

#[test]
fn a_pack_edit_to_a_waiting_happening_is_not_overwritten_when_it_starts() {
    let tmp = tempfile::tempdir().unwrap();
    edit_pack(tmp.path());
    let mut sim = docked_at_wharf(Some(crowd()));
    let host = galaxy_script::ScriptHost::new(None);
    let report = host.load_layer(tmp.path(), sim.world(), sim.history());
    assert!(report.accepted(), "{report:#?}");
    host.attach(&mut sim);
    sim.add_system(Box::new(galaxy_progress::ProgressSystem::default()));
    common::step(&mut sim, 1);
    wharf_docking(&mut sim);
    // The docking is its cause; the same tick the pack rewrites its line. The start
    // waits a tick for the new line rather than write the old one over it.
    common::step(&mut sim, 1);
    let h = crowd_of(&sim, CROWD);
    assert_eq!(h.line.text, "I've updated the line.");
    assert!(h.run.is_upcoming());
    common::step(&mut sim, 1);
    let h = crowd_of(&sim, CROWD);
    assert!(h.is_active(), "{:?}", h.run);
    assert_eq!(h.line.text, "I've updated the line.");
    let said = common::lines(&sim, galaxy_contract::talk::Channel::DockTalk);
    assert!(said.iter().any(|l| l.line.text == "I've updated the line."));
    assert!(
        !said
            .iter()
            .any(|l| l.line.text == "I've kept the old line.")
    );
}

#[test]
fn a_pack_writing_after_a_start_in_the_same_tick_loses_instead_of_undoing_it() {
    let tmp = tempfile::tempdir().unwrap();
    edit_pack(tmp.path());
    let mut sim = docked_at_wharf(Some(crowd()));
    let host = galaxy_script::ScriptHost::new(None);
    let report = host.load_layer(tmp.path(), sim.world(), sim.history());
    assert!(report.accepted(), "{report:#?}");
    // Progress before the host: its start applies first.
    sim.add_system(Box::new(galaxy_progress::ProgressSystem::default()));
    host.attach(&mut sim);
    common::step(&mut sim, 1);
    wharf_docking(&mut sim);
    // The pack's rewrite, worked out from the waiting record, loses to the start.
    sim.step(&PlayerInput::default()).unwrap();
    let h = crowd_of(&sim, CROWD);
    assert!(h.is_active(), "the start stands: {:?}", h.run);
    assert_eq!(h.line.text, "I've kept the old line.");
    // The pack's handler runs again, finds it started, and leaves it.
    common::step(&mut sim, 2);
    let h = crowd_of(&sim, CROWD);
    assert!(h.is_active(), "{:?}", h.run);
    assert_eq!(h.line.text, "I've kept the old line.");
}
