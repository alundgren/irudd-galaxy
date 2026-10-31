//! The living station on a GPU, `#[ignore]`d (they need an adapter):
//!
//! - the docked frame time at Lantern Wharf with the tilt and life on, against quiet and
//!   against the flat view with no life (`--nocapture` prints the numbers);
//! - shots of every station docked, busy, written to `GALAXY_SHOT_DIR` when it's set.
//!
//! `cargo xtask cargo test --release -p galaxy-app --test living_station_gpu -- --ignored --nocapture`

use std::path::PathBuf;
use std::time::Instant;

use galaxy_app::session::Session;
use galaxy_app::ui::UiState;
use galaxy_app::view::life::LifeLevel;
use galaxy_app::view::{CameraRig, TILT, View};
use galaxy_contract::parts::PartState;
use galaxy_contract::prelude::*;
use render::Renderer;
use render::glam::Vec2 as G2;

const ICE: WareId = WareId::from_static("ware.ice");

fn packs() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../packs")
}

/// Docked at `station`, busy: many pulses, news, a part going up, ice just sold.
fn busy(station: &StationId) -> Session {
    let mut state = galaxy_core::new_game(&galaxy_app::game::core_data(&packs()), 1).unwrap();
    let w = &mut state.world;
    let st = w.stations.get(station).unwrap().clone();
    let id = w.player.ship.clone().unwrap();
    let ship = w.ships.get_mut(&id).unwrap();
    ship.docked_at = Some(station.clone());
    ship.position = st.position;
    ship.region = st.region.clone();
    ship.hold.cargo.insert(ICE, 10);
    w.economy
        .stations
        .entry(station.clone())
        .or_default()
        .pulses_milli = 24_000;
    let rumor: RumorId = "rumor.bench_news".parse().unwrap();
    w.rumors.insert(
        rumor.clone(),
        Rumor {
            id: rumor,
            claim: "There's ice in the Shoals again.".into(),
            is_true: true,
            planted_by: None,
            heard_at: [station.clone()].into_iter().collect(),
            heard_by_player: false,
            source: None,
        },
    );
    if let Some(p) = w
        .station_parts
        .values()
        .find(|p| p.station == *station && p.room == galaxy_contract::parts::RoomKind::Dockmaster)
        .map(|p| p.id.clone())
    {
        w.station_parts.get_mut(&p).unwrap().state = PartState::Building;
    }
    let mut s = Session::new(state);
    for _ in 0..3 {
        s.tick(PlayerInput::default());
    }
    if s.world()
        .markets
        .get(station)
        .is_some_and(|m| m.lines.contains_key(&ICE))
    {
        s.request_trade(TradeRequest {
            ship: s.flagship().unwrap().id.clone(),
            station: station.clone(),
            ware: ICE,
            side: TradeSide::Sell,
            quantity: 10,
            limit: None,
        });
    }
    for _ in 0..40 {
        s.tick(PlayerInput::default());
    }
    s
}

fn rig(s: &Session, viewport: G2) -> CameraRig {
    let goal = galaxy_app::frame::camera_goal(s, &UiState::default(), 1.0, viewport).unwrap();
    let mut rig = CameraRig::new(G2::ZERO);
    rig.settle(&goal, s.world());
    rig
}

#[test]
#[ignore = "needs a GPU adapter"]
fn docked_frame_time_with_the_tilt_and_life_on() {
    let wharf = StationId::from_static("station.lantern_wharf");
    let s = busy(&wharf);
    let mut r = Renderer::new_headless(1280, 720).unwrap();
    let viewport = r.logical_size();
    let ui_state = UiState::default();
    const FRAMES: u32 = 600;
    for (label, level, flat) in [
        ("tilt + life (full)", LifeLevel::Full, false),
        ("tilt + life (quiet)", LifeLevel::Quiet, false),
        ("flat, no life", LifeLevel::Full, true),
    ] {
        let mut view = View::new();
        view.life = level;
        view.sync(&mut r, s.world());
        let mut rig = rig(&s, viewport);
        if flat {
            rig.tilt = TILT;
        }
        let frame = |i: u32| {
            galaxy_app::frame::compose(&view, &s, &rig, &ui_state, 1.0, i as f32 / 60.0, viewport).0
        };
        // Warm up (pipelines, glyphs), waiting for the GPU at the end.
        for i in 0..30 {
            r.render(&frame(i)).unwrap();
        }
        r.render_to_rgba(&frame(30)).unwrap();
        // Then time the frames; the last is read back, so all the GPU's work counts.
        let mut compose = 0.0f64;
        let mut meshes = 0;
        let t0 = Instant::now();
        for i in 0..FRAMES {
            let t = Instant::now();
            let scene = frame(i);
            compose += t.elapsed().as_secs_f64();
            meshes = scene.meshes.len();
            if i + 1 == FRAMES {
                r.render_to_rgba(&scene).unwrap();
            } else {
                r.render(&scene).unwrap();
            }
        }
        let total = t0.elapsed().as_secs_f64();
        let per = total / f64::from(FRAMES) * 1000.0;
        println!(
            "{label}: {per:.2} ms a frame ({:.0} fps), compose {:.3} ms, {meshes} meshes",
            1000.0 / per,
            compose / f64::from(FRAMES) * 1000.0,
        );
        assert!(per < 33.0, "{label}: {per:.2} ms a frame");
    }
}

#[test]
#[ignore = "needs a GPU adapter"]
fn shots_of_every_station_docked_and_busy() {
    let Some(dir) = std::env::var_os("GALAXY_SHOT_DIR").map(PathBuf::from) else {
        return;
    };
    std::fs::create_dir_all(&dir).unwrap();
    let stations: Vec<StationId> = galaxy_core::new_game(&galaxy_app::game::core_data(&packs()), 1)
        .unwrap()
        .world
        .stations
        .keys()
        .cloned()
        .collect();
    let mut r = Renderer::new_headless(1280, 720).unwrap();
    let viewport = r.logical_size();
    for station in stations {
        let s = busy(&station);
        for (tag, level) in [("full", LifeLevel::Full), ("quiet", LifeLevel::Quiet)] {
            let mut view = View::new();
            view.life = level;
            view.sync(&mut r, s.world());
            let rig = rig(&s, viewport);
            let (scene, _) = galaxy_app::frame::compose(
                &view,
                &s,
                &rig,
                &UiState::default(),
                1.0,
                2.0,
                viewport,
            );
            let img = r.render_to_rgba(&scene).unwrap();
            let name = station.as_str().trim_start_matches("station.");
            img.save_png(dir.join(format!("{name}-{tag}.png"))).unwrap();
        }
    }
}
