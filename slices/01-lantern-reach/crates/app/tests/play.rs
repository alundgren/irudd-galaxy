//! Headless tests of the playable loop: the session, input, pause, dock trades, saves.
//! No window, GPU or audio device; the one GPU test is `#[ignore]`.

use std::path::PathBuf;

use galaxy_app::input::{Key, Keys, click_target, player_input};
use galaxy_app::saves::Saver;
use galaxy_app::session::Session;
use galaxy_app::trades::Settled;
use galaxy_app::ui::{self, Amount, UiState};
use galaxy_contract::prelude::*;
use galaxy_core::{Save, SimState};

const WHARF: StationId = StationId::from_static("station.lantern_wharf");
const RATIONS: WareId = WareId::from_static("ware.rations");
const ICE: WareId = WareId::from_static("ware.ice");

fn packs() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../packs")
}

fn new_state() -> SimState {
    galaxy_core::new_game(&galaxy_app::game::core_data(&packs()), 1).unwrap()
}

/// A session a few ticks in, so start-up re-pricing has settled.
fn settled_session() -> Session {
    let mut s = Session::new(new_state());
    for _ in 0..5 {
        s.tick(PlayerInput::default());
    }
    s
}

fn flagship(s: &Session) -> &Ship {
    s.flagship().unwrap()
}

fn paused_facts(s: &Session) -> Vec<Fact> {
    s.sim()
        .history()
        .facts()
        .iter()
        .filter(|f| f.kind.as_str() == "paused")
        .cloned()
        .collect()
}

#[test]
fn every_pause_opening_writes_a_paused_fact() {
    let mut s = Session::new(new_state());
    s.begin();
    s.tick(PlayerInput::default());
    assert_eq!(s.session(), 1);
    let tick = s.world().meta.tick;

    assert!(s.pause());
    assert!(!s.pause(), "already open");
    // The galaxy stops: nothing ticks while the card is up.
    for _ in 0..10 {
        assert!(!s.tick(PlayerInput::default()).stepped);
    }
    assert_eq!(s.world().meta.tick, tick);
    assert!(
        paused_facts(&s).is_empty(),
        "it commits after the card closes"
    );

    s.resume();
    s.tick(PlayerInput::default());
    let facts = paused_facts(&s);
    assert_eq!(facts.len(), 1);
    assert_eq!(
        facts[0].payload,
        FactPayload::Event(Event::Paused { session: 1 })
    );

    // A second opening is a second fact, newer than the first.
    s.toggle_pause();
    s.toggle_pause();
    s.tick(PlayerInput::default());
    let facts = paused_facts(&s);
    assert_eq!(facts.len(), 2);
    assert!(facts[1].id > facts[0].id);
    // No counter moves for a pause.
    assert_eq!(
        s.world()
            .counters
            .get(CounterKind::Sessions, &CounterScope::Player),
        1
    );
}

#[test]
fn a_pause_left_open_at_quit_is_saved_and_commits_next_time() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("p.sqlite");
    let mut s = Session::new(new_state());
    s.begin();
    s.tick(PlayerInput::default());
    s.pause();
    let save = Save::create(&path, s.state()).unwrap();
    drop(save);
    let mut again = Session::new(Save::open(&path).unwrap().load().unwrap());
    again.tick(PlayerInput::default());
    assert_eq!(paused_facts(&again).len(), 1);
}

fn buy_rations(s: &Session) -> TradeRequest {
    TradeRequest {
        ship: flagship(s).id.clone(),
        station: WHARF,
        ware: RATIONS,
        side: TradeSide::Buy,
        quantity: 1,
        limit: None,
    }
}

/// Another writer (a tender, an NPC craft, a script) moves the Wharf's rations line in
/// the same tick the player trades it.
fn contend(s: &mut Session) {
    let line = s.world().markets.get(&WHARF).unwrap().lines[&RATIONS].clone();
    let mut moved = line.clone();
    moved.stock -= 5;
    s.submit(
        CommandBatch::new(Cause::system("test", "a tender buys rations")).with(
            Command::UpdateMarketLine {
                station: WHARF,
                ware: RATIONS,
                from: Box::new(line),
                to: Box::new(moved),
            },
        ),
    );
}

