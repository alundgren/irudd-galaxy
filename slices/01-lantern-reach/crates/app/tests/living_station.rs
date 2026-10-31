//! A station that lives (#47, 4.1), headless: the docked tilt and picking through it,
//! life driven by the sim (stock piles, the crane with your sale, the crowd, the
//! lamplighter, workers at scaffold), the themes, the calm caps and the quiet setting,
//! and that nothing moving ever covers a module's click target or its sign.

use std::path::PathBuf;

use galaxy_app::session::Session;
use galaxy_app::ui::{self, UiState};
use galaxy_app::view::life::{self, LifeFrame, LifeInput, LifeLevel, Role};
use galaxy_app::view::themes::{Prop, Themes};
use galaxy_app::view::{CameraRig, DOCK_TILT, TILT};
use galaxy_contract::parts::PartState;
use galaxy_contract::prelude::*;
use render::glam::{Vec2 as G2, Vec3};
use render::{Camera, Rect};

const WHARF: StationId = StationId::from_static("station.lantern_wharf");
const ICE: WareId = WareId::from_static("ware.ice");
const MARKET: StationPartId = StationPartId::from_static("part.wharf.market");
const DESIGN: G2 = G2::new(1280.0, 720.0);

fn packs() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../packs")
}

fn new_state() -> galaxy_core::SimState {
    galaxy_core::new_game(&galaxy_app::game::core_data(&packs()), 1).unwrap()
}

/// A session a few ticks in, docked at the Wharf, after `edit` changed its world.
fn session_with(edit: impl FnOnce(&mut WorldState)) -> Session {
    let mut state = new_state();
    edit(&mut state.world);
    let mut s = Session::new(state);
    for _ in 0..5 {
        s.tick(PlayerInput::default());
    }
    s
}

/// The flagship docked at `station`, wherever it was.
fn dock_at(world: &mut WorldState, station: &StationId) {
    let pos = world.stations.get(station).unwrap().position;
    let region = world.stations.get(station).unwrap().region.clone();
    let id = world.player.ship.clone().unwrap();
    let ship = world.ships.get_mut(&id).unwrap();
    ship.docked_at = Some(station.clone());
    ship.position = pos;
    ship.region = region;
}

fn theme<'a>(world: &WorldState, station: &StationId) -> &'a galaxy_app::view::themes::Theme {
    Themes::bundled().for_station(world, station)
}

fn life_at(s: &Session, station: &StationId, level: LifeLevel, time: f32) -> LifeFrame {
    let world = s.world();
    life::frame(&LifeInput {
        world,
        history: s.sim().history(),
        station,
        theme: theme(world, station),
        level,
        time,
    })
}

fn rig_for(s: &Session, ui_state: &UiState, viewport: G2) -> CameraRig {
    let goal = galaxy_app::frame::camera_goal(s, ui_state, 1.0, viewport).unwrap();
    let mut rig = CameraRig::new(G2::ZERO);
    rig.settle(&goal, s.world());
    rig
}

#[test]
fn docking_tilts_the_camera_and_flight_and_the_cradle_keep_their_own() {
    let s = session_with(|_| {});
    let goal = |ui: &UiState| galaxy_app::frame::camera_goal(&s, ui, 1.0, DESIGN).unwrap();
    assert_eq!(goal(&UiState::default()).tilt, DOCK_TILT);
    assert!((35.0..=45.0).contains(&DOCK_TILT));
    let room = UiState {
        room: Some(MARKET),
        ..UiState::default()
    };
    assert_eq!(
        goal(&room).tilt,
        DOCK_TILT,
        "a room keeps the station's tilt"
    );
    let cradle = UiState {
        room: Some(StationPartId::from_static("part.wharf.wharf")),
        ..UiState::default()
    };
    assert_eq!(goal(&cradle).tilt, TILT, "the cradle keeps its own framing");

    let flying = session_with(|w| {
        let id = w.player.ship.clone().unwrap();
        w.ships.get_mut(&id).unwrap().docked_at = None;
    });
    let g = galaxy_app::frame::camera_goal(&flying, &UiState::default(), 1.0, DESIGN).unwrap();
    assert_eq!(g.tilt, TILT, "flight stays on the plane's gentle tilt");

    // Undocking tilts back over the glide, and a click lands it at once.
    let mut rig = rig_for(&s, &UiState::default(), DESIGN);
    assert_eq!(rig.tilt, DOCK_TILT);
    rig.begin_glide();
    rig.glide_to(g.center, rig.zoom, TILT, 0.2);
    assert!(rig.tilt < DOCK_TILT && rig.tilt > TILT, "{}", rig.tilt);
    rig.skip_glide();
    rig.glide_to(g.center, rig.zoom, TILT, 0.0);
    assert_eq!(rig.tilt, TILT);
    assert!(!rig.gliding());
}

