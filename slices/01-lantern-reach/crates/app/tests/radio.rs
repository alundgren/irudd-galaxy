//! The radio in the real game (#47, 5.1), headless: discovery recorded once per save
//! through the one write path with its notice in the ship log, and the mix by distance
//! (volume, static, crossfade, tuning, off, docked). No window, GPU or audio device.

use galaxy_app::radio::{Radio, RadioFrame, Tune};
use galaxy_app::session::Session;
use galaxy_app::view::themes::Themes;
use galaxy_contract::prelude::*;
use galaxy_contract::talk::Channel;
use galaxy_core::{Save, SimState};
use galaxy_mind::radio::FOUND_FACT;

const WHARF: StationId = StationId::from_static("station.lantern_wharf");
const KETTLE: StationId = StationId::from_static("station.kettle_dock");

fn data() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../packs/core/data")
}

/// A new game with the flagship flying at `at`, still.
fn flying_at(at: Vec2) -> SimState {
    let mut state = galaxy_core::new_game(&data(), 1).unwrap();
    let id = state.world.player.ship.clone().unwrap();
    let ship = state.world.ships.get_mut(&id).unwrap();
    ship.docked_at = None;
    ship.position = at;
    ship.velocity = Vec2::ZERO;
    state
}

fn station_at(state: &SimState, s: &StationId) -> Vec2 {
    state.world.stations.get(s).unwrap().position
}

/// Open space in the Shallows, out of every channel's range.
const NOWHERE: Vec2 = Vec2::new(-1200.0, -1300.0);

fn move_to(s: &mut Session, at: Vec2) {
    let ship = s.world().player.ship.clone().unwrap();
    s.submit(
        CommandBatch::new(Cause::system("test", "move")).with(Command::SetShipMotion {
            ship,
            position: at,
            velocity: Vec2::ZERO,
            heading: 0.0,
        }),
    );
    run(s, 3);
}

fn run(s: &mut Session, ticks: usize) {
    for _ in 0..ticks {
        s.tick(PlayerInput::default());
    }
}

fn found(s: &Session, channel: &str) -> Vec<FactId> {
    s.sim()
        .history()
        .facts()
        .iter()
        .filter(|f| {
            f.kind.as_str() == FOUND_FACT
                && f.subjects
                    .other
                    .as_ref()
                    .is_some_and(|o| o.as_str() == channel)
        })
        .map(|f| f.id)
        .collect()
}

fn notices(s: &Session, text: &str) -> usize {
    s.world()
        .talk
        .on(Channel::MapNote)
        .filter(|l| l.line.text == text)
        .count()
}

