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

/// A player at the keys: A/D to face where they want to go and W to get there (and to
/// fly off a dock), Space for the one thing in reach, the dock screen's buttons.
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

    /// Turns toward `to` with A/D and holds W while there's room to stop, then lets go
    /// and lets the auto-brake park the ship within `within` of it.
    fn fly_to(&mut self, to: Vec2, within: f32) {
        for _ in 0..3000 {
            let ship = self.ship();
            if ship.position.distance(to) <= within && ship.velocity == Vec2::ZERO {
                self.let_go();
                self.ticks(3);
                return;
            }
            // The keys `steer_to` would press, rounded to what a keyboard can do: turn
            // while more than half a tick's turn is left, W while slower than wanted.
            let feel = galaxy_ship::flight::Feel::of_ship(self.s.world(), ship).unwrap();
            let motion = galaxy_ship::flight::Motion::of(ship);
            let axes = galaxy_ship::flight::steer_to(&feel, motion, to, within);
            let along = ship.velocity.x * motion.forward().x + ship.velocity.y * motion.forward().y;
            for (key, on) in [
                (Key::D, axes.x > 0.5),
                (Key::A, axes.x < -0.5),
                (Key::W, axes.y > 0.0 && axes.y * feel.speed > along),
                (Key::S, false),
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

    /// A tap of Space.
    fn tap(&mut self) {
        self.keys.press_action();
        self.ticks(2);
    }

    /// Flies off: a tap of W.
    fn undock(&mut self) {
        assert!(self.docked().is_some());
        self.keys.press(Key::W);
        self.ticks(1);
        self.keys.release(Key::W);
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
                .filter(|r| galaxy_ship::space::rock_region(world, r) == Some(&self.ship().region))
                .min_by(|a, b| a.position.distance(at).total_cmp(&b.position.distance(at)))
                .expect("an ice rock")
                .clone();
            let reach =
                galaxy_ship::space::rock_radius(world, &rock) + world.ship_rules.mining.reach * 0.5;
            let spot = rock.position + (at - rock.position).normalize_or_zero() * reach;
            self.fly_to(spot, 10.0);
            self.keys.press(Key::Space);
            for _ in 0..1200 {
                self.tick();
                let world = self.s.world();
                let full = self.ship().hold.total()
                    >= galaxy_contract::modifiers::hold_capacity(world, self.ship());
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
        assert_eq!(
            self.ship().hold.count(ware),
            0,
            "{:?}",
            self.s.trades.notice
        );
    }

    /// Picks `choice` from `npc` at the dock, as its button does.
    fn answer(&mut self, npc: &str, choice: &str) {
        assert!(
            self.s.reply(npc.parse().unwrap(), choice.parse().unwrap()),
            "docked to answer"
        );
        self.ticks(5);
    }

    fn buy(&mut self, req: ServiceRequest) {
        self.s.request_service(req);
        self.ticks(3);
    }

    /// Asks the dockmaster what changed, as the button does, and waits for the answer.
    fn ask(&mut self) -> hooks::Answer {
        assert!(self.s.ask_what_changed(), "a dockmaster to ask");
        for _ in 0..10 {
            self.tick();
            if let Some(a) = &self.s.answer {
                return a.clone();
            }
        }
        panic!("the dockmaster never answered");
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
    play_act_one(&mut p);

    // Dock talk came from the core pack's line pools, each line booked as it was said.
    let said_lines =
        p.s.sim()
            .history()
            .facts()
            .iter()
            .filter(|f| f.kind.as_str() == "core.npc_line")
            .count();
    assert!(
        said_lines > 0,
        "the people of the Shallows said authored lines"
    );
}

/// Plays b01 to b05 at the keys and the dock screen, checking the goal line follows.
fn play_act_one(p: &mut Pilot) {
    // gm-examples is installed on a new game and runs beside core.
    let packs: Vec<String> =
        p.s.game()
            .reports()
            .iter()
            .map(|r| r.pack.to_string())
            .collect();
    assert_eq!(
        packs,
        [
            "core",
            "gm.ancients_bearing",
            "gm.chronicle",
            "gm.compact_plant",
            "gm.kettle_growth",
            "gm.rock_tagging",
            "gm.shortage_tuning",
            "gm.sixth_station",
            "gm.territory_map"
        ]
    );
    p.tick();
    assert_eq!(
        p.goal().as_deref(),
        Some(
            "Ilse needs ice for Odo's greenhouse at Kettle Dock. Fly off with W, turn with A and D, and look for the pale rocks."
        )
    );
    assert_eq!(hooks::goal_place(p.s.world()), Some(KETTLE));
    // Before anyone is named in a goal, Ilse names herself, the ship, Odo and Rimewater.
    p.ticks(3);
    let ilse = said(&p.s, &ILSE);
    assert!(
        ilse.iter().any(|t| t.contains("I'm Ilse")
            && t.contains("The Wren's yours")
            && t.contains("Odo at Kettle Dock")
            && t.contains("Rimewater")),
        "{ilse:?}"
    );

    // b01: fly off.
    p.undock();
    p.ticks(3);
    assert_eq!(status(&p.s, "beat.b01_undock"), Some(BeatStatus::Completed));
    assert_eq!(
        p.goal().as_deref(),
        Some(
            "Ilse wants a full hold of ice: hold Space beside a pale rock until your hold is full."
        )
    );

    // b02: a full hold of ice, by hand.
    p.mine_ice_until_full();
    p.ticks(3);
    assert_eq!(
        status(&p.s, "beat.b02_mine_by_hand"),
        Some(BeatStatus::Completed)
    );
    assert!(
        p.goal()
            .unwrap()
            .starts_with("Ilse asks you to take the ice to Odo")
    );

    // b03: Odo asks; nothing leaves the hold until the player picks Deliver. Then he
    // takes the ice for the greenhouse and says so; Kettle grows a dome.
    p.dock_at(&KETTLE);
    p.ticks(5);
    assert_eq!(p.ship().hold.count(&ICE), 20, "docking hands nothing over");
    p.answer("npc.odo_fenn", "choice.deliver_ice");
    assert_eq!(
        status(&p.s, "beat.b03_first_delivery"),
        Some(BeatStatus::Completed)
    );
    assert_eq!(p.ship().hold.count(&ICE), 0, "handed over");
    assert!(
        said(&p.s, &ODO)
            .iter()
            .any(|t| t == "20 ice. The beans will hear about this."),
        "{:?}",
        said(&p.s, &ODO)
    );
    // It waits as a bubble at Kettle Dock, naming Odo and the dock.
    let waiting: Vec<_> =
        p.s.talk
            .queue(&KETTLE)
            .filter_map(|id| p.s.world().talk.lines.get(&id))
            .collect();
    assert!(waiting.iter().any(|l| {
        l.line.speaker.as_ref() == Some(&ODO) && l.line.place.as_ref() == Some(&KETTLE)
    }));
    assert_eq!(p.s.world().stations.get(&KETTLE).unwrap().build.stage, 1);
    // Odo names Orrin, the hullwright, as the goal turns to him.
    assert!(
        said(&p.s, &ODO)
            .iter()
            .any(|t| t.starts_with("Orrin Vale, the hullwright back at the Wharf")),
        "{:?}",
        said(&p.s, &ODO)
    );
    assert_eq!(
        p.goal().as_deref(),
        Some(
            "Orrin, the Wharf's hullwright, can fit a mining lens for 600 credits. Sell what you mine to pay for it."
        )
    );
    assert_eq!(
        hooks::goal_place(p.s.world()),
        Some(WHARF),
        "Orrin is at the Wharf"
    );

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
    assert!(
        p.s.world().player.fittings.contains(&LENS),
        "{:?}",
        p.s.services.notice
    );
    assert_eq!(p.s.world().player.credits, before - 600 * CENTS_PER_CREDIT);
    p.ticks(3);
    assert_eq!(
        status(&p.s, "beat.b04_first_sale"),
        Some(BeatStatus::Completed)
    );
    assert_eq!(p.goal().as_deref(), Some("P or Esc · pause"));

    // b05: the PAUSED card. The fact commits on the first tick after it closes.
    p.s.pause();
    p.ticks(5);
    p.s.resume();
    p.ticks(4);
    assert_eq!(status(&p.s, "beat.b05_pause"), Some(BeatStatus::Completed));
    // The trips that paid for the lens made the dockings b06 waits for: it opens at once.
    assert_eq!(
        p.goal().as_deref(),
        Some(
            "Orrin can fit a nav chip at the Wharf: click a point near your ship, the Wren, and she flies there."
        )
    );

    let goals: Vec<String> = p.goals.iter().flatten().cloned().collect();
    assert_eq!(goals.len(), 6, "{goals:#?}");
    assert!(goals[0].starts_with("Ilse needs ice"));
    assert!(goals[1].starts_with("Ilse wants a full hold"));
    assert!(goals[2].starts_with("Ilse asks you to take the ice"));
    assert!(goals[3].starts_with("Orrin, the Wharf's hullwright, can fit a mining lens"));
    assert!(goals[4].starts_with("P or Esc"));
    assert!(goals[5].starts_with("Orrin can fit a nav chip"));
}

/// The play path and the script path agree past Act 1: from b05's end, at the keys and
/// the dock screen through the real composition, b06 (the nav chip, paid for with ice)
/// and b07 (Ilse's plate, by lane, to Rook at Cinder Hook).
#[test]
fn act_one_crosses_into_act_two_at_the_keys() {
    let mut p = Pilot::new(session(new_state(true)));
    play_act_one(&mut p);
    assert_eq!(status(&p.s, "beat.b06_nav_chip"), Some(BeatStatus::Active));

    // b06: more ice to Kettle until the chip is affordable, then Orrin fits it.
    let chip = UpgradeId::from_static("upgrade.nav_chip");
    let mut trips = 0;
    let chip_cost = p.s.world().progress.rules.upgrades.get(&chip).unwrap().cost;
    while p.s.world().player.credits < chip_cost {
        p.undock();
        p.mine_ice_until_full();
        p.dock_at(&KETTLE);
        p.sell_all(&ICE);
        trips += 1;
        // Kettle's ice price sits low once the player has sold it plenty: about 50 cr
        // a hold with the lens.
        assert!(
            trips <= 5,
            "the nav chip should take at most five extra ice runs"
        );
    }
    p.undock();
    p.dock_at(&WHARF);
    p.buy(ServiceRequest::Fit {
        upgrade: chip.clone(),
    });
    assert!(
        p.s.world().player.fittings.contains(&chip),
        "{:?}",
        p.s.services.notice
    );
    p.ticks(3);
    assert_eq!(
        status(&p.s, "beat.b06_nav_chip"),
        Some(BeatStatus::Completed)
    );

    // b07: docked at the Wharf with an empty hold, Ilse asks; Accept loads her plate.
    let plate = WareId::from_static("ware.refined_plate");
    p.ticks(3);
    assert!(p.ship().hold.consigned.is_empty(), "nothing loaded unasked");
    p.answer("npc.ilse_marrow", "choice.accept_consignment");
    assert_eq!(p.ship().hold.count(&plate), 0, "none of it the pilot's");
    assert_eq!(p.ship().hold.total(), 20, "Ilse's plate aboard");
    assert_eq!(
        p.goal().as_deref(),
        Some(
            "Carry Ilse's 20 plate to Rook at Cinder Hook and pick Deliver. The lane to Cinder Drift starts east of the Wharf."
        )
    );
    // Into the Cinder lane's mouth by hand, Space, and ride it.
    p.undock();
    let lane =
        p.s.world()
            .lanes
            .get("lane.shallows_cinder")
            .unwrap()
            .clone();
    p.fly_to(lane.a_gate, 10.0);
    p.tap();
    for _ in 0..3000 {
        p.tick();
        if p.ship().region == lane.b && p.ship().activity.transit.is_none() {
            break;
        }
    }
    assert_eq!(p.ship().region, lane.b, "the lane carried the ship");
    let hook = StationId::from_static("station.cinder_hook");
    p.dock_at(&hook);
    p.ticks(5);
    p.answer("npc.rook_adeyemi", "choice.deliver_plate");
    assert_eq!(
        status(&p.s, "beat.b07_cinder_drift"),
        Some(BeatStatus::Completed)
    );
    assert!(p.ship().hold.consigned.is_empty(), "handed to Rook");
    // Rook's gift is on his list; it's fitted when the player fits it.
    let chart = UpgradeId::from_static("upgrade.lane_chart");
    assert!(!p.s.world().player.fittings.contains(&chart));
    p.buy(ServiceRequest::Fit {
        upgrade: chart.clone(),
    });
    assert!(p.s.world().player.fittings.contains(&chart), "Rook's gift");
    let rook = NpcId::from_static("npc.rook_adeyemi");
    assert!(
        said(&p.s, &rook).iter().any(|t| t.contains("Ilse's plate")),
        "{:?}",
        said(&p.s, &rook)
    );
}

/// Act 3 up to b14: b01 to b13 done, with what they earned, docked at the Wharf, one
/// docking short of Ringwise.
fn before_b14() -> SimState {
    let mut state = new_state(false);
    let w = &mut state.world;
    let beats = [
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
    ];
    for (i, id) in beats.iter().enumerate() {
        w.progress.beats.insert(
            BeatId::new(*id).unwrap(),
            BeatState {
                pack: "core".parse().unwrap(),
                act: 1,
                kind: BeatKind::Directed,
                status: BeatStatus::Completed,
                step: 0,
                step_id: None,
                goal: String::new(),
                giver: None,
                since: None,
                step_since: None,
                done_since: None,
                started: Tick(i as u64),
            },
        );
    }
    for u in [
        "upgrade.mining_lens",
        "upgrade.nav_chip",
        "upgrade.lane_chart",
        "upgrade.order_slate",
        "upgrade.loop_log",
    ] {
        w.player.fittings.insert(UpgradeId::new(u).unwrap());
    }
    let ringwise = w.progress.rules.traits.get("trait.ringwise").unwrap().times;
    let have = w.counters.get(CounterKind::Dockings, &CounterScope::Player);
    w.counters.bump(
        CounterKind::Dockings,
        &CounterScope::Player,
        (ringwise - 1).saturating_sub(have),
    );
    state
}

/// b14 through the dock: Orrin's question shows as a button on his bubble (and key 1),
/// and picking it goes out as `PlayerInput::replies` through the session; the core pack
/// answers, b14 completes, the choice closes and his bubble is done.
#[test]
fn orrins_question_is_answered_with_his_bubbles_button() {
    let mut p = Pilot::new(session(before_b14()));
    p.tick();
    // The docking that makes Ringwise: the habit b14 waits on.
    p.undock();
    p.dock_at(&WHARF);
    p.ticks(5);
    assert!(
        p.s.world()
            .player
            .traits
            .contains(&TraitId::from_static("trait.ringwise"))
    );
    assert_eq!(
        status(&p.s, "beat.b14_pilot_changes"),
        Some(BeatStatus::Active)
    );
    let orrin = NpcId::from_static("npc.orrin_vale");
    let open = hooks::open_choices(p.s.world(), p.s.sim().history(), &WHARF);
    assert_eq!(open.len(), 1, "{open:?}");
    assert_eq!(open[0].npc, orrin);
    assert_eq!(open[0].choice.as_str(), "choice.ask_orrin");

    // Orrin's bubble draws it as a button; clicking it is the session's reply. Until
    // then a click on the bubble doesn't move past it.
    let canvas = render::glam::Vec2::new(1280.0, 720.0);
    while p.s.bubble().is_some_and(|b| b.advances()) {
        assert!(p.s.next_line());
    }
    let bubble = p.s.bubble().expect("Orrin's question waits");
    assert_eq!(bubble.line.speaker.as_ref(), Some(&orrin));
    assert!(!p.s.next_line(), "it waits for an answer");
    let mut o = render::Overlay::default();
    let bounds = render::Rect::new(18.0, 120.0, 460.0, 500.0);
    let (_, buttons) = ui::talk::dock_bubble(
        &mut o,
        p.s.world(),
        &bubble,
        bounds.center(),
        bounds,
        &UiState::default(),
    );
    let action = buttons
        .iter()
        .find_map(|b| match &b.action {
            ui::UiAction::Reply { npc, choice } => Some((npc.clone(), choice.clone())),
            _ => None,
        })
        .expect("an answer button");
    assert!(o.texts.iter().any(|t| t.text == "1  What do you mean?"));
    for b in &buttons {
        assert!(b.rect.y >= 0.0 && b.rect.y + b.rect.h <= canvas.y, "{b:?}");
    }
    assert!(p.s.reply(action.0, action.1));
    p.ticks(4);
    assert_eq!(
        status(&p.s, "beat.b14_pilot_changes"),
        Some(BeatStatus::Completed)
    );
    assert!(
        said(&p.s, &orrin)
            .iter()
            .any(|t| t == "You don't dock any more, you arrive. The ring knows you."),
        "{:?}",
        said(&p.s, &orrin)
    );
    assert!(
        hooks::open_choices(p.s.world(), p.s.sim().history(), &WHARF).is_empty(),
        "answered"
    );
    let asked = bubble.talk.expect("a line in the talk log");
    assert!(
        !p.s.talk.queue(&WHARF).any(|id| id == asked),
        "his question's bubble is done"
    );
    // A reply where nobody asked anything is dropped: no key does anything now.
    assert!(!p.s.reply_to_open(0));
}

#[test]
fn the_core_pack_alone_plays_b01_and_core_only_saves_stay_core_only() {
    let state = new_state(false);
    assert!(!state.world.player.flags.contains(&game::GM_FLAG));
    let mut p = Pilot::new(session(state));
    let packs: Vec<String> =
        p.s.game()
            .reports()
            .iter()
            .map(|r| r.pack.to_string())
            .collect();
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
    p.answer("npc.odo_fenn", "choice.deliver_ice");
    assert_eq!(
        status(&p.s, "beat.b03_first_delivery"),
        Some(BeatStatus::Completed)
    );
    p.undock();
    p.dock_at(&WHARF);
    p.ticks(3);
    let delta = credits - p.s.world().player.credits;
    p.s.submit(
        CommandBatch::new(Cause::system("test", "the player's savings"))
            .with(Command::AdjustCredits { delta }),
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
    assert_eq!(lens.effect, "You mine half again as fast");
    assert_eq!(
        lens.blocked.as_deref(),
        Some("Mining lens costs 600 cr. You have 100 cr.")
    );
    assert!(
        services.iter().any(|e| e.can_ask),
        "the dockmaster can be asked"
    );

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
        CommandBatch::new(Cause::system("test", "a windfall")).with(Command::AdjustCredits {
            delta: 1000 * CENTS_PER_CREDIT,
        }),
    );
    s.tick(PlayerInput::default());
    s.request_service(ServiceRequest::Fit { upgrade: LENS });
    s.tick(PlayerInput::default());
    assert!(s.world().player.fittings.contains(&LENS));
    assert_eq!(s.services.notice, None);
    assert_eq!(s.world().player.credits, 500 * CENTS_PER_CREDIT);
    // Once fitted it stays on the list as the player's, with nothing left to buy (#44).
    let services = hooks::station_services(s.world(), s.sim().history(), &WHARF);
    let lens: Vec<_> = services
        .iter()
        .flat_map(|e| &e.offers)
        .filter(|o| o.request == ServiceRequest::Fit { upgrade: LENS })
        .collect();
    assert_eq!(lens.len(), 1);
    assert!(lens[0].owned, "{:?}", lens[0]);
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
    assert!(
        buttons
            .iter()
            .any(|b| b.action == ui::UiAction::Service(ServiceRequest::Fit { upgrade: LENS }))
    );
    assert!(
        buttons
            .iter()
            .any(|b| b.action == ui::UiAction::AskWhatChanged)
    );
    assert!(o.texts.iter().any(|t| t.text == "Mining lens  ·  600 cr"));
    for b in &buttons {
        assert!(b.rect.y + b.rect.h <= canvas.y, "{b:?} fits");
    }
}

#[test]
fn every_authored_station_has_an_ask_what_changed_button() {
    let s = session(new_state(false));
    let canvas = render::glam::Vec2::new(1280.0, 720.0);
    let stations: Vec<StationId> = s.world().stations.keys().cloned().collect();
    assert_eq!(stations.len(), 5);
    for station in &stations {
        let services = hooks::station_services(s.world(), s.sim().history(), station);
        let dockmaster = services
            .iter()
            .find(|e| e.can_ask)
            .unwrap_or_else(|| panic!("{station}: no dockmaster to ask"));
        let keeper = dockmaster.keeper.as_ref().expect("a resident dockmaster");
        assert_eq!(s.world().npcs.get(keeper).unwrap().role, "dockmaster");
        let mut o = render::Overlay::default();
        let buttons = ui::dock_panel(
            &mut o,
            s.world(),
            s.sim().history(),
            station,
            &UiState::default(),
            ui::Notices::default(),
            canvas,
        );
        assert!(
            buttons
                .iter()
                .any(|b| b.action == ui::UiAction::AskWhatChanged),
            "{station}: no Ask button"
        );
    }
}

/// Every fact the talk log cites, with how often.
fn citations(s: &Session) -> std::collections::BTreeMap<FactId, usize> {
    let mut n = std::collections::BTreeMap::new();
    for l in s.world().talk.lines.values() {
        for f in &l.line.cites {
            *n.entry(*f).or_default() += 1;
        }
    }
    n
}

#[test]
fn asking_right_after_docking_never_repeats_the_ambient_line() {
    for pending in [true, false] {
        let mut p = Pilot::new(session(new_state(false)));
        p.ticks(3);
        p.undock();
        // Something Ilse has a line about: Kettle's first pane.
        p.s.submit(
            CommandBatch::new(Cause::system("test", "a stage")).with(Command::RecordFact {
                fact: Box::new(galaxy_contract::history::FactDraft {
                    kind: FactKind::from_static("core.stage_built"),
                    subjects: Subjects {
                        station: Some(KETTLE),
                        ..Subjects::default()
                    },
                    amount: Some(1),
                    note: "Kettle Dock stage 1".into(),
                }),
            }),
        );
        p.ticks(1);
        let stage = p.s.sim().history().last_fact_id().unwrap();
        // Dock, and stop on the tick the docking commits: Ilse's ambient line about it is
        // offered on the next tick; the selected line returns asynchronously.
        let wharf = p.s.world().stations.get(&WHARF).unwrap().clone();
        p.fly_to(wharf.position, wharf.dock_radius * 0.5);
        p.keys.press_action();
        for _ in 0..10 {
            p.tick();
            if p.docked().is_some() {
                break;
            }
        }
        assert_eq!(p.docked(), Some(WHARF));
        if !pending {
            p.ticks(3);
        }
        // If the player asks while the pick is pending, the authored answer wins and
        // retires the fact before the pick returns. After the ambient line has landed,
        // the answer omits it. Either ordering must say the fact exactly once.
        let answer = p.ask();
        assert_eq!(
            answer
                .lines
                .iter()
                .filter(|l| l.cites.contains(&stage))
                .count(),
            usize::from(pending),
            "{answer:?}"
        );
        assert!(
            said(&p.s, &ILSE)
                .iter()
                .any(|t| t.starts_with("Odo's got a pane up at Kettle")),
            "{:?}",
            said(&p.s, &ILSE)
        );
        p.ticks(3);
        assert_eq!(citations(&p.s).get(&stage), Some(&1));
        assert!(
            citations(&p.s).values().all(|n| *n == 1),
            "{:?}",
            citations(&p.s)
        );
    }
}

/// The Wharf at its most crowded: Orrin offers the docking clamp, the hold extender and
/// the rock cradle (and the lens), five wares in the hold make the hold panel tall.
fn crowded_wharf() -> Session {
    let mut state = new_state(false);
    let w = &mut state.world;
    w.player
        .fittings
        .insert(UpgradeId::from_static("upgrade.nav_chip"));
    w.counters
        .bump(CounterKind::Dockings, &CounterScope::Player, 20);
    w.counters
        .bump(CounterKind::RocksDepleted, &CounterScope::Player, 5);
    let ship = w.player.ship.clone().unwrap();
    let hold = &mut w.ships.get_mut(&ship).unwrap().hold.cargo;
    for ware in [
        "ware.ice",
        "ware.iron_ore",
        "ware.refined_plate",
        "ware.glassite",
        "ware.rations",
    ] {
        hold.insert(WareId::new(ware).unwrap(), 2);
    }
    let mut s = session(state);
    // Enough sold for the hold extender.
    s.submit(
        CommandBatch::new(Cause::system("test", "past sales")).with(Command::RecordFact {
            fact: Box::new(galaxy_contract::history::FactDraft {
                kind: FactKind::from_static("sell"),
                subjects: Subjects {
                    ship: Some(ship),
                    ..Subjects::default()
                },
                amount: Some(200),
                note: "past sales".into(),
            }),
        }),
    );
    s.tick(PlayerInput::default());
    s
}

#[test]
fn a_crowded_dock_screen_keeps_undock_and_every_service_on_screen() {
    let s = crowded_wharf();
    let offered: Vec<ServiceRequest> =
        hooks::station_services(s.world(), s.sim().history(), &WHARF)
            .into_iter()
            .flat_map(|e| e.offers)
            .filter(|o| !o.owned)
            .map(|o| o.request)
            .collect();
    for u in [
        "upgrade.docking_clamp",
        "upgrade.hold_extender",
        "upgrade.rock_cradle",
    ] {
        let fit = ServiceRequest::Fit {
            upgrade: UpgradeId::new(u).unwrap(),
        };
        assert!(offered.contains(&fit), "{u} is on offer: {offered:?}");
    }
    let notices = ui::Notices {
        trade: Some("Kettle Dock isn't buying iron ore right now."),
        service: Some("Docking clamp costs 700 cr. You have 400 cr."),
        fitted: None,
    };
    for canvas in [
        render::glam::Vec2::new(960.0, 540.0),
        render::glam::Vec2::new(1280.0, 720.0),
    ] {
        // Page through the services with the panel's own Up / Down buttons.
        let mut ui_state = UiState::default();
        let mut seen = std::collections::BTreeSet::new();
        for _ in 0..20 {
            let mut o = render::Overlay::default();
            let buttons = ui::dock_panel(
                &mut o,
                s.world(),
                s.sim().history(),
                &WHARF,
                &ui_state,
                notices,
                canvas,
            );
            for b in &buttons {
                assert!(
                    b.rect.y >= 0.0 && b.rect.y + b.rect.h <= canvas.y,
                    "{canvas}: {b:?} is on screen"
                );
                assert!(b.rect.x >= 0.0 && b.rect.x + b.rect.w <= canvas.x);
            }
            for p in &o.panels {
                assert!(p.rect.y + p.rect.h <= canvas.y + 0.5, "{canvas}: {p:?}");
            }
            assert!(
                o.texts
                    .iter()
                    .any(|t| t.text == ui::DOCK_FOOT && t.pos.y + t.size <= canvas.y),
                "{canvas}: how to undock shows"
            );
            for b in &buttons {
                match &b.action {
                    ui::UiAction::Service(r) => {
                        seen.insert(format!("{r:?}"));
                    }
                    ui::UiAction::AskWhatChanged => {
                        seen.insert("ask".to_owned());
                    }
                    _ => {}
                }
            }
            let down = buttons.iter().find_map(|b| match b.action {
                ui::UiAction::ServicesFrom(n) if n > ui_state.services_first => Some(n),
                _ => None,
            });
            let Some(next) = down else { break };
            ui_state.services_first = next;
        }
        for r in &offered {
            assert!(
                seen.contains(&format!("{r:?}")),
                "{canvas}: {r:?} reachable"
            );
        }
        assert!(seen.contains("ask"), "{canvas}: Ask what changed reachable");
    }
}

#[test]
fn asking_what_changed_cites_facts_and_records_the_question() {
    let s = at_the_wharf_after_b03(800 * CENTS_PER_CREDIT);
    let mut p = Pilot::new(s);
    p.buy(ServiceRequest::Fit { upgrade: LENS });
    assert!(p.s.world().player.fittings.contains(&LENS));
    // Out and back: the lens went on since the previous docking here.
    p.undock();
    p.dock_at(&WHARF);
    p.ticks(3);
    let answer = p.ask();
    assert_eq!(answer.speaker, Some(ILSE));
    // Orrin said so himself when he fitted it, in the first person with its effect,
    // so the answer doesn't tell that fact again.
    let orrin = NpcId::from_static("npc.orrin_vale");
    assert!(
        said(&p.s, &orrin)
            .iter()
            .any(|t| t == "There: I've fitted your mining lens. You mine half again as fast."),
        "{:?}",
        said(&p.s, &orrin)
    );
    for line in &answer.lines {
        assert!(line.validate().is_ok(), "{line:?}");
        for f in &line.cites {
            assert!(p.s.sim().history().fact(*f).is_some(), "cites a real fact");
        }
    }
    let asked =
        p.s.sim()
            .history()
            .facts()
            .iter()
            .filter(|f| f.kind == hooks::ASKED)
            .count();
    assert_eq!(asked, 1);
    // The digest lines commit with the next tick.
    p.ticks(1);
    // Said, so neither ambient talk nor the next question tells those facts again.
    for line in answer
        .lines
        .iter()
        .filter(|l| l.channel == Channel::DockTalk)
    {
        assert!(
            p.s.world()
                .talk
                .lines
                .values()
                .any(|l| l.line.cites == line.cites)
        );
    }
    let again = p.ask();
    assert!(again.lines.is_empty(), "{:?}", again.lines);
}

/// News arriving while the player is at the Wharf, after its ambient dock talk.
fn news_fact(p: &mut Pilot, kind: &str, subjects: Subjects, amount: Option<i64>) -> FactId {
    let kind = FactKind::new(kind).unwrap();
    p.s.submit(
        CommandBatch::new(Cause::system("test", "news from the Reach")).with(Command::RecordFact {
            fact: Box::new(galaxy_contract::history::FactDraft {
                kind: kind.clone(),
                subjects,
                amount,
                note: "News from the Reach".into(),
            }),
        }),
    );
    p.tick();
    p.s.sim()
        .history()
        .last(&HistoryQuery {
            kind: Some(kind),
            ..HistoryQuery::default()
        })
        .unwrap()
        .id
}

fn assert_distinct_news(p: &Pilot, answer: &hooks::Answer) {
    let mut facts = std::collections::BTreeSet::new();
    let mut places = std::collections::BTreeSet::new();
    for line in &answer.lines {
        let mut line_places = std::collections::BTreeSet::new();
        for cite in &line.cites {
            assert!(facts.insert(*cite), "a repeated fact: {answer:?}");
            let fact = p.s.sim().history().fact(*cite).unwrap();
            line_places.insert(fact.subjects.station.clone());
        }
        for place in line_places {
            assert!(places.insert(place), "a repeated place: {answer:?}");
        }
    }
}

/// The Wharf's answer tells what changed here or nearby (#38): Kettle and the Wharf,
/// never the Hook or Brassmouth, and several Kettle facts still leave room for a
/// Wharf digest line.
#[test]
fn asking_what_changed_leaves_room_for_other_places_after_several_kettle_facts() {
    let mut p = Pilot::new(session(new_state(false)));
    p.ticks(3);
    // The re-check's three eligible Ilse lines, all about Kettle.
    let stage = news_fact(
        &mut p,
        "core.stage_built",
        Subjects {
            station: Some(KETTLE),
            ..Subjects::default()
        },
        Some(1),
    );
    let delivery = news_fact(
        &mut p,
        "deliver",
        Subjects {
            station: Some(KETTLE),
            other: Some(AnyId::from_static("ledger.kettle_greenhouse")),
            ..Subjects::default()
        },
        None,
    );
    let shortage = news_fact(
        &mut p,
        "core.shortage_started",
        Subjects {
            station: Some(KETTLE),
            ..Subjects::default()
        },
        None,
    );
    let hook = news_fact(
        &mut p,
        "core.territory_changed",
        Subjects {
            station: Some(StationId::from_static("station.cinder_hook")),
            faction: Some(FactionId::from_static("faction.wharf_cooperative")),
            ..Subjects::default()
        },
        None,
    );
    // Far away: never told at the Wharf.
    let brass = news_fact(
        &mut p,
        "test.stage_built",
        Subjects {
            station: Some(StationId::from_static("station.brassmouth_exchange")),
            ..Subjects::default()
        },
        Some(1),
    );
    // No authored line: the digest must still have room after the pack's picks.
    let wharf = news_fact(
        &mut p,
        "test.stage_built",
        Subjects {
            station: Some(WHARF),
            ..Subjects::default()
        },
        Some(1),
    );
    // A second Kettle fact with a digest must not get its own line either.
    let kettle_digest = news_fact(
        &mut p,
        "test.stage_built",
        Subjects {
            station: Some(KETTLE),
            ..Subjects::default()
        },
        Some(2),
    );

    let answer = p.ask();
    assert_distinct_news(&p, &answer);
    assert_eq!(answer.lines.len(), 2, "{answer:?}");
    for fact in [stage, wharf] {
        assert!(answer.lines.iter().any(|l| l.cites == [fact]), "{answer:?}");
    }
    assert_eq!(answer.digest.len(), 1);
    assert_eq!(answer.digest[0].cites, [wharf]);
    p.tick(); // Commit the digest before inspecting what was actually said.
    let said = citations(&p.s);
    for fact in [stage, wharf] {
        assert_eq!(said.get(&fact), Some(&1));
    }
    for fact in [hook, brass] {
        assert!(!said.contains_key(&fact), "far news isn't told here");
    }
    for fact in [delivery, shortage, kettle_digest] {
        assert!(
            !said.contains_key(&fact),
            "skipped news must not be retired"
        );
    }
    // Later questions can tell the skipped news, but never the same fact again.
    let again = p.ask();
    assert_distinct_news(&p, &again);
    assert!(
        again
            .lines
            .iter()
            .flat_map(|l| &l.cites)
            .all(|f| !said.contains_key(f))
    );
}

#[test]
fn asking_what_changed_shares_places_and_facts_with_the_chronicle() {
    use galaxy_contract::talk::TalkLine;

    let mut p = Pilot::new(session(new_state(false)));
    p.ticks(3);
    let kettle = news_fact(
        &mut p,
        "core.stage_built",
        Subjects {
            station: Some(KETTLE),
            ..Subjects::default()
        },
        Some(1),
    );
    let wharf = news_fact(
        &mut p,
        "test.stage_built",
        Subjects {
            station: Some(WHARF),
            ..Subjects::default()
        },
        Some(1),
    );
    let mut entries = Vec::new();
    for station in [
        Some(StationId::from_static("station.cinder_hook")),
        None,
        Some(KETTLE),
        Some(WHARF),
    ] {
        entries.push(news_fact(
            &mut p,
            "test.chronicle_news",
            Subjects {
                station,
                ..Subjects::default()
            },
            None,
        ));
    }
    let mut batch = CommandBatch::new(Cause::system("test", "the chronicle records the news"));
    // Newest entries repeat places covered by the pack and digest. They must not
    // crowd the older, eligible chronicle entry out of the answer; the Hook's entry
    // is too far away to tell at the Wharf (#38).
    for fact in [entries[0], entries[1], entries[1], entries[2], entries[3]] {
        batch.push(Command::Say {
            line: Box::new(TalkLine {
                channel: Channel::Chronicle,
                speaker: Some(ILSE),
                place: Some(WHARF), // Place is where said; citations name the news.
                text: "Ilse adds news to the chronicle.".into(),
                cites: vec![fact],
                about: Vec::new(),
                choices: Vec::new(),
            }),
        });
    }
    p.s.submit(batch);
    p.tick();
    let answer = p.ask();
    assert_distinct_news(&p, &answer);
    assert_eq!(answer.lines.len(), 3, "{answer:?}");
    assert_eq!(answer.digest.len(), 1);
    assert_eq!(answer.digest[0].cites, [wharf]);
    assert!(answer.lines.iter().any(|l| l.cites == [kettle]));
    let chronicle: Vec<_> = answer
        .lines
        .iter()
        .filter(|l| l.channel == Channel::Chronicle)
        .flat_map(|l| &l.cites)
        .copied()
        .collect();
    assert_eq!(chronicle, [entries[1]]);
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
        let rock =
            p.s.world()
                .rocks
                .get("rock.shallows.ice.09")
                .unwrap()
                .clone();
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
    galaxy_app::script::run(&mut s, &mut ui, "w,wait*5").unwrap();
    let at = s.flagship().unwrap().position;
    let target = at + Vec2::new(150.0, -150.0); // clear of the Wharf's ring
    galaxy_app::script::run(
        &mut s,
        &mut ui,
        &format!(":click:{}:{}", target.x, target.y),
    )
    .unwrap();
    assert!(matches!(
        s.flagship().unwrap().orders.orders.first(),
        Some(Order::Fly {
            to: FlyTarget::Point { .. }
        })
    ));
    galaxy_app::script::run(&mut s, &mut ui, "wait*200").unwrap();
    assert!(s.flagship().unwrap().position.distance(target) < 5.0);

    // Click a rock: mine it. Shift-click Kettle Dock: queued after it.
    let rock = s
        .world()
        .rocks
        .get("rock.shallows.ice.09")
        .unwrap()
        .position;
    let kettle = s.world().stations.get(&KETTLE).unwrap().position;
    galaxy_app::script::run(
        &mut s,
        &mut ui,
        &format!(
            ":click:{}:{},:qclick:{}:{}",
            rock.x, rock.y, kettle.x, kettle.y
        ),
    )
    .unwrap();
    let orders = s.flagship().unwrap().orders.orders.clone();
    assert!(
        matches!(orders.first(), Some(Order::Mine { .. })),
        "{orders:?}"
    );
    assert!(
        matches!(orders.get(1), Some(Order::Dock { station }) if *station == KETTLE),
        "{orders:?}"
    );
    // The queue runs: mine until full, then dock at Kettle, where the player hands
    // Odo the ice (b03) by picking Deliver.
    galaxy_app::script::run(&mut s, &mut ui, "wait*2400").unwrap();
    assert_eq!(s.flagship().unwrap().docked_at, Some(KETTLE));
    let mined = s
        .world()
        .counters
        .get(CounterKind::UnitsMined, &CounterScope::Player);
    assert_eq!(mined, 20);
    galaxy_app::script::run(&mut s, &mut ui, ":reply:choice.deliver_ice,wait*6").unwrap();
    assert_eq!(
        status(&s, "beat.b03_first_delivery"),
        Some(BeatStatus::Completed)
    );

    // Map travel: click another region; the route goes through the lanes.
    galaxy_app::script::run(&mut s, &mut ui, ":mapclick:region.cinder_drift").unwrap();
    assert!(matches!(
        s.flagship().unwrap().orders.orders.first(),
        Some(Order::Fly {
            to: FlyTarget::Region { .. }
        })
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
        notes
            .iter()
            .any(|n| n.contains("OPENROUTER_API_KEY isn't set")),
        "{notes:?}"
    );
    assert!(game.take_notes().is_empty(), "said once");
    check_authored_default(&mut game);
}

#[test]
fn switching_the_mind_off_keeps_the_scripts_complete_default() {
    let opts = GameOptions {
        packs: packs(),
        gm: false,
        mind: MindMode::Off,
        hot_reload: false,
    };
    let mut game = Game::open(new_state(false), &opts).unwrap();
    check_authored_default(&mut game);
}

fn check_authored_default(game: &mut Game) {
    for _ in 0..3 {
        game.step(&PlayerInput::default()).unwrap();
    }
    for (kind, amount) in [
        ("core.stage_built", Some(1)),
        ("core.shortage_started", None),
    ] {
        game.sim_mut().submit(
            CommandBatch::new(Cause::system("test", "Kettle news")).with(Command::RecordFact {
                fact: Box::new(FactDraft {
                    kind: FactKind::new(kind).unwrap(),
                    subjects: Subjects {
                        station: Some(KETTLE),
                        ..Subjects::default()
                    },
                    amount,
                    note: "news".into(),
                }),
            }),
        );
    }
    game.step(&PlayerInput::default()).unwrap();
    game.sim_mut()
        .submit(
            CommandBatch::new(Cause::system("test", "arrival")).with(Command::Emit {
                event: Event::Dock {
                    ship: ShipId::from_static("ship.player"),
                    station: WHARF,
                },
            }),
        );
    for _ in 0..5 {
        game.step(&PlayerInput::default()).unwrap();
    }
    let candidates = game
        .sim()
        .history()
        .facts()
        .iter()
        .find_map(|f| match &f.payload {
            FactPayload::Event(Event::TalkRequested { request }) => Some(&request.candidates),
            _ => None,
        })
        .unwrap();
    assert_eq!(candidates.len(), 2);
    let said: Vec<_> = game
        .sim()
        .world()
        .talk
        .on(Channel::DockTalk)
        .filter(|s| candidates.iter().any(|c| s.line.cites.contains(&c.cite)))
        .collect();
    assert_eq!(said.len(), 1);
    assert_eq!(said[0].line.text, candidates[0].text);
    assert!(
        !game
            .sim()
            .history()
            .facts()
            .iter()
            .any(|f| f.kind.as_str() == "mind.spend")
    );
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
    state
        .world
        .ships
        .get_mut(&id)
        .unwrap()
        .hold
        .cargo
        .insert(ICE, n);
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
fn handing_over_the_rest_of_a_stack_clears_the_split_mark() {
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
    assert_eq!(p.s.world().economy.split_stacks.get(&ICE), Some(&WHARF));
    // The other 6 leave the hold without a sale (a delivery, a hand-over).
    let ship = p.ship().id.clone();
    let cargo = |delta: i64| {
        CommandBatch::new(Cause::system("test", "cargo")).with(Command::AdjustCargo {
            ship: ship.clone(),
            ware: ICE,
            delta,
        })
    };
    p.s.submit(cargo(-6));
    p.ticks(2);
    assert_eq!(p.ship().hold.count(&ICE), 0);
    assert!(
        p.s.world().economy.split_stacks.is_empty(),
        "the mark retired"
    );
    // A new stack, sold whole elsewhere: not a split sale.
    p.undock();
    p.s.submit(cargo(8));
    p.dock_at(&KETTLE);
    p.sell_all(&ICE);
    p.ticks(2);
    assert_eq!(p.ship().hold.count(&ICE), 0);
    assert!(facts_of(&p.s, "split_sale").is_empty());
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
    assert!(
        facts_of(&p.s, "far_docking").is_empty(),
        "Kettle was never seen"
    );
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
        CommandBatch::new(Cause::system("test", "hand Thimble some ice")).with(
            Command::SetOrders {
                ship: id.clone(),
                orders: transfer(&thimble),
            },
        ),
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

/// Talk at the dock (#47 Phase 2): Ilse's welcome waits as a bubble at the Wharf, ends
/// when the ship leaves and comes back at the next docking there; what's said in flight
/// goes to the log and its icon counts it; the book's record round-trips.
#[test]
fn talk_waits_at_its_dock_and_what_is_said_in_flight_goes_to_the_log() {
    use galaxy_app::ui::log::{self, Entry};
    use galaxy_app::ui::talk::{TalkBook, Voice};
    let mut p = Pilot::new(session(new_state(false)));
    p.ticks(5);
    let welcome = p.s.bubble().expect("Ilse's welcome");
    assert_eq!(welcome.line.speaker.as_ref(), Some(&ILSE));
    assert_eq!(welcome.voice, Voice::Person);
    assert!(welcome.advances(), "no answers: a click moves on");
    let id = welcome.talk.unwrap();

    // Flying off ends the bubbles; the welcome still waits at the Wharf.
    p.undock();
    assert!(p.s.bubble().is_none(), "nothing lingers in flight");
    assert!(p.s.talk.queue(&WHARF).any(|t| t == id));
    let (w, h) = (p.s.world(), p.s.sim().history());
    let visits = log::visits(w, h);
    assert_eq!(visits.len(), 1, "the opening stay is a visit");
    assert_eq!(visits[0].station, WHARF);
    assert!(visits[0].until.is_some());
    assert!(visits[0].bubbles().iter().any(|l| l.id == id));

    // A person's line and a notice said in flight: both go to the log, counted new.
    let cite = h.last_fact_id().unwrap();
    let line = |channel, speaker: Option<NpcId>, text: &str| Command::Say {
        line: Box::new(galaxy_contract::talk::TalkLine {
            channel,
            speaker,
            place: Some(KETTLE),
            text: text.to_owned(),
            cites: vec![cite],
            about: Vec::new(),
            choices: Vec::new(),
        }),
    };
    let before = p.s.talk.log_new();
    p.s.submit(
        CommandBatch::new(Cause::system("test", "news in flight"))
            .with(line(Channel::DockTalk, Some(ODO), "The beans are up."))
            .with(line(Channel::Chronicle, None, "A greenhouse ring lit up.")),
    );
    p.ticks(1);
    assert_eq!(p.s.talk.log_new(), before + 2, "the icon counts both");
    assert!(p.s.bubble().is_none(), "no box mid-flight");
    let (w, h) = (p.s.world(), p.s.sim().history());
    let entries = log::entries(w, &log::visits(w, h));
    assert!(matches!(&entries[0], Entry::Notice(l) if l.line.text == "A greenhouse ring lit up."));
    assert!(matches!(&entries[1], Entry::Said(l) if l.line.speaker == Some(ODO)));
    assert!(matches!(entries.last(), Some(Entry::Visit(0))));
    let mut ui_state = UiState::default();
    ui_state.toggle_log(&mut p.s.talk);
    assert!(ui_state.log_open);
    assert_eq!(p.s.talk.log_new(), 0, "opened: nothing new");

    // The record keeps it all.
    let record = p.s.talk.record();
    assert_eq!(TalkBook::from_record(&record).record(), record);

    // Back at the Wharf, the welcome is shown first; a click moves past it.
    p.dock_at(&WHARF);
    assert_eq!(p.s.bubble().and_then(|b| b.talk), Some(id));
    assert!(p.s.next_line());
    assert!(!p.s.talk.queue(&WHARF).any(|t| t == id), "seen");
    // Skip clears the rest of what waits here.
    p.s.skip_talk();
    assert!(p.s.bubble().is_none_or(|b| !b.advances()));
}

/// The log pins what the player has open (#47 Phase 2): the job from Ilse, her offer to
/// carry plate while it's unanswered, then the plate itself once accepted.
#[test]
fn the_log_pins_jobs_open_offers_and_cargo_for_someone() {
    use galaxy_app::ui::log;
    let run = galaxy_app::harness::jump(&packs(), 1, false, 7).unwrap();
    let mut p = Pilot::new(Session::with_game(run.game));
    p.ticks(3);
    assert_eq!(p.docked(), Some(WHARF));
    let pins = |p: &Pilot| {
        let (w, h) = (p.s.world(), p.s.sim().history());
        log::pinned(w, h, &log::visits(w, h))
    };
    let ilse = "Ilse Marrow  ·  Lantern Wharf";
    assert!(pins(&p).iter().any(|x| x.tag == "Job" && x.head == ilse));
    assert!(
        pins(&p).iter().any(|x| x.tag == "Offer" && x.head == ilse),
        "{:?}",
        pins(&p)
    );
    // Click to her question, then accept.
    while p.s.bubble().is_some_and(|b| b.advances()) {
        assert!(p.s.next_line());
    }
    let ask = p.s.bubble().expect("Ilse's offer");
    assert_eq!(ask.line.speaker.as_ref(), Some(&ILSE));
    let accept = ask
        .choices
        .iter()
        .find(|(_, c)| c.text.starts_with("Accept"))
        .map(|(_, c)| c.choice.clone())
        .expect("an Accept answer");
    assert!(p.s.reply(ILSE, accept));
    p.ticks(4);
    let pinned = pins(&p);
    assert!(
        !pinned.iter().any(|x| x.tag == "Offer" && x.head == ilse),
        "answered: {pinned:?}"
    );
    let cargo = pinned
        .iter()
        .find(|x| x.tag == "Cargo")
        .expect("the plate is pinned");
    assert!(cargo.body.contains("Rook") && cargo.body.contains("Cinder Hook"));
    // The confirmation of what came aboard waits as a bubble here.
    let shown: Vec<_> = std::iter::from_fn(|| {
        let b = p.s.bubble().filter(|b| b.advances())?;
        p.s.next_line();
        Some(b)
    })
    .take(20)
    .collect();
    assert!(
        shown
            .iter()
            .any(|b| b.voice == galaxy_app::ui::talk::Voice::Confirmed),
        "{shown:?}"
    );
}

/// A line waiting for an answer never holds the rest hostage: asking what changed while
/// Ilse's offer waits, a click sets the offer aside, the answer shows, and the offer
/// comes back after it.
#[test]
fn a_waiting_offer_steps_aside_for_what_was_said_after_it() {
    let run = galaxy_app::harness::jump(&packs(), 1, false, 7).unwrap();
    let mut p = Pilot::new(Session::with_game(run.game));
    p.ticks(3);
    while p.s.bubble().is_some_and(|b| b.advances()) {
        assert!(p.s.next_line());
    }
    let offer = p.s.bubble().expect("Ilse's offer");
    assert!(!offer.choices.is_empty());
    assert!(!p.s.next_line(), "alone, it stays");
    assert!(p.s.ask_what_changed());
    p.ticks(4);
    let waiting = p.s.bubble().unwrap();
    assert_eq!(waiting.talk, offer.talk, "oldest first");
    assert!(waiting.more > 0 && waiting.clickable());
    assert!(p.s.next_line(), "set aside");
    let answer = p.s.bubble().unwrap();
    assert_ne!(answer.talk, offer.talk, "the answer shows");
    let mut back = false;
    for _ in 0..10 {
        if p.s.bubble().unwrap().talk == offer.talk {
            back = true;
            break;
        }
        assert!(p.s.next_line());
    }
    assert!(back, "the offer comes back after the rest");
    assert!(!p.s.next_line(), "and waits for its answer");
}
