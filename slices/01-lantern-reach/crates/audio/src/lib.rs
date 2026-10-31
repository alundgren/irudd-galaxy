//! `galaxy-audio`: the game's sound, behind a small wrapper around kira.
//!
//! Five short cues ([`Cue`]), each a parameter set ([`cue::CueSpec`]) rendered offline
//! with fundsp into kira static sounds once, at startup ([`synth`]). Nothing is
//! synthesised on the audio thread. [`Director`] applies the anti-fatigue rules (rate
//! limits, no take twice in a row, pitch and level variation, thinning long runs) and
//! [`Audio`] plays its choices on a UI or a world track under the master track, with
//! the volumes from the player's [`Settings`] file. The rules are in `README.md`.
//!
//! The radio ([`radio`]) plays on a third track: each channel heard loops its music at
//! its signal's gain, with static mixed in at the edges ([`Audio::radio`]).
//!
//! Sound is never required. If no device opens (no sound card, a headless machine, a
//! busy driver) [`Audio::open`] returns a silent [`Audio`] and the game carries on.

pub mod analysis;
pub mod cue;
pub mod director;
pub mod radio;
pub mod settings;
pub mod synth;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use kira::sound::FromFileError;
use kira::sound::static_sound::{StaticSoundData, StaticSoundHandle, StaticSoundSettings};
use kira::sound::streaming::{StreamingSoundData, StreamingSoundHandle};
use kira::track::{TrackBuilder, TrackHandle};
use kira::{
    AudioManager, AudioManagerSettings, Decibels, DefaultBackend, Easing, Frame, PlaybackRate,
    StartTime, Tween,
};

pub use analysis::Metrics;
pub use cue::{Bus, Cue};
pub use director::{Director, Play};
pub use settings::Settings;

/// The open device: the manager, its three tracks, every cue's takes and the radio's
/// sounds.
struct Output {
    manager: AudioManager<DefaultBackend>,
    ui: TrackHandle,
    world: TrackHandle,
    radio: TrackHandle,
    /// `takes[cue.index()][take]`.
    takes: Vec<Vec<StaticSoundData>>,
    /// The placeholder loops by name, and the static, ready to loop.
    loops: BTreeMap<&'static str, StaticSoundData>,
    hiss: StaticSoundData,
    /// The channels playing now, by key, and the static.
    voices: BTreeMap<String, RadioVoice>,
    static_voice: Option<(StaticSoundHandle, f32)>,
}

/// One radio channel playing.
struct RadioVoice {
    source: radio::Source,
    handle: Playing,
    /// The gain last asked for.
    gain: f32,
}

enum Playing {
    Loop(StaticSoundHandle),
    Track(StreamingSoundHandle<FromFileError>),
}

impl Playing {
    fn set_gain(&mut self, gain: f32, secs: f32) {
        let db = Decibels(settings::volume_db(gain));
        match self {
            Playing::Loop(h) => h.set_volume(db, tween(secs)),
            Playing::Track(h) => h.set_volume(db, tween(secs)),
        }
    }

    fn stop(&mut self, secs: f32) {
        match self {
            Playing::Loop(h) => h.stop(tween(secs)),
            Playing::Track(h) => h.stop(tween(secs)),
        }
    }
}

/// Where changes to the settings go.
enum Store {
    /// Nowhere: no settings file.
    Nowhere,
    /// The file, written in the background.
    File(settings::Writer),
    /// The file didn't load at startup, so it's never written. Changes last the session.
    Protected { path: PathBuf, told: bool },
}

/// The game's audio. Silent when no device could be opened.
pub struct Audio {
    out: Option<Output>,
    /// Why there is no device, when there isn't one.
    problem: Option<String>,
    director: Director,
    settings: Settings,
    /// Where changes to the settings are kept.
    store: Store,
    /// Something about the settings file worth telling the player.
    note: Option<String>,
    paused: bool,
    clock: Instant,
}

impl Audio {
    /// Opens the default output device and renders the cues. `settings_file` is read
    /// (and written with the defaults if missing); `None` uses the defaults. Never
    /// fails: without a device the game is silent and [`Audio::problem`] says why.
    pub fn open(settings_file: Option<PathBuf>) -> Self {
        let mut audio = Self::without_device(settings_file);
        match Output::open(&audio.settings) {
            Ok(out) => audio.out = Some(out),
            Err(e) => audio.problem = Some(e),
        }
        audio
    }