#[test]
fn without_the_app_a_contended_dock_trade_is_lost() {
    // The bug the app works around: one `PlayerInput.trades` request that loses the
    // line's compare-and-set is never retried by the economy.
    let s = settled_session();
    let req = buy_rations(&s);
    let mut sim = galaxy_app::game::game(s.state().clone());
    let input = PlayerInput {
        trades: vec![req],
        ..PlayerInput::default()
    };
    // Same contention, submitted on the raw sim.
    let line = sim.world().markets.get(&WHARF).unwrap().lines[&RATIONS].clone();
    let mut moved = line.clone();
    moved.stock -= 5;
    sim.submit(
        CommandBatch::new(Cause::system("test", "a tender buys rations")).with(
            Command::UpdateMarketLine {
                station: WHARF,
                ware: RATIONS,
                from: Box::new(line),
                to: Box::new(moved),
            },
        ),
    );
    sim.step(&input);
    sim.step(&PlayerInput::default());
    let ship = sim.world().player.ship.clone().unwrap();
    assert_eq!(
        sim.world().ships.get(&ship).unwrap().hold.count(&RATIONS),
        0
    );
}

#[test]
fn a_contended_dock_trade_is_retried_until_it_applies() {
    let mut s = settled_session();
    let credits = s.world().player.credits;
    let req = buy_rations(&s);
    s.request_trade(req);
    assert_eq!(s.trades.pending(), 1);
    contend(&mut s);

    let out = s.tick(PlayerInput::default());
    assert_eq!(
        out.trade,
        Some(Settled::Retrying),
        "lost the line this tick"
    );
    assert_eq!(flagship(&s).hold.count(&RATIONS), 0);
    assert_eq!(s.trades.pending(), 1, "the intent is kept");

    let out = s.tick(PlayerInput::default());
    assert_eq!(out.trade, Some(Settled::Applied));
    assert_eq!(flagship(&s).hold.count(&RATIONS), 1);
    assert!(s.world().player.credits < credits);
    assert_eq!(s.trades.pending(), 0);
    assert_eq!(s.trades.notice, None);
}

#[test]
fn a_trade_that_cant_happen_is_refused_with_one_calm_line() {
    let mut s = settled_session();
    // Nothing to sell.
    s.request_trade(TradeRequest {
        ship: flagship(&s).id.clone(),
        station: WHARF,
        ware: ICE,
        side: TradeSide::Sell,
        quantity: 1,
        limit: None,
    });
    assert_eq!(s.trades.pending(), 0);
    assert_eq!(s.trades.notice.as_deref(), Some("You have no ice to sell."));
    // More than the credits buy.
    let mut req = buy_rations(&s);
    req.quantity = 20;
    s.request_trade(req);
    let notice = s.trades.notice.clone().unwrap();
    assert!(notice.starts_with("That costs "), "{notice}");
    assert!(!notice.contains('\n'));
}

#[test]
fn a_trade_refused_after_it_was_queued_says_why() {
    let mut s = settled_session();
    // Asked for while affordable; another buy spends the money first.
    let two = TradeRequest {
        quantity: 2,
        ..buy_rations(&s)
    };
    s.request_trade(two.clone());
    s.request_trade(two);
    for _ in 0..4 {
        s.tick(PlayerInput::default());
    }
    assert_eq!(s.trades.pending(), 0);
    assert_eq!(flagship(&s).hold.count(&RATIONS), 2);
    let notice = s.trades.notice.clone().unwrap_or_default();
    assert!(notice.starts_with("That costs "), "{notice}");
}

#[test]
fn buy_max_fits_the_hold_stock_and_credits() {
    let s = settled_session();
    let lines = ui::dock_lines(s.world(), &WHARF);
    let i = lines.iter().position(|l| l.ware == RATIONS).unwrap();
    let req = ui::trade_request(s.world(), &WHARF, &lines[i], Amount::Most).unwrap();
    assert!(req.quantity >= 1);
    assert!(galaxy_economy::check_trade(s.world(), &req).is_ok());
    let more = TradeRequest {
        quantity: req.quantity + 1,
        ..req
    };
    assert!(galaxy_economy::check_trade(s.world(), &more).is_err());
}

fn buy_one_ration_order() -> OrderQueue {
    OrderQueue {
        orders: vec![Order::Buy {
            ware: RATIONS,
            quantity: Quantity::Units { n: 1 },
        }],
        ..OrderQueue::default()
    }
}

#[test]
fn another_buy_on_the_line_cant_acknowledge_a_dock_trade() {
    // Something else buys the very same thing for the flagship ahead of the dock
    // trade: same ship, station, ware and quantity, but not the dock screen's batch.
    let mut s = settled_session();
    let req = buy_rations(&s);
    s.request_trade(req.clone());
    let mut other = galaxy_economy::trade::trade_batch(s.world(), &req).unwrap();
    other.cause = Cause::system("test", "someone else's buy for the flagship");
    s.submit(other);

    let out = s.tick(PlayerInput::default());
    assert_eq!(
        flagship(&s).hold.count(&RATIONS),
        1,
        "the other buy went in"
    );
    assert_eq!(out.trade, Some(Settled::Retrying), "not the dock trade");
    assert_eq!(s.trades.pending(), 1);
    let out = s.tick(PlayerInput::default());
    assert_eq!(out.trade, Some(Settled::Applied));
    assert_eq!(flagship(&s).hold.count(&RATIONS), 2);
    assert_eq!(s.trades.pending(), 0);
}

