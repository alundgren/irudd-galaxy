//! The whole game, headless, through the real composition (`galaxy_app::game::Game`:
//! native systems, the script host with `core` and `gm-examples`, progress, and the
//! mind on the seeded `FakeMind`), driven the way a player drives it: WASD and Space
//! through `Keys`, dock-screen trades and purchases through the session.

use std::path::PathBuf;

use galaxy_app::game::{self, Game, GameOptions, MindMode};
use galaxy_app::hooks;
use galaxy_app::input::{Key, Keys};
use galaxy_app::session::Session;
use galaxy_app::trades::Settled;
use galaxy_app::ui::{self, Amount, UiState};
use galaxy_contract::prelude::*;
use galaxy_contract::talk::Channel;
use galaxy_core::SimState;

const WHARF: StationId = StationId::from_static("station.lantern_wharf");
const KETTLE: StationId = StationId::from_static("station.kettle_dock");
const ICE: WareId = WareId::from_static("ware.ice");
const LENS: UpgradeId = UpgradeId::from_static("upgrade.mining_lens");
const EXTENDER: UpgradeId = UpgradeId::from_static("upgrade.hold_extender");
const ODO: NpcId = NpcId::from_static("npc.odo_fenn");
const ILSE: NpcId = NpcId::from_static("npc.ilse_marrow");

fn packs() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../packs")
}

fn new_state(gm: bool) -> SimState {
    game::new_state(&packs(), 1, gm).unwrap()
}

fn session(state: SimState) -> Session {
    let opts = GameOptions {
        packs: packs(),
        gm: true,
        mind: MindMode::Fake { seed: 1 },
        hot_reload: false,
    };
    let mut s = Session::with_game(Game::open(state, &opts).unwrap());
    s.begin();
    s
}

/// A player at the keys: WASD toward where they want to go, Space for the one thing in
/// reach, the dock screen's buttons.
struct Pilot {
    s: Session,
    keys: Keys,
    /// Every goal line the HUD showed, in order, without repeats.
    goals: Vec<Option<String>>,
}

impl Pilot {
    fn new(s: Session) -> Self {
        Self {
            s,
            keys: Keys::default(),
            goals: Vec::new(),
        }
    }

    fn ship(&self) -> &Ship {
        self.s.flagship().unwrap()
    }

    fn docked(&self) -> Option<StationId> {
        self.ship().docked_at.clone()
    }

    fn tick(&mut self) {
        let docked = self.docked().is_some();
        let input = self.keys.tick_input(docked, None);
        self.s.tick(input);
        let goal = hooks::goal_line(self.s.world());
        if self.goals.last() != Some(&goal) {
            self.goals.push(goal);
        }
    }

    fn ticks(&mut self, n: u32) {
        for _ in 0..n {
            self.tick();
        }
    }

    fn goal(&self) -> Option<String> {
        hooks::goal_line(self.s.world())
    }

    fn let_go(&mut self) {
        for k in [Key::W, Key::A, Key::S, Key::D, Key::Space] {
            self.keys.release(k);
        }
    }

    /// Holds the WASD keys that point at `to` until within `within` of it.
    fn fly_to(&mut self, to: Vec2, within: f32) {
        for _ in 0..3000 {
            let at = self.ship().position;
            let d = to - at;
            if d.length() <= within {
                self.let_go();
                self.ticks(3);
                return;
            }
            let tol = within * 0.5;
            for (key, on) in [
                (Key::D, d.x > tol),
                (Key::A, d.x < -tol),
                (Key::W, d.y > tol),
                (Key::S, d.y < -tol),
            ] {
                if on {
                    self.keys.press(key);
                } else {
                    self.keys.release(key);
                }
            }
            self.tick();
        }
        panic!("never got to {to:?} (at {:?})", self.ship().position);
    }

    /// A tap of Space (or the Undock button).
    fn tap(&mut self) {
        self.keys.press_action();
        self.ticks(2);
    }

    fn undock(&mut self) {
        assert!(self.docked().is_some());
        self.tap();
        self.ticks(3);
        assert!(self.docked().is_none(), "undocked");
    }

