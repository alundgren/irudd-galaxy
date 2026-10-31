//! The hullwright's cradle, headless (#47, 3.2): the drag-to-fit state machine (pick →
//! hover → valid or invalid → drop → request), the click path, budgets, the locker,
//! Make's confirm and the cradle's geometry, through the app's own `Session` and room
//! state. No window or GPU.

use std::path::PathBuf;

use galaxy_app::game::{self, Game, GameOptions, MindMode};
use galaxy_app::session::Session;
use galaxy_app::ui::rooms::fit::{self, FIT_SECONDS, Over, Source, WharfAction};
use galaxy_app::ui::{self, UiAction, UiState};
use galaxy_app::view::cradle;
use galaxy_contract::parts::RoomKind;
use galaxy_contract::prelude::*;
use galaxy_core::SimState;
use render::kit::{LookId, Socket, SocketRing};

const WHARF: StationId = StationId::from_static("station.lantern_wharf");
const HOOK: StationId = StationId::from_static("station.cinder_hook");
const LENS: UpgradeId = UpgradeId::from_static("upgrade.mining_lens");
const PODS: UpgradeId = UpgradeId::from_static("upgrade.hold_pods");
const FINS: UpgradeId = UpgradeId::from_static("upgrade.swift_fins");
const LAMP: UpgradeId = UpgradeId::from_static("upgrade.seam_lamp");
const NAV: UpgradeId = UpgradeId::from_static("upgrade.nav_chip");
const EXTENDER: UpgradeId = UpgradeId::from_static("upgrade.hold_extender");
const LONG_DRILL: UpgradeId = UpgradeId::from_static("upgrade.long_drill");
const DRILL: SocketId = SocketId::from_static("socket.drill");
const HOLD: SocketId = SocketId::from_static("socket.hold");
const ENGINE: SocketId = SocketId::from_static("socket.engine");

fn packs() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../packs")
}

/// A new game docked at the Wharf with money to spend, the lens on offer, enough sold
/// for hold pods, the nav chip in the rack, and `tweak` on top.
fn at_the_wharf(tweak: impl FnOnce(&mut SimState)) -> (Session, UiState) {
    let mut state = game::new_state(&packs(), 1, false).unwrap();
    let w = &mut state.world;
    w.player.credits = 5_000 * 100;
    w.player.fittings.insert(NAV);
    // The lens is on offer once b03 is done.
    w.progress.beats.insert(
        BeatId::new("beat.b03_first_delivery").unwrap(),
        galaxy_contract::progress::BeatState {
            pack: "core".parse().unwrap(),
            act: 1,
            kind: galaxy_contract::progress::BeatKind::Directed,
            status: BeatStatus::Completed,
            step: 0,
            step_id: None,
            goal: String::new(),
            giver: None,
            since: None,
            step_since: None,
            done_since: None,
            started: Tick(0),
        },
    );
    tweak(&mut state);
    let opts = GameOptions {
        packs: packs(),
        gm: false,
        mind: MindMode::Fake { seed: 1 },
        hot_reload: false,
    };
    let mut s = Session::with_game(Game::open(state, &opts).unwrap());
    s.begin();
    // Enough sold for hold pods.
    let ship = s.world().player.ship.clone().unwrap();
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
    let station = s.flagship().unwrap().docked_at.clone().expect("docked");
    let ui_state = UiState {
        room: ui::rooms::of_kind(s.world(), &station, RoomKind::Wharf),
        ..UiState::default()
    };
    assert!(ui_state.room.is_some(), "{station} has a wharf room");
    (s, ui_state)
}

fn ticks(s: &mut Session, n: usize) {
    for _ in 0..n {
        s.tick(PlayerInput::default());
    }
}

/// Lets the request go out as the window does, and settles it.
fn send(s: &mut Session, ui: &mut UiState, req: ServiceRequest) {
    fit::send(s, &mut ui.wharf, req);
    ticks(s, 3);
    fit::settle(s, &mut ui.wharf);
}

fn spec(s: &Session, ui: &UiState) -> cradle::CradleSpec {
    let station = s.flagship().unwrap().docked_at.clone().unwrap();
    let part = ui::station::room_here(s.world(), &station, ui.room.as_ref().unwrap()).unwrap();
    cradle::spec(s.world(), s.sim().history(), &station, &part, &ui.wharf)
        .expect("the cradle shows")
}

