//! The HUD laid out against itself (#34, #35), talk bubbles and the ship log among it
//! (#47 Phase 2), all active goals with their sources (#46), the station card's fitted
//! state and faction (#44), hints that retire once they've taught (1.8), and the radio
//! in flight (#47, 5.1). Pure rect maths over real worlds: no window or GPU.

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

/// Three of the longest lines the talk log takes.
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

/// The busiest flying HUD: goals, long talk lines, wares in the hold, undocked.
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
    let w = s.world();
    let hint = longest_hint(w);
    for size in SIZES {
        let canvas = ui::canvas(G2::new(size.0, size.1));
        let goal = ui::goal_rect(w, canvas).expect("goals show");
        let hold = ui::hold_rect(w, canvas).expect("the hold shows");
        // The icon with a wide "new" tag.
        let icon = ui::log::icon_area(120, canvas);
        let pill = ui::hint_rect(&hint, canvas, Some(icon));
        // The mining readout is the same pill.
        let mining = ui::hint_rect("Mining ice  ·  1.5 a second", canvas, Some(icon));
        let log = ui::log::panel_rect(canvas);
        let radio = ui::radio::rect(canvas);
        assert_apart(
            size,
            &[
                ("goal", goal),
                ("hold", hold),
                ("log icon", icon),
                ("hint", pill),
                ("radio", radio),
            ],
            canvas,
        );
        assert!(!overlaps(&mining, &radio), "{size:?}: mining pill on the radio");
        assert!(
            !overlaps(&mining, &icon),
            "{size:?}: mining pill on the icon"
        );
        // The open log keeps clear of the hold, the icon and the pill (the goal banner
        // steps aside for it).
        assert_apart(
            size,
            &[
                ("log", log),
                ("hold", hold),
                ("log icon", icon),
                ("hint", pill),
                ("radio", radio),
            ],
            canvas,
        );
    }
}

/// The radio as `galaxy play` shows it for a session: its frame from the world.
fn radio_ui(s: &Session) -> galaxy_app::radio::RadioUi {
    let mut radio = galaxy_app::radio::Radio::new(
        s.game().radio().clone(),
        galaxy_app::view::themes::Themes::bundled().clone(),
        true,
    );
    radio.update(s.world(), s.sim().history()).ui
}

#[test]
fn the_radio_shows_in_flight_where_its_measured_with_its_buttons_and_hides_docked() {
    // Flying beside the Wharf, with the busiest HUD.
    let mut state = new_state();
    add_goals(&mut state.world);
    let wharf = state.world.stations.get(&WHARF).unwrap().position;
    let ship = flagship_mut(&mut state.world);
    ship.docked_at = None;
    ship.position = wharf + Vec2::new(150.0, 0.0);
    let s = Session::new(state);
    let radio = radio_ui(&s);
    assert!(radio.show && radio.on);
    assert_eq!(radio.channel.as_deref(), Some("Lantern Wharf radio"));
    assert!(radio.signal > 0.99, "close by: full signal");
    let ui_state = UiState {
        radio: radio.clone(),
        ..UiState::default()
    };
    let view = galaxy_app::view::View::new();
    let rig = galaxy_app::view::CameraRig::new(G2::ZERO);
    for size in SIZES {
        let viewport = G2::new(size.0, size.1);
        let canvas = ui::canvas(viewport);
        let scale = ui::ui_scale(viewport);
        let (scene, buttons) =
            galaxy_app::frame::compose(&view, &s, &rig, &ui_state, 1.0, 0.0, viewport);
        let at = ui::radio::rect(canvas);
        let scaled = Rect::new(at.x * scale, at.y * scale, at.w * scale, at.h * scale);
        assert!(
            scene.overlay.panels.iter().any(|p| p.rect == scaled),
            "{size:?}: the radio is drawn where it's measured"
        );
        // Off, Prev and Next are buttons inside it (Prev and Next only with a second
        // channel in range; here there's one).
        let tunes: Vec<_> = buttons
            .iter()
            .filter(|b| matches!(b.action, ui::UiAction::Radio(_)))
            .collect();
        assert_eq!(tunes.len(), 1, "{size:?}: Off, with one channel in range");
        assert!(tunes.iter().all(|b| b.rect.x >= scaled.x
            && b.rect.y >= scaled.y
            && b.rect.x + b.rect.w <= scaled.x + scaled.w + 0.5
            && b.rect.y + b.rect.h <= scaled.y + scaled.h + 0.5));
        // The signal is bars and a word; the channel's name and a text bit show.
        for want in ["Strong", "Lantern Wharf radio", "Off"] {
            assert!(
                scene.overlay.texts.iter().any(|t| t.text == want),
                "{size:?}: `{want}`"
            );
        }
        // Text on the radio is pure white or black (black only on its buttons).
        for t in scene.overlay.texts.iter().filter(|t| {
            t.pos.x >= scaled.x && t.pos.y >= scaled.y && t.pos.y <= scaled.y + scaled.h
        }) {
            assert!(
                t.color == render::Color::WHITE || t.color == render::Color::BLACK,
                "{size:?}: `{}` in {:?}",
                t.text,
                t.color
            );
        }
    }
    // Docked: no radio.
    let docked = Session::new(new_state());
    let radio = radio_ui(&docked);
    assert!(!radio.show);
    let ui_state = UiState {
        radio,
        ..UiState::default()
    };
    let (_, buttons) =
        galaxy_app::frame::compose(&view, &docked, &rig, &ui_state, 1.0, 0.0, ui::DESIGN);
    assert!(
        !buttons
            .iter()
            .any(|b| matches!(b.action, ui::UiAction::Radio(_)))
    );
}