    fn dock_at(&mut self, station: &StationId) {
        let s = self.s.world().stations.get(station).unwrap().clone();
        self.fly_to(s.position, s.dock_radius * 0.5);
        self.tap();
        self.ticks(3);
        assert_eq!(self.docked().as_ref(), Some(station), "docked");
    }

    /// Flies to the nearest ice rock with ice left and holds Space beside it until the
    /// hold is full.
    fn mine_ice_until_full(&mut self) {
        for _ in 0..20 {
            let cap = galaxy_contract::modifiers::hold_capacity(self.s.world(), self.ship());
            if self.ship().hold.total() >= cap {
                return;
            }
            let at = self.ship().position;
            let world = self.s.world();
            let rock = world
                .rocks
                .values()
                .filter(|r| r.remaining > 5)
                .filter(|r| {
                    world
                        .rock_types
                        .get(&r.rock_type)
                        .is_some_and(|t| t.ware == ICE)
                })
                .filter(|r| {
                    galaxy_ship::space::rock_region(world, r) == Some(&self.ship().region)
                })
                .min_by(|a, b| {
                    a.position
                        .distance(at)
                        .total_cmp(&b.position.distance(at))
                })
                .expect("an ice rock")
                .clone();
            let reach = galaxy_ship::space::rock_radius(world, &rock)
                + world.ship_rules.mining.reach * 0.5;
            self.fly_to(rock.position, reach);
            self.keys.press(Key::Space);
            for _ in 0..1200 {
                self.tick();
                let world = self.s.world();
                let full =
                    self.ship().hold.total() >= galaxy_contract::modifiers::hold_capacity(world, self.ship());
                let spent = world.rocks.get(&rock.id).is_none_or(|r| r.remaining == 0);
                if full || spent {
                    break;
                }
            }
            self.keys.release(Key::Space);
            self.ticks(2);
        }
        panic!("the hold never filled");
    }

    /// Sells everything of `ware` with the dock screen's "Sell all".
    fn sell_all(&mut self, ware: &WareId) {
        let station = self.docked().unwrap();
        let world = self.s.world();
        let lines = ui::dock_lines(world, &station);
        let line = lines.iter().find(|l| &l.ware == ware).unwrap();
        let req = ui::trade_request(world, &station, line, Amount::Most).unwrap();
        self.s.request_trade(req);
        for _ in 0..30 {
            let docked = self.docked().is_some();
            let input = self.keys.tick_input(docked, None);
            let out = self.s.tick(input);
            if matches!(out.trade, Some(Settled::Applied | Settled::Refused)) {
                break;
            }
        }
        assert_eq!(self.ship().hold.count(ware), 0, "{:?}", self.s.trades.notice);
    }

    fn buy(&mut self, req: ServiceRequest) {
        self.s.request_service(req);
        self.ticks(3);
    }
}

fn status(s: &Session, beat: &str) -> Option<BeatStatus> {
    s.world().progress.beats.get(beat).map(|b| b.status)
}

fn said(s: &Session, by: &NpcId) -> Vec<String> {
    s.world()
        .talk
        .on(Channel::DockTalk)
        .filter(|l| l.line.speaker.as_ref() == Some(by))
        .map(|l| l.line.text.clone())
        .collect()
}