fn ring(spec: &cradle::CradleSpec, socket: Socket) -> SocketRing {
    spec.rings[Socket::ALL.iter().position(|s| *s == socket).unwrap()]
}

#[test]
fn dragging_the_lens_to_the_drill_mount_fits_it_with_the_animation() {
    let (mut s, mut ui) = at_the_wharf(|_| {});
    let lens = WharfAction::Part {
        part: LENS,
        from: Source::Tray,
    };
    // Pick: the press picks it up.
    assert!(ui.wharf.press(s.world(), &lens));
    // Hover: the drill mount lights as a drop target and the ghost lens sits on it.
    ui.wharf.hover(Some(Over::Socket(DRILL)));
    let lit = spec(&s, &ui);
    assert_eq!(ring(&lit, Socket::Nose), SocketRing::DropTarget);
    assert_eq!(ring(&lit, Socket::Tail), SocketRing::Empty);
    assert_eq!(lit.ghost, Some(LookId::Lens));
    // The preview shows the change before letting go.
    let p = ui.wharf.held_preview(s.world(), s.sim().history()).unwrap();
    assert!(p.refusal.is_none(), "{:?}", p.refusal);
    assert!(p.after.mining_rate > p.before.mining_rate);
    assert_eq!(p.budget_after.power_used, p.budget_before.power_used + 2);
    // Drop: one request, and the fitting animation starts.
    let req = ui.wharf.release(s.world(), s.sim().history(), true);
    assert_eq!(req, Some(ServiceRequest::Fit { upgrade: LENS }));
    assert!(ui.wharf.held.is_none());
    let credits = s.world().player.credits;
    send(&mut s, &mut ui, req.unwrap());
    assert!(s.world().player.fittings.contains(&LENS));
    assert_eq!(s.world().player.credits, credits - 600 * 100);
    assert!(
        s.services
            .fitted
            .as_deref()
            .is_some_and(|f| f.contains("Fitted mining lens")),
        "a readable confirmation: {:?}",
        s.services.fitted
    );
    // During the animation the crane carries the lens; the ship doesn't wear it yet.
    let mid = spec(&s, &ui);
    assert_eq!(mid.carried.map(|c| c.0), Some(LookId::Lens));
    assert_eq!(mid.look.nose, None);
    ui.wharf.advance(FIT_SECONDS * 0.8);
    assert!(spec(&s, &ui).pose.lift < mid.pose.lift, "the hull dips");
    ui.wharf.advance(FIT_SECONDS);
    assert!(ui.wharf.fitting.is_none());
    // After: the ship wears it in the cradle, its socket rings gold, and in flight too.
    let after = spec(&s, &ui);
    assert_eq!(after.look.nose, Some(LookId::Lens));
    assert_eq!(ring(&after, Socket::Nose), SocketRing::Fitted);
    let flagship = s.flagship().unwrap().clone();
    assert_eq!(
        galaxy_app::view::fit_look::ship_look(s.world(), &flagship).nose,
        Some(LookId::Lens)
    );
}

#[test]
fn a_click_on_the_part_then_a_socket_fits_it_and_a_click_skips_the_animation() {
    let (mut s, mut ui) = at_the_wharf(|_| {});
    let pods = WharfAction::Part {
        part: PODS,
        from: Source::Tray,
    };
    // A press let go where it was: picked up, not dragged.
    assert!(ui.wharf.press(s.world(), &pods));
    assert_eq!(ui.wharf.release(s.world(), s.sim().history(), false), None);
    let held = ui.wharf.held.clone().expect("picked up");
    assert!(!held.dragging);
    let station = WHARF;
    let req = ui.wharf.click(
        s.world(),
        s.sim().history(),
        &station,
        &WharfAction::Socket(HOLD),
    );
    assert_eq!(req, Some(ServiceRequest::Fit { upgrade: PODS }));
    send(&mut s, &mut ui, req.unwrap());
    assert!(s.world().player.fittings.contains(&PODS));
    assert!(ui.wharf.skip(), "a click ends the animation");
    assert!(ui.wharf.fitting.is_none());
}

