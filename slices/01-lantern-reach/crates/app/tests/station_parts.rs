//! Station parts change through data alone (issue #47, 3.1): a game-master pack closes
//! the wharf for repairs and reopens it, puts a canteen up and takes it away, all
//! through `ctx.parts` at a calm moment. Each change shows where the player looks: the
//! keeper's line (or the rollback line) as their talk, the part's state and the key's
//! ribbon. Away from the dock, nothing changes.

use std::path::{Path, PathBuf};

use galaxy_app::game::{Game, GameOptions};
use galaxy_app::input::{Key, Keys};
use galaxy_app::session::Session;
use galaxy_app::ui::{self, UiState};
use galaxy_contract::parts::{PartState, Ribbon};
use galaxy_contract::prelude::*;
use galaxy_contract::talk::Channel;

const WHARF: StationId = StationId::from_static("station.lantern_wharf");
const WHARF_ROOM: StationPartId = StationPartId::from_static("part.wharf.wharf");
const MARKET: StationPartId = StationPartId::from_static("part.wharf.market");
const CANTEEN: StationPartId = StationPartId::from_static("gm.wharf_repairs.canteen");
const ICE: WareId = WareId::from_static("ware.ice");
const RATIONS: WareId = WareId::from_static("ware.rations");
const PACK: PackId = PackId::from_static("gm.wharf_repairs");

const CLOSED_LINE: &str = "Cradle's down for re-tarring. I fit at Kettle Dock till the pitch sets.";
const OPEN_LINE: &str = "Cradle's back up and the pitch is hard. Bring me your ship.";
const CANTEEN_LINE: &str = "We're raising a canteen frame by the lamps. Soup soon.";
const CANTEEN_GONE: &str = "We've taken the canteen frame down to re-seal it.";

fn packs() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../packs")
}

/// The example pack: each sale the player makes at the Wharf toggles the wharf closed and open; the
/// first purchase there puts a canteen up (building), the next takes it away; leaving
/// the dock tries to close the market, which must do nothing.
fn write_pack(dir: &Path) {
    std::fs::create_dir_all(dir.join("scripts")).unwrap();
    std::fs::write(
        dir.join("manifest.toml"),
        r#"id = "gm.wharf_repairs"
name = "The wharf closes for repairs"
version = "0.1.0"
uses = ["events.sell", "events.buy", "events.undock", "stations.read", "stations.write", "talk.write"]

[[script]]
id = "main"
file = "scripts/main.luau"
handlers = ["on.sell", "on.buy", "on.undock"]

[rollback]
speaker = "npc.orrin_vale"
place = "station.lantern_wharf"
text = "I've put the cradle back as it was."
"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("scripts/main.luau"),
        format!(
            r#"--!strict
local G = require("@galaxy")

return G.script({{
	id = "main",
	on = {{
		sell = function(ev: G.SellEvent, ctx: G.Ctx)
			if ev.ship ~= "ship.player" or ev.station ~= "station.lantern_wharf" then
				return
			end
			local part = ctx.parts:get("part.wharf.wharf")
			if part == nil then
				return
			end
			local why = ctx:cause({{ summary = "the cradle is re-tarred", cites = {{ ev.fact }} }})
			if part.state == "closed" then
				part.state = "open"
				part.line = "{OPEN_LINE}"
			else
				part.state = "closed"
				part.line = "{CLOSED_LINE}"
			end
			ctx.parts:put(part, why)
		end,
		buy = function(ev: G.BuyEvent, ctx: G.Ctx)
			if ev.ship ~= "ship.player" or ev.station ~= "station.lantern_wharf" then
				return
			end
			local why = ctx:cause({{ summary = "a canteen for the pier", cites = {{ ev.fact }} }})
			if ctx.parts:get("gm.wharf_repairs.canteen") == nil then
				ctx.parts:put({{
					id = "gm.wharf_repairs.canteen",
					station = "station.lantern_wharf",
					room = "canteen",
					module = "dome",
					anchor = "quay.northwest",
					keeper = "npc.ilse_marrow",
					state = "building",
					sign = "Canteen · opens when Kettle ships rations",
					line = "{CANTEEN_LINE}",
					rollback_line = "{CANTEEN_GONE}",
				}}, why)
			else
				ctx.parts:remove("gm.wharf_repairs.canteen", why)
			end
		end,
		undock = function(ev: G.UndockEvent, ctx: G.Ctx)
			if ev.ship ~= "ship.player" then
				return
			end
			local part = ctx.parts:get("part.wharf.market")
			if part == nil then
				return
			end
			local why = ctx:cause({{ summary = "too late for a calm moment", cites = {{ ev.fact }} }})
			part.state = "closed"
			part.line = "I've shut the stalls."
			ctx.parts:put(part, why)
		end,
	}},
}})
"#
        ),
    )
    .unwrap();
}