#[test]
fn a_channel_is_discovered_once_per_save_with_a_notice_in_the_log() {
    let state = flying_at(NOWHERE);
    let kettle = station_at(&state, &KETTLE);
    let mut s = Session::new(state);
    run(&mut s, 5);
    assert!(found(&s, "radio.kettle_dock").is_empty(), "out of range");

    move_to(&mut s, kettle + Vec2::new(300.0, 0.0));
    let facts = found(&s, "radio.kettle_dock");
    assert_eq!(facts.len(), 1, "recorded when it comes into range");
    let fact = s.sim().history().fact(facts[0]).unwrap();
    assert_eq!(fact.subjects.station, Some(KETTLE));
    // Through the one write path, with a system cause.
    let cause = s.sim().history().cause(fact.cause).unwrap();
    assert!(matches!(&cause.cause.kind, CauseKind::System { name } if name == "radio"));
    // The ship log's notice, about the station, citing it.
    let line = s
        .world()
        .talk
        .on(Channel::MapNote)
        .find(|l| l.line.cites.contains(&fact.id))
        .expect("a notice cites the discovery");
    assert_eq!(line.line.text, "New signal: Kettle Dock radio");
    assert_eq!(line.line.place, Some(KETTLE));

    // Staying, leaving and coming back: still once.
    run(&mut s, 30);
    move_to(&mut s, NOWHERE);
    move_to(&mut s, kettle + Vec2::new(0.0, 200.0));
    assert_eq!(found(&s, "radio.kettle_dock").len(), 1);
    assert_eq!(notices(&s, "New signal: Kettle Dock radio"), 1);

    // Save, load and fly back in: once per save.
    let dir = std::env::temp_dir().join(format!("galaxy-radio-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("radio.sqlite");
    let _ = std::fs::remove_file(&path);
    Save::create(&path, s.state()).unwrap();
    let loaded = Save::open(&path).unwrap().load().unwrap();
    let mut again = Session::new(loaded);
    move_to(&mut again, NOWHERE);
    move_to(&mut again, kettle + Vec2::new(100.0, 0.0));
    assert_eq!(found(&again, "radio.kettle_dock").len(), 1);
    assert_eq!(notices(&again, "New signal: Kettle Dock radio"), 1);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_discovery_saved_before_its_notice_is_said_after_the_load() {
    let state = flying_at(NOWHERE);
    let kettle = station_at(&state, &KETTLE);
    let mut s = Session::new(state);
    let ship = s.world().player.ship.clone().unwrap();
    s.submit(
        CommandBatch::new(Cause::system("test", "move")).with(Command::SetShipMotion {
            ship,
            position: kettle,
            velocity: Vec2::ZERO,
            heading: 0.0,
        }),
    );
    // The move applies in the first tick and the discovery in the second; stop there.
    run(&mut s, 2);
    assert_eq!(found(&s, "radio.kettle_dock").len(), 1);
    assert_eq!(
        notices(&s, "New signal: Kettle Dock radio"),
        0,
        "not said yet"
    );
    let mut again = Session::new(s.state().clone());
    run(&mut again, 2);
    assert_eq!(notices(&again, "New signal: Kettle Dock radio"), 1);
    assert_eq!(found(&again, "radio.kettle_dock").len(), 1);
}

#[test]
fn docked_nothing_is_discovered() {
    let mut s = Session::new(galaxy_core::new_game(&data(), 1).unwrap());
    assert!(
        s.flagship().unwrap().docked_at.is_some(),
        "a new game starts docked"
    );
    run(&mut s, 10);
    assert!(found(&s, "radio.lantern_wharf").is_empty());
}

fn radio(s: &Session) -> Radio {
    Radio::new(s.game().radio().clone(), Themes::bundled().clone(), true)
}

fn frame(r: &mut Radio, s: &Session) -> RadioFrame {
    r.update(s.world(), s.sim().history())
}

fn gain(f: &RadioFrame, channel: &str) -> f32 {
    f.mix
        .voices
        .iter()
        .find(|v| v.key == channel)
        .map_or(0.0, |v| v.gain)
}

#[test]
fn volume_and_static_follow_the_distance_and_out_of_range_is_quiet() {
    let state = flying_at(NOWHERE);
    let kettle = station_at(&state, &KETTLE);
    let mut s = Session::new(state);
    let mut r = radio(&s);
    let quiet = frame(&mut r, &s);
    assert!(quiet.mix.voices.is_empty() && quiet.mix.static_gain == 0.0);
    assert!(quiet.ui.show && quiet.ui.channel.is_none(), "No signal");

    // Flying out from Kettle Dock, away from the Wharf: louder near, static at the edge.
    let range = s.game().radio().channel("radio.kettle_dock").unwrap().range;
    let mut last_gain = f32::MAX;
    let mut statics = Vec::new();
    for share in [0.1, 0.4, 0.6, 0.8, 0.95] {
        move_to(&mut s, kettle + Vec2::new(range * share, 0.0));
        let f = frame(&mut r, &s);
        let g = gain(&f, "radio.kettle_dock");
        assert!(g > 0.0 && g <= last_gain, "{share}: {g} after {last_gain}");
        assert_eq!(f.ui.channel.as_deref(), Some("Kettle Dock radio"));
        last_gain = g;
        statics.push(f.mix.static_gain);
    }
    assert_eq!(statics[0], 0.0, "clear near the station");
    assert_eq!(statics[2], 0.0, "clear while the signal is good");
    assert!(statics[3] > 0.0 && statics[4] > 0.0, "static at the edge");
    // Past the edge: quiet again.
    move_to(&mut s, kettle + Vec2::new(range * 1.05, 0.0));
    let f = frame(&mut r, &s);
    assert!(f.mix.voices.is_empty() && f.mix.static_gain == 0.0);
    // The placeholder loop the music library names (nothing generated here).
    move_to(&mut s, kettle);
    let f = frame(&mut r, &s);
    let galaxy_mind::TrackSource::Placeholder(name) =
        galaxy_mind::MusicLibrary::placeholders().track("radio.kettle_dock")
    else {
        panic!("a library with no cache plays a placeholder");
    };
    assert_eq!(
        f.mix.voices[0].source,
        galaxy_audio::radio::Source::Placeholder(name.into())
    );
    assert_eq!(
        r.entries("radio.kettle_dock"),
        2,
        "entries are counted (view only)"
    );
}

#[test]
fn overlapping_channels_crossfade_and_tuning_picks_one() {
    let state = flying_at(NOWHERE);
    let (wharf, kettle) = (station_at(&state, &WHARF), station_at(&state, &KETTLE));
    let mut s = Session::new(state);
    let mut r = radio(&s);
    // From nearer the Wharf to nearer Kettle Dock, through the overlap.
    let mut handed_over = false;
    let mut last = (f32::MAX, 0.0);
    for i in 3..=7 {
        let t = i as f32 / 10.0;
        move_to(&mut s, wharf + (kettle - wharf) * t);
        let f = frame(&mut r, &s);
        let (gw, gk) = (
            gain(&f, "radio.lantern_wharf"),
            gain(&f, "radio.kettle_dock"),
        );
        assert!(gw <= last.0 + 1e-6 && gk >= last.1 - 1e-6, "{t}: {gw} {gk}");
        if gw > 0.0 && gk > 0.0 {
            handed_over = true;
            assert_eq!(f.ui.others, 1);
            let stronger = if gw >= gk {
                "Lantern Wharf radio"
            } else {
                "Kettle Dock radio"
            };
            assert_eq!(f.ui.channel.as_deref(), Some(stronger), "{t}");
        }
        last = (gw, gk);
    }
    assert!(handed_over, "the two channels overlap somewhere between");

    // In the overlap, tune by hand: that channel alone plays.
    move_to(&mut s, wharf + (kettle - wharf) * 0.5);
    let f = frame(&mut r, &s);
    assert_eq!(f.mix.voices.len(), 2);
    let playing = f.ui.channel.clone().unwrap();
    assert!(r.tune(s.world(), Tune::Next));
    let f = frame(&mut r, &s);
    assert!(f.ui.tuned);
    assert_eq!(f.mix.voices.len(), 1);
    assert_ne!(f.ui.channel.as_ref(), Some(&playing), "Next moved on");
    r.tune(s.world(), Tune::Prev);
    assert_eq!(frame(&mut r, &s).ui.channel, Some(playing));
    // R: next, and past the last channel, off; then on again.
    let mut presses = 0;
    while r.is_on() {
        r.tune(s.world(), Tune::Cycle);
        presses += 1;
        assert!(presses <= 3);
    }
    let off = frame(&mut r, &s);
    assert!(off.mix.voices.is_empty() && off.mix.static_gain == 0.0);
    assert!(off.ui.show && !off.ui.on);
    assert!(r.tune(s.world(), Tune::Cycle), "R turns it back on");
    assert!(!r.tune(s.world(), Tune::Toggle), "the Off button");
    assert!(r.tune(s.world(), Tune::Toggle), "and On");
    // A hand-tuned channel out of range lets go: the strongest plays again.
    while !(frame(&mut r, &s).ui.tuned
        && frame(&mut r, &s).ui.channel.as_deref() == Some("Lantern Wharf radio"))
    {
        r.tune(s.world(), Tune::Next);
    }
    move_to(&mut s, kettle);
    let f = frame(&mut r, &s);
    assert!(!f.ui.tuned);
    assert_eq!(f.ui.channel.as_deref(), Some("Kettle Dock radio"));
}

#[test]
fn docked_the_radio_fades_out_and_hides() {
    let mut s = Session::new(galaxy_core::new_game(&data(), 1).unwrap());
    let mut r = radio(&s);
    let f = frame(&mut r, &s);
    assert!(!f.ui.show);
    assert!(f.mix.voices.is_empty() && f.mix.static_gain == 0.0);
    run(&mut s, 1);
}
