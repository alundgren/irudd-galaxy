//! The HUD laid out against itself (#34, #35), all active goals with their sources
//! (#46), the station card's fitted state and faction (#44), and hints that retire once
//! they've taught (1.8). Pure rect maths over real worlds: no window or GPU.

use std::collections::BTreeMap;
use std::path::PathBuf;

use galaxy_app::hooks::{self, OpenChoice};
use galaxy_app::session::Session;
use galaxy_app::ui::hints::Acts;
use galaxy_app::ui::{self, HintBook, HintKind, HintRules, UiState, overlaps};
use galaxy_contract::prelude::*;
use galaxy_contract::progress::{BeatKind, BeatState, BeatStatus};
use galaxy_contract::talk::{Channel, SaidLine, TalkLine};
use galaxy_core::SimState;
use render::Rect;
use render::glam::Vec2 as G2;

const WHARF: StationId = StationId::from_static("station.lantern_wharf");
const HOOK: StationId = StationId::from_static("station.cinder_hook");
const ILSE: NpcId = NpcId::from_static("npc.ilse_marrow");
const ORRIN: NpcId = NpcId::from_static("npc.orrin_vale");
const ROOK: NpcId = NpcId::from_static("npc.rook_adeyemi");
const ODO: NpcId = NpcId::from_static("npc.odo_fenn");

/// Window sizes at the design size and above, including odd shapes. Smaller windows
/// get the same canvas scaled down.
const SIZES: [(f32, f32); 8] = [
    (1280.0, 720.0),
    (960.0, 540.0),
    (1366.0, 768.0),
    (1600.0, 900.0),
    (1920.0, 1080.0),
    (2560.0, 1440.0),
    (1280.0, 1024.0),
    (2560.0, 1080.0),
];

fn data() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../packs/core/data")
}

fn new_state() -> SimState {
    galaxy_core::new_game(&data(), 1).unwrap()
}

fn rules() -> HintRules {
    HintRules::load(&data()).unwrap()
}

fn flagship_mut(w: &mut WorldState) -> &mut Ship {
    let id = w.player.ship.clone().unwrap();
    w.ships.get_mut(&id).unwrap()
}

fn beat(id: &'static str, giver: Option<NpcId>, goal: &str, started: u64) -> (BeatId, BeatState) {
    (
        BeatId::from_static(id),
        BeatState {
            pack: PackId::from_static("core"),
            act: 1,
            kind: BeatKind::Directed,
            status: BeatStatus::Active,
            step: 0,
            step_id: None,
            goal: goal.to_owned(),
            giver,
            since: None,
            step_since: None,
            done_since: None,
            started: Tick(started),
        },
    )
}

/// Four active goals, two of them long enough to wrap, from four people.
fn add_goals(w: &mut WorldState) {
    let long = "Fly into Kettle Dock's ring, press Space, and give Odo the ice. Then sell the rest of what you mined at the Wharf.";
    for (id, state) in [
        beat("beat.t1", Some(ILSE), long, 1),
        beat(
            "beat.t2",
            Some(ORRIN),
            "Orrin, the Wharf's hullwright, can fit a mining lens for 600 credits.",
            2,
        ),
        beat("beat.t3", Some(ROOK), long, 3),
        beat(
            "beat.t4",
            Some(ODO),
            "Bring 30 glassite to Odo at Kettle Dock.",
            4,
        ),
    ] {
        w.progress.beats.insert(id, state);
    }
}

/// Three of the longest lines the talk panel takes.
fn add_talk(w: &mut WorldState) {
    let text = "I've been watching the lanes since before your hull was cut, and I can tell you the current runs warmer east of the Wharf this time of the shift. Mind the ore barges; they don't turn for anyone, least of all a Wren with a full hold. If you see Rook, tell him the plate is coming, and that I said so. He'll grumble, but he'll know what it means and he'll wait.";
    assert!(text.len() <= 400);
    for i in 0..3u64 {
        let line = TalkLine {
            channel: Channel::DockTalk,
            speaker: Some(ILSE),
            place: Some(WHARF),
            text: text.to_owned(),
            cites: vec![FactId(1)],
            about: Vec::new(),
            choices: Vec::new(),
        };
        assert!(line.validate().is_ok());
        let id = TalkId(i + 1);
        w.talk.lines.insert(
            id,
            SaidLine {
                id,
                tick: Tick(0),
                line,
            },
        );
    }
}