#[test]
fn modules_and_the_station_key_pick_through_the_tilted_camera() {
    let s = session_with(|_| {});
    for ui_state in [
        UiState::default(),
        UiState {
            room: Some(MARKET),
            ..UiState::default()
        },
    ] {
        for viewport in [DESIGN, G2::new(1920.0, 1080.0), G2::new(800.0, 450.0)] {
            let rig = rig_for(&s, &ui_state, viewport);
            let camera = rig.camera();
            assert_eq!(rig.tilt, DOCK_TILT);
            let scale = ui::ui_scale(viewport);
            let world = s.world();
            let targets = ui::station::module_targets(world, &WHARF, &camera, viewport, scale);
            assert_eq!(targets.len(), 3);
            let (_, buttons) = galaxy_app::frame::compose(
                &galaxy_app::view::View::new(),
                &s,
                &rig,
                &ui_state,
                1.0,
                0.0,
                viewport,
            );
            for t in &targets {
                let part = world.station_parts.get(&t.part).unwrap();
                let (at, _) = galaxy_app::view::modules::module_point(world, part).unwrap();
                // Where the tilted camera shows the module's middle is its target...
                let shown = camera
                    .world_to_screen(at.extend(galaxy_app::view::modules::LIFT * 0.5), viewport)
                    .unwrap();
                assert!((shown / scale - t.at).length() < 1e-3);
                // ...and a click there, in window pixels, opens it (unless a panel covers
                // it, as the open room covers its own module's far side).
                let opens = |p: G2| {
                    ui::hit(&buttons, p)
                        .is_some_and(|a| matches!(a, ui::UiAction::OpenRoom(id) if *id == t.part))
                };
                let covered = ui::hit(&buttons, shown).is_some_and(|a| {
                    !matches!(a, ui::UiAction::OpenRoom(_)) && !matches!(a, ui::UiAction::Absorb)
                });
                assert!(
                    opens(shown) || covered,
                    "{} at {shown} in {viewport}",
                    t.part
                );
                // The plane point under the click is the module's own.
                let back = camera
                    .screen_to_plane(shown, viewport, galaxy_app::view::modules::LIFT * 0.5)
                    .unwrap();
                assert!(back.truncate().distance(at) < 0.5, "{back} vs {at}");
            }
            // The station key opens every part in one click, tilted or not.
            for p in ui::station::parts_here(world, &WHARF) {
                assert!(
                    buttons
                        .iter()
                        .any(|b| matches!(&b.action, ui::UiAction::OpenRoom(id) if *id == p.id)),
                    "{}",
                    p.id
                );
            }
        }
    }
}

