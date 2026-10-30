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

// --- PR #31 review regressions ---------------------------------------------------------

/// A copy of a pack folder (scripts, lib, manifest) under `to`.
fn copy_pack(from: &std::path::Path, to: &std::path::Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.is_dir() {
            copy_pack(&path, &to.join(entry.file_name()));
        } else {
            std::fs::copy(&path, to.join(entry.file_name())).unwrap();
        }
    }
}

/// The review's probe: a pack that places a reward-bearing buoy at every docking.
fn reward_probe(dir: &std::path::Path, uses: &[&str], read: bool) {
    std::fs::create_dir_all(dir.join("scripts")).unwrap();
    let uses: Vec<String> = uses.iter().map(|u| format!("\"{u}\"")).collect();
    std::fs::write(
        dir.join("manifest.toml"),
        format!(
            "id = \"gm.reward_probe\"\nname = \"Reward probe\"\nversion = \"0.1.0\"\n\
             depends = [\"core\"]\nuses = [{}]\n[[script]]\nid = \"main\"\n\
             file = \"scripts/main.luau\"\nhandlers = [\"on.dock\"]\n[rollback]\n\
             speaker = \"npc.rook_adeyemi\"\nplace = \"station.cinder_hook\"\n\
             text = \"Never mind that buoy.\"\n",
            uses.join(", ")
        ),
    )
    .unwrap();
    std::fs::write(
        dir.join("scripts/main.luau"),
        format!(
            "--!strict\nlocal G = require(\"@galaxy\")\n\
             return G.script({{id=\"main\",on={{dock=function(ev:G.DockEvent,ctx:G.Ctx)\n\
             local why=ctx:cause({{summary=\"a probe buoy\",cites={{ev.fact}}}})\n\
             ctx.kit:buoy(\"gm.reward_probe.buoy\",{{region=\"region.cinder_drift\",position={{x=6700,y=1900}},radius=40,planted_by=\"faction.tollway_compact\",read={read},\
             reward={{person=\"npc.rook_adeyemi\",choice=\"gm.reward_probe.claim\",credits=12345,standing={{[\"faction.wharf_cooperative\"]=80}}}}}},why)\n\
             ctx.talk:offer({{place=\"station.cinder_hook\",text=\"Rook has a buoy for you.\",cites={{ev.fact}}}},why)\n\
             end}}}})\n"
        ),
    )
    .unwrap();
}

#[test]
fn a_buoy_reward_needs_the_packs_credit_and_standing_capabilities() {
    let p = late_pilot("compact_plant", |_| {});
    let check = |uses: &[&str], read: bool| {
        let dir = tempfile::tempdir().unwrap();
        reward_probe(dir.path(), uses, read);
        let r = check_dir(
            dir.path(),
            CheckOptions {
                world: Some(p.world()),
                history: Some(p.sim.history()),
                analyzer: None,
                require_typecheck: false,
            },
        )
        .report;
        (r.accepted(), format!("{r:?}"))
    };
    // The review's probe: only kit.write and talk, an already-read buoy, 12,345 cr and
    // 80 standing. The host refuses it.
    let (ok, why) = check(&["events.dock", "kit.write", "talk.write"], true);
    assert!(!ok, "{why}");
    let (ok, why) = check(&["events.dock", "kit.write", "talk.write"], false);
    assert!(!ok && why.contains("beats.write"), "{why}");
    let (ok, why) = check(
        &["events.dock", "kit.write", "talk.write", "beats.write"],
        false,
    );
    assert!(!ok && why.contains("standing.write"), "{why}");
    let all = [
        "events.dock",
        "kit.write",
        "talk.write",
        "beats.write",
        "standing.write",
    ];
    let (ok, why) = check(&all, true);
    assert!(!ok && why.contains("read = false"), "{why}");
    let (ok, why) = check(&all, false);
    assert!(ok, "{why}");
}

fn compact_pilot() -> pilot::Pilot {
    late_pilot("compact_plant", |s| {
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
    })
}

