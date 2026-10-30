mod common;
use common::*;
use galaxy_contract::{canon::CanonStatus, pack::StepOutcome, prelude::*};
use galaxy_script::{CheckOptions, check_dir, read_pack};
#[test]
fn every_example_has_strict_types_and_valid_hooks() {
    let sim = new_sim(1);
    let analyzer = galaxy_script::analyzer::find(None).expect("cargo xtask fetch-luau");
    for name in [
        "shortage_tuning",
        "territory_map",
        "compact_plant",
        "rock_tagging",
        "ancients_bearing",
        "sixth_station",
        "chronicle",
    ] {
        let r = check_dir(
            &examples().join(name),
            CheckOptions {
                world: Some(sim.world()),
                history: Some(sim.history()),
                analyzer: Some(&analyzer),
                require_typecheck: true,
            },
        )
        .report;
        assert!(r.accepted(), "{name}: {r:#?}");
    }
}
#[test]
fn canon_refuses_forbidden_wrong_opener_and_early_revelations() {
    let mut sim = new_sim(1);
    let mut pack = read_pack(&examples().join("ancients_bearing")).unwrap();
    assert_eq!(
        galaxy_script::canon::check(&pack, sim.world(), true).outcome,
        StepOutcome::Failed
    );
    pack.manifest.opens = vec!["canon.no_living_ancients".into()];
    assert_eq!(
        galaxy_script::canon::check(&pack, sim.world(), false).outcome,
        StepOutcome::Failed
    );
    pack.manifest.opens = vec!["canon.fragments_sing".into()];
    assert_eq!(
        galaxy_script::canon::check(&pack, sim.world(), false).outcome,
        StepOutcome::Failed
    );
    pack.manifest.opens.clear();
    pack.manifest.asserts = vec!["canon.lens_purpose".into()];
    assert_eq!(
        galaxy_script::canon::check(&pack, sim.world(), false).outcome,
        StepOutcome::Failed
    );
    pack.manifest.asserts = vec!["canon.lantern_makers".into()];
    assert_eq!(
        galaxy_script::canon::check(&pack, sim.world(), false).outcome,
        StepOutcome::Passed
    );
    let before = sim.world().canon.clone();
    sim.submit(
        CommandBatch::new(Cause::script(
            pack_id("gm.test"),
            "one",
            "open a permitted test node",
        ))
        .with(Command::SetCanonStatus {
            id: "canon.second_fragment".into(),
            status: CanonStatus::Open,
        }),
    );
    step(&mut sim, 1);
    let causes = sim.causes_where(|c| matches!(c.cause.kind, CauseKind::Script { .. }));
    sim.queue_rollback(&causes, "Nell sets her notes aside");
    step(&mut sim, 1);
    assert_eq!(sim.world().canon, before);
}
#[test]
fn the_sixth_station_recipe_passes_the_shared_bible_and_bad_paint_does_not() {
    let source = std::fs::read_to_string(examples().join("sixth_station/lib/recipe.luau")).unwrap();
    let json = source
        .split("[=[")
        .nth(1)
        .unwrap()
        .split("]=]")
        .next()
        .unwrap();
    let mut recipe: galaxy_kit::StationRecipe = serde_json::from_str(json).unwrap();
    assert_eq!(
        recipe.validate_art(galaxy_kit::art_bible::ArtBible::bundled()),
        Ok(())
    );
    recipe.palette.hull = galaxy_kit::color::Color::hex(0x808080);
    assert!(
        recipe
            .validate_art(galaxy_kit::art_bible::ArtBible::bundled())
            .is_err()
    );
}