/// The busiest flying HUD: goals, a full talk panel, wares in the hold, undocked.
fn busy_flying() -> Session {
    let mut state = new_state();
    let w = &mut state.world;
    add_goals(w);
    add_talk(w);
    let ship = flagship_mut(w);
    ship.docked_at = None;
    for (ware, n) in [("ware.ice", 6), ("ware.iron_ore", 5), ("ware.glassite", 4)] {
        ship.hold.cargo.insert(WareId::new(ware).unwrap(), n);
    }
    Session::new(state)
}

/// The longest hint the pill can show in the slice.
fn longest_hint(world: &WorldState) -> String {
    let region = world
        .regions
        .values()
        .map(|r| r.name.as_str())
        .max_by_key(|n| n.len())
        .unwrap();
    let station = world
        .stations
        .values()
        .map(|s| s.name.as_str())
        .max_by_key(|n| n.len())
        .unwrap();
    [
        format!("Space  ·  ride the lantern current to {region}"),
        format!("Space  ·  dock at {station}"),
        "Mining refined plate  ·  1.25 a second".to_owned(),
        "Hold Space  ·  mine ancients fragment".to_owned(),
    ]
    .into_iter()
    .max_by_key(|t| t.len())
    .unwrap()
}

fn inside(r: &Rect, canvas: G2) -> bool {
    r.x >= 0.0 && r.y >= 0.0 && r.x + r.w <= canvas.x + 0.5 && r.y + r.h <= canvas.y + 0.5
}

fn assert_apart(size: (f32, f32), rects: &[(&str, Rect)], canvas: G2) {
    for (i, (a, ra)) in rects.iter().enumerate() {
        assert!(inside(ra, canvas), "{size:?}: {a} {ra:?} is off the canvas");
        for (b, rb) in &rects[i + 1..] {
            assert!(
                !overlaps(ra, rb),
                "{size:?}: {a} {ra:?} overlaps {b} {rb:?}"
            );
        }
    }
}

#[test]
fn nothing_on_the_flying_hud_overlaps() {
    let s = busy_flying();
    let (w, h) = (s.world(), s.sim().history());
    let hint = longest_hint(w);
    for size in SIZES {
        let canvas = ui::canvas(G2::new(size.0, size.1));
        let goal = ui::goal_rect(w, canvas).expect("goals show");
        let hold = ui::hold_rect(w, canvas).expect("the hold shows");
        let talk = ui::talk_rect(w, h, None, &[], canvas).expect("talk shows");
        let pill = ui::hint_rect(&hint, canvas, Some(talk));
        // The mining readout is the same pill.
        let mining = ui::hint_rect("Mining ice  ·  1.5 a second", canvas, Some(talk));
        assert_apart(
            size,
            &[
                ("goal", goal),
                ("hold", hold),
                ("talk", talk),
                ("hint", pill),
            ],
            canvas,
        );
        assert!(
            !overlaps(&mining, &talk),
            "{size:?}: mining pill under talk"
        );
    }
}

#[test]
fn the_talk_panel_gives_way_under_a_tall_goal_banner() {
    let s = busy_flying();
    let (w, h) = (s.world(), s.sim().history());
    let canvas = ui::canvas(G2::new(1280.0, 720.0));
    let goal = ui::goal_rect(w, canvas).unwrap();
    let talk = ui::talk_rect(w, h, None, &[], canvas).unwrap();
    assert!(talk.y >= goal.y + goal.h, "{talk:?} under {goal:?}");
    // The drawn panel is the measured one.
    let mut o = render::Overlay::default();
    ui::talk(&mut o, w, h, None, &[], &UiState::default(), canvas);
    assert_eq!(o.panels[0].rect, talk);
}