#[test]
fn the_radio_says_its_signal_in_words_and_bars_and_rotates_its_bits() {
    assert_eq!(ui::radio::strength(1.0), "Strong");
    assert_eq!(ui::radio::strength(0.5), "Fair");
    assert_eq!(ui::radio::strength(0.1), "Faint");
    assert_eq!(ui::radio::strength(0.0), "None");
    assert_eq!(ui::radio::bars(1.0), 4);
    assert_eq!(ui::radio::bars(0.3), 2);
    assert_eq!(ui::radio::bars(0.01), 1, "any signal lights a bar");
    assert_eq!(ui::radio::bars(0.0), 0);
    let bits = vec!["a".to_owned(), "b".to_owned()];
    assert_eq!(ui::radio::bit_at(&bits, 0.0), Some("a"));
    assert_eq!(ui::radio::bit_at(&bits, ui::radio::BIT_SECONDS + 0.1), Some("b"));
    assert_eq!(ui::radio::bit_at(&bits, 2.0 * ui::radio::BIT_SECONDS), Some("a"));
    assert_eq!(ui::radio::bit_at(&[], 3.0), None);
}

#[test]
fn the_open_log_takes_the_goal_banners_place_and_pins_every_job() {
    let s = busy_flying();
    let w = s.world();
    let ui_state = UiState {
        log_open: true,
        ..UiState::default()
    };
    let view = galaxy_app::view::View::new();
    let rig = galaxy_app::view::CameraRig::new(G2::ZERO);
    let (scene, _) = galaxy_app::frame::compose(&view, &s, &rig, &ui_state, 1.0, 0.0, ui::DESIGN);
    let goals = hooks::goals(w);
    for g in &goals {
        // Each job once: pinned in the log, not on the banner as well.
        let shown = scene
            .overlay
            .texts
            .iter()
            .filter(|t| t.text == g.source)
            .count();
        assert_eq!(shown, 1, "{}", g.source);
    }
    let log = ui::log::panel_rect(ui::DESIGN);
    assert!(
        scene.overlay.panels.iter().any(|p| p.rect == log),
        "the panel is drawn where it's measured"
    );
}

#[test]
fn the_hint_pill_keeps_clear_of_the_log_icon_and_centres_without_it() {
    let canvas = G2::new(1280.0, 720.0);
    let text = "Space  ·  ride the lantern current to Brassmouth Narrows";
    let alone = ui::hint_rect(text, canvas, None);
    assert!((alone.center().x - 640.0).abs() < 0.5, "centred: {alone:?}");
    // Something wide in the corner pushes it right, at the same height.
    let wide = Rect::new(18.0, 600.0, 460.0, 102.0);
    let moved = ui::hint_rect(text, canvas, Some(wide));
    assert!(!overlaps(&moved, &wide), "{moved:?}");
    assert_eq!(moved.y, alone.y, "it stays low");
    // The log icon is small: the pill stays centred.
    let icon = ui::log::icon_area(3, canvas);
    assert_eq!(ui::hint_rect(text, canvas, Some(icon)), alone);
}