/// Rook's proof choice said since `since`, the way the talk panel lists open choices.
fn proof_offered(p: &pilot::Pilot, since: Tick) -> bool {
    p.world()
        .talk
        .on(galaxy_contract::talk::Channel::DockTalk)
        .any(|l| {
            l.tick > since
                && l.line.speaker.as_ref().map(|s| s.as_str()) == Some("npc.rook_adeyemi")
                && l.line
                    .choices
                    .iter()
                    .any(|c| c.id.as_str() == "gm.compact_plant.claim")
        })
}

fn dock_at(p: &mut pilot::Pilot, station: StationId) -> Tick {
    let since = p.world().meta.tick;
    let at = p.world().stations.get(&station).unwrap().position;
    place_player(p, at, Some(station));
    since
}

#[test]
fn rooks_proof_reward_waits_at_cinder_hook_until_it_is_claimed() {
    let mut p = compact_pilot();
    p.idle(4);
    let buoy = p.world().survey_buoys["gm.compact_plant.buoy"].clone();
    place_player(&mut p, buoy.position, None);
    p.press();
    // The review's path: dock at the Observatory first, then at Cinder Hook.
    let since = dock_at(&mut p, pilot::GDO);
    assert!(
        p.has_fact("gm.rumor_exposed"),
        "the plant is exposed anywhere"
    );
    assert!(!proof_offered(&p, since), "Rook offers it at the Hook");
    let since = dock_at(&mut p, pilot::HOOK);
    assert!(
        proof_offered(&p, since),
        "the Hook docking offers the proof"
    );
    // Leaving without claiming doesn't lose it.
    dock_at(&mut p, pilot::GDO);
    let since = dock_at(&mut p, pilot::HOOK);
    assert!(
        proof_offered(&p, since),
        "the next Hook docking offers it again"
    );
    let (money, coop) = (
        p.credits(),
        p.world()
            .factions
            .get("faction.wharf_cooperative")
            .unwrap()
            .standing,
    );
    p.reply("npc.rook_adeyemi", "gm.compact_plant.claim");
    assert_eq!(p.credits(), money + 300);
    // Paid as a standing deed: clamped, with its facts.
    assert!(
        p.world()
            .factions
            .get("faction.wharf_cooperative")
            .unwrap()
            .standing
            > coop
    );
    assert!(p.has_fact("standing_changed"));
    assert!(p.has_fact("survey_reward_claimed"));
    let since = dock_at(&mut p, pilot::HOOK);
    assert!(
        !proof_offered(&p, since),
        "a claimed proof isn't offered again"
    );
    assert_eq!(p.rejected, 0, "{:?}", p.rejections);
}

#[test]
fn reading_a_buoy_in_the_rollback_tick_does_not_bring_it_back() {
    let mut p = compact_pilot();
    p.idle(4);
    let buoy = p.world().survey_buoys["gm.compact_plant.buoy"].clone();
    place_player(&mut p, buoy.position, None);
    // Queue the pack's rollback while the ship is beside its unread buoy, then step
    // with Space held.
    let pack = read_pack(&examples().join("compact_plant")).unwrap();
    let causes = galaxy_script::rollback::causes_of(&p.sim, &pack.manifest.id, &pack.revision);
    galaxy_script::rollback::rollback(&mut p.sim, &causes, pack.manifest.rollback.as_ref());
    for _ in 0..3 {
        p.tick(&PlayerInput {
            action: true,
            ..Default::default()
        });
    }
    assert!(p.world().survey_buoys.is_empty(), "the removal wins");
    assert!(!p.has_fact("survey_buoy_read"));
    assert_eq!(p.rejected, 0, "{:?}", p.rejections);
}

#[test]
fn tending_a_lamp_in_the_retirement_tick_leaves_no_lamp() {
    let mut p = late_pilot("rock_tagging", |_| {});
    let pack = read_pack(&examples().join("rock_tagging")).unwrap();
    p.sim.submit(
        CommandBatch::new(Cause::script(
            pack.manifest.id.clone(),
            pack.revision.clone(),
            "Juno teaches rock lamps",
        ))
        .with(Command::SetFlag {
            flag: "mech.rock_tagging".parse().unwrap(),
            on: true,
        }),
    );
    p.idle(1);
    let rock = p.world().rocks.get("rock.cinder.iron.02").unwrap().clone();
    place_player(&mut p, rock.position, None);
    let causes = galaxy_script::rollback::causes_of(&p.sim, &pack.manifest.id, &pack.revision);
    galaxy_script::rollback::rollback(&mut p.sim, &causes, pack.manifest.rollback.as_ref());
    p.tick(&PlayerInput {
        tag: true,
        ..Default::default()
    });
    p.idle(2);
    assert!(!p.world().player.flags.contains("mech.rock_tagging"));
    assert!(
        p.world().rock_tags.is_empty(),
        "no lamp outlives the retirement"
    );
    assert_eq!(p.rejected, 0, "{:?}", p.rejections);
}

