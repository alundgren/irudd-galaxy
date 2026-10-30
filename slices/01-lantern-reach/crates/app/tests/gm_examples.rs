//! Each GM layer is proposed with the actual CLI and sandbox on a suitable save.
use galaxy_contract::prelude::*;
use galaxy_core::{Save, Sim};
use serde_json::Value;
use std::{
    path::{Path, PathBuf},
    process::Command as Process,
};
#[path = "../../script/tests/common/mod.rs"]
mod common;
use common::pilot::Setup;
const NAMES: &[&str] = &[
    "shortage_tuning",
    "territory_map",
    "compact_plant",
    "rock_tagging",
    "ancients_bearing",
    "sixth_station",
    "chronicle",
];
fn slice() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}
fn cli(args: &[&str]) -> Value {
    let out = Process::new(env!("CARGO_BIN_EXE_galaxy"))
        .args(args)
        .env("GALAXY_PACKS", slice().join("packs"))
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).unwrap()
}
fn ready(name: &str, path: &Path) {
    let mut state = galaxy_core::new_game(&slice().join("packs/core/data"), 7).unwrap();
    let mut setup = Setup { state: &mut state };
    setup.done(&[
        "beat.b01_undock",
        "beat.b02_mine_by_hand",
        "beat.b03_first_delivery",
        "beat.b04_first_sale",
        "beat.b05_pause",
        "beat.b06_nav_chip",
        "beat.b07_cinder_drift",
        "beat.b08_chasing_a_gap",
        "beat.b09_hook_dispute",
        "beat.b10_rumor",
        "beat.b11_orders",
        "beat.b12_ice_shortage",
        "beat.b13_first_tender",
        "beat.b14_pilot_changes",
        "beat.b15_fragment",
        "beat.b16_hullwrights_question",
        "beat.b17_coming_home",
    ]);
    setup.counter(CounterKind::Dockings, 50);
    setup.counter(CounterKind::RocksDepleted, 10);
    setup.flags(&["cap.loops"]);
    let station = match name {
        "ancients_bearing" => common::pilot::GDO,
        "rock_tagging" => common::pilot::HOOK,
        _ => common::pilot::WHARF,
    };
    setup.docked_at(&station);
    if name == "shortage_tuning" {
        setup.world().progress.beats.remove("beat.b12_ice_shortage");
    }
    if name == "compact_plant" {
        setup.world().progress.beats.remove("beat.b10_rumor");
    }
    for id in ["canon.fragment_is_lens_shard", "canon.fragments_sing"] {
        state.world.canon.nodes.get_mut(id).unwrap().status =
            galaxy_contract::canon::CanonStatus::Open;
    }
    let mut sim = Sim::new(state);
    let rock = sim
        .world()
        .rocks
        .values()
        .find(|r| r.field.as_str() == "field.shallows.ice")
        .unwrap()
        .id
        .clone();
    let mut facts = CommandBatch::new(Cause::system(
        "example_setup",
        "the player's earlier journey",
    ));
    facts.push(Command::Emit {
        event: Event::Mine {
            ship: common::pilot::PLAYER,
            rock,
            ware: common::pilot::ICE,
            amount: 100,
        },
    });
    facts.push(Command::Emit {
        event: Event::Sell {
            ship: common::pilot::PLAYER,
            station: common::pilot::GDO,
            ware: common::pilot::ICE,
            quantity: 100,
            unit_price: 1000,
        },
    });
    facts.push(Command::RecordFact {
        fact: Box::new(FactDraft {
            kind: "core.dispute_tally".parse().unwrap(),
            subjects: Subjects {
                station: Some(common::pilot::HOOK),
                faction: Some("faction.wharf_cooperative".parse().unwrap()),
                ..Default::default()
            },
            amount: Some(30000),
            note: "The pilot supplied the Cooperative depot".into(),
        }),
    });
    sim.submit(facts);
    sim.step(&PlayerInput::default()).unwrap();
    sim.submit(
        CommandBatch::new(Cause::system("example_setup", "a calm docking")).with(Command::Emit {
            event: Event::Dock {
                ship: common::pilot::PLAYER,
                station,
            },
        }),
    );
    Save::create(path, sim.state()).unwrap();
}
#[test]
fn each_example_passes_strict_proposal_lands_and_rolls_back() {
    let dir = tempfile::tempdir().unwrap();
    for name in NAMES {
        eprintln!("checking {name}");
        let path = dir.path().join(format!("{name}.sqlite"));
        ready(name, &path);
        let original = Save::open(&path).unwrap().load().unwrap().world;
        let save = path.to_str().unwrap();
        let pack = slice().join("packs/gm-examples").join(name);
        let pack = pack.to_str().unwrap();
        let report = cli(&["pack", "check", pack, "--save", save]);
        assert!(!report.to_string().contains("typecheck skipped"));
        let report = cli(&["gm", "propose", pack, "--save", save]);
        for step in ["types", "canon", "sandbox", "legibility"] {
            assert!(
                report["steps"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|s| s["step"] == step && s["outcome"] == "passed"),
                "{name} {step}: {report}"
            );
        }
        cli(&["sim", "run", "--save", save, "--ticks", "4"]);
        let status = cli(&["gm", "status", "--save", save]);
        let change = status["applied"][0]["change"].as_str().unwrap();
        let before = Save::open(&path).unwrap().load().unwrap();
        assert_eq!(status["applied"][0]["state"], "running", "{status}");
        match *name {
            "shortage_tuning" => assert_eq!(
                before.world.progress.tunables["shortage.rimewater_ice.way_a_quota"].value,
                TunableValue::Number(50.0)
            ),
            "territory_map" => assert_eq!(before.world.map_overlays.len(), 4),
            "compact_plant" => assert!(matches!(
                before.world.progress.tunables["rumor.ember_shoals.override"].value,
                TunableValue::List(_)
            )),
            "ancients_bearing" => assert_eq!(
                before.world.canon.nodes["canon.second_fragment"].status,
                galaxy_contract::canon::CanonStatus::Open
            ),
            "sixth_station" => assert!(before.world.stations.contains("gm.sixth_station.halt")),
            "chronicle" => assert!(
                before
                    .world
                    .talk
                    .on(galaxy_contract::talk::Channel::Chronicle)
                    .next()
                    .is_some()
            ),
            "rock_tagging" => assert!(
                before
                    .world
                    .talk
                    .on(galaxy_contract::talk::Channel::DockTalk)
                    .any(|l| l.line.text.contains("Juno"))
            ),
            _ => unreachable!(),
        }
        if *name == "sixth_station"
            && let Some(path) = std::env::var_os("GALAXY_EXAMPLE_SHOT_SAVE")
        {
            let mut shot = before.clone();
            Setup { state: &mut shot }.docked_at(&"gm.sixth_station.halt".parse().unwrap());
            Save::create(Path::new(&path), &shot).unwrap();
        }
        cli(&["gm", "rollback", change, "--save", save]);
        let after = Save::open(&path).unwrap().load().unwrap();
        assert!(after.world.map_overlays.is_empty(), "{name}");
        assert_eq!(
            after.world.canon.nodes["canon.second_fragment"].status,
            galaxy_contract::canon::CanonStatus::Sealed
        );
        if *name == "sixth_station" {
            let station = after.world.stations.get("gm.sixth_station.halt").unwrap();
            assert!(station.services.is_empty());
            assert_eq!(station.build.stage, 0);
            assert!(!after.world.markets.contains("gm.sixth_station.halt"));
            assert!(!after.world.npcs.contains("gm.sixth_station.wynn"));
        }
        if *name == "shortage_tuning" {
            for id in [
                "shortage.rimewater_ice.stations",
                "shortage.rimewater_ice.way_a_quota",
            ] {
                assert_eq!(
                    after.world.progress.tunables[id].value,
                    original.progress.tunables[id].value
                );
            }
        }
        if *name == "compact_plant" {
            assert_eq!(
                after.world.progress.tunables["rumor.ember_shoals.override"].value,
                original.progress.tunables["rumor.ember_shoals.override"].value
            );
            assert!(after.world.survey_buoys.is_empty());
        }
        if *name == "rock_tagging" {
            assert!(!after.world.ships.contains("gm.rock_tagging.magpie"));
            assert!(after.world.rock_tags.is_empty());
        }
        assert!(
            after
                .world
                .talk
                .on(galaxy_contract::talk::Channel::Chronicle)
                .next()
                .is_some(),
            "rollback is remembered"
        );
    }
}