#[test]
fn act_one_plays_through_b01_to_b05_with_the_goal_line_following() {
    let mut p = Pilot::new(session(new_state(true)));
    // gm-examples is installed on a new game and runs beside core.
    let packs: Vec<String> = p.s.game().reports().iter().map(|r| r.pack.to_string()).collect();
    assert_eq!(packs, ["core", "gm.kettle_growth"]);
    p.tick();
    assert_eq!(
        p.goal().as_deref(),
        Some("Ilse needs ice for the Kettle Dock greenhouse. Undock and look for the pale rocks.")
    );
    assert_eq!(hooks::goal_place(p.s.world()), Some(KETTLE));

    // b01: undock.
    p.undock();
    p.ticks(3);
    assert_eq!(status(&p.s, "beat.b01_undock"), Some(BeatStatus::Completed));
    assert_eq!(
        p.goal().as_deref(),
        Some("Hold Space beside an ice rock until your hold is full.")
    );

    // b02: a full hold of ice, by hand.
    p.mine_ice_until_full();
    p.ticks(3);
    assert_eq!(status(&p.s, "beat.b02_mine_by_hand"), Some(BeatStatus::Completed));
    assert!(p.goal().unwrap().starts_with("Fly into Kettle Dock's ring"));

    // b03: Odo takes the ice for the greenhouse and says so; Kettle grows a dome.
    p.dock_at(&KETTLE);
    p.ticks(5);
    assert_eq!(status(&p.s, "beat.b03_first_delivery"), Some(BeatStatus::Completed));
    assert_eq!(p.ship().hold.count(&ICE), 0, "handed over");
    assert!(
        said(&p.s, &ODO).iter().any(|t| t == "20 ice. The beans will hear about this."),
        "{:?}",
        said(&p.s, &ODO)
    );
    // The talk panel shows it, naming Odo and Kettle Dock.
    let talk = hooks::talk_lines(p.s.world(), p.s.sim().history(), 3);
    assert!(talk.iter().any(|l| l.speaker.as_ref() == Some(&ODO)
        && l.place.as_ref() == Some(&KETTLE)));
    assert_eq!(p.s.world().stations.get(&KETTLE).unwrap().build.stage, 1);
    assert_eq!(
        p.goal().as_deref(),
        Some("Orrin can fit a mining lens for 600 credits. Sell what you mine to pay for it.")
    );
    assert_eq!(hooks::goal_place(p.s.world()), Some(WHARF), "Orrin is at the Wharf");

    // b04: mine and sell until the lens is affordable, then have Orrin fit it.
    let mut trips = 0;
    while p.s.world().player.credits < 600 * CENTS_PER_CREDIT {
        p.undock();
        p.mine_ice_until_full();
        p.dock_at(&KETTLE);
        p.sell_all(&ICE);
        trips += 1;
        assert!(trips < 8, "selling ice should pay for the lens");
    }
    p.undock();
    p.dock_at(&WHARF);
    let before = p.s.world().player.credits;
    p.buy(ServiceRequest::Fit { upgrade: LENS });
    assert!(p.s.world().player.fittings.contains(&LENS), "{:?}", p.s.services.notice);
    assert_eq!(p.s.world().player.credits, before - 600 * CENTS_PER_CREDIT);
    p.ticks(3);
    assert_eq!(status(&p.s, "beat.b04_first_sale"), Some(BeatStatus::Completed));
    assert_eq!(
        p.goal().as_deref(),
        Some("Press P or Esc whenever you like. The whole galaxy waits for you.")
    );

    // b05: the PAUSED card. The fact commits on the first tick after it closes.
    p.s.pause();
    p.ticks(5);
    p.s.resume();
    p.ticks(4);
    assert_eq!(status(&p.s, "beat.b05_pause"), Some(BeatStatus::Completed));
    assert_eq!(p.goal(), None, "Act 1 is done; b06 waits on more dockings");

    let goals: Vec<String> = p.goals.iter().flatten().cloned().collect();
    assert_eq!(goals.len(), 5, "{goals:#?}");
    assert!(goals[0].starts_with("Ilse needs ice"));
    assert!(goals[1].starts_with("Hold Space"));
    assert!(goals[2].starts_with("Fly into Kettle Dock"));
    assert!(goals[3].starts_with("Orrin can fit a mining lens"));
    assert!(goals[4].starts_with("Press P or Esc"));

    // The fast-model layer ran on the fake (never the network) and booked its picks.
    let picks = p
        .s
        .sim()
        .history()
        .facts()
        .iter()
        .filter(|f| f.kind.as_str() == galaxy_mind::LINE_FACT)
        .count();
    assert!(picks > 0, "dock talk picked at least one authored line");
}