/// A bubble with the longest line the talk log takes and four answers.
fn long_bubble() -> ui::talk::Bubble {
    let text = "I've been watching the lanes since before your hull was cut, and I can tell you the current runs warmer east of the Wharf this time of the shift. Mind the ore barges; they don't turn for anyone, least of all a Wren with a full hold. If you see Rook, tell him the plate is coming, and that I said so. He'll grumble, but he'll know what it means and he'll wait.";
    let choices: Vec<OpenChoice> = (0..4)
        .map(|i| OpenChoice {
            npc: ILSE,
            choice: AnyId::new(format!("choice.t{i}")).unwrap(),
            text: "Tell me more about the lanes".to_owned(),
        })
        .collect();
    let line = TalkLine {
        channel: Channel::DockTalk,
        speaker: Some(ILSE),
        place: Some(WHARF),
        text: text.to_owned(),
        cites: vec![FactId(1)],
        about: Vec::new(),
        choices: choices
            .iter()
            .map(|c| galaxy_contract::talk::TalkChoice {
                id: c.choice.clone(),
                text: c.text.clone(),
            })
            .collect(),
    };
    assert!(line.validate().is_ok());
    ui::talk::Bubble::new(Some(TalkId(1)), line, &choices, 3)
}

const MARKET: StationPartId = StationPartId::from_static("part.wharf.market");
const WHARF_ROOM: StationPartId = StationPartId::from_static("part.wharf.wharf");