fn session(dir: &Path) -> Session {
    let mut state = galaxy_app::game::new_state(&packs(), 1, false).unwrap();
    let ship = state.world.player.ship.clone().unwrap();
    state
        .world
        .ships
        .get_mut(&ship)
        .unwrap()
        .hold
        .cargo
        .insert(ICE, 6);
    let mut game = Game::open(state, &GameOptions::headless(packs(), 1)).unwrap();
    write_pack(dir);
    let report = game
        .scripts()
        .load_layer(dir, game.sim().world(), game.sim().history());
    assert!(report.accepted(), "{report:#?}");
    game.scripts().clone().install(game.sim_mut());
    let mut s = Session::with_game(game);
    for _ in 0..3 {
        s.tick(PlayerInput::default());
    }
    s
}

fn trade(s: &mut Session, ware: WareId, side: TradeSide) {
    let ship = s.flagship().unwrap().id.clone();
    s.request_trade(TradeRequest {
        ship,
        station: WHARF,
        ware,
        side,
        quantity: 1,
        limit: None,
    });
    // The trade settles, then the pack's handler answers its event.
    for _ in 0..8 {
        s.tick(PlayerInput::default());
    }
}

/// The newest dock talk at the Wharf from `npc`.
fn last_said(s: &Session, npc: &str) -> Option<galaxy_contract::talk::TalkLine> {
    s.world()
        .talk
        .on(Channel::DockTalk)
        .filter(|l| {
            l.line.speaker.as_ref().is_some_and(|n| n.as_str() == npc)
                && l.line.place.as_ref() == Some(&WHARF)
        })
        .last()
        .map(|l| l.line.clone())
}

fn part<'a>(s: &'a Session, id: &StationPartId) -> Option<&'a StationPart> {
    s.world().station_parts.get(id)
}

/// The key's texts and the open room's texts for `id`.
fn key_and_room(s: &Session, id: &StationPartId) -> (Vec<String>, Vec<String>) {
    let w = s.world();
    let mut key = render::Overlay::default();
    ui::station::key(&mut key, w, &WHARF, &UiState::default(), None, ui::DESIGN);
    let mut room = render::Overlay::default();
    ui::rooms::panel(
        &mut room,
        w,
        s.sim().history(),
        &WHARF,
        id,
        &UiState::default(),
        ui::Notices::default(),
        ui::DESIGN,
    )
    .unwrap();
    let texts = |o: &render::Overlay| o.texts.iter().map(|t| t.text.clone()).collect();
    (texts(&key), texts(&room))
}