#[test]
fn the_wrong_socket_sends_nothing_and_the_keeper_says_why() {
    let (s, mut ui) = at_the_wharf(|_| {});
    ui.wharf.press(
        s.world(),
        &WharfAction::Part {
            part: LENS,
            from: Source::Tray,
        },
    );
    ui.wharf.hover(Some(Over::Socket(ENGINE)));
    assert_eq!(ring(&spec(&s, &ui), Socket::Tail), SocketRing::Empty);
    assert_eq!(
        spec(&s, &ui).ghost,
        None,
        "no ghost on a socket it can't go on"
    );
    assert_eq!(ui.wharf.release(s.world(), s.sim().history(), true), None);
    let said = ui.wharf.said.clone().expect("the keeper says why");
    assert!(said.refusal);
    assert_eq!(said.text, "That goes on the drill mount.");
    assert!(ui.wharf.fitting.is_none());
    // Let go over nothing: it goes back, quietly.
    ui.wharf.press(
        s.world(),
        &WharfAction::Part {
            part: LENS,
            from: Source::Tray,
        },
    );
    assert_eq!(ui.wharf.release(s.world(), s.sim().history(), true), None);
    assert!(ui.wharf.held.is_none());
}

#[test]
fn an_over_budget_fit_is_shown_and_refused_in_the_keepers_words() {
    // Lens (2) and seam lamp (2) fill the Wren's 4 power; swift fins draw 1 more.
    let (s, mut ui) = at_the_wharf(|state| {
        let w = &mut state.world;
        w.player.fittings.insert(LENS);
        w.player.fittings.insert(LAMP);
        w.player
            .flags
            .insert(FlagId::new("blueprint.swift_fins").unwrap());
        let ship = w.player.ship.clone().unwrap();
        w.ships
            .get_mut(&ship)
            .unwrap()
            .hold
            .cargo
            .insert(WareId::from_static("ware.refined_plate"), 4);
    });
    ui.wharf.press(
        s.world(),
        &WharfAction::Part {
            part: FINS,
            from: Source::Tray,
        },
    );
    ui.wharf.hover(Some(Over::Socket(ENGINE)));
    let p = ui.wharf.held_preview(s.world(), s.sim().history()).unwrap();
    assert!(p.budget_after.over_power());
    assert_eq!(
        (p.budget_after.power_used, p.budget_after.power_cap),
        (5, 4)
    );
    let refusal = p.refusal.expect("refused");
    assert!(
        refusal.contains("5 power") && refusal.contains("4"),
        "{refusal}"
    );
    // The engine doesn't light, and the drop sends nothing and spends nothing.
    assert_eq!(ring(&spec(&s, &ui), Socket::Tail), SocketRing::Empty);
    assert_eq!(ui.wharf.release(s.world(), s.sim().history(), true), None);
    assert!(
        ui.wharf.make.is_none(),
        "no confirm for a fit that can't happen"
    );
    assert!(ui.wharf.said.as_ref().is_some_and(|s| s.refusal));
}

#[test]
fn a_heavy_fit_previews_slower_top_speed_and_turn() {
    let (s, mut ui) = at_the_wharf(|_| {});
    ui.wharf.press(
        s.world(),
        &WharfAction::Part {
            part: PODS,
            from: Source::Tray,
        },
    );
    let p = ui.wharf.held_preview(s.world(), s.sim().history()).unwrap();
    assert!(p.after.hold > p.before.hold);
    assert!(p.after.top_speed < p.before.top_speed, "{p:?}");
    assert!(p.after.turn_rate < p.before.turn_rate, "{p:?}");
    assert!(p.budget_after.excess_mass() > 0);
}