fn late_pilot(name: &str, setup: impl FnOnce(&mut pilot::Setup<'_>)) -> pilot::Pilot {
    let mut p = pilot::Pilot::with(7, |s| {
        s.done(&[
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
        setup(s);
    });
    assert!(
        p.host
            .load(&examples().join(name), p.world(), p.sim.history())
            .accepted()
    );
    let station = p.docked().cloned().unwrap();
    p.sim.submit(
        CommandBatch::new(Cause::system("test", "a calm docking")).with(Command::Emit {
            event: Event::Dock {
                ship: PLAYER,
                station,
            },
        }),
    );
    p
}

fn place_player(p: &mut pilot::Pilot, position: Vec2, dock: Option<StationId>) {
    let mut ship = p.ship().clone();
    ship.position = position;
    ship.velocity = Vec2::ZERO;
    ship.region = pilot::CINDER;
    ship.docked_at = dock.clone();
    p.sim.submit(
        CommandBatch::new(Cause::system("test", "arrive at the test site")).with(
            Command::PutShip {
                id: ship.id.clone(),
                value: Some(Box::new(ship)),
            },
        ),
    );
    p.idle(1);
    if let Some(station) = dock {
        p.sim.submit(
            CommandBatch::new(Cause::system("test", "dock at the test site")).with(Command::Emit {
                event: Event::Dock {
                    ship: PLAYER,
                    station,
                },
            }),
        );
        p.idle(3);
    }
}

fn undo_layer(p: &mut pilot::Pilot, name: &str) {
    let pack = read_pack(&examples().join(name)).unwrap();
    let causes = galaxy_script::rollback::causes_of(&p.sim, &pack.manifest.id, &pack.revision);
    assert!(!causes.is_empty());
    galaxy_script::rollback::rollback(&mut p.sim, &causes, pack.manifest.rollback.as_ref());
    p.idle(3);
    assert_eq!(p.rejected, 0, "{:?}", p.rejections);
}

#[test]
fn juno_demonstrates_before_teaching_and_the_lamps_roll_back() {
    let mut p = late_pilot("rock_tagging", |s| {
        s.flags(&["cap.loops", "cap.queues", "cap.click_mine"]);
        s.counter(CounterKind::RocksDepleted, 10);
        s.docked_at(&pilot::HOOK);
    });
    p.idle(4);
    p.reply("npc.juno_brask", "gm.rock_tagging.learn");
    assert!(!p.world().player.flags.contains("mech.rock_tagging"));
    p.wait("Magpie's lamp", |p| {
        p.sim
            .history()
            .last(&galaxy_contract::history::HistoryQuery {
                kind: Some("tagged".parse().unwrap()),
                ship: Some("gm.rock_tagging.magpie".parse().unwrap()),
                ..Default::default()
            })
            .is_some()
    });
    let at = p.world().stations.get(&pilot::HOOK).unwrap().position;
    place_player(&mut p, at, Some(pilot::HOOK));
    p.reply("npc.juno_brask", "gm.rock_tagging.learn");
    assert!(p.world().player.flags.contains("mech.rock_tagging"));
    let rock = p.world().rocks.get("rock.cinder.iron.02").unwrap().clone();
    place_player(&mut p, rock.position, None);
    p.tick(&PlayerInput {
        tag: true,
        ..Default::default()
    });
    assert!(p.world().rock_tags.contains(&rock.id));
    p.orders(vec![galaxy_contract::verbs::Order::MineField {
        field: rock.field.clone(),
        prefer_tagged: true,
    }]);
    p.wait("tagged mining produces ore", |p| p.cargo(&pilot::ORE) >= 3);
    assert!(
        p.ship()
            .activity
            .mining
            .as_ref()
            .is_some_and(|m| p.world().rock_tags.contains(&m.rock))
    );
    undo_layer(&mut p, "rock_tagging");
    assert!(p.world().rock_tags.is_empty());
    assert!(!p.world().player.flags.contains("mech.rock_tagging"));
    assert!(!p.world().ships.contains("gm.rock_tagging.magpie"));
}

#[test]
fn the_buoy_exposes_the_plant_and_earned_rewards_survive_rollback() {
    let mut p = late_pilot("compact_plant", |s| {
        s.world().rumors.insert(
            "rumor.ember_shoals_vein".parse().unwrap(),
            Rumor {
                id: "rumor.ember_shoals_vein".parse().unwrap(),
                claim: "The Shoals have glassite".into(),
                is_true: false,
                planted_by: Some("faction.tollway_compact".parse().unwrap()),
                heard_at: [pilot::BRASS].into_iter().collect(),
                heard_by_player: true,
                source: None,
            },
        );
    });
    p.idle(4);
    let buoy = p.world().survey_buoys["gm.compact_plant.buoy"].clone();
    place_player(&mut p, buoy.position, None);
    p.press();
    assert!(p.world().survey_buoys["gm.compact_plant.buoy"].read);
    let at = p.world().stations.get(&pilot::HOOK).unwrap().position;
    place_player(&mut p, at, Some(pilot::HOOK));
    let money = p.credits();
    p.reply("npc.rook_adeyemi", "gm.compact_plant.claim");
    assert_eq!(p.credits(), money + 300);
    let standing = p.world().factions.clone();
    p.reply("npc.rook_adeyemi", "gm.compact_plant.claim");
    assert_eq!(p.credits(), money + 300, "proof is paid only once");
    undo_layer(&mut p, "compact_plant");
    assert_eq!(p.credits(), money + 300);
    assert_eq!(p.world().factions, standing);
    assert!(p.world().survey_buoys.is_empty());
}

#[test]
fn shortage_history_tracks_mined_lots_through_purchases_and_transfers() {
    let mut p = late_pilot("shortage_tuning", |s| {
        s.world().progress.beats.remove("beat.b12_ice_shortage");
        s.tunable("shortage.rimewater_ice.mined", 100_000.0);
    });
    let mut batch = CommandBatch::new(Cause::system("test", "the pilot's cargo history"));
    for (kind, ship, other, station, rock, amount) in [
        ("buy", "ship.player", None, Some(pilot::BRASS), None, 80),
        (
            "mine",
            "ship.player",
            None,
            None,
            Some("rock.shallows.ice.01"),
            100,
        ),
        (
            "transfer",
            "ship.player",
            Some("craft.cargo_test"),
            None,
            None,
            100,
        ),
        ("sell", "ship.player", None, Some(pilot::GDO), None, 80),
        (
            "transfer",
            "craft.cargo_test",
            Some("ship.player"),
            None,
            None,
            100,
        ),
        ("sell", "ship.player", None, Some(pilot::WHARF), None, 100),
    ] {
        batch.push(Command::RecordFact {
            fact: Box::new(FactDraft {
                kind: kind.parse().unwrap(),
                subjects: Subjects {
                    ship: Some(ship.parse().unwrap()),
                    other: other.map(|s| s.parse().unwrap()),
                    station,
                    ware: Some(ICE),
                    rock: rock.map(|s| s.parse().unwrap()),
                    ..Default::default()
                },
                amount: Some(amount),
                note: "A cargo ledger entry".into(),
            }),
        });
    }
    p.sim.submit(batch);
    p.idle(4);
    let TunableValue::List(stations) =
        &p.world().progress.tunables["shortage.rimewater_ice.stations"].value
    else {
        panic!("buyer list")
    };
    assert_eq!(
        stations,
        &[
            pilot::KETTLE.to_string(),
            pilot::GDO.to_string(),
            pilot::WHARF.to_string()
        ]
    );
    assert_eq!(
        p.world().progress.tunables["shortage.rimewater_ice.way_a_quota"].value,
        TunableValue::Number(50.0)
    );
}

#[test]
fn chronicle_finds_news_beyond_a_page_of_unrelated_history_and_keeps_it() {
    let mut p = late_pilot("chronicle", |_| {});
    let mut batch = CommandBatch::new(Cause::system("test", "a long quiet interval"));
    for _ in 0..65 {
        batch.push(Command::RecordFact {
            fact: Box::new(FactDraft {
                kind: "quiet_interval".parse().unwrap(),
                subjects: Subjects::default(),
                amount: None,
                note: "A quiet interval".into(),
            }),
        });
    }
    batch.push(Command::RecordFact {
        fact: Box::new(FactDraft {
            kind: "gm.station_founded".parse().unwrap(),
            subjects: Subjects {
                station: Some(pilot::WHARF),
                npc: Some("npc.ilse_marrow".parse().unwrap()),
                ..Default::default()
            },
            amount: None,
            note: "A new station opened".into(),
        }),
    });
    p.sim.submit(batch);
    p.idle(4);
    let lines: Vec<_> = p
        .world()
        .talk
        .on(galaxy_contract::talk::Channel::Chronicle)
        .cloned()
        .collect();
    assert!(
        lines.iter().any(
            |l| l.line.text.contains("Lantern Wharf") && l.line.text.contains("station opened")
        )
    );
    assert!(
        lines
            .iter()
            .all(|l| l.line.text.split_whitespace().count() <= 25)
    );
    p.reply("npc.ilse_marrow", "choice.what_changed");
    assert!(p.heard("npc.ilse_marrow", "station opened"));
    undo_layer(&mut p, "chronicle");
    for line in lines {
        assert!(p.world().talk.lines.contains_key(&line.id));
    }
}