#[test]
fn the_core_pack_alone_plays_b01_and_core_only_saves_stay_core_only() {
    let state = new_state(false);
    assert!(!state.world.player.flags.contains(&game::GM_FLAG));
    let mut p = Pilot::new(session(state));
    let packs: Vec<String> = p.s.game().reports().iter().map(|r| r.pack.to_string()).collect();
    assert_eq!(packs, ["core"]);
    p.tick();
    p.undock();
    p.ticks(3);
    assert_eq!(status(&p.s, "beat.b01_undock"), Some(BeatStatus::Completed));
}

/// A session docked at the Wharf, b03 done, with `credits`.
fn at_the_wharf_after_b03(credits: i64) -> Session {
    let mut p = Pilot::new(session(new_state(true)));
    p.tick();
    p.undock();
    p.mine_ice_until_full();
    p.dock_at(&KETTLE);
    p.ticks(5);
    assert_eq!(status(&p.s, "beat.b03_first_delivery"), Some(BeatStatus::Completed));
    p.undock();
    p.dock_at(&WHARF);
    p.ticks(3);
    let delta = credits - p.s.world().player.credits;
    p.s.submit(
        CommandBatch::new(Cause::system("test", "the player's savings")).with(Command::AdjustCredits { delta }),
    );
    p.ticks(1);
    p.s
}

#[test]
fn station_services_list_offers_and_refuse_calmly() {
    let mut s = at_the_wharf_after_b03(100 * CENTS_PER_CREDIT);
    let (world, history) = (s.world(), s.sim().history());
    let services = hooks::station_services(world, history, &WHARF);
    let hullwright = services
        .iter()
        .find(|e| e.keeper_name.as_deref() == Some("Orrin Vale"))
        .expect("Orrin's list");
    let lens = hullwright
        .offers
        .iter()
        .find(|o| o.request == ServiceRequest::Fit { upgrade: LENS })
        .expect("the lens is on Orrin's list after b03");
    assert_eq!(lens.price, "600 cr");
    assert_eq!(lens.effect, "Mining rate x1.5");
    assert_eq!(
        lens.blocked.as_deref(),
        Some("Mining lens costs 600 cr. You have 100 cr.")
    );
    assert!(services.iter().any(|e| e.can_ask), "the dockmaster can be asked");

    // A refused purchase: one calm line, nothing charged.
    s.request_service(ServiceRequest::Fit { upgrade: LENS });
    assert_eq!(
        s.services.notice.as_deref(),
        Some("Mining lens costs 600 cr. You have 100 cr.")
    );
    s.tick(PlayerInput::default());
    assert!(!s.world().player.fittings.contains(&LENS));
    assert_eq!(s.world().player.credits, 100 * CENTS_PER_CREDIT);
    // Something not on offer yet.
    s.request_service(ServiceRequest::Fit { upgrade: EXTENDER });
    assert_eq!(
        s.services.notice.as_deref(),
        Some("Hold extender isn't on offer yet.")
    );

    // With the money it goes through, and the notice clears.
    s.submit(
        CommandBatch::new(Cause::system("test", "a windfall"))
            .with(Command::AdjustCredits { delta: 1000 * CENTS_PER_CREDIT }),
    );
    s.tick(PlayerInput::default());
    s.request_service(ServiceRequest::Fit { upgrade: LENS });
    s.tick(PlayerInput::default());
    assert!(s.world().player.fittings.contains(&LENS));
    assert_eq!(s.services.notice, None);
    assert_eq!(s.world().player.credits, 500 * CENTS_PER_CREDIT);
    // Once fitted it leaves the list.
    let services = hooks::station_services(s.world(), s.sim().history(), &WHARF);
    assert!(services
        .iter()
        .flat_map(|e| &e.offers)
        .all(|o| o.request != ServiceRequest::Fit { upgrade: LENS }));
}