#[test]
fn a_fitted_part_dragged_to_the_locker_comes_off_and_drags_back_on() {
    let (mut s, mut ui) = at_the_wharf(|state| {
        state.world.player.fittings.insert(LENS);
    });
    // A press on the drill mount picks the fitted lens up.
    assert!(ui.wharf.press(s.world(), &WharfAction::Socket(DRILL)));
    assert_eq!(ui.wharf.held.as_ref().unwrap().from, Source::Socket);
    ui.wharf.hover(Some(Over::Locker));
    let p = ui.wharf.held_preview(s.world(), s.sim().history()).unwrap();
    assert!(p.removing && p.after.mining_rate < p.before.mining_rate);
    let req = ui.wharf.release(s.world(), s.sim().history(), true);
    assert_eq!(req, Some(ServiceRequest::Remove { upgrade: LENS }));
    send(&mut s, &mut ui, req.unwrap());
    let ship = s.world().player.ship.clone().unwrap();
    assert!(!s.world().player.fittings.contains(&LENS));
    assert!(galaxy_contract::fit::locker(s.world(), &ship).any(|p| p == &LENS));
    assert!(s.world().player.fittings.contains(&NAV), "controls stay");
    assert!(
        fit::done_line(s.world(), &ServiceRequest::Remove { upgrade: LENS })
            .is_some_and(|l| l.contains("locker")),
    );
    // From the locker back onto the drill mount: a refit.
    assert!(ui.wharf.press(
        s.world(),
        &WharfAction::Part {
            part: LENS,
            from: Source::Locker,
        },
    ));
    ui.wharf.hover(Some(Over::Socket(DRILL)));
    let req = ui.wharf.release(s.world(), s.sim().history(), true);
    assert_eq!(req, Some(ServiceRequest::Refit { upgrade: LENS }));
    send(&mut s, &mut ui, req.unwrap());
    assert!(s.world().player.fittings.contains(&LENS));
    assert!(!galaxy_contract::fit::locker(s.world(), &ship).any(|p| p == &LENS));
    assert!(
        s.world().player.fittings.contains(&NAV),
        "controls survive the refit"
    );
}

#[test]
fn clicking_a_fitted_part_chooses_it_and_remove_takes_it_off() {
    let (mut s, mut ui) = at_the_wharf(|state| {
        state.world.player.fittings.insert(LENS);
    });
    assert!(ui.wharf.press(s.world(), &WharfAction::Socket(DRILL)));
    assert_eq!(ui.wharf.release(s.world(), s.sim().history(), false), None);
    assert_eq!(ui.wharf.chosen, Some(LENS));
    let req = ui.wharf.click(
        s.world(),
        s.sim().history(),
        &WHARF,
        &WharfAction::Remove(LENS),
    );
    send(&mut s, &mut ui, req.unwrap());
    assert!(!s.world().player.fittings.contains(&LENS));
    // Controls never come off: a module says so, and the desk refuses a removal.
    ui.wharf.click(
        s.world(),
        s.sim().history(),
        &WHARF,
        &WharfAction::Module(NAV),
    );
    assert!(ui.wharf.said.is_some());
    s.request_service(ServiceRequest::Remove { upgrade: NAV });
    assert!(s.services.notice.is_some());
    ticks(&mut s, 3);
    assert!(s.world().player.fittings.contains(&NAV));
}

#[test]
fn make_always_shows_what_it_uses_before_anything_is_spent() {
    let plate = WareId::from_static("ware.refined_plate");
    let (mut s, mut ui) = at_the_wharf(|state| {
        let w = &mut state.world;
        w.player
            .flags
            .insert(FlagId::new("blueprint.swift_fins").unwrap());
        let ship = w.player.ship.clone().unwrap();
        w.ships
            .get_mut(&ship)
            .unwrap()
            .hold
            .cargo
            .insert(plate.clone(), 4);
    });
    let ship = s.world().player.ship.clone().unwrap();
    let hold = |s: &Session| {
        s.world()
            .ships
            .get(&ship)
            .unwrap()
            .hold
            .cargo
            .get(&plate)
            .copied()
    };
    // A Make part dropped on its socket opens the confirm; nothing is spent yet.
    ui.wharf.press(
        s.world(),
        &WharfAction::Part {
            part: FINS,
            from: Source::Tray,
        },
    );
    ui.wharf.hover(Some(Over::Socket(ENGINE)));
    assert_eq!(ui.wharf.release(s.world(), s.sim().history(), true), None);
    assert_eq!(ui.wharf.make, Some((FINS, true)));
    ticks(&mut s, 2);
    assert_eq!(hold(&s), Some(4));
    let bill = galaxy_progress::hullwright::make_bill(s.world(), &FINS);
    assert_eq!(bill.len(), 1);
    assert_eq!((bill[0].need, bill[0].have), (4, 4));
    // Not now: closed, still nothing spent.
    ui.wharf
        .click(s.world(), s.sim().history(), &WHARF, &WharfAction::Cancel);
    assert!(ui.wharf.make.is_none());
    // The row's Make button: the same confirm; confirming makes it into the locker.
    ui.wharf.click(
        s.world(),
        s.sim().history(),
        &WHARF,
        &WharfAction::Make {
            part: FINS,
            fit: false,
        },
    );
    let req = ui.wharf.click(
        s.world(),
        s.sim().history(),
        &WHARF,
        &WharfAction::ConfirmMake,
    );
    assert_eq!(
        req,
        Some(ServiceRequest::Make {
            upgrade: FINS,
            fit: false
        })
    );
    send(&mut s, &mut ui, req.unwrap());
    assert_eq!(hold(&s).unwrap_or(0), 0, "the plate went into the fins");
    assert!(galaxy_contract::fit::locker(s.world(), &ship).any(|p| p == &FINS));
}