#[test]
fn the_hint_pill_keeps_clear_of_the_talk_panel_and_centres_without_it() {
    let canvas = G2::new(1280.0, 720.0);
    let text = "Space  ·  ride the lantern current to Brassmouth Narrows";
    let alone = ui::hint_rect(text, canvas, None);
    assert!((alone.center().x - 640.0).abs() < 0.5, "centred: {alone:?}");
    let talk = Rect::new(18.0, 600.0, ui::TALK_WIDTH, 102.0);
    let moved = ui::hint_rect(text, canvas, Some(talk));
    assert!(!overlaps(&moved, &talk), "{moved:?}");
    assert_eq!(moved.y, alone.y, "it stays low");
    // A talk panel that doesn't reach the pill leaves it centred.
    let short = Rect::new(18.0, 300.0, ui::TALK_WIDTH, 100.0);
    assert_eq!(ui::hint_rect(text, canvas, Some(short)), alone);
}

#[test]
fn docked_the_card_talk_goal_and_hold_keep_apart() {
    let mut state = new_state();
    let w = &mut state.world;
    add_goals(w);
    add_talk(w);
    let ship = flagship_mut(w);
    for (ware, n) in [("ware.ice", 6), ("ware.iron_ore", 5), ("ware.glassite", 4)] {
        ship.hold.cargo.insert(WareId::new(ware).unwrap(), n);
    }
    let s = Session::new(state);
    let (w, h) = (s.world(), s.sim().history());
    let choices: Vec<OpenChoice> = (0..3)
        .map(|i| OpenChoice {
            npc: ILSE,
            choice: AnyId::new(format!("choice.t{i}")).unwrap(),
            text: "Tell me more about the lanes".to_owned(),
        })
        .collect();
    for size in SIZES {
        let canvas = ui::canvas(G2::new(size.0, size.1));
        let mut o = render::Overlay::default();
        ui::dock_panel(
            &mut o,
            w,
            h,
            &WHARF,
            &UiState::default(),
            ui::Notices::default(),
            canvas,
        );
        let card = o.panels[0].rect;
        let talk = ui::talk_rect(w, h, None, &choices, canvas).unwrap();
        let goal = ui::goal_rect(w, canvas).unwrap();
        let hold = ui::hold_rect(w, canvas).unwrap();
        assert!(
            !overlaps(&card, &talk),
            "{size:?}: card {card:?} over talk {talk:?}"
        );
        assert!(
            !overlaps(&talk, &goal),
            "{size:?}: talk {talk:?} under goal {goal:?}"
        );
        assert!(!overlaps(&talk, &hold), "{size:?}: talk under the hold");
    }
}

#[test]
fn on_the_map_the_goal_banner_sits_beside_the_title() {
    let s = busy_flying();
    let w = s.world();
    for size in SIZES {
        let canvas = ui::canvas(G2::new(size.0, size.1));
        let title = ui::map_title_rect();
        let goal = ui::map_goal_rect(w, canvas).unwrap();
        let hold = ui::hold_rect(w, canvas).unwrap();
        assert_apart(
            size,
            &[("title", title), ("goal", goal), ("hold", hold)],
            canvas,
        );
    }
}

#[test]
fn every_active_goal_shows_with_who_gave_it_and_where() {
    let mut state = new_state();
    add_goals(&mut state.world);
    state
        .world
        .progress
        .beats
        .extend([beat("beat.t5", None, "Nobody asked for this one.", 5)]);
    let goals = hooks::goals(&state.world);
    let sources: Vec<&str> = goals.iter().map(|g| g.source.as_str()).collect();
    assert_eq!(
        sources,
        [
            "Odo Fenn  ·  Kettle Dock",
            "Rook Adeyemi  ·  Cinder Hook",
            "Orrin Vale  ·  Lantern Wharf",
            "Ilse Marrow  ·  Lantern Wharf",
        ],
        "newest first; a goal with no giver isn't shown"
    );
    assert_eq!(
        hooks::goal_line(&state.world).as_deref(),
        Some("Bring 30 glassite to Odo at Kettle Dock.")
    );
    // All of them are drawn, each under its source.
    let mut o = render::Overlay::default();
    ui::goal(&mut o, &state.world, ui::DESIGN);
    assert_eq!(o.panels.len(), 1, "one banner");
    for g in &goals {
        assert!(o.texts.iter().any(|t| t.text == g.source));
        assert!(o.texts.iter().any(|t| t.text == g.goal));
    }
    for t in &o.texts {
        assert_eq!(
            t.color,
            render::Color::BLACK,
            "{}: black on parchment",
            t.text
        );
    }
}