#[test]
fn the_dock_trade_wins_over_the_flagships_own_order() {
    // The review's case: the docked flagship has a queued buy of one ration, and the
    // player asks the dock screen for one more. Both must happen, and the dock screen
    // must only report its own.
    let mut state = settled_session().state().clone();
    let id = state.world.player.ship.clone().unwrap();
    state.world.ships.get_mut(&id).unwrap().orders = buy_one_ration_order();
    let mut s = Session::new(state);
    s.request_trade(buy_rations(&s));

    let out = s.tick(PlayerInput::default());
    assert_eq!(
        out.trade,
        Some(Settled::Applied),
        "the explicit action goes first"
    );
    assert_eq!(flagship(&s).hold.count(&RATIONS), 1);
    let buys = |s: &Session| {
        s.last_events()
            .iter()
            .filter(|e| matches!(e.event, Event::Buy { .. }))
            .count()
    };
    assert_eq!(buys(&s), 1, "the order lost the line this tick");
    for _ in 0..10 {
        s.tick(PlayerInput::default());
    }
    assert_eq!(flagship(&s).hold.count(&RATIONS), 2, "the order buys after");
    let orders = &flagship(&s).orders;
    assert!(
        orders.current >= orders.orders.len(),
        "and its order is done"
    );
    assert_eq!(s.trades.pending(), 0);
}

#[test]
fn the_dock_trade_wins_over_a_craft_on_the_same_line() {
    let mut state = settled_session().state().clone();
    let mut craft = state
        .world
        .ships
        .get(state.world.player.ship.as_ref().unwrap())
        .unwrap()
        .clone();
    craft.id = ShipId::from_static("craft.contender");
    craft.orders = buy_one_ration_order();
    state.world.ships.put(craft.clone());
    let mut s = Session::new(state);
    s.request_trade(buy_rations(&s));
    assert_eq!(s.tick(PlayerInput::default()).trade, Some(Settled::Applied));
    for _ in 0..20 {
        s.tick(PlayerInput::default());
    }
    let craft_hold = s.world().ships.get(&craft.id).unwrap().hold.count(&RATIONS);
    assert_eq!(craft_hold, 1, "the craft buys a tick later");
    assert_eq!(
        flagship(&s).hold.count(&RATIONS),
        1,
        "exactly one, no duplicate"
    );
    assert_eq!(s.trades.pending(), 0);
}

#[test]
fn a_space_tap_between_ticks_still_undocks() {
    let mut s = Session::new(new_state());
    let mut keys = Keys::default();
    // Down and up between two fixed ticks.
    keys.event(Key::Space, true, false);
    keys.event(Key::Space, false, false);
    s.tick(keys.tick_input(true, None));
    assert!(flagship(&s).docked_at.is_none(), "the tap reached the sim");
}

#[test]
fn space_held_across_the_pause_card_doesnt_toggle_again() {
    let mut s = Session::new(new_state());
    let mut keys = Keys::default();
    keys.event(Key::Space, true, false);
    s.tick(keys.tick_input(true, None));
    assert!(flagship(&s).docked_at.is_none());
    // P with Space still down, then P again (the window clears the keys both ways).
    s.pause();
    keys.clear();
    s.resume();
    keys.clear();
    for _ in 0..3 {
        // The OS keeps repeating the held key.
        assert!(!keys.event(Key::Space, true, true));
        s.tick(keys.tick_input(false, None));
    }
    assert!(
        flagship(&s).docked_at.is_none(),
        "a repeat isn't another press"
    );
    // A real release and press still docks (the ship is still in the ring).
    keys.event(Key::Space, false, false);
    keys.event(Key::Space, true, false);
    s.tick(keys.tick_input(false, None));
    assert!(flagship(&s).docked_at.is_some());
}

