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
    let opts = galaxy_app::game::GameOptions::headless(packs(), 1);
    let mut game = galaxy_app::game::Game::open(s.state().clone(), &opts).unwrap();
    let sim = game.sim_mut();
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
fn flying_off_undocks_and_space_doesnt() {
    let mut s = Session::new(new_state());
    let mut keys = Keys::default();
    // Docked, Space does nothing: there's no undock confirm.
    keys.event(Key::Space, true, false);
    keys.event(Key::Space, false, false);
    s.tick(keys.tick_input(true, None));
    s.tick(keys.tick_input(true, None));
    assert!(flagship(&s).docked_at.is_some(), "Space doesn't undock");
    // A tap of W flies off.
    keys.event(Key::W, true, false);
    s.tick(keys.tick_input(true, None));
    keys.event(Key::W, false, false);
    assert!(flagship(&s).docked_at.is_none(), "flying off undocks");
    s.tick(keys.tick_input(false, None));
    assert!(flagship(&s).docked_at.is_none());
}

#[test]
fn flight_keys_held_into_a_docking_dont_undock_until_pressed_again() {
    let mut s = Session::new(new_state());
    let mut ui = UiState::default();
    galaxy_app::script::run(&mut s, &mut ui, "w,wait*3").unwrap();
    assert!(flagship(&s).docked_at.is_none());
    // Space in the ring with W held: it docks, and stays docked while W is down.
    let mut keys = Keys::default();
    keys.press(Key::W);
    keys.press(Key::Space);
    s.tick(keys.tick_input(false, None));
    assert!(flagship(&s).docked_at.is_some(), "docked");
    keys.release(Key::Space);
    for _ in 0..10 {
        s.tick(keys.tick_input(true, None));
    }
    assert!(flagship(&s).docked_at.is_some(), "W held from the approach");
    keys.release(Key::W);
    s.tick(keys.tick_input(true, None));
    keys.press(Key::W);
    s.tick(keys.tick_input(true, None));
    assert!(flagship(&s).docked_at.is_none(), "a fresh press flies off");
}