#[test]
fn the_hullwright_shows_what_is_fitted_and_every_card_names_its_holder() {
    let mut state = new_state();
    let w = &mut state.world;
    w.player
        .fittings
        .insert(UpgradeId::from_static("upgrade.mining_lens"));
    let s = Session::new(state);
    let (w, h) = (s.world(), s.sim().history());
    let services = hooks::station_services(w, h, &WHARF);
    let lens = services
        .iter()
        .flat_map(|e| &e.offers)
        .find(|o| o.name == "Mining lens")
        .expect("the fitted lens still shows under Orrin");
    assert!(lens.owned);
    let mut o = render::Overlay::default();
    let buttons = ui::dock_panel(
        &mut o,
        w,
        h,
        &WHARF,
        &UiState::default(),
        ui::Notices::default(),
        ui::DESIGN,
    );
    assert!(o.texts.iter().any(|t| t.text == "Fitted"), "a Fitted state");
    assert!(
        !buttons.iter().any(|b| matches!(
            &b.action,
            ui::UiAction::Service(ServiceRequest::Fit { upgrade }) if upgrade.as_str() == "upgrade.mining_lens"
        )),
        "no button for what's already fitted"
    );
    // Every station's card says who holds it; nobody owns Cinder Hook at the start.
    for st in w.stations.values() {
        let holder = ui::holder(w, st);
        assert!(!holder.is_empty());
        if st.id == HOOK {
            assert!(
                matches!(holder.as_str(), "Unclaimed" | "Contested"),
                "{holder}"
            );
        }
    }
}

/// A world with the flagship undocked beside an ice rock, mining it or not.
fn at_ice(mining: bool) -> WorldState {
    let mut w = new_state().world;
    let rock = w
        .rocks
        .values()
        .find(|r| r.rock_type.as_str() == "rock_type.ice")
        .unwrap()
        .clone();
    let region = galaxy_ship::space::rock_region(&w, &rock).unwrap().clone();
    let ship = flagship_mut(&mut w);
    ship.docked_at = None;
    ship.region = region;
    ship.position = rock.position + Vec2::new(10.0, 0.0);
    ship.activity.mining = mining.then(|| galaxy_contract::ship::MiningRun {
        rock: rock.id.clone(),
        progress: 0.0,
        mined: 0,
    });
    w
}

fn hint_shows(world: &WorldState, book: &HintBook) -> bool {
    let mut o = render::Overlay::default();
    ui::hint(&mut o, world, book, None, ui::DESIGN);
    !o.panels.is_empty()
}

#[test]
fn an_action_hint_retires_after_the_player_has_done_it_a_few_times() {
    let rules = rules();
    let times = rules.retire_after(HintKind::Mine);
    assert!(times >= 2, "the data teaches more than once");
    let idle = at_ice(false);
    let mining = at_ice(true);
    assert_eq!(ui::hint_kind(&idle), Some(HintKind::Mine));
    let mut book = HintBook::new(rules.clone(), BTreeMap::new());
    for i in 0..times {
        assert!(hint_shows(&idle, &book), "run {i}: still teaching");
        // The player starts mining, then stops.
        book.observe(Acts::of(&idle), &mining);
        book.observe(Acts::of(&mining), &idle);
    }
    assert!(!hint_shows(&idle, &book), "retired");
    assert!(book.take_changed());
    // The count is the save's record: a book from it keeps the hint retired.
    let again = HintBook::new(rules, book.counts().clone());
    assert!(!hint_shows(&idle, &again));
    assert_eq!(book.counts()["mine"], times);
}