#[test]
fn space_undocks_wasd_flies_and_holding_space_mines() {
    let mut s = Session::new(new_state());
    let mut ui = UiState::default();
    assert!(flagship(&s).docked_at.is_some());
    galaxy_app::script::run(&mut s, &mut ui, "space,wait*3,d*200").unwrap();
    let ship = flagship(&s);
    assert!(ship.docked_at.is_none());
    assert!(ship.position.x > 150.0, "{:?}", ship.position);
    let let_go = ship.position;
    // Arcade flight: it brakes to a stop within a few ticks and stays there.
    galaxy_app::script::run(&mut s, &mut ui, "wait*5").unwrap();
    let before = flagship(&s).position;
    assert!(before.distance(let_go) < 5.0);
    galaxy_app::script::run(&mut s, &mut ui, "wait*30").unwrap();
    assert_eq!(
        flagship(&s).position,
        before,
        "it stops when the keys are let go"
    );
    galaxy_app::script::run(&mut s, &mut ui, "space*240").unwrap();
    assert!(flagship(&s).hold.count(&ICE) > 0, "mined some ice");
}

#[test]
fn docked_keys_select_and_dont_fly() {
    let s = Session::new(new_state());
    let mut keys = Keys::default();
    keys.press(Key::W);
    let docked = flagship(&s).docked_at.is_some();
    assert!(docked);
    assert_eq!(player_input(&keys, docked, None).thrust, Vec2::ZERO);
}

#[test]
fn clicks_target_rings_rocks_and_points() {
    let s = Session::new(new_state());
    let w = s.world();
    let region = flagship(&s).region.clone();
    let wharf = w.stations.get(&WHARF).unwrap();
    assert_eq!(
        click_target(w, &region, wharf.position),
        ClickTarget::Station { station: WHARF }
    );
    let rock = w
        .rocks
        .values()
        .find(|r| galaxy_ship::space::rock_region(w, r) == Some(&region))
        .unwrap();
    assert_eq!(
        click_target(w, &region, rock.position),
        ClickTarget::Rock {
            rock: rock.id.clone()
        }
    );
    let empty = Vec2::new(-1900.0, -1900.0);
    assert_eq!(
        click_target(w, &region, empty),
        ClickTarget::Point { at: empty }
    );
}

#[test]
fn tier_zero_ignores_clicks() {
    let mut s = settled_session();
    let mut ui = UiState::default();
    galaxy_app::script::run(&mut s, &mut ui, "space,wait*3").unwrap();
    let at = flagship(&s).position;
    let click = Click {
        target: ClickTarget::Point {
            at: Vec2::new(at.x + 300.0, at.y),
        },
        queue: false,
    };
    s.tick(player_input(&Keys::default(), false, Some(click)));
    for _ in 0..30 {
        s.tick(PlayerInput::default());
    }
    assert!(flagship(&s).position.distance(at) < 1.0);
}

#[test]
fn the_saver_writes_in_the_background() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bg.sqlite");
    let mut s = Session::new(new_state());
    let saver = Saver::start(Save::create(&path, s.state()).unwrap());
    for _ in 0..20 {
        s.tick(PlayerInput::default());
        saver.save(s.state().clone());
    }
    let hash = s.sim().hash();
    saver.finish(Some(s.state().clone())).unwrap();
    let loaded = Save::open(&path).unwrap().load().unwrap();
    assert_eq!(galaxy_core::state_hash(&loaded), hash);
}

#[test]
fn the_goal_line_moves_on_with_act_one() {
    let mut s = Session::new(new_state());
    assert_eq!(
        galaxy_app::hooks::goal_line(s.world()).as_deref(),
        Some("Bring ice to Kettle Dock")
    );
    let mut ui = UiState::default();
    // Mine, fly to Kettle Dock, dock, sell the ice.
    galaxy_app::script::run(
        &mut s,
        &mut ui,
        "space,wait*3,d*200,space*400,wait*2,d+s*354,s*50,wait*5,space,wait*3,:tradeall",
    )
    .unwrap();
    assert_eq!(
        flagship(&s).docked_at.as_ref().map(StationId::as_str),
        Some("station.kettle_dock")
    );
    assert_eq!(flagship(&s).hold.count(&ICE), 0);
    assert_eq!(
        galaxy_app::hooks::goal_line(s.world()).as_deref(),
        Some("Sell iron ore at Lantern Wharf")
    );
}

#[test]
#[ignore = "needs a GPU adapter"]
fn galaxy_shot_renders_a_new_save() {
    let dir = tempfile::tempdir().unwrap();
    let save = dir.path().join("shot.sqlite");
    let out = dir.path().join("shot.png");
    Save::create(&save, &new_state()).unwrap();
    for map in [false, true] {
        let v = galaxy_app::shot::shot(&galaxy_app::shot::ShotArgs {
            save: save.clone(),
            out: out.clone(),
            map,
            width: 640,
            height: 360,
            zoom: None,
            keys: Some("space,wait*3,d*60".into()),
            write: false,
        })
        .unwrap();
        assert_eq!(v["width"], 640);
        let bytes = std::fs::read(&out).unwrap();
        assert_eq!(&bytes[1..4], b"PNG");
    }
}