#[test]
fn space_held_across_the_pause_card_doesnt_toggle_again() {
    let mut s = Session::new(new_state());
    let mut ui = UiState::default();
    galaxy_app::script::run(&mut s, &mut ui, "w,wait*3").unwrap();
    let mut keys = Keys::default();
    keys.event(Key::Space, true, false);
    s.tick(keys.tick_input(false, None));
    assert!(flagship(&s).docked_at.is_some(), "Space docks in the ring");
    // Fly off with Space still down.
    galaxy_app::script::run(&mut s, &mut ui, "w,wait*3").unwrap();
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
fn flying_off_undocks_wasd_flies_and_holding_space_mines() {
    let mut s = Session::new(new_state());
    let mut ui = UiState::default();
    assert!(flagship(&s).docked_at.is_some());
    // D only turns the ship: it doesn't fly off. W does, and the Wren starts out
    // facing east, toward Rimewater's ice.
    galaxy_app::script::run(&mut s, &mut ui, "d,wait*3").unwrap();
    assert!(flagship(&s).docked_at.is_some(), "turning doesn't undock");
    galaxy_app::script::run(&mut s, &mut ui, "w,wait*3,w*75").unwrap();
    let ship = flagship(&s);
    assert!(ship.docked_at.is_none());
    assert!(ship.position.x > 150.0, "{:?}", ship.position);
    let let_go = ship.position;
    // Let go: it coasts, and the auto-brake stops it in a second.
    galaxy_app::script::run(&mut s, &mut ui, "wait*30").unwrap();
    assert_eq!(flagship(&s).velocity, Vec2::ZERO);
    let before = flagship(&s).position;
    let coasted = before.distance(let_go);
    assert!((60.0..90.0).contains(&coasted), "coasted {coasted} u");
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
fn docked_keys_fly_off_the_dock_screen_is_mouse_first() {
    let s = Session::new(new_state());
    let mut keys = Keys::default();
    keys.press(Key::W);
    assert!(flagship(&s).docked_at.is_some());
    assert_eq!(player_input(&keys, None).thrust, Vec2::new(0.0, 1.0));
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
    galaxy_app::script::run(&mut s, &mut ui, "w,wait*3").unwrap();
    let at = flagship(&s).position;
    let click = Click {
        target: ClickTarget::Point {
            at: Vec2::new(at.x + 300.0, at.y),
        },
        queue: false,
    };
    s.tick(player_input(&Keys::default(), Some(click)));
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

fn inside(r: &render::Rect, viewport: render::glam::Vec2) -> bool {
    let eps = 0.01;
    r.x >= -eps && r.y >= -eps && r.x + r.w <= viewport.x + eps && r.y + r.h <= viewport.y + eps
}

fn overlaps(a: &render::Rect, b: &render::Rect) -> bool {
    a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h
}

/// The frame's panels and buttons at `viewport`, built exactly as play builds them,
/// with the camera where play puts it.
fn frame_at(s: &Session, ui_state: &UiState, viewport: render::glam::Vec2) -> FrameParts {
    let view = galaxy_app::view::View::new();
    let mut rig = galaxy_app::view::CameraRig::new(render::glam::Vec2::ZERO);
    if let Some(goal) = galaxy_app::frame::camera_goal(s, ui_state, 1.0, viewport) {
        rig.center = goal.center;
        if let Some(z) = goal.zoom {
            rig.zoom = z;
        }
    }
    let (scene, buttons) = galaxy_app::frame::compose(&view, s, &rig, ui_state, 1.0, 0.0, viewport);
    FrameParts {
        panels: scene.overlay.panels.iter().map(|p| p.rect).collect(),
        foot: scene
            .overlay
            .texts
            .iter()
            .find(|t| t.text == ui::DOCK_FOOT)
            .map(|t| render::Rect::new(t.pos.x, t.pos.y, ui::text_width(&t.text, t.size), t.size)),
        buttons,
    }
}

struct FrameParts {
    panels: Vec<render::Rect>,
    buttons: Vec<ui::Button>,
    /// Where the open room's "fly off to undock" foot is, if it shows.
    foot: Option<render::Rect>,
}

/// A docked session with a busy screen: a refusal notice and a hold with several wares.
fn busy_dock_session() -> Session {
    let mut state = settled_session().state().clone();
    let id = state.world.player.ship.clone().unwrap();
    let wares: Vec<WareId> = state.world.wares.keys().take(4).cloned().collect();
    let ship = state.world.ships.get_mut(&id).unwrap();
    for w in wares {
        ship.hold.cargo.insert(w, 1);
    }
    let mut s = Session::new(state);
    s.request_trade(TradeRequest {
        quantity: 999,
        ..buy_rations(&s)
    });
    assert!(s.trades.notice.is_some());
    s
}

const SIZES: [(f32, f32); 7] = [
    (640.0, 360.0),
    (800.0, 450.0),
    (960.0, 540.0),
    (1280.0, 720.0),
    (1024.0, 768.0),
    (1920.0, 1080.0),
    (2560.0, 1080.0),
];

/// The Wharf's market room open.
fn market_open() -> UiState {
    UiState {
        room: Some(MARKET.clone()),
        ..UiState::default()
    }
}

const MARKET: StationPartId = StationPartId::from_static("part.wharf.market");

#[test]
fn the_market_room_fits_small_and_large_windows() {
    let s = busy_dock_session();
    for (w, h) in SIZES {
        let viewport = render::glam::Vec2::new(w, h);
        let f = frame_at(&s, &market_open(), viewport);
        for b in &f.buttons {
            assert!(inside(&b.rect, viewport), "{w}x{h}: {b:?} is clipped");
        }
        for p in &f.panels {
            assert!(inside(p, viewport), "{w}x{h}: panel {p:?} is clipped");
        }
        let foot = f.foot.expect("how to undock shows");
        assert!(inside(&foot, viewport), "{w}x{h}: {foot:?} is clipped");
        let trades = f
            .buttons
            .iter()
            .filter(|b| matches!(b.action, ui::UiAction::Trade { .. }))
            .count();
        let lines = ui::dock_lines(s.world(), &WHARF).len();
        assert_eq!(trades, 2 * lines, "{w}x{h}: every market action shows");
    }
}

#[test]
fn the_room_keeps_clear_of_the_goal_the_hold_and_the_key_and_leaves_the_station_in_view() {
    let s = busy_dock_session();
    for (w, h) in SIZES {
        let canvas = ui::canvas(render::glam::Vec2::new(w, h));
        let mut o = render::Overlay::default();
        ui::goal(&mut o, s.world(), canvas);
        ui::hold(&mut o, s.world(), canvas);
        let top: Vec<_> = o.panels.iter().map(|p| p.rect).collect();
        assert!(!top.is_empty());
        if top.len() == 2 {
            assert!(!overlaps(&top[0], &top[1]), "{w}x{h}: goal under the hold");
        }
        let mut d = render::Overlay::default();
        let (dock, _) = ui::rooms::panel(
            &mut d,
            s.world(),
            s.sim().history(),
            &WHARF,
            &MARKET,
            &market_open(),
            ui::Notices {
                trade: s.trades.notice.as_deref(),
                service: None,
                fitted: None,
            },
            canvas,
        )
        .expect("the Wharf has a market");
        assert_eq!(d.panels[0].rect, dock, "drawn where it's measured");
        for t in &top {
            assert!(!overlaps(&dock, t), "{w}x{h}: room {dock:?} under {t:?}");
        }
        let key = ui::station::key_area(s.world(), &WHARF, canvas).unwrap();
        assert!(!overlaps(&dock, &key), "{w}x{h}: room on the key");
        // The station stays in view: the room covers at most about 45% of the screen.
        assert!(
            dock.w <= canvas.x * ui::station::ROOM_SHARE + 0.5,
            "{w}x{h}: {dock:?} is wider than 45%"
        );
    }
}

/// The station names marked in gold at the screen edge (the goal marker).
fn goal_markers(s: &Session, ui_state: &UiState) -> Vec<String> {
    let view = galaxy_app::view::View::new();
    let at = flagship(s).position;
    let rig = galaxy_app::view::CameraRig::new(render::glam::Vec2::new(at.x, at.y));
    let viewport = render::glam::Vec2::new(1280.0, 720.0);
    let (scene, _) = galaxy_app::frame::compose(&view, s, &rig, ui_state, 1.0, 0.0, viewport);
    let stations: Vec<&str> = s
        .world()
        .stations
        .values()
        .map(|st| st.name.as_str())
        .collect();
    scene
        .overlay
        .texts
        .iter()
        .filter(|t| t.color == render::palette::MUSTARD && stations.contains(&t.text.as_str()))
        .map(|t| t.text.clone())
        .collect()
}

/// Discovery over hand-holding: no waypoint to the goal's station unless the player
/// asks for it (`play --goal-marker`).
#[test]
fn the_goal_marker_is_off_unless_asked_for() {
    let s = settled_session();
    assert!(goal_markers(&s, &UiState::default()).is_empty());
    let asked = UiState {
        goal_marker: true,
        ..UiState::default()
    };
    assert_eq!(goal_markers(&s, &asked), vec!["Kettle Dock".to_string()]);
}

#[test]
fn a_long_market_scrolls_and_keeps_the_selection_in_view() {
    const EXTRA: [WareId; 10] = [
        WareId::from_static("ware.test_a"),
        WareId::from_static("ware.test_b"),
        WareId::from_static("ware.test_c"),
        WareId::from_static("ware.test_d"),
        WareId::from_static("ware.test_e"),
        WareId::from_static("ware.test_f"),
        WareId::from_static("ware.test_g"),
        WareId::from_static("ware.test_h"),
        WareId::from_static("ware.test_i"),
        WareId::from_static("ware.test_j"),
    ];
    let mut state = busy_dock_session().state().clone();
    let market = state.world.markets.get_mut(&WHARF).unwrap();
    let line = market.lines[&RATIONS].clone();
    for w in EXTRA {
        market.lines.insert(w, line.clone());
    }
    let s = Session::new(state);
    let n = ui::dock_lines(s.world(), &WHARF).len();
    assert!(n > 10);
    let viewport = render::glam::Vec2::new(960.0, 540.0);
    for selected in [0, n / 2, n - 1] {
        let ui_state = UiState {
            selected,
            ..market_open()
        };
        let f = frame_at(&s, &ui_state, viewport);
        for b in &f.buttons {
            assert!(inside(&b.rect, viewport), "{b:?} is clipped");
        }
        let rows: Vec<usize> = f
            .buttons
            .iter()
            .filter_map(|b| match b.action {
                ui::UiAction::Select(i) => Some(i),
                _ => None,
            })
            .collect();
        assert!(rows.len() < n, "it scrolls rather than overflowing");
        assert!(rows.contains(&selected), "row {selected} in view: {rows:?}");
        assert!(f.foot.is_some_and(|r| inside(&r, viewport)));
    }
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
            keys: Some("d,wait*3,d*60".into()),
            write: false,
        })
        .unwrap();
        assert_eq!(v["width"], 640);
        let bytes = std::fs::read(&out).unwrap();
        assert_eq!(&bytes[1..4], b"PNG");
    }
}