    /// No device at all; every call is a no-op. For tests and `galaxy shot`.
    pub fn silent() -> Self {
        Self {
            out: None,
            problem: None,
            director: Director::new(),
            settings: Settings::default(),
            store: Store::Nowhere,
            note: None,
            paused: false,
            clock: Instant::now(),
        }
    }

    /// The settings from `settings_file`, read as [`Audio::open`] reads them, but no
    /// device.
    pub fn without_device(settings_file: Option<PathBuf>) -> Self {
        let mut audio = Self::silent();
        if let Some(path) = settings_file {
            let loaded = Settings::load_or_create(&path);
            audio.settings = loaded.settings;
            audio.note = loaded.note;
            audio.store = if loaded.writable {
                Store::File(settings::Writer::start(path))
            } else {
                Store::Protected { path, told: false }
            };
        }
        audio
    }

    /// True when an output device is open.
    pub fn has_device(&self) -> bool {
        self.out.is_some()
    }

    /// Why the device didn't open, if it didn't.
    pub fn problem(&self) -> Option<&str> {
        self.problem.as_deref()
    }

    /// A problem reading or writing the settings file, if there was one.
    pub fn settings_note(&self) -> Option<String> {
        match &self.store {
            Store::File(w) => w.last_error(),
            _ => None,
        }
        .or_else(|| self.note.clone())
    }

    /// Waits for the settings writes queued so far. They finish on their own; this is
    /// for tests and the end of play.
    pub fn finish_writes(&mut self) {
        if let Store::File(w) = &mut self.store {
            w.finish();
        }
    }

    pub fn settings(&self) -> Settings {
        self.settings
    }

    /// One line for the end-of-play summary.
    pub fn status(&self) -> String {
        match (&self.out, &self.problem) {
            (Some(_), _) if self.settings.muted => "muted".to_owned(),
            (Some(_), _) => format!("on (master {:.2})", self.settings.master),
            (None, Some(p)) => format!("no device: {p}"),
            (None, None) => "silent".to_owned(),
        }
    }

    /// Asks for `cue`, seeded by the event behind it (a fact ID, a tick). The director
    /// may decline (rate limit, thinning); nothing world-side plays while paused.
    pub fn play(&mut self, cue: Cue, seed: u64) {
        let now = self.clock.elapsed().as_secs_f64();
        if self.paused && cue.spec().bus == Bus::World {
            return;
        }
        let Some(play) = self.director.decide(cue, seed, now) else {
            return;
        };
        let Some(out) = &mut self.out else { return };
        let Some(data) = out.takes[cue.index()].get(play.take) else {
            return;
        };
        let data = data
            .volume(Decibels(play.gain_db))
            .playback_rate(PlaybackRate(play.rate))
            .start_time(StartTime::Delayed(Duration::from_secs_f32(play.delay)));
        let track = match play.bus {
            Bus::Ui => &mut out.ui,
            Bus::World => &mut out.world,
        };
        // A full sound queue just drops the cue.
        let _ = track.play(data);
    }

    /// The PAUSED card: the world track fades out (and what's ringing with it), and
    /// comes back when the card closes. The UI track stays.
    pub fn set_paused(&mut self, paused: bool) {
        if self.paused == paused {
            return;
        }
        self.paused = paused;
        let (world, radio) = (self.settings.world, self.settings.radio);
        if let Some(out) = &mut self.out {
            let level = |v: f32| {
                if paused {
                    (Decibels::SILENCE, 0.3)
                } else {
                    (Decibels(settings::volume_db(v)), 0.6)
                }
            };
            let (db, secs) = level(world);
            out.world.set_volume(db, tween(secs));
            let (db, secs) = level(radio);
            out.radio.set_volume(db, tween(secs));
        }
    }

    /// The radio's Off and On (the HUD's button, or R): kept in the settings file like
    /// the mute. Off silences the radio at once.
    pub fn set_radio_on(&mut self, on: bool) {
        if self.settings.radio_on == on {
            return;
        }
        self.settings.radio_on = on;
        if !on {
            self.radio(&radio::Mix::default());
        }
        self.keep("radio switch");
    }