#[test]
fn the_cradle_rings_and_labels_are_click_and_drop_targets_on_screen() {
    let (s, ui_state) = at_the_wharf(|state| {
        state.world.player.fittings.insert(LENS);
    });
    let (world, history) = (s.world(), s.sim().history());
    let canvas = render::glam::Vec2::new(1280.0, 720.0);
    let part = ui::station::room_here(world, &WHARF, ui_state.room.as_ref().unwrap()).unwrap();
    let free = ui::station::bubble_bounds(world, &WHARF, true, 0, canvas);
    let (center, zoom) = cradle::camera_goal(world, &part, free, canvas).unwrap();
    let camera = cradle::camera_at(center, zoom);
    let sockets = cradle::screen_sockets(world, &WHARF, &part, &camera, canvas, 1.0);
    // The Wren's four sockets, inside the room the panel leaves.
    assert_eq!(sockets.len(), 4, "{sockets:?}");
    for s in &sockets {
        assert!(ui::contains(&free, s.at), "{s:?} in {free:?}");
        assert!(s.radius >= cradle::MIN_TARGET);
    }
    let room = ui::rooms::Room {
        world,
        history,
        station: &WHARF,
        part: &part,
        ui: &ui_state,
        notices: ui::Notices::default(),
    };
    let mut o = render::Overlay::default();
    let buttons = ui::rooms::cradle::furniture(&mut o, &room, &camera, canvas, 1.0, free);
    for s in &sockets {
        let hit = ui::hit(&buttons, s.at);
        assert_eq!(
            hit,
            Some(&UiAction::Wharf(WharfAction::Socket(s.socket.clone()))),
            "{s:?}"
        );
        assert_eq!(
            ui::rooms::cradle::over_of(hit),
            Some(Over::Socket(s.socket.clone()))
        );
    }
    // Labels name each socket and what's in it; the rack's modules are chips.
    assert!(o.texts.iter().any(|t| t.text == "Drill mount"));
    assert!(o.texts.iter().any(|t| t.text.starts_with("Mining lens")));
    assert!(o.texts.iter().any(|t| t.text == "Control rack"));
    assert!(o.texts.iter().any(|t| t.text == "Nav chip"));
    assert!(
        o.texts
            .iter()
            .all(|t| t.color == render::Color::BLACK || t.color == render::Color::WHITE)
    );
}

#[test]
fn junos_rough_rig_shows_only_the_drill_mount() {
    let (s, _) = at_the_wharf(|_| {});
    let shown = cradle::shown_sockets(s.world(), &HOOK);
    assert_eq!(shown, vec![(DRILL, Socket::Nose)]);
    assert_eq!(cradle::shown_sockets(s.world(), &WHARF).len(), 4);
}

/// The open room at `canvas` at the Wharf: its rect, text and buttons.
fn room_panel(
    s: &Session,
    ui: &UiState,
    canvas: render::glam::Vec2,
) -> (render::Rect, render::Overlay, Vec<ui::Button>) {
    let mut o = render::Overlay::default();
    let (rect, buttons) = ui::rooms::panel(
        &mut o,
        s.world(),
        s.sim().history(),
        &WHARF,
        ui.room.as_ref().unwrap(),
        ui,
        ui::Notices::default(),
        canvas,
    )
    .expect("the wharf's room");
    (rect, o, buttons)
}