#[test]
fn ore_ticks_sound_as_ice_enters_the_hold_during_a_long_run() {
    use galaxy_app::sound::Cues;
    use galaxy_audio::Cue;
    const ICE_ROCK: RockId = RockId::from_static("rock.shallows.ice.03");
    let mut state = new_state();
    let w = &mut state.world;
    let rock = w.rocks.get(&ICE_ROCK).unwrap().clone();
    let region = w.fields.get(&rock.field).unwrap().region.clone();
    let spot = rock.position + Vec2::new(galaxy_ship::space::rock_radius(w, &rock) + 15.0, 0.0);
    let me = w.player.ship.clone().unwrap();
    let ship = w.ships.get_mut(&me).unwrap();
    ship.docked_at = None;
    ship.region = region;
    ship.position = spot;
    ship.velocity = Vec2::ZERO;
    let mut s = Session::new(state);
    let cap = s
        .world()
        .hulls
        .get(&flagship(&s).hull)
        .unwrap()
        .hold_capacity;
    let mut cues = Cues::new(s.world());
    let mine = PlayerInput {
        action: true,
        ..PlayerInput::default()
    };
    let mut heard = Vec::new();
    let mut ticks_before_the_end = 0;
    for _ in 0..3000 {
        let out = s.tick(mine.clone());
        let got = cues.tick(s.world(), s.last_events(), &out);
        let ended = s
            .last_events()
            .iter()
            .any(|e| matches!(e.event, Event::Mine { .. }));
        if !ended {
            ticks_before_the_end += got.iter().filter(|(c, _)| *c == Cue::OreTick).count();
        }
        heard.extend(got);
        if flagship(&s).hold.total() >= u64::from(cap) {
            break;
        }
    }
    assert_eq!(flagship(&s).hold.count(&ICE), cap, "a full hold of ice");
    // A tick per tick that moved ice, while the run was still going.
    assert!(
        ticks_before_the_end + 1 >= cap as usize && ticks_before_the_end > 1,
        "{ticks_before_the_end} ore ticks before the run ended; hold {cap}"
    );
    // The unit that filled the hold plays hold-full instead of its tick.
    assert_eq!(heard.last().map(|(c, _)| *c), Some(Cue::HoldFull));
    // Letting go afterwards adds nothing.
    let out = s.tick(PlayerInput::default());
    assert!(cues.tick(s.world(), s.last_events(), &out).is_empty());
}