#[test]
fn a_new_sixth_station_revision_reopens_its_rolled_back_scaffold() {
    const HALT: &str = "gm.sixth_station.halt";
    let mut p = late_pilot("sixth_station", |s| {
        s.counter(CounterKind::Dockings, 50);
    });
    let rock = p
        .world()
        .rocks
        .values()
        .find(|r| r.field == pilot::RIMEWATER)
        .unwrap()
        .id
        .clone();
    p.sim.submit(
        CommandBatch::new(Cause::system("test", "the pilot's ice workings")).with(Command::Emit {
            event: Event::Mine {
                ship: PLAYER,
                rock,
                ware: ICE,
                amount: 100,
            },
        }),
    );
    p.idle(4);
    let founded = p
        .world()
        .stations
        .get(HALT)
        .cloned()
        .expect("Wynn founds the Halt");
    let recipe = p.world().station_recipes[HALT].clone();
    let market = p.world().markets.get(HALT).cloned().unwrap();
    undo_layer(&mut p, "sixth_station");
    let scaffold = p.world().stations.get(HALT).cloned().unwrap();
    assert!(scaffold.services.is_empty());
    assert!(!p.world().markets.contains(HALT));
    let closed = (scaffold, p.world().station_recipes[HALT].clone());

    // A corrected revision of the same pack.
    let dir = tempfile::tempdir().unwrap();
    copy_pack(&examples().join("sixth_station"), dir.path());
    let main = dir.path().join("scripts/main.luau");
    let source = std::fs::read_to_string(&main).unwrap();
    std::fs::write(
        &main,
        source.replace("Slow work, sound tanks.", "Slow work, sounder tanks."),
    )
    .unwrap();
    let report = check_dir(
        dir.path(),
        CheckOptions {
            world: Some(p.world()),
            history: Some(p.sim.history()),
            analyzer: None,
            require_typecheck: false,
        },
    )
    .report;
    assert!(report.accepted(), "{report:#?}");
    assert!(
        p.host
            .load(dir.path(), p.world(), p.sim.history())
            .accepted()
    );
    let revised = read_pack(dir.path()).unwrap();
    // What the bridge's apply batch does for a pack a rollback switched off.
    p.sim.submit(
        CommandBatch::new(Cause::script(
            revised.manifest.id.clone(),
            revised.revision.clone(),
            "the game master applied the revised sixth station",
        ))
        .with(Command::SetPackOff {
            pack: revised.manifest.id.clone(),
            off: false,
        }),
    );
    p.idle(1);
    let at = p.docked().cloned().unwrap();
    dock_at(&mut p, at);
    assert_eq!(
        p.world().stations.get(HALT),
        Some(&founded),
        "the ring reopens"
    );
    assert_eq!(p.world().station_recipes[HALT], recipe);
    assert_eq!(p.world().markets.get(HALT), Some(&market));
    assert!(p.world().npcs.contains("gm.sixth_station.wynn"));
    assert_eq!(p.rejected, 0, "{:?}", p.rejections);

    // Rolling the revision back shuts the same scaffold again.
    let causes =
        galaxy_script::rollback::causes_of(&p.sim, &revised.manifest.id, &revised.revision);
    galaxy_script::rollback::rollback(&mut p.sim, &causes, revised.manifest.rollback.as_ref());
    p.idle(3);
    assert_eq!(p.world().stations.get(HALT), Some(&closed.0));
    assert_eq!(p.world().station_recipes[HALT], closed.1);
    assert!(!p.world().markets.contains(HALT));
    assert!(!p.world().npcs.contains("gm.sixth_station.wynn"));
    assert_eq!(p.rejected, 0, "{:?}", p.rejections);
}