    /// Plays the radio as `mix` says: a channel new to the mix starts its loop (or its
    /// generated track) from silence and fades up, one already playing glides to its
    /// new gain, and one gone from the mix fades out and stops. The same for the
    /// static. Call it every frame; small changes are skipped. Silent while the radio
    /// is off.
    pub fn radio(&mut self, mix: &radio::Mix) {
        let Some(out) = &mut self.out else { return };
        let off = radio::Mix::default();
        let mix = if self.settings.radio_on { mix } else { &off };
        out.radio_mix(mix);
    }

    /// N: mutes or unmutes everything and keeps the choice in the settings file,
    /// written in the background. A file that didn't load at startup is never written:
    /// the choice lasts the session, and that's said once. Returns true when now muted.
    pub fn toggle_mute(&mut self) -> bool {
        self.settings.muted = !self.settings.muted;
        let s = self.settings;
        if let Some(out) = &mut self.out {
            out.manager
                .main_track()
                .set_volume(Decibels(master_db(&s)), tween(0.15));
        }
        self.keep("mute");
        s.muted
    }

    /// Writes the settings in the background, unless the file didn't load at startup:
    /// then the change lasts the session, and that's said once.
    fn keep(&mut self, what: &str) {
        let s = self.settings;
        match &mut self.store {
            Store::Nowhere => {}
            Store::File(w) => w.write(s),
            Store::Protected { path, told } => {
                if !*told {
                    *told = true;
                    let line = format!(
                        "{}: didn't load at startup, so it isn't overwritten; \
                         the {what} holds for this session only",
                        path.display()
                    );
                    eprintln!("sound: {line}");
                    self.note = Some(line);
                }
            }
        }
    }
}

impl Output {
    fn open(s: &Settings) -> Result<Self, String> {
        let mut manager = AudioManager::<DefaultBackend>::new(AudioManagerSettings::default())
            .map_err(|e| e.to_string())?;
        manager
            .main_track()
            .set_volume(Decibels(master_db(s)), Tween::default());
        let mut track = |v: f32| {
            manager
                .add_sub_track(TrackBuilder::new().volume(Decibels(settings::volume_db(v))))
                .map_err(|e| e.to_string())
        };
        let ui = track(s.ui)?;
        let world = track(s.world)?;
        let radio = track(s.radio)?;
        let takes = Cue::ALL
            .iter()
            .map(|c| {
                synth::render(&c.spec().patch)
                    .iter()
                    .map(|t| sound_data(t))
                    .collect()
            })
            .collect();
        let looped = |samples: &[f32]| sound_data(samples).loop_region(..);
        let loops = radio::LOOPS
            .iter()
            .filter_map(|&name| radio::render_loop(name).map(|s| (name, looped(&s))))
            .collect();
        Ok(Self {
            manager,
            ui,
            world,
            radio,
            takes,
            loops,
            hiss: looped(&radio::render_static()),
            voices: BTreeMap::new(),
            static_voice: None,
        })
    }