#[test]
fn stock_piles_follow_the_market_and_the_crane_carries_the_players_sale() {
    let mut s = session_with(|w| {
        let id = w.player.ship.clone().unwrap();
        w.ships.get_mut(&id).unwrap().hold.cargo.insert(ICE, 20);
    });
    let before = life_at(&s, &WHARF, LifeLevel::Full, 0.0);
    let ice = |f: &LifeFrame| f.stock.iter().filter(|c| c.ware == ICE).count();
    let line = s.world().markets.get(&WHARF).unwrap().lines[&ICE].clone();
    assert_eq!(ice(&before) as u32, life::crates_for(&line));
    let crane = before
        .crane
        .clone()
        .expect("the Wharf's market has a crane");
    assert!(crane.load.is_none(), "nothing to carry before a sale");

    s.request_trade(TradeRequest {
        ship: s.flagship().unwrap().id.clone(),
        station: WHARF,
        ware: ICE,
        side: TradeSide::Sell,
        quantity: 20,
        limit: None,
    });
    for _ in 0..3 {
        s.tick(PlayerInput::default());
    }
    assert_eq!(
        s.flagship().unwrap().hold.count(&ICE),
        0,
        "the sale went through"
    );
    let after = life_at(&s, &WHARF, LifeLevel::Full, 0.0);
    let line = s.world().markets.get(&WHARF).unwrap().lines[&ICE].clone();
    assert_eq!(ice(&after) as u32, life::crates_for(&line));
    assert!(ice(&after) >= ice(&before), "selling ice grows the pile");
    let berth_side = after.crane.as_ref().unwrap();
    let (ware, load) = berth_side.load.clone().expect("the crane lifts the sale");
    assert_eq!(ware, ICE);

    // A few seconds on, it has swung the crate over to the yard, then set it down.
    for _ in 0..90 {
        s.tick(PlayerInput::default());
    }
    let mid = life_at(&s, &WHARF, LifeLevel::Full, 0.0);
    if let Some((_, at)) = mid.crane.as_ref().and_then(|c| c.load.clone()) {
        assert!(at.distance(load) > 1.0, "the load moves");
    }
    for _ in 0..120 {
        s.tick(PlayerInput::default());
    }
    let done = life_at(&s, &WHARF, LifeLevel::Full, 0.0);
    assert!(done.crane.unwrap().load.is_none(), "and lets go");
}

#[test]
fn the_lamplighter_lights_one_more_lamp_per_station_pulse() {
    let lit = |pulses: i64| {
        let s = session_with(|w| {
            w.economy.stations.entry(WHARF).or_default().pulses_milli = pulses;
        });
        let f = life_at(&s, &WHARF, LifeLevel::Quiet, 0.0);
        let lamps = f.lamps.len();
        let lighter = f
            .figures
            .iter()
            .filter(|f| f.role == Role::Lamplighter)
            .count();
        (f.lamps.iter().filter(|l| l.lit).count(), lamps, lighter)
    };
    // The game's start (docking at the Wharf) is a pulse or so of its own.
    let (base, lamps, lighter) = lit(0);
    assert!(lamps >= 6, "{lamps} lamps");
    assert!(base < 3, "{base}");
    assert_eq!(lighter, 1);
    assert_eq!(lit(1_000).0, base + 1);
    assert_eq!(lit(3_400).0, base + 3, "whole pulses only");
    assert_eq!(lit(4_000).0, base + 4);
    assert_eq!(
        lit(500_000).0,
        lamps,
        "every lamp, once the station is busy"
    );
}

#[test]
fn a_crowd_gathers_when_news_lands_and_workers_come_with_scaffold() {
    let crowd = |f: &LifeFrame| f.figures.iter().filter(|f| f.role == Role::Crowd).count();
    let workers = |f: &LifeFrame| f.figures.iter().filter(|f| f.role == Role::Worker).count();
    let quiet = session_with(|_| {});
    let f = life_at(&quiet, &WHARF, LifeLevel::Full, 0.0);
    assert_eq!(crowd(&f), 0, "no news, no crowd");
    assert_eq!(workers(&f), 0, "nothing is being built");

    let busy = session_with(|w| {
        let id: RumorId = "rumor.test_ice_glut".parse().unwrap();
        w.rumors.insert(
            id.clone(),
            Rumor {
                id,
                claim: "Kettle Dock is drowning in ice.".into(),
                is_true: false,
                planted_by: None,
                heard_at: [WHARF].into_iter().collect(),
                heard_by_player: false,
                source: None,
            },
        );
        w.station_parts.get_mut(&MARKET).unwrap().state = PartState::Building;
    });
    let f = life_at(&busy, &WHARF, LifeLevel::Full, 0.0);
    let cap = theme(busy.world(), &WHARF).calm.max_crowd as usize;
    assert!((2..=cap).contains(&crowd(&f)), "{}", crowd(&f));
    assert!(workers(&f) >= 1, "workers at the scaffold");
    let quiet = life_at(&busy, &WHARF, LifeLevel::Quiet, 0.0);
    assert!(
        crowd(&quiet) <= 2 && crowd(&quiet) >= 1,
        "quiet keeps a small crowd"
    );
    assert!(quiet.figures.iter().all(|f| !f.moving), "and nothing moves");
}