#[test]
fn a_pack_closes_the_wharf_for_repairs_and_reopens_it_through_data_alone() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = session(dir.path());
    assert_eq!(part(&s, &WHARF_ROOM).unwrap().state, PartState::Open);

    // Closed: the keeper's line, citing the cause; the part's state; the key's ribbon.
    trade(&mut s, ICE, TradeSide::Sell);
    let wharf = part(&s, &WHARF_ROOM).unwrap();
    assert_eq!(wharf.state, PartState::Closed, "{wharf:?}");
    assert_eq!(wharf.line, CLOSED_LINE);
    let cause = wharf.cause.as_ref().expect("the change names its cause");
    assert_eq!(cause.script.as_ref(), Some(&PACK));
    assert_eq!(ui::station::ribbon(s.world(), wharf), Some(Ribbon::Closed));
    let said = last_said(&s, "npc.orrin_vale").expect("Orrin says why");
    assert_eq!(said.text, CLOSED_LINE);
    assert_eq!(said.cites, cause.facts, "his line cites the cause");
    let (key, room) = key_and_room(&s, &WHARF_ROOM);
    assert!(key.iter().any(|t| t == "Closed"), "{key:?}");
    assert!(room.iter().any(|t| t == "CLOSED"), "{room:?}");
    assert!(room.iter().any(|t| t.contains(CLOSED_LINE)), "{room:?}");
    let mut o = render::Overlay::default();
    let (_, buttons) = ui::rooms::panel(
        &mut o,
        s.world(),
        s.sim().history(),
        &WHARF,
        &WHARF_ROOM,
        &UiState::default(),
        ui::Notices::default(),
        ui::DESIGN,
    )
    .unwrap();
    assert!(
        !buttons
            .iter()
            .any(|b| matches!(b.action, ui::UiAction::Service(_))),
        "nothing to fit while it's closed"
    );

    // Reopened: open again, a Changed ribbon, and the keeper says so.
    trade(&mut s, ICE, TradeSide::Sell);
    let wharf = part(&s, &WHARF_ROOM).unwrap();
    assert_eq!(wharf.state, PartState::Open);
    assert_eq!(ui::station::ribbon(s.world(), wharf), Some(Ribbon::Changed));
    assert_eq!(last_said(&s, "npc.orrin_vale").unwrap().text, OPEN_LINE);

    // A new part goes up as building, with a New ribbon and a key icon...
    trade(&mut s, RATIONS, TradeSide::Buy);
    let canteen = part(&s, &CANTEEN).expect("the canteen is up");
    assert_eq!(canteen.state, PartState::Building);
    assert_eq!(ui::station::ribbon(s.world(), canteen), Some(Ribbon::New));
    assert!(
        ui::station::key_rects(s.world(), &WHARF, ui::DESIGN)
            .iter()
            .any(|(id, _)| id == &CANTEEN)
    );
    assert_eq!(last_said(&s, "npc.ilse_marrow").unwrap().text, CANTEEN_LINE);
    // ...and taken away again with its rollback line.
    trade(&mut s, RATIONS, TradeSide::Buy);
    assert!(part(&s, &CANTEEN).is_none(), "removed");
    assert_eq!(last_said(&s, "npc.ilse_marrow").unwrap().text, CANTEEN_GONE);

    // Away from the dock nothing changes, and it isn't a fault.
    let mut keys = Keys::default();
    keys.event(Key::W, true, false);
    s.tick(keys.tick_input(true, None));
    keys.event(Key::W, false, false);
    for _ in 0..4 {
        s.tick(keys.tick_input(false, None));
    }
    assert!(s.flagship().unwrap().docked_at.is_none());
    assert_eq!(part(&s, &MARKET).unwrap().state, PartState::Open);
    let health = &s.world().packs.get(&PACK).unwrap().health;
    assert!(
        health.values().all(|h| h.aborts == 0 && !h.quarantined),
        "{health:?}"
    );
}

#[test]
fn an_older_save_without_parts_gets_them_on_open() {
    let mut state = galaxy_app::game::new_state(&packs(), 1, false).unwrap();
    state.world.station_parts = Default::default();
    let mut game = Game::open(state, &GameOptions::headless(packs(), 1)).unwrap();
    assert!(game.sim().world().station_parts.is_empty());
    game.step(&PlayerInput::default());
    let w = game.sim().world();
    let fresh = galaxy_core::new_game(&galaxy_app::game::core_data(&packs()), 1).unwrap();
    assert_eq!(
        w.station_parts, fresh.world.station_parts,
        "the authored parts"
    );
    // A second open of that save queues nothing more.
    assert_eq!(
        galaxy_core::parts_backfill(&galaxy_app::game::core_data(&packs()), w).unwrap(),
        None
    );
}

const HALT: StationId = StationId::from_static("gm.sixth_station.halt");
const WYNN: NpcId = NpcId::from_static("gm.sixth_station.wynn");
const KETTLE: StationId = StationId::from_static("station.kettle_dock");

/// A session docked at a station a game-master pack founded during play, like the
/// example `gm.sixth_station`'s Meltwater Halt: a stocked market and a dockmaster, and
/// no station parts. It's in the save, as an older save that founded it would have it.
fn docked_at_a_founded_halt() -> Session {
    let mut state = galaxy_app::game::new_state(&packs(), 1, false).unwrap();
    let w = &mut state.world;
    let mut halt = w.stations.get(&KETTLE).unwrap().clone();
    halt.id = HALT;
    halt.name = "Meltwater Halt".into();
    halt.position = Vec2::new(300.0, 850.0);
    halt.services = [Service::Market, Service::Dockmaster].into_iter().collect();
    w.stations.insert(HALT, halt.clone());
    let mut market = w.markets.get(&KETTLE).unwrap().clone();
    market.station = HALT;
    w.markets.insert(HALT, market);
    w.npcs.insert(
        WYNN,
        Npc {
            id: WYNN,
            name: "Wynn Halloran".into(),
            role: "dockmaster".into(),
            faction: halt.owner.clone(),
            location: Some(HALT),
        },
    );
    let ship = w.player.ship.clone().unwrap();
    let ship = w.ships.get_mut(&ship).unwrap();
    ship.region = halt.region.clone();
    ship.position = halt.position;
    ship.velocity = Vec2::new(0.0, 0.0);
    ship.docked_at = Some(HALT);
    ship.hold.cargo.insert(ICE, 6);
    let game = Game::open(state, &GameOptions::headless(packs(), 1)).unwrap();
    let mut s = Session::with_game(game);
    s.tick(PlayerInput::default());
    assert_eq!(s.flagship().unwrap().docked_at, Some(HALT));
    assert_eq!(galaxy_core::parts::of_station(s.world(), &HALT).count(), 0);
    s
}