    /// See [`Audio::radio`].
    fn radio_mix(&mut self, mix: &radio::Mix) {
        const AUDIBLE: f32 = 0.002;
        const STEP: f32 = 0.01;
        let wanted: BTreeMap<&str, &radio::Voice> = mix
            .voices
            .iter()
            .filter(|v| v.gain > AUDIBLE)
            .map(|v| (v.key.as_str(), v))
            .collect();
        // Gone from the mix, or now playing something else: fade out and stop.
        let gone: Vec<String> = self
            .voices
            .iter()
            .filter(|(k, v)| wanted.get(k.as_str()).is_none_or(|w| w.source != v.source))
            .map(|(k, _)| k.clone())
            .collect();
        for k in gone {
            if let Some(mut v) = self.voices.remove(&k) {
                v.handle.stop(0.8);
            }
        }
        for (key, want) in wanted {
            if let Some(v) = self.voices.get_mut(key) {
                if (v.gain - want.gain).abs() > STEP {
                    v.gain = want.gain;
                    v.handle.set_gain(want.gain, 0.25);
                }
                continue;
            }
            if let Some(mut handle) = self.start(&want.source) {
                handle.set_gain(want.gain, 0.6);
                self.voices.insert(
                    key.to_owned(),
                    RadioVoice {
                        source: want.source.clone(),
                        handle,
                        gain: want.gain,
                    },
                );
            }
        }
        let hiss = mix.static_gain;
        match &mut self.static_voice {
            Some((h, _)) if hiss <= AUDIBLE => {
                h.stop(tween(0.5));
                self.static_voice = None;
            }
            Some((h, g)) => {
                if (*g - hiss).abs() > STEP {
                    *g = hiss;
                    h.set_volume(Decibels(settings::volume_db(hiss)), tween(0.25));
                }
            }
            None if hiss > AUDIBLE => {
                let data = self.hiss.volume(Decibels::SILENCE);
                if let Ok(mut h) = self.radio.play(data) {
                    h.set_volume(Decibels(settings::volume_db(hiss)), tween(0.4));
                    self.static_voice = Some((h, hiss));
                }
            }
            None => {}
        }
    }

    /// Starts `source` looping from silence on the radio track. A generated track that
    /// won't open or decode plays the calm placeholder instead.
    fn start(&mut self, source: &radio::Source) -> Option<Playing> {
        if let radio::Source::File(path) = source {
            if let Ok(data) = StreamingSoundData::from_file(path) {
                let data = data.loop_region(..).volume(Decibels::SILENCE);
                if let Ok(h) = self.radio.play(data) {
                    return Some(Playing::Track(h));
                }
            }
            eprintln!(
                "sound: {} won't play; the radio plays its placeholder",
                path.display()
            );
        }
        let name = match source {
            radio::Source::Placeholder(n) => n.as_str(),
            radio::Source::File(_) => radio::CALM,
        };
        let data = self
            .loops
            .get(name)
            .or_else(|| self.loops.get(radio::CALM))?;
        let data = data.volume(Decibels::SILENCE);
        self.radio.play(data).ok().map(Playing::Loop)
    }
}

fn master_db(s: &Settings) -> f32 {
    if s.muted {
        -60.0
    } else {
        settings::volume_db(s.master)
    }
}

fn tween(secs: f32) -> Tween {
    Tween {
        duration: Duration::from_secs_f32(secs),
        easing: Easing::Linear,
        ..Tween::default()
    }
}

fn sound_data(samples: &[f32]) -> StaticSoundData {
    StaticSoundData {
        sample_rate: synth::SAMPLE_RATE,
        frames: samples
            .iter()
            .map(|&s| Frame::from_mono(s))
            .collect::<Arc<[Frame]>>(),
        settings: StaticSoundSettings::default(),
        slice: None,
    }
}

/// The design limits a cue's every take is checked against.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Limits {
    /// Peak plus the most the per-play variation adds, in dBFS.
    pub max_peak_db: f32,
    pub max_rms_db: f32,
    pub max_duration: f32,
    pub max_centroid_hz: f32,
    pub max_harsh_share: f32,
    pub max_dc: f32,
    pub max_edge: f32,
    /// Seconds to half the peak, at least: no hard transients.
    pub min_rise: f32,
}

impl Limits {
    /// Frequent cues (a tick per unit mined, a confirm per trade) are held to tighter
    /// loudness and brightness limits than the occasional chimes.
    pub fn of(cue: Cue) -> Self {
        let frequent = matches!(cue, Cue::OreTick | Cue::Confirm);
        Self {
            // At least 3 dB of headroom, after variation.
            max_peak_db: -3.0,
            max_rms_db: if frequent { -28.0 } else { -20.0 },
            max_duration: match cue {
                Cue::OreTick => 0.15,
                Cue::Confirm => 0.3,
                _ => 2.0,
            },
            max_centroid_hz: if frequent { 1800.0 } else { 1200.0 },
            max_harsh_share: if frequent { 0.05 } else { 0.02 },
            max_dc: 1e-4,
            max_edge: 1e-3,
            min_rise: 0.001,
        }
    }

