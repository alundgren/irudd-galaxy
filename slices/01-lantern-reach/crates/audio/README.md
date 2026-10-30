# galaxy-audio

The game's sound: five short cues, synthesised in code, played through kira. Earlier
prototypes had a mining-beam loop and an engine loop, and both were rejected as
annoying. They were continuous tones tied to held keys, so they told the player
nothing new after the first second and there was no way to get away from them. These
rules are here so that doesn't happen again (background: `research/assets-and-audio.html`).

## Rules

1. **No loops tied to held keys.** Nothing sounds because a key is down. Cues come
   from what the sim committed (an event) or from a dock action that went through.
2. **A sound only when it tells the player something.** Docked, undocked, ore in the
   hold, the hold is full, the trade went through. Flying makes no sound, and neither
   does a mining beam.
3. **Variation on anything that repeats.** Frequent cues have several takes at
   different pitches, and the same take never plays twice in a row. Each play also
   gets a small pitch and level offset and, for ore ticks, a random start delay so
   repeats never land on a beat. The variation is seeded by the event's fact ID, so it
   is reproducible.
4. **Rate limits and thinning.** Each cue has a minimum gap between plays. Ore ticks
   thin out over a mining run: the first few sound at full level, each later tick is
   a little quieter, down to a floor, and after the onset only some of them sound. The
   result is an occasional pebble, not a beat. A pause in mining starts a fresh run.
5. **Soft spectrum.** Sine fundamentals with a few overtones that die away faster, a
   gentle low-pass, rounded attacks and almost nothing in the harsh 2 to 5 kHz band.
6. **A loudness budget.** UI sits under the world, frequent cues sit under the
   occasional ones, and every take is measured before it ships (see below).
7. **Silence is fine.** If you're not sure a cue is right, leave it out.

## Cues

| Cue | When | Bus | Sound |
|---|---|---|---|
| `confirm` | A dock trade went through | UI | A soft wooden "tok" at B4. 4 takes. |
| `dock` | The flagship docked | world | D4 then A4, a warm rising fifth |
| `undock` | The flagship left a station | world | A4 then D4, softer and rounder: the chime let go |
| `ore_tick` | Ore entered the hold (once per tick at most) | world | A tiny stone tick around A5, thinned over a run. 6 takes. |
| `hold_full` | The hold filled while mining (replaces that tick) | world | A3, D4, F#4 rolled: a low, settled chord, once |

Everything is in D major, so cues never clash. Each cue is a `CueSpec` in
`src/cue.rs`: a `Patch` (root pitch, notes, timbre, envelope, takes, peak level) and a
`Playback` (bus, per-play variation, rate limit, jitter, thinning). These are plain
numbers on purpose. A game-master motif can later be the same parameter set, rendered
and checked the same way.

## How it works

- `synth`: renders a patch offline with fundsp into mono takes at 48 kHz. Each note
  is a sine fundamental plus two overtones, each under its own envelope (a
  raised-cosine attack, then an exponential decay), with an optional burst of
  band-passed noise on the onset. That goes through a low-pass and a DC blocker. The
  mix gets a closing cosine fade and is scaled to the patch's peak level. This runs
  once at startup and takes a few milliseconds. The audio thread only plays the
  finished samples.
- `director`: the rules above, as a pure function of cue, seed and clock. It is
  tested headless.
- `Audio`: a kira manager with UI and world sub-tracks under the main track. While the
  game is paused the world track fades to silence; the UI track keeps playing. If no
  device opens, the game is silent and carries on.
- `settings`: `sound.toml` in the save folder, written with the defaults the first
  time:

  ```toml
  # The Lantern Reach: sound. Volumes go from 0.0 (off) to 1.0 (full).
  # N in the game mutes and unmutes.
  master = 0.50
  ui = 0.60
  world = 0.80
  muted = false
  ```

  A file that can't be read leaves the defaults in place and is never overwritten that
  session: N still mutes, in memory, and says once that it isn't kept. Changes are
  written on a background thread, never on the frame.

## Checking without ears

```sh
cargo test -p galaxy-audio                               # renders and measures every take
cargo run -p galaxy-app -- audio render --out /tmp/cues  # writes the WAVs and prints the numbers
```

Every take is checked against `Limits` (in `src/lib.rs`):

| | Frequent (`ore_tick`, `confirm`) | Occasional (chimes) |
|---|---|---|
| Peak, after per-play variation | at most -3 dBFS | at most -3 dBFS |
| RMS over the cue | at most -28 dBFS | at most -20 dBFS |
| Length | 0.15 s / 0.3 s | 2 s |
| Spectral centroid | at most 1800 Hz | at most 1200 Hz |
| Energy share in 2 to 5 kHz | at most 5% | at most 2% |
| DC offset | at most 0.0001 | at most 0.0001 |
| First and last sample | within 0.001 of zero (no click) | same |
| Time to half the peak | at least 1 ms (no hard transient) | same |

`galaxy audio render` exits 5 if any take falls outside its limits. The WAVs aren't
committed. Numbers can't judge taste: the real acceptance test is a long play session
on headphones and laptop speakers, with a written list of anything that grates.