#[test]
fn every_locker_part_has_a_pick_target() {
    // Five parts: more than one row of chips holds.
    let five = [EXTENDER, PODS, LONG_DRILL, LENS, LAMP];
    let (mut s, mut ui) = at_the_wharf(|state| {
        let ship = state.world.player.ship.clone().unwrap();
        let locker = state.world.progress.fit.lockers.entry(ship).or_default();
        locker.extend(five.clone());
    });
    let inside = |outer: &render::Rect, r: &render::Rect| {
        r.x >= outer.x
            && r.y >= outer.y
            && r.x + r.w <= outer.x + outer.w
            && r.y + r.h <= outer.y + outer.h
    };
    // The minimum window, the design size, larger, and wide.
    for (w, h) in [
        (960.0, 540.0),
        (1280.0, 720.0),
        (1920.0, 1080.0),
        (2560.0, 1080.0),
    ] {
        let canvas = ui::canvas(render::glam::Vec2::new(w, h));
        let screen = render::Rect::new(0.0, 0.0, canvas.x, canvas.y);
        ui.wharf.locker_page = 0;
        let mut found = std::collections::BTreeSet::new();
        for page in 0.. {
            assert!(page < five.len(), "the pages end at {w}x{h}");
            let (rect, o, buttons) = room_panel(&s, &ui, canvas);
            assert!(inside(&screen, &rect), "{rect:?} on {w}x{h}");
            for b in &buttons {
                if let UiAction::Wharf(WharfAction::Part {
                    part,
                    from: Source::Locker,
                }) = &b.action
                {
                    // On screen, in the room, and what the mouse finds there.
                    assert!(inside(&rect, &b.rect), "{part} at {:?} in {rect:?}", b.rect);
                    assert_eq!(ui::hit(&buttons, b.rect.center()), Some(&b.action));
                    found.insert(part.clone());
                }
            }
            let next = buttons.iter().rev().find_map(|b| match &b.action {
                UiAction::Wharf(WharfAction::LockerPage(n)) if *n > ui.wharf.locker_page => {
                    Some((*n, b.rect))
                }
                _ => None,
            });
            let Some((n, at)) = next else { break };
            // The pager reads literally, in the room, and a click there turns the page.
            assert!(o.texts.iter().any(|t| t.text == "Next"));
            assert!(o.texts.iter().any(|t| t.text == "Prev"));
            assert!(
                o.texts
                    .iter()
                    .any(|t| t.text.starts_with("Parts 1–") || page > 0)
            );
            assert!(inside(&rect, &at));
            let hit = ui::hit(&buttons, at.center()).cloned();
            let Some(UiAction::Wharf(a)) = hit else {
                panic!("Next at {at:?} on {w}x{h}");
            };
            assert_eq!(a, WharfAction::LockerPage(n));
            ui.wharf.click(s.world(), s.sim().history(), &WHARF, &a);
        }
        assert_eq!(
            found,
            five.iter().cloned().collect(),
            "every locker part can be picked up at {w}x{h}"
        );
    }
    // The last part, by click: picked up from the locker, then a click on its socket
    // refits it.
    let lamp = WharfAction::Part {
        part: LAMP,
        from: Source::Locker,
    };
    ui.wharf.click(s.world(), s.sim().history(), &WHARF, &lamp);
    assert_eq!(ui.wharf.held.as_ref().map(|h| h.from), Some(Source::Locker));
    let socket = fit::socket_of(s.world(), &LAMP).unwrap();
    let req = ui.wharf.click(
        s.world(),
        s.sim().history(),
        &WHARF,
        &WharfAction::Socket(socket),
    );
    assert_eq!(req, Some(ServiceRequest::Refit { upgrade: LAMP }));
    send(&mut s, &mut ui, req.unwrap());
    assert!(s.world().player.fittings.contains(&LAMP));
    assert!(ui.wharf.skip(), "a click ends the animation");
    // And one on a later page, by drag.
    assert!(ui.wharf.press(
        s.world(),
        &WharfAction::Part {
            part: LENS,
            from: Source::Locker,
        },
    ));
    ui.wharf.hover(Some(Over::Socket(DRILL)));
    let req = ui.wharf.release(s.world(), s.sim().history(), true);
    assert_eq!(req, Some(ServiceRequest::Refit { upgrade: LENS }));
    // Fewer parts than pages: the page shown is the last one there is.
    send(&mut s, &mut ui, req.unwrap());
    let canvas = ui::canvas(render::glam::Vec2::new(1280.0, 720.0));
    let (_, _, buttons) = room_panel(&s, &ui, canvas);
    let chips = buttons
        .iter()
        .filter(|b| {
            matches!(
                b.action,
                UiAction::Wharf(WharfAction::Part {
                    from: Source::Locker,
                    ..
                })
            )
        })
        .count();
    assert_eq!(chips, 3, "the three left all show in one row");
}