    /// What's wrong with a take's measurements, if anything.
    pub fn check(&self, cue: Cue, m: &Metrics) -> Vec<String> {
        let mut bad = Vec::new();
        let peak = m.peak_db + cue.spec().play.db;
        if peak > self.max_peak_db {
            bad.push(format!("peak {peak:.1} dBFS > {}", self.max_peak_db));
        }
        if m.rms_db > self.max_rms_db {
            bad.push(format!("rms {:.1} dBFS > {}", m.rms_db, self.max_rms_db));
        }
        if m.duration > self.max_duration {
            bad.push(format!("{:.2} s > {} s", m.duration, self.max_duration));
        }
        if m.centroid_hz > self.max_centroid_hz {
            bad.push(format!(
                "centroid {:.0} Hz > {}",
                m.centroid_hz, self.max_centroid_hz
            ));
        }
        if m.harsh_share > self.max_harsh_share {
            bad.push(format!(
                "2-5 kHz share {:.3} > {}",
                m.harsh_share, self.max_harsh_share
            ));
        }
        if m.dc.abs() > self.max_dc {
            bad.push(format!("dc {:.5} > {}", m.dc, self.max_dc));
        }
        if m.edge > self.max_edge {
            bad.push(format!("edge {:.4} > {} (a click)", m.edge, self.max_edge));
        }
        if m.rise < self.min_rise {
            bad.push(format!(
                "reaches half its peak in {:.1} ms < {} ms",
                m.rise * 1000.0,
                self.min_rise * 1000.0
            ));
        }
        bad
    }
}

/// One rendered take, measured.
#[derive(Clone, Debug)]
pub struct TakeReport {
    pub cue: Cue,
    pub take: u32,
    pub file: Option<PathBuf>,
    pub metrics: Metrics,
    pub problems: Vec<String>,
}

/// Renders every take of every cue, measures it against [`Limits`], and writes it to
/// `<dir>/<cue>_<take>.wav` when `dir` is given (`galaxy audio render`).
pub fn render_all(dir: Option<&Path>) -> std::io::Result<Vec<TakeReport>> {
    if let Some(d) = dir {
        std::fs::create_dir_all(d)?;
    }
    let mut reports = Vec::new();
    for cue in Cue::ALL {
        let limits = Limits::of(cue);
        for (take, samples) in synth::render(&cue.spec().patch).iter().enumerate() {
            let metrics = analysis::measure(samples, synth::SAMPLE_RATE);
            let file = match dir {
                Some(d) => {
                    let p = d.join(format!("{}_{take}.wav", cue.name()));
                    analysis::write_wav(&p, samples, synth::SAMPLE_RATE)?;
                    Some(p)
                }
                None => None,
            };
            reports.push(TakeReport {
                cue,
                take: take as u32,
                file,
                problems: limits.check(cue, &metrics),
                metrics,
            });
        }
    }
    Ok(reports)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn silent_audio_has_no_device_and_ignores_cues() {
        let mut a = Audio::silent();
        assert!(!a.has_device());
        assert!(a.problem().is_none());
        a.play(Cue::Dock, 1);
        a.set_paused(true);
        a.play(Cue::OreTick, 2);
        assert!(a.toggle_mute());
        assert!(!a.toggle_mute());
    }

    #[test]
    fn mute_is_written_in_the_background() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(settings::FILE);
        let mut a = Audio::without_device(Some(path.clone()));
        assert!(a.toggle_mute());
        a.finish_writes();
        assert!(a.settings_note().is_none(), "{:?}", a.settings_note());
        assert!(Settings::load_or_create(&path).settings.muted);
    }

    #[test]
    fn a_settings_file_that_didnt_load_is_never_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(settings::FILE);
        let broken = "# mine\nmaster = \"loud\"\n";
        std::fs::write(&path, broken).unwrap();
        let mut a = Audio::without_device(Some(path.clone()));
        let startup = a.settings_note().unwrap();
        assert!(a.toggle_mute(), "the mute still applies, in memory");
        let told = a.settings_note().unwrap();
        assert_ne!(told, startup, "the first change says it isn't kept");
        assert!(!a.toggle_mute());
        assert!(a.toggle_mute());
        assert!(a.settings().muted);
        assert_eq!(a.settings_note().unwrap(), told, "said once");
        a.finish_writes();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), broken);
    }
}