/// Every station busy: lots of pulses, news, a part being built and a sale just made.
fn busy_everywhere(station: &StationId) -> Session {
    let station = station.clone();
    let mut s = session_with(|w| {
        dock_at(w, &station);
        for t in w.economy.stations.values_mut() {
            t.pulses_milli = 90_000;
        }
        w.economy
            .stations
            .entry(station.clone())
            .or_default()
            .pulses_milli = 90_000;
        let id: RumorId = "rumor.test_news".parse().unwrap();
        w.rumors.insert(
            id.clone(),
            Rumor {
                id,
                claim: "Lamps are cheap at the Observatory.".into(),
                is_true: false,
                planted_by: None,
                heard_at: [station.clone()].into_iter().collect(),
                heard_by_player: false,
                source: None,
            },
        );
        let part = w
            .station_parts
            .values()
            .find(|p| p.station == station && p.room != galaxy_contract::parts::RoomKind::Market)
            .map(|p| p.id.clone());
        if let Some(part) = part {
            w.station_parts.get_mut(&part).unwrap().state = PartState::Building;
        }
        let id = w.player.ship.clone().unwrap();
        w.ships.get_mut(&id).unwrap().hold.cargo.insert(ICE, 10);
    });
    if s.world()
        .markets
        .get(&station)
        .is_some_and(|m| m.lines.contains_key(&ICE))
    {
        s.request_trade(TradeRequest {
            ship: s.flagship().unwrap().id.clone(),
            station,
            ware: ICE,
            side: TradeSide::Sell,
            quantity: 10,
            limit: None,
        });
        s.tick(PlayerInput::default());
        s.tick(PlayerInput::default());
    }
    s
}

#[test]
fn every_station_keeps_its_themes_calm_caps_and_quiet_stills_it() {
    let stations: Vec<StationId> = new_state().world.stations.keys().cloned().collect();
    for station in stations {
        let s = busy_everywhere(&station);
        let world = s.world();
        let t = theme(world, &station);
        for time in [0.0, 3.0, 17.5] {
            let f = life_at(&s, &station, LifeLevel::Full, time);
            assert!(f.walkers() <= t.calm.max_walkers as usize, "{station}");
            assert!(f.particles <= t.calm.max_particles, "{station}");
            let crowd = f.figures.iter().filter(|f| f.role == Role::Crowd).count();
            assert!(crowd <= t.calm.max_crowd as usize, "{station}");
            let real = world
                .ships
                .values()
                .filter(|sh| {
                    world.player.ship.as_ref() != Some(&sh.id)
                        && sh.docked_at.is_none()
                        && sh
                            .position
                            .distance(world.stations.get(&station).unwrap().position)
                            < world.stations.get(&station).unwrap().dock_radius * 4.0
                })
                .count();
            assert!(
                f.craft.is_empty() || f.craft.len() + real <= t.calm.max_moving_craft as usize,
                "{station}: {} filler, {real} real",
                f.craft.len()
            );
            if t.has(Prop::LampString) {
                assert!(!f.lamps.is_empty(), "{station} has lamps");
            }
            let q = life_at(&s, &station, LifeLevel::Quiet, time);
            assert_eq!(q.walkers(), 0, "{station}: quiet has no walkers");
            assert!(q.craft.is_empty(), "{station}: quiet has no filler craft");
            assert!(q.figures.iter().all(|f| !f.moving), "{station}");
            assert!(q.particles <= f.particles);
            // What the numbers say stays: stock and lamps.
            assert_eq!(q.stock.len(), f.stock.len(), "{station}");
            // (Full may hold the newest lamp dark while the lamplighter walks to it.)
            let (ql, fl) = (
                q.lamps.iter().filter(|l| l.lit).count(),
                f.lamps.iter().filter(|l| l.lit).count(),
            );
            assert!(fl == ql || fl + 1 == ql, "{station}: {fl} vs {ql}");
        }
        let f = life_at(&s, &station, LifeLevel::Full, 1.0);
        assert!(
            f.walkers() > 0,
            "{station}: a busy station has people about"
        );
    }
}

