//! The cues and their parameters.
//!
//! A cue is a [`Patch`] (what it sounds like: notes, timbre, envelope, takes) and a
//! [`Playback`] (how it behaves in play: bus, variation, rate limit, thinning). Both are
//! plain numbers, so a game master could later define a motif as a parameter set and
//! the engine would render and check it the same way (see `README.md`).

/// The five cues the slice plays.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Cue {
    /// A dock-screen action went through (a trade).
    Confirm,
    /// The flagship docked: two warm notes, rising.
    Dock,
    /// The flagship left a station: the chime's two notes falling, softer.
    Undock,
    /// Ore entered the hold: a tiny soft tick, thinned out over a mining run.
    OreTick,
    /// The hold just filled while mining: three low notes, once.
    HoldFull,
}

impl Cue {
    pub const ALL: [Cue; 5] = [
        Cue::Confirm,
        Cue::Dock,
        Cue::Undock,
        Cue::OreTick,
        Cue::HoldFull,
    ];

    /// File-name and log name.
    pub fn name(self) -> &'static str {
        match self {
            Cue::Confirm => "confirm",
            Cue::Dock => "dock",
            Cue::Undock => "undock",
            Cue::OreTick => "ore_tick",
            Cue::HoldFull => "hold_full",
        }
    }

    pub fn index(self) -> usize {
        self as usize
    }

    /// The cue's parameters.
    pub fn spec(self) -> CueSpec {
        match self {
            Cue::Confirm => confirm(),
            Cue::Dock => dock(),
            Cue::Undock => undock(),
            Cue::OreTick => ore_tick(),
            Cue::HoldFull => hold_full(),
        }
    }
}

/// Which mixer track a cue plays on. UI sits below the world.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Bus {
    Ui,
    World,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CueSpec {
    pub bus: Bus,
    pub patch: Patch,
    pub play: Playback,
}

/// What a cue sounds like. Rendered offline into one or more takes.
#[derive(Clone, Debug, PartialEq)]
pub struct Patch {
    /// The root pitch in Hz; notes are semitones from it.
    pub root_hz: f32,
    pub notes: Vec<Note>,
    pub timbre: Timbre,
    pub envelope: Envelope,
    /// Takes rendered for variation (at least 1).
    pub takes: u32,
    /// Takes are spread evenly over this many cents either side of the root.
    pub take_spread_cents: f32,
    /// The peak level of every take, in dBFS. The loudest a cue gets before the tracks.
    pub peak_db: f32,
}

/// One note of a motif.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Note {
    /// Semitones from the root.
    pub semitones: f32,
    /// Seconds from the start of the cue.
    pub at: f32,
    /// Linear level, 0..=1.
    pub gain: f32,
}

/// A soft struck tone: a sine fundamental, two overtones that die away faster, an
/// optional breath of filtered noise on the onset, all through a gentle low-pass.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Timbre {
    /// `(ratio to the fundamental, level)` of the two overtones. Ratio 2 and 3 are
    /// warm and harmonic; 2.76 or 5.4 are glassy.
    pub overtones: [(f32, f32); 2],
    /// Overtones decay this much faster than the fundamental (above 1 keeps it warm).
    pub overtone_damping: f32,
    /// Level of the noise onset, 0..=1 (the "knock" of a pebble or wood).
    pub air: f32,
    /// Centre of the noise onset's band, in Hz.
    pub air_hz: f32,
    /// The low-pass on everything, in Hz. Keeps the harsh 2 to 5 kHz band quiet.
    pub cutoff_hz: f32,
}

/// A rounded envelope: a raised-cosine attack, then an exponential decay, and a short
/// cosine fade at the end so every cue ends in silence.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Envelope {
    /// Seconds to full level.
    pub attack: f32,
    /// The fundamental's decay time constant, in seconds.
    pub decay: f32,
    /// The cue's length in seconds, fade included.
    pub length: f32,
    /// The closing fade, in seconds.
    pub fade: f32,
}

/// How a cue behaves in play.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Playback {
    /// Random pitch offset per play, up to this many cents either way.
    pub cents: f32,
    /// Random level offset per play, up to this many dB either way.
    pub db: f32,
    /// A play sooner than this many seconds after the last one is dropped.
    pub min_interval: f32,
    /// Random start delay, up to this many seconds, so repeats never fall on a grid.
    pub jitter: f32,
    /// Thinning over a run of repeats, for cues that follow a held action.
    pub thin: Option<Thinning>,
}

/// A run is a series of plays with less than `gap` seconds between them. Each play in
/// a run is `step_db` quieter than the one before, down to `floor_db`, and after the
/// first `always` plays only some still sound (`keep` of them). So a long mining run
/// fades to an occasional pebble instead of a beat.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Thinning {
    pub gap: f32,
    pub step_db: f32,
    pub floor_db: f32,
    pub always: u32,
    pub keep: f32,
}