#[test]
fn the_dock_screen_draws_services_with_working_buttons() {
    let s = at_the_wharf_after_b03(800 * CENTS_PER_CREDIT);
    let mut o = render::Overlay::default();
    let canvas = render::glam::Vec2::new(1280.0, 720.0);
    let buttons = ui::dock_panel(
        &mut o,
        s.world(),
        s.sim().history(),
        &WHARF,
        &UiState::default(),
        ui::Notices::default(),
        canvas,
    );
    assert!(buttons
        .iter()
        .any(|b| b.action == ui::UiAction::Service(ServiceRequest::Fit { upgrade: LENS })));
    assert!(buttons.iter().any(|b| b.action == ui::UiAction::AskWhatChanged));
    assert!(o.texts.iter().any(|t| t.text == "Mining lens  ·  600 cr"));
    for b in &buttons {
        assert!(b.rect.y + b.rect.h <= canvas.y, "{b:?} fits");
    }
}

#[test]
fn asking_what_changed_cites_facts_and_records_the_question() {
    let mut s = at_the_wharf_after_b03(40 * CENTS_PER_CREDIT);
    let answer = s.ask_what_changed().cloned().expect("the Wharf has a dockmaster");
    assert_eq!(answer.speaker, Some(ILSE));
    assert!(!answer.lines.is_empty(), "the dome went up at Kettle since the first visit");
    for line in &answer.lines {
        assert!(line.validate().is_ok(), "{line:?}");
        for f in &line.cites {
            assert!(s.sim().history().fact(*f).is_some(), "cites a real fact");
        }
    }
    s.tick(PlayerInput::default());
    let asked = s
        .sim()
        .history()
        .facts()
        .iter()
        .filter(|f| f.kind == hooks::ASKED)
        .count();
    assert_eq!(asked, 1);
    // Said, so ambient dock talk won't cite those facts again.
    for line in answer.lines.iter().filter(|l| l.channel == Channel::DockTalk) {
        assert!(s.world().talk.lines.values().any(|l| l.line.cites == line.cites));
    }
}

#[test]
fn modifiers_change_mining_rate_and_hold() {
    // The same few seconds of mining by hand, without and with the lens.
    let mined = |fit: &[UpgradeId]| {
        let mut state = new_state(false);
        state.world.player.fittings.extend(fit.iter().cloned());
        let mut p = Pilot::new(session(state));
        p.tick();
        p.undock();
        let rock = p.s.world().rocks.get("rock.shallows.ice.09").unwrap().clone();
        let reach = galaxy_ship::space::rock_radius(p.s.world(), &rock)
            + p.s.world().ship_rules.mining.reach * 0.5;
        p.fly_to(rock.position, reach);
        p.keys.press(Key::Space);
        p.ticks(150);
        p.keys.release(Key::Space);
        p.ship().hold.count(&ICE)
    };
    let plain = mined(&[]);
    let lens = mined(&[LENS]);
    assert!(plain >= 3, "{plain}");
    assert!(
        lens as f32 >= plain as f32 * 1.4,
        "the lens mines faster: {lens} vs {plain}"
    );

    // The hold extender: 10 more units than the Wren's 20, everywhere the hold counts.
    let mut state = new_state(false);
    state.world.player.fittings.insert(EXTENDER);
    let mut p = Pilot::new(session(state));
    p.tick();
    p.undock();
    p.mine_ice_until_full();
    assert_eq!(p.ship().hold.count(&ICE), 30);
}