/// The canvas rectangles a click target and its sign take, as the UI lays them out.
fn keep_clear(world: &WorldState, station: &StationId, camera: &Camera, viewport: G2) -> Vec<Rect> {
    let scale = ui::ui_scale(viewport);
    let mut out = Vec::new();
    for t in ui::station::module_targets(world, station, camera, viewport, scale) {
        let part = world.station_parts.get(&t.part).unwrap();
        out.push(Rect::new(
            t.at.x - t.radius,
            t.at.y - t.radius,
            t.radius * 2.0,
            t.radius * 2.0,
        ));
        let word = match part.state {
            PartState::Open => part.title().to_owned(),
            PartState::Closed => format!("{}  ·  Closed", part.title()),
            PartState::Building => format!("{}  ·  Building", part.title()),
        };
        let w = ui::text_width(&word, 14.0) + 20.0;
        out.push(Rect::new(
            t.at.x - w * 0.5,
            t.at.y + t.radius * 0.9,
            w,
            24.0,
        ));
    }
    out
}

#[test]
fn nothing_moving_covers_a_click_target_or_a_sign() {
    let stations: Vec<StationId> = new_state().world.stations.keys().cloned().collect();
    let mut checked = 0;
    for station in stations {
        let mut s = busy_everywhere(&station);
        let rooms: Vec<Option<StationPartId>> = std::iter::once(None)
            .chain(
                ui::station::parts_here(s.world(), &station)
                    .iter()
                    .filter(|p| p.room != galaxy_contract::parts::RoomKind::Wharf)
                    .map(|p| Some(p.id.clone())),
            )
            .collect();
        // Through the crane's whole swing.
        for _ in 0..8 {
            for room in &rooms {
                let ui_state = UiState {
                    room: room.clone(),
                    ..UiState::default()
                };
                for viewport in [DESIGN, G2::new(1920.0, 1080.0), G2::new(960.0, 540.0)] {
                    let camera = rig_for(&s, &ui_state, viewport).camera();
                    let scale = ui::ui_scale(viewport);
                    let avoid = keep_clear(s.world(), &station, &camera, viewport);
                    for time in [0.0, 4.0, 11.0, 29.0] {
                        let f = life_at(&s, &station, LifeLevel::Full, time);
                        for (foot, h) in f.movers() {
                            let a = camera.world_to_screen(foot, viewport).unwrap() / scale;
                            let b = camera
                                .world_to_screen(foot + Vec3::Z * h, viewport)
                                .unwrap()
                                / scale;
                            let lo = a.min(b);
                            let hi = a.max(b);
                            for r in &avoid {
                                let hit = lo.x < r.x + r.w
                                    && r.x < hi.x
                                    && lo.y < r.y + r.h
                                    && r.y < hi.y;
                                assert!(
                                    !hit,
                                    "{station} {room:?} {viewport} t={time}: a mover at {foot} \
                                     ({a}..{b}) covers {r:?}"
                                );
                            }
                            checked += 1;
                        }
                    }
                }
            }
            for _ in 0..24 {
                s.tick(PlayerInput::default());
            }
        }
    }
    assert!(checked > 500, "{checked} movers checked");
}

