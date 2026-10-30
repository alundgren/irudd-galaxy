use galaxy_app::harness::{self, Profile, RunOptions};
use galaxy_contract::prelude::*;
use galaxy_contract::verbs::Order;
use std::path::PathBuf;

fn packs() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../packs")
}
fn exercise(gm: bool, profile: Profile, hull: &str) {
    let game = harness::fresh(&packs(), 7, gm, profile).unwrap();
    let run = harness::run(
        game,
        RunOptions {
            prestige: hull.parse().unwrap(),
            ..RunOptions::default()
        },
    )
    .unwrap();
    let report_dir = packs().join("../target/harness");
    std::fs::create_dir_all(&report_dir).unwrap();
    std::fs::write(
        report_dir.join(match (gm, profile, hull) {
            (true, _, _) => "run-a.json",
            (false, Profile::Slice, _) => "full-run-b.json",
            (false, Profile::Travel, _) => "economy-run-b.json",
            (false, _, "hull.heron") => "heron-run-b.json",
            _ => "run-b.json",
        }),
        serde_json::to_vec_pretty(&run.report).unwrap(),
    )
    .unwrap();
    eprintln!(
        "run {}: tick {}, completed {}, dockings {}, hash {}, error {}",
        if gm { "A" } else { "B" },
        run.report["tick"],
        run.report["completed"],
        run.report["dockings"],
        run.report["hash"],
        run.report["error"]
    );
    assert_eq!(run.report["ok"], true, "{}", run.report["error"]);
    assert_eq!(run.report["completed"], 17);
    let history = run.game.sim().history();
    for id in harness::BEATS {
        assert_eq!(
            history
                .facts()
                .iter()
                .filter(|f| f.kind.as_str() == "beat_completed"
                    && f.subjects.other.as_ref().is_some_and(|s| s.as_str() == id))
                .count(),
            1,
            "{id}"
        );
    }
    for p in run.game.sim().world().packs.values() {
        for h in p.health.values() {
            assert_eq!(h.aborts, 0);
            assert!(!h.quarantined);
        }
    }
    assert!(
        run.report["credits"]
            .as_array()
            .unwrap()
            .iter()
            .all(|c| c["centicredits"].as_i64().unwrap() >= 0)
    );
    // Every change type whose pack is installed must have happened; the others are
    // reported as absent, so Run A tightens as game-master packs land.
    let types = run.report["gm_change_types"].as_array().unwrap();
    assert_eq!(types.len(), harness::CHANGE_TYPES.len());
    for t in types {
        let installed = gm
            && run
                .game
                .reports()
                .iter()
                .any(|r| r.accepted() && r.pack.as_str() == t["pack"]);
        let change = t["change"].as_str().unwrap();
        assert_eq!(
            harness::change_seen(change, run.game.sim().world(), history),
            installed,
            "{t}"
        );
        assert_eq!(
            t["status"],
            if installed {
                "checked"
            } else {
                "not_installed"
            },
            "{t}"
        );
    }
    if gm {
        assert!(
            history
                .facts()
                .iter()
                .any(|f| f.kind.as_str() == "gm.kettle_growth.stage_built"
                    && f.amount.is_some_and(|n| n >= 2))
        );
    } else {
        assert_eq!(run.report["gm_facts"], 0);
        assert_eq!(run.report["gm_causes"], 0);
        assert!(history.causes().values().all(|c| !matches!(&c.cause.kind, CauseKind::Script { pack,.. } if pack.as_str().starts_with("gm."))));
    }
    for stall in history
        .facts()
        .iter()
        .filter(|f| f.kind.as_str() == "loop_stalled")
    {
        // SetOrders is a player verb with no synthetic assignment fact. A new
        // lap proves recovery; otherwise verify that the documented fallback
        // replaced the stalled route and is still active in the actual craft.
        let reassigned = stall.subjects.ship.as_ref().is_some_and(|id| {
            if id.as_str() != "craft.thimble" {
                return false;
            }
            let w = run.game.sim().world();
            let mut expected = w.player.loops["Wharf plate run"].clone();
            for order in &mut expected {
                if let Order::Sell { floor, .. } = order {
                    *floor = Some(4000);
                }
            }
            w.ships.get(id).is_some_and(|s| {
                matches!(s.orders.orders.get(s.orders.current),
                    Some(Order::Loop { orders, .. }) if orders == &expected)
            })
        });
        assert!(
            history.facts().iter().any(|f| f.tick > stall.tick
                && f.subjects.ship == stall.subjects.ship
                && f.kind.as_str() == "loop_lap")
                || reassigned,
            "unanswered stall {stall:?}"
        );
    }
}