#[test]
fn click_controls_work_once_their_flags_are_on() {
    use galaxy_ship::controls::{Capability, set_capability};
    let state = new_state(false);
    let mut s = session(state);
    let caps = [
        Capability::ClickFly,
        Capability::ClickMine,
        Capability::ClickDock,
        Capability::MapTravel,
        Capability::Queues,
    ];
    let commands = caps
        .iter()
        .map(|c| set_capability(s.world(), *c, true))
        .collect();
    s.submit(CommandBatch {
        cause: Cause::system("test", "every click control"),
        commands,
    });
    s.tick(PlayerInput::default());
    let mut ui = UiState::default();
    // Undock, then click a point near the ship: it flies there and stops.
    galaxy_app::script::run(&mut s, &mut ui, "space,wait*5").unwrap();
    let at = s.flagship().unwrap().position;
    let target = at + Vec2::new(150.0, -150.0); // clear of the Wharf's ring
    galaxy_app::script::run(&mut s, &mut ui, &format!(":click:{}:{}", target.x, target.y))
        .unwrap();
    assert!(matches!(
        s.flagship().unwrap().orders.orders.first(),
        Some(Order::Fly { to: FlyTarget::Point { .. } })
    ));
    galaxy_app::script::run(&mut s, &mut ui, "wait*200").unwrap();
    assert!(s.flagship().unwrap().position.distance(target) < 5.0);

    // Click a rock: mine it. Shift-click Kettle Dock: queued after it.
    let rock = s.world().rocks.get("rock.shallows.ice.09").unwrap().position;
    let kettle = s.world().stations.get(&KETTLE).unwrap().position;
    galaxy_app::script::run(
        &mut s,
        &mut ui,
        &format!(":click:{}:{},:qclick:{}:{}", rock.x, rock.y, kettle.x, kettle.y),
    )
    .unwrap();
    let orders = s.flagship().unwrap().orders.orders.clone();
    assert!(matches!(orders.first(), Some(Order::Mine { .. })), "{orders:?}");
    assert!(
        matches!(orders.get(1), Some(Order::Dock { station }) if *station == KETTLE),
        "{orders:?}"
    );
    // The queue runs: mine until full, then dock at Kettle (where b03 hands the ice to
    // Odo).
    galaxy_app::script::run(&mut s, &mut ui, "wait*2400").unwrap();
    assert_eq!(s.flagship().unwrap().docked_at, Some(KETTLE));
    let mined = s
        .world()
        .counters
        .get(CounterKind::UnitsMined, &CounterScope::Player);
    assert_eq!(mined, 20);
    assert_eq!(status(&s, "beat.b03_first_delivery"), Some(BeatStatus::Completed));

    // Map travel: click another region; the route goes through the lanes.
    galaxy_app::script::run(&mut s, &mut ui, ":mapclick:region.cinder_drift").unwrap();
    assert!(matches!(
        s.flagship().unwrap().orders.orders.first(),
        Some(Order::Fly { to: FlyTarget::Region { .. } })
    ));
}

#[test]
fn the_mind_without_a_key_says_authored_lines() {
    // Live mode with no key falls back to no model, says so once, and never calls out.
    if std::env::var_os("OPENROUTER_API_KEY").is_some() {
        return; // A developer's live key: this test is about running without one.
    }
    let opts = GameOptions {
        packs: packs(),
        gm: true,
        mind: MindMode::Live,
        hot_reload: false,
    };
    let mut game = Game::open(new_state(true), &opts).unwrap();
    let notes = game.take_notes();
    assert!(
        notes.iter().any(|n| n.contains("OPENROUTER_API_KEY isn't set")),
        "{notes:?}"
    );
    assert!(game.take_notes().is_empty(), "said once");
}

fn facts_of<'a>(s: &'a Session, kind: &str) -> Vec<&'a Fact> {
    s.sim()
        .history()
        .facts()
        .iter()
        .filter(|f| f.kind.as_str() == kind)
        .collect()
}

fn with_ice(mut state: SimState, n: u32) -> SimState {
    let id = state.world.player.ship.clone().unwrap();
    state.world.ships.get_mut(&id).unwrap().hold.cargo.insert(ICE, n);
    state
}