/// The first of `avoid` that a world line `from..to`, `width` world units thick, crosses
/// on the canvas.
fn line_covers(
    camera: &Camera,
    viewport: G2,
    avoid: &[Rect],
    (from, to): (Vec3, Vec3),
    width: f32,
) -> Option<(G2, Rect)> {
    let scale = ui::ui_scale(viewport);
    let px = |p: Vec3| camera.world_to_screen(p, viewport).unwrap() / scale;
    let half = (px(from + Vec3::X * width * 0.5) - px(from)).length();
    (0..=64).find_map(|i| {
        let p = px(from.lerp(to, i as f32 / 64.0));
        avoid
            .iter()
            .find(|r| {
                p.x + half > r.x
                    && p.x - half < r.x + r.w
                    && p.y + half > r.y
                    && p.y - half < r.y + r.h
            })
            .map(|r| (p, *r))
    })
}

/// Checks the crane's jib, hook line and load against every click target and sign, in
/// every room's framing and window size, full and quiet. Returns whether it carried.
fn crane_keeps_clear(s: &Session, station: &StationId, rooms: &[Option<StationPartId>]) -> bool {
    let mut carrying = false;
    for level in [LifeLevel::Full, LifeLevel::Quiet] {
        let f = life_at(s, station, level, 0.0);
        let Some(c) = &f.crane else {
            continue;
        };
        carrying |= c.load.is_some();
        for room in rooms {
            let ui_state = UiState {
                room: room.clone(),
                ..UiState::default()
            };
            for viewport in [DESIGN, G2::new(1920.0, 1080.0), G2::new(960.0, 540.0)] {
                let camera = rig_for(s, &ui_state, viewport).camera();
                let avoid = keep_clear(s.world(), station, &camera, viewport);
                let mut lines = c.lines().to_vec();
                if let Some((_, at)) = &c.load {
                    lines.push((*at, *at + Vec3::Z * render::kit::life::CRATE_SIZE * f.scale));
                }
                for (i, line) in lines.into_iter().enumerate() {
                    let what = ["jib", "hook", "load"][i];
                    if let Some((p, r)) = line_covers(&camera, viewport, &avoid, line, f.scale) {
                        panic!(
                            "{station} {room:?} {viewport} {level:?} tick {}: the crane's {what} at {p} covers {r:?}",
                            s.world().meta.tick.0
                        );
                    }
                }
            }
        }
    }
    carrying
}

#[test]
fn the_crane_keeps_clear_of_the_observatory_market_after_selling_ice() {
    // Seed 1, docked at the Observatory, 10 ice sold: 3.3 seconds on, the jib used to
    // cross the middle of the Market's click target.
    let station = StationId::from_static("station.glass_drift_observatory");
    let mut s = session_with(|w| {
        dock_at(w, &station);
        let id = w.player.ship.clone().unwrap();
        w.ships.get_mut(&id).unwrap().hold.cargo.insert(ICE, 10);
    });
    s.request_trade(TradeRequest {
        ship: s.flagship().unwrap().id.clone(),
        station: station.clone(),
        ware: ICE,
        side: TradeSide::Sell,
        quantity: 10,
        limit: None,
    });
    for _ in 0..99 {
        s.tick(PlayerInput::default());
    }
    assert_eq!(s.world().meta.tick.0, 104);
    assert_eq!(s.flagship().unwrap().hold.count(&ICE), 0, "sold");
    assert!(
        crane_keeps_clear(&s, &station, &[None]),
        "the crane carries the sale"
    );
}