#[test]
fn the_mining_readout_fades_once_learned_and_comes_back_for_a_new_rate() {
    let rules = rules();
    let times = rules.retire_after(HintKind::MiningRate);
    let idle = at_ice(false);
    let mining = at_ice(true);
    assert_eq!(ui::hint_kind(&mining), Some(HintKind::MiningRate));
    let mut book = HintBook::new(rules, BTreeMap::new());
    for i in 0..times {
        book.observe(Acts::of(&idle), &mining);
        assert!(hint_shows(&mining, &book), "run {i}: the readout shows");
        // It stays up while that run lasts.
        book.observe(Acts::of(&mining), &mining);
        assert!(hint_shows(&mining, &book), "run {i}: through the run");
        book.observe(Acts::of(&mining), &idle);
    }
    book.observe(Acts::of(&idle), &mining);
    assert!(!hint_shows(&mining, &book), "learned");
    // A fitted lens changes the rate: the new rate teaches again.
    let mut lensed = mining.clone();
    lensed
        .player
        .fittings
        .insert(UpgradeId::from_static("upgrade.mining_lens"));
    book.observe(Acts::of(&idle), &idle);
    book.observe(Acts::of(&idle), &lensed);
    assert!(hint_shows(&lensed, &book), "the lens's rate shows");
}

#[test]
fn the_full_hold_notice_fades_after_being_seen() {
    let rules = rules();
    let times = rules.retire_after(HintKind::HoldFull);
    let mut full = at_ice(false);
    let cap = {
        let ship = flagship_mut(&mut full).clone();
        galaxy_contract::modifiers::hold_capacity(&full, &ship)
    };
    flagship_mut(&mut full)
        .hold
        .cargo
        .insert(WareId::from_static("ware.ice"), u32::try_from(cap).unwrap());
    assert_eq!(ui::hint_kind(&full), Some(HintKind::HoldFull));
    let mut away = full.clone();
    flagship_mut(&mut away).position = Vec2::new(-9000.0, -9000.0);
    assert_eq!(ui::hint_kind(&away), None);
    let mut book = HintBook::new(rules, BTreeMap::new());
    for i in 0..times {
        book.observe(Acts::of(&away), &full);
        assert!(hint_shows(&full, &book), "sighting {i}");
        book.observe(Acts::of(&full), &away);
    }
    book.observe(Acts::of(&away), &full);
    assert!(!hint_shows(&full, &book), "learned");
}

/// `hint_kind` mirrors `context_hint`'s checks: wherever the pill has text, the hint
/// knows which one it is, so it can retire.
#[test]
fn every_context_hint_has_a_kind() {
    let base = at_ice(false);
    let mut points: Vec<(RegionId, Vec2)> = Vec::new();
    for s in base.stations.values() {
        for d in [0.0, 0.6, 1.0, 1.4] {
            points.push((
                s.region.clone(),
                s.position + Vec2::new(s.dock_radius * d, 0.0),
            ));
        }
    }
    for r in base.rocks.values() {
        if let Some(region) = galaxy_ship::space::rock_region(&base, r) {
            points.push((region.clone(), r.position + Vec2::new(12.0, 0.0)));
        }
    }
    for lane in base.lanes.values() {
        for (region, at) in [(&lane.a, lane.a_gate), (&lane.b, lane.b_gate)] {
            points.push((region.clone(), at));
        }
    }
    let mut kinds = std::collections::BTreeSet::new();
    for (region, at) in points {
        let mut w = base.clone();
        let ship = flagship_mut(&mut w);
        ship.region = region;
        ship.position = at;
        let text = ui::context_hint(&w);
        let kind = ui::hint_kind(&w);
        assert_eq!(text.is_some(), kind.is_some(), "{text:?} vs {kind:?}");
        kinds.extend(kind);
    }
    for k in [HintKind::Dock, HintKind::Ride, HintKind::Mine] {
        assert!(kinds.contains(&k), "{k:?} was sampled");
    }
}