/// Root pitches: everything sits in D major so cues never clash.
const D4: f32 = 293.66;
const A3: f32 = 220.0;
const B4: f32 = 493.88;
const A5: f32 = 880.0;

fn note(semitones: f32, at: f32, gain: f32) -> Note {
    Note {
        semitones,
        at,
        gain,
    }
}

fn confirm() -> CueSpec {
    CueSpec {
        bus: Bus::Ui,
        patch: Patch {
            root_hz: B4,
            notes: vec![note(0.0, 0.0, 1.0)],
            timbre: Timbre {
                overtones: [(2.0, 0.22), (3.0, 0.06)],
                overtone_damping: 2.5,
                air: 0.15,
                air_hz: 900.0,
                cutoff_hz: 2200.0,
            },
            envelope: Envelope {
                attack: 0.004,
                decay: 0.045,
                length: 0.22,
                fade: 0.05,
            },
            takes: 4,
            take_spread_cents: 30.0,
            peak_db: -16.0,
        },
        play: Playback {
            cents: 15.0,
            db: 1.0,
            min_interval: 0.08,
            jitter: 0.0,
            thin: None,
        },
    }
}

fn dock() -> CueSpec {
    CueSpec {
        bus: Bus::World,
        patch: Patch {
            root_hz: D4,
            // D4 then A4: a rising fifth.
            notes: vec![note(0.0, 0.0, 0.9), note(7.0, 0.17, 1.0)],
            timbre: Timbre {
                overtones: [(2.0, 0.25), (3.0, 0.08)],
                overtone_damping: 3.0,
                air: 0.0,
                air_hz: 1000.0,
                cutoff_hz: 2000.0,
            },
            envelope: Envelope {
                attack: 0.008,
                decay: 0.32,
                length: 1.7,
                fade: 0.5,
            },
            takes: 1,
            take_spread_cents: 0.0,
            peak_db: -10.0,
        },
        play: Playback {
            cents: 0.0,
            db: 0.5,
            min_interval: 1.0,
            jitter: 0.0,
            thin: None,
        },
    }
}

fn undock() -> CueSpec {
    CueSpec {
        bus: Bus::World,
        patch: Patch {
            root_hz: D4,
            // A4 then D4, softer and rounder: the chime let go.
            notes: vec![note(7.0, 0.0, 0.8), note(0.0, 0.15, 1.0)],
            timbre: Timbre {
                overtones: [(2.0, 0.15), (3.0, 0.04)],
                overtone_damping: 3.0,
                air: 0.0,
                air_hz: 1000.0,
                cutoff_hz: 1600.0,
            },
            envelope: Envelope {
                attack: 0.025,
                decay: 0.24,
                length: 1.2,
                fade: 0.4,
            },
            takes: 1,
            take_spread_cents: 0.0,
            peak_db: -13.0,
        },
        play: Playback {
            cents: 0.0,
            db: 0.5,
            min_interval: 1.0,
            jitter: 0.0,
            thin: None,
        },
    }
}

fn ore_tick() -> CueSpec {
    CueSpec {
        bus: Bus::World,
        patch: Patch {
            root_hz: A5,
            notes: vec![note(0.0, 0.0, 1.0)],
            timbre: Timbre {
                // Slightly inharmonic: stone, not a bell.
                overtones: [(2.32, 0.3), (3.9, 0.08)],
                overtone_damping: 2.0,
                air: 0.35,
                air_hz: 1200.0,
                cutoff_hz: 2600.0,
            },
            envelope: Envelope {
                attack: 0.003,
                decay: 0.014,
                length: 0.09,
                fade: 0.03,
            },
            takes: 6,
            take_spread_cents: 350.0,
            peak_db: -22.0,
        },
        play: Playback {
            cents: 40.0,
            db: 2.0,
            min_interval: 0.3,
            jitter: 0.12,
            thin: Some(Thinning {
                gap: 3.0,
                step_db: 1.2,
                floor_db: -9.0,
                always: 4,
                keep: 0.6,
            }),
        },
    }
}

fn hold_full() -> CueSpec {
    CueSpec {
        bus: Bus::World,
        patch: Patch {
            root_hz: A3,
            // A3, D4, F#4: a low D major chord, rolled. Round and settled.
            notes: vec![
                note(0.0, 0.0, 0.85),
                note(5.0, 0.13, 0.9),
                note(9.0, 0.26, 1.0),
            ],
            timbre: Timbre {
                overtones: [(2.0, 0.18), (3.0, 0.05)],
                overtone_damping: 3.0,
                air: 0.0,
                air_hz: 1000.0,
                cutoff_hz: 1500.0,
            },
            envelope: Envelope {
                attack: 0.015,
                decay: 0.38,
                length: 1.9,
                fade: 0.5,
            },
            takes: 1,
            take_spread_cents: 0.0,
            peak_db: -11.0,
        },
        play: Playback {
            cents: 0.0,
            db: 0.0,
            min_interval: 2.0,
            jitter: 0.0,
            thin: None,
        },
    }
}