#[test]
fn selling_part_of_a_stack_here_and_the_rest_elsewhere_is_a_split_sale() {
    let mut p = Pilot::new(session(with_ice(new_state(false), 10)));
    p.ticks(3);
    let world = p.s.world();
    let line = ui::dock_lines(world, &WHARF)
        .into_iter()
        .find(|l| l.ware == ICE)
        .unwrap();
    let mut req = ui::trade_request(world, &WHARF, &line, Amount::One).unwrap();
    req.quantity = 4;
    p.s.request_trade(req);
    p.ticks(4);
    assert_eq!(p.ship().hold.count(&ICE), 6);
    assert!(facts_of(&p.s, "split_sale").is_empty());
    assert_eq!(p.s.world().economy.split_stacks.get(&ICE), Some(&WHARF));
    p.undock();
    p.dock_at(&KETTLE);
    p.sell_all(&ICE);
    p.ticks(2);
    let split = facts_of(&p.s, "split_sale");
    assert_eq!(split.len(), 1);
    assert_eq!(split[0].subjects.station, Some(KETTLE));
    assert_eq!(split[0].subjects.ware, Some(ICE));
    assert!(p.s.world().economy.split_stacks.is_empty());
}

#[test]
fn docking_where_the_snapshot_is_old_is_a_far_docking() {
    let mut state = new_state(false);
    state.world.economy.rules.scouting.far_docking = 1;
    let mut p = Pilot::new(session(state));
    p.ticks(3);
    p.undock();
    p.dock_at(&KETTLE);
    p.ticks(2);
    assert!(facts_of(&p.s, "far_docking").is_empty(), "Kettle was never seen");
    p.undock();
    p.dock_at(&WHARF);
    p.ticks(2);
    let far = facts_of(&p.s, "far_docking");
    assert_eq!(far.len(), 1, "the Wharf was last seen two dockings ago");
    assert_eq!(far[0].subjects.station, Some(WHARF));
    assert_eq!(far[0].subjects.ship, p.s.world().player.ship);
}

#[test]
fn a_tandem_hands_cargo_to_a_tender_alongside() {
    let thimble = ShipId::from_static("craft.thimble");
    let mut state = with_ice(new_state(false), 10);
    let id = state.world.player.ship.clone().unwrap();
    let mut tender = state.world.ships.get(&id).unwrap().clone();
    tender.id = thimble.clone();
    tender.name = "Thimble".into();
    tender.hull = HullId::from_static("hull.tender");
    tender.hold = Hold::default();
    state.world.ships.insert(thimble.clone(), tender);
    state.world.ships.get_mut(&id).unwrap().hull = HullId::from_static("hull.tandem");
    let mut s = session(state);
    s.tick(PlayerInput::default());
    let transfer = |to: &ShipId| OrderQueue {
        orders: vec![Order::Transfer {
            ware: ICE,
            quantity: Quantity::Units { n: 6 },
            to: to.clone(),
        }],
        ..OrderQueue::default()
    };
    s.submit(
        CommandBatch::new(Cause::system("test", "hand Thimble some ice")).with(Command::SetOrders {
            ship: id.clone(),
            orders: transfer(&thimble),
        }),
    );
    for _ in 0..4 {
        s.tick(PlayerInput::default());
    }
    assert_eq!(s.world().ships.get(&thimble).unwrap().hold.count(&ICE), 6);
    assert_eq!(s.flagship().unwrap().hold.count(&ICE), 4);
    let t = facts_of(&s, "transfer");
    assert_eq!(t.len(), 1);
    assert_eq!(t[0].subjects.ship.as_ref(), Some(&id));
    assert_eq!(t[0].amount, Some(6));
}

#[test]
fn finishing_a_queue_of_three_orders_is_queue_done() {
    let mut s = session(new_state(false));
    s.tick(PlayerInput::default());
    let id = s.world().player.ship.clone().unwrap();
    s.submit(
        CommandBatch::new(Cause::system("test", "a short queue")).with(Command::SetOrders {
            ship: id.clone(),
            orders: OrderQueue {
                orders: vec![
                    Order::Undock {},
                    Order::Wait {
                        until: Condition::HoldEmpty,
                    },
                    Order::Dock { station: WHARF },
                ],
                ..OrderQueue::default()
            },
        }),
    );
    for _ in 0..120 {
        s.tick(PlayerInput::default());
    }
    let done = facts_of(&s, "queue_done");
    assert_eq!(done.len(), 1);
    assert_eq!(done[0].subjects.ship.as_ref(), Some(&id));
    assert_eq!(done[0].amount, Some(3));
}