#[test]
fn journey_a_with_installed_gm_examples() {
    exercise(true, Profile::Fast, "hull.tandem");
}
#[test]
fn journey_b_core_only() {
    exercise(false, Profile::Fast, "hull.tandem");
}
#[test]
#[ignore = "full authored pacing; run explicitly before changing the journey route"]
fn full_slice_journey() {
    exercise(false, Profile::Slice, "hull.tandem");
}
#[test]
#[ignore = "authored economy and activity gates with faster travel"]
fn authored_economy_journey() {
    exercise(false, Profile::Travel, "hull.tandem");
}
#[test]
#[ignore = "alternative prestige branch"]
fn heron_journey() {
    exercise(false, Profile::Fast, "hull.heron");
}

#[test]
#[ignore = "explicit golden regeneration; review fixture diffs"]
fn record_act_one_golden() {
    let game = harness::fresh(&packs(), 7, false, Profile::Fast).unwrap();
    let mut run = harness::run(
        game,
        RunOptions {
            until: 5,
            record: true,
            ..RunOptions::default()
        },
    )
    .unwrap();
    assert_eq!(run.report["ok"], true, "{}", run.report["error"]);
    run.recording.initial = None;
    run.recording.profile = Profile::Fast;
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/golden/act1.json");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, serde_json::to_vec_pretty(&run.recording).unwrap()).unwrap();
}

#[test]
fn act_one_golden_and_first_diverging_store() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/golden/act1.json");
    let mut recording: harness::replay::Recording =
        serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    let game = harness::replay::replay(&packs(), &recording).unwrap();
    assert_eq!(
        game.sim().world().progress.beats[harness::BEATS[4]].status,
        BeatStatus::Completed
    );
    let tick = recording.checkpoints[0].tick;
    // Lose the player's first flight input, the undock. Replay must identify the first
    // affected checkpoint and the ship store, without changing any expected hashes.
    let press = recording
        .inputs
        .iter_mut()
        .find_map(|run| match &mut run.input {
            harness::replay::Action::Input(input) if input.thrust != Vec2::ZERO => Some(input),
            _ => None,
        })
        .expect("undock thrust");
    press.thrust = Vec2::ZERO;
    let error = harness::replay::replay(&packs(), &recording)
        .err()
        .expect("mismatch");
    assert!(error.contains(&format!("tick {tick}")), "{error}");
    assert!(error.contains("world.ships"), "{error}");
}

#[test]
fn jump_retains_real_completion_facts_and_stops_before_next_input() {
    let game = harness::fresh(&packs(), 11, false, Profile::Fast).unwrap();
    let run = harness::run(
        game,
        RunOptions {
            until: 4,
            at_start: true,
            ..RunOptions::default()
        },
    )
    .unwrap();
    assert_eq!(run.report["ok"], true);
    assert_eq!(
        run.game.sim().world().progress.beats[harness::BEATS[3]].status,
        BeatStatus::Active
    );
    assert_eq!(
        run.game
            .sim()
            .history()
            .facts()
            .iter()
            .filter(|f| f.kind.as_str() == "beat_completed")
            .count(),
        3
    );
    let dir = tempfile::tempdir().unwrap();
    let save =
        galaxy_core::Save::create(dir.path().join("jump.sqlite"), run.game.sim().state()).unwrap();
    let loaded = save.load().unwrap();
    assert_eq!(loaded, *run.game.sim().state());
}