/// The buttons of the room the UI has open now, drawn as play draws it.
fn open_room_buttons(s: &Session, ui_state: &UiState) -> Vec<ui::UiAction> {
    let part = galaxy_app::frame::open_room(s, ui_state).expect("a room is open");
    let mut o = render::Overlay::default();
    let (_, buttons) = ui::rooms::panel(
        &mut o,
        s.world(),
        s.sim().history(),
        &HALT,
        &part.id,
        ui_state,
        ui::Notices::default(),
        ui::DESIGN,
    )
    .expect("the room draws");
    buttons.into_iter().map(|b| b.action).collect()
}

#[test]
fn a_station_with_no_parts_still_opens_its_market_and_office() {
    let mut s = docked_at_a_founded_halt();
    // The key offers both rooms, for one click each.
    let mut key = render::Overlay::default();
    let icons = ui::station::key(
        &mut key,
        s.world(),
        &HALT,
        &UiState::default(),
        None,
        ui::DESIGN,
    );
    let opens: Vec<&StationPartId> = icons
        .iter()
        .filter_map(|b| match &b.action {
            ui::UiAction::OpenRoom(id) => Some(id),
            _ => None,
        })
        .collect();
    assert_eq!(opens.len(), 2, "an office and a market: {opens:?}");
    let texts: Vec<&str> = key.texts.iter().map(|t| t.text.as_str()).collect();
    assert!(
        texts.contains(&"Office") && texts.contains(&"Market"),
        "{texts:?}"
    );
    assert!(texts.contains(&"Wynn"), "kept by the dockmaster: {texts:?}");

    // The market: buy and sell.
    let mut ui_state = UiState::default();
    galaxy_app::script::run(&mut s, &mut ui_state, ":room:market").unwrap();
    let market = open_room_buttons(&s, &ui_state);
    assert!(
        market
            .iter()
            .any(|a| matches!(a, ui::UiAction::Trade { .. })),
        "{market:?}"
    );
    // Kettle's ice line, where the station buys it.
    ui_state.selected = ui::dock_lines(s.world(), &HALT)
        .iter()
        .position(|l| l.ware == ICE && l.side == TradeSide::Sell)
        .expect("the halt buys ice");
    let credits = s.world().player.credits;
    galaxy_app::script::run(&mut s, &mut ui_state, ":trade").unwrap();
    assert!(s.world().player.credits > credits, "the ice sold");

    // The office: ask what changed, and Wynn answers.
    galaxy_app::script::run(&mut s, &mut ui_state, ":room:dockmaster").unwrap();
    let office = open_room_buttons(&s, &ui_state);
    assert!(office.contains(&ui::UiAction::AskWhatChanged), "{office:?}");
    assert!(s.ask_what_changed(), "Wynn can be asked");
    assert!(office.contains(&ui::UiAction::CloseRoom));
}

#[test]
fn authored_stations_need_no_stand_in_rooms() {
    let s = Session::with_game(
        Game::open(
            galaxy_app::game::new_state(&packs(), 1, false).unwrap(),
            &GameOptions::headless(packs(), 1),
        )
        .unwrap(),
    );
    for station in s.world().stations.keys() {
        let key: Vec<StationPartId> = ui::station::key_rects(s.world(), station, ui::DESIGN)
            .into_iter()
            .map(|(id, _)| id)
            .collect();
        let parts: Vec<StationPartId> = ui::station::parts_here(s.world(), station)
            .into_iter()
            .map(|p| p.id.clone())
            .collect();
        assert_eq!(key, parts, "{station}: the key is its parts alone");
    }
}