#[test]
fn docked_the_bubble_room_key_plate_goal_hold_and_log_icon_keep_apart() {
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
    let bubble = long_bubble();
    for size in SIZES {
        let canvas = ui::canvas(G2::new(size.0, size.1));
        let mut o = render::Overlay::default();
        let (card, _) = ui::rooms::panel(
            &mut o,
            w,
            h,
            &WHARF,
            &MARKET,
            &UiState::default(),
            ui::Notices::default(),
            canvas,
        )
        .unwrap();
        let goal = ui::goal_rect(w, canvas).unwrap();
        let hold = ui::hold_rect(w, canvas).unwrap();
        let icon = ui::log::icon_area(5, canvas);
        let key = ui::station::key_area(w, &WHARF, canvas).unwrap();
        let plate = ui::station::plate_rect(w, &WHARF);
        let bounds = ui::station::bubble_bounds(w, &WHARF, true, 5, canvas);
        // Wherever the speaker's spot is on screen, even off the room's edges.
        for anchor in [
            bounds.center(),
            G2::new(bounds.x, bounds.y),
            G2::new(bounds.x + bounds.w, bounds.y + bounds.h),
            G2::new(-40.0, canvas.y * 0.5),
        ] {
            let mut o = render::Overlay::default();
            let (rect, buttons) =
                ui::talk::dock_bubble(&mut o, w, &bubble, anchor, bounds, &UiState::default());
            assert_apart(
                size,
                &[
                    ("bubble", rect),
                    ("room", card),
                    ("goal", goal),
                    ("hold", hold),
                    ("log icon", icon),
                    ("key", key),
                    ("plate", plate),
                ],
                canvas,
            );
            // Every answer is a button inside the bubble, numbered as its key.
            let answers: Vec<_> = buttons
                .iter()
                .filter(|b| matches!(b.action, ui::UiAction::Reply { .. }))
                .collect();
            assert_eq!(answers.len(), 4, "{size:?}");
            for b in &answers {
                assert!(
                    b.rect.x >= rect.x
                        && b.rect.x + b.rect.w <= rect.x + rect.w + 0.5
                        && b.rect.y >= rect.y
                        && b.rect.y + b.rect.h <= rect.y + rect.h + 0.5,
                    "{size:?}: answer {:?} outside {rect:?}",
                    b.rect
                );
            }
            assert!(o.texts.iter().any(|t| t.text.starts_with("1  ")));
            assert!(o.texts.iter().any(|t| t.text.starts_with("4  ")));
        }
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
    // Orrin's wharf room lists it as Fitted.
    let mut o = render::Overlay::default();
    let (_, buttons) = ui::rooms::panel(
        &mut o,
        w,
        h,
        &WHARF,
        &WHARF_ROOM,
        &UiState::default(),
        ui::Notices::default(),
        ui::DESIGN,
    )
    .unwrap();
    assert!(o.texts.iter().any(|t| t.text == "Fitted"), "a Fitted state");
    assert!(
        !buttons.iter().any(|b| matches!(
            &b.action,
            ui::UiAction::Service(ServiceRequest::Fit { upgrade }) if upgrade.as_str() == "upgrade.mining_lens"
        )),
        "no button for what's already fitted"
    );
    // Every station's plate says who holds it; nobody owns Cinder Hook at the start.
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

/// The ship log never grows with history (#47 Phase 2): three hundred notices fill the
/// same panel as three, one page at a time; notices are dark tickets marked "Notice",
/// apart from people's lines.
#[test]
fn the_log_pages_instead_of_growing_and_marks_each_kind() {
    let canvas = ui::DESIGN;
    let draw = |notices: u64| {
        let mut state = new_state();
        let w = &mut state.world;
        add_goals(w);
        for i in 0..notices {
            let id = TalkId(i + 1);
            let line = TalkLine {
                channel: Channel::Chronicle,
                speaker: None,
                place: Some(HOOK),
                text: format!("A chandlery opened at Cinder Hook, number {i}."),
                cites: vec![FactId(1)],
                about: Vec::new(),
                choices: Vec::new(),
            };
            w.talk.lines.insert(
                id,
                SaidLine {
                    id,
                    tick: Tick(10 + i),
                    line,
                },
            );
            w.talk.last = i + 1;
        }
        let s = Session::new(state);
        let mut o = render::Overlay::default();
        let ui_state = UiState {
            log_open: true,
            ..UiState::default()
        };
        let buttons = ui::log::panel(&mut o, s.world(), s.sim().history(), &ui_state, canvas);
        (o, buttons)
    };
    let (few, _) = draw(3);
    let (many, buttons) = draw(300);
    assert_eq!(few.panels[0].rect, many.panels[0].rect, "same size");
    let notices = |o: &render::Overlay| o.texts.iter().filter(|t| t.text == "NOTICE").count();
    assert_eq!(notices(&few), 3);
    assert!(
        (1..=12).contains(&notices(&many)),
        "one page: {}",
        notices(&many)
    );
    assert!(
        many.texts.iter().any(|t| t.text.ends_with(" of 300")),
        "it says how far it goes"
    );
    assert!(
        buttons
            .iter()
            .any(|b| matches!(b.action, ui::UiAction::LogFrom(n) if n > 0)),
        "Older"
    );
    // Pinned jobs head the panel with their word.
    assert!(many.texts.iter().any(|t| t.text == "JOB"));
    // A notice's text is white on its dark ticket; everything else black on light.
    for t in &many.texts {
        assert!(
            t.color == render::Color::WHITE || t.color == render::Color::BLACK,
            "{}: pure white or black",
            t.text
        );
    }
    assert!(
        many.texts
            .iter()
            .any(|t| t.text.starts_with("A chandlery") && t.color == render::Color::WHITE)
    );
}

#[test]
fn docked_overview_plate_key_goal_hold_and_log_keep_apart_and_every_plate_names_its_holder() {
    let mut state = new_state();
    add_goals(&mut state.world);
    let s = Session::new(state);
    let w = s.world();
    for size in SIZES {
        let canvas = ui::canvas(G2::new(size.0, size.1));
        for station in w.stations.keys() {
            let key = ui::station::key_area(w, station, canvas).expect("every station has parts");
            let plate = ui::station::plate_rect(w, station);
            let bounds = ui::station::bubble_bounds(w, station, false, 120, canvas);
            assert!(
                bounds.w > 200.0 && bounds.h > 120.0,
                "{size:?} {station}: {bounds:?}"
            );
            assert_apart(
                size,
                &[
                    ("plate", plate),
                    ("key", key),
                    ("goal", ui::goal_rect(w, canvas).unwrap()),
                    ("hold", ui::hold_rect(w, canvas).unwrap()),
                    ("log icon", ui::log::icon_area(120, canvas)),
                ],
                canvas,
            );
            // The plate shows the holder, including at Cinder Hook (#44).
            let mut o = render::Overlay::default();
            ui::station::plate(&mut o, w, station);
            let st = w.stations.get(station).unwrap();
            assert!(
                o.texts.iter().any(|t| t.text == ui::holder(w, st)),
                "{station}"
            );
            for t in &o.texts {
                assert_eq!(
                    t.color,
                    render::Color::BLACK,
                    "{}: black on parchment",
                    t.text
                );
            }
        }
    }
}