/// The prestige card is modal and every text draws after every panel, so the dock
/// screen and talk must not leave their words on top of the card's.
#[test]
fn nothing_shows_through_the_prestige_card() {
    let run = harness::run(
        harness::fresh(&packs(), 7, false, Profile::Fast).unwrap(),
        RunOptions {
            until: 16,
            at_start: true,
            ..RunOptions::default()
        },
    )
    .unwrap();
    let session = galaxy_app::session::Session::with_game(run.game);
    assert!(session.flagship().unwrap().docked_at.is_some());
    let ui_state = galaxy_app::ui::UiState {
        confirm: Some(HullId::from_static("hull.tandem")),
        ..galaxy_app::ui::UiState::default()
    };
    for (w, h) in [(960.0, 540.0), (1280.0, 720.0), (1920.0, 1080.0)] {
        let viewport = render::glam::Vec2::new(w, h);
        let canvas = galaxy_app::ui::canvas(viewport);
        let mut card = render::Overlay::default();
        let buttons = galaxy_app::ui::prestige_card(
            &mut card,
            session.world(),
            session.sim().history(),
            &HullId::from_static("hull.tandem"),
            &ui_state,
            canvas,
        );
        assert_eq!(buttons.len(), 2, "the offer stands");
        galaxy_app::ui::scale_overlay(&mut card, galaxy_app::ui::ui_scale(viewport));
        let r = card.panels[0].rect;
        let own: Vec<_> = card.texts.iter().map(|t| t.text.clone()).collect();
        let view = galaxy_app::view::View::new();
        let rig = galaxy_app::view::CameraRig::new(render::glam::Vec2::ZERO);
        let (scene, buttons) =
            galaxy_app::frame::compose(&view, &session, &rig, &ui_state, 1.0, 0.0, viewport);
        assert_eq!(buttons.len(), 2, "{w}x{h}: only the card's buttons work");
        for t in &scene.overlay.texts {
            let inside =
                t.pos.x >= r.x && t.pos.x <= r.x + r.w && t.pos.y >= r.y && t.pos.y <= r.y + r.h;
            assert!(
                !inside || own.contains(&t.text),
                "{w}x{h}: `{}` shows through the card",
                t.text
            );
        }
    }
}

#[test]
fn f3_panel_is_small_and_contains_diagnostics() {
    let game = harness::fresh(&packs(), 7, false, Profile::Fast).unwrap();
    let mut session = galaxy_app::session::Session::with_game(game);
    session.tick(PlayerInput::default());
    let mut overlay = render::Overlay::default();
    galaxy_app::ui::debug_overlay(&mut overlay, &session);
    assert_eq!(overlay.panels.len(), 1);
    assert!(overlay.panels[0].rect.h < 200.0);
    assert!(overlay.texts.len() >= 4);
}

#[test]
fn an_early_pause_does_not_replace_the_pause_after_the_lens() {
    let mut game = harness::fresh(&packs(), 9, false, Profile::Fast).unwrap();
    harness::replay::apply(&mut game, &harness::replay::Action::Pause).unwrap();
    let run = harness::run(
        game,
        RunOptions {
            until: 5,
            ..RunOptions::default()
        },
    )
    .unwrap();
    assert_eq!(run.report["ok"], true);
    let history = run.game.sim().history().facts();
    let started = history
        .iter()
        .find(|f| {
            f.kind.as_str() == "beat_started"
                && f.subjects
                    .other
                    .as_ref()
                    .is_some_and(|id| id.as_str() == harness::BEATS[3])
        })
        .unwrap();
    let pauses: Vec<_> = history
        .iter()
        .filter(|f| f.kind.as_str() == "paused")
        .collect();
    assert_eq!(pauses.len(), 2);
    assert!(pauses[0].tick < started.tick);
    assert!(pauses[1].tick > started.tick);
    assert_eq!(run.report["completed"], 5);
}

#[test]
fn the_chronicle_is_checked_on_its_talk_channel() {
    use galaxy_contract::talk::{Channel, SaidLine, TalkLine};
    let mut state = galaxy_app::game::new_state(&packs(), 7, true).unwrap();
    let installed = |pack: &str| pack == "gm.chronicle";
    let chronicle = |state: &galaxy_core::SimState| {
        harness::change_types(&state.world, &state.history, &installed, true)
            .into_iter()
            .find(|t| t["change"] == "chronicle")
            .unwrap()["status"]
            .clone()
    };
    assert_eq!(chronicle(&state), "missing");
    // The hook writes entries as talk lines; no `gm.chronicle` fact kind exists.
    state.world.talk.last = 1;
    state.world.talk.lines.insert(
        TalkId(1),
        SaidLine {
            id: TalkId(1),
            tick: Tick(1),
            line: TalkLine {
                channel: Channel::Chronicle,
                speaker: Some("npc.ilse_marrow".parse().unwrap()),
                place: Some("station.lantern_wharf".parse().unwrap()),
                text: "At Lantern Wharf, a greenhouse ring lit up. Ilse remembers.".into(),
                cites: vec![FactId(1)],
                about: vec![],
                choices: vec![],
            },
        },
    );
    assert_eq!(chronicle(&state), "checked");
    let others = harness::change_types(&state.world, &state.history, &installed, true);
    assert!(
        others
            .iter()
            .filter(|t| t["change"] != "chronicle")
            .all(|t| t["status"] == "not_installed")
    );
}