#[test]
fn the_crane_keeps_clear_through_its_whole_swing_at_every_station() {
    let stations: Vec<StationId> = new_state().world.stations.keys().cloned().collect();
    for station in stations {
        // Docked there, selling ten of something its market buys.
        let ware = new_state().world.markets.get(&station).and_then(|m| {
            m.lines
                .iter()
                .find(|(_, l)| l.role == MarketRole::Buys)
                .map(|(w, _)| w.clone())
        });
        let Some(ware) = ware else {
            continue;
        };
        let mut s = session_with(|w| {
            dock_at(w, &station);
            let id = w.player.ship.clone().unwrap();
            w.ships
                .get_mut(&id)
                .unwrap()
                .hold
                .cargo
                .insert(ware.clone(), 10);
        });
        let id = s.flagship().unwrap().id.clone();
        s.request_trade(TradeRequest {
            ship: id,
            station: station.clone(),
            ware: ware.clone(),
            side: TradeSide::Sell,
            quantity: 10,
            limit: None,
        });
        s.tick(PlayerInput::default());
        assert_eq!(
            s.flagship().unwrap().hold.count(&ware),
            0,
            "{station}: sold"
        );
        let rooms: Vec<Option<StationPartId>> = std::iter::once(None)
            .chain(
                ui::station::parts_here(s.world(), &station)
                    .iter()
                    .filter(|p| p.room != galaxy_contract::parts::RoomKind::Wharf)
                    .map(|p| Some(p.id.clone())),
            )
            .collect();
        assert!(
            life_at(&s, &station, LifeLevel::Full, 0.0).crane.is_some(),
            "{station}: its market has a crane"
        );
        // Every three ticks from the sale until well after the load is set down.
        let mut carried = 0;
        for _ in 0..70 {
            carried += usize::from(crane_keeps_clear(&s, &station, &rooms));
            for _ in 0..3 {
                s.tick(PlayerInput::default());
            }
        }
        // The carry takes six seconds (60 checks): the load is seen all the way.
        assert!(
            carried >= 59,
            "{station}: carried for {carried} of 60 checks"
        );
    }
}

#[test]
fn craft_glide_in_to_their_berth_and_out_again() {
    // Undocked inside the Wharf's ring, the flagship docks with Space.
    let mut s = session_with(|w| {
        let id = w.player.ship.clone().unwrap();
        w.ships.get_mut(&id).unwrap().docked_at = None;
    });
    let id = s.flagship().unwrap().id.clone();
    let mut keys = galaxy_app::input::Keys::default();
    keys.event(galaxy_app::input::Key::Space, true, false);
    let input = keys.tick_input(false, None);
    s.tick(input);
    keys.event(galaxy_app::input::Key::Space, false, false);
    for _ in 0..3 {
        if s.flagship().unwrap().docked_at.is_some() {
            break;
        }
        s.tick(keys.tick_input(false, None));
    }
    assert_eq!(s.flagship().unwrap().docked_at, Some(WHARF), "docked");
    let moving = galaxy_app::view::berthing(s.world(), Some(s.sim().history()));
    let (station, docking, k) = moving.get(&id).cloned().expect("just docked: gliding in");
    assert_eq!(station, WHARF);
    assert!(docking && k < 0.2, "{k}");
    for _ in 0..90 {
        s.tick(PlayerInput::default());
    }
    let moving = galaxy_app::view::berthing(s.world(), Some(s.sim().history()));
    assert!(
        moving.get(&id).is_none_or(|m| m.2 >= 1.0),
        "and settled at its berth"
    );
}

#[test]
fn the_pause_card_has_the_station_life_switch() {
    let mut s = session_with(|_| {});
    s.pause();
    let rig = rig_for(&s, &UiState::default(), DESIGN);
    let (scene, buttons) = galaxy_app::frame::compose(
        &galaxy_app::view::View::new(),
        &s,
        &rig,
        &UiState::default(),
        1.0,
        0.0,
        DESIGN,
    );
    assert!(buttons.is_empty(), "nothing under the card is clickable");
    let card = scene.overlay.paused.expect("the card shows");
    assert_eq!(card.toggles, [LifeLevel::Full.label()]);
    let rects = card.toggle_rects(DESIGN);
    let body = card.rect(DESIGN);
    assert_eq!(rects.len(), 1);
    let r = rects[0];
    assert!(
        r.x >= body.x
            && r.x + r.w <= body.x + body.w
            && r.y >= body.y
            && r.y + r.h <= body.y + body.h,
        "{r:?} inside {body:?}"
    );
    assert!(
        ui::text_width(LifeLevel::Quiet.label(), 18.0) < r.w - 16.0,
        "the label fits"
    );
    assert_eq!(LifeLevel::Full.toggled(), LifeLevel::Quiet);
    assert_eq!(LifeLevel::Quiet.toggled(), LifeLevel::Full);
}
