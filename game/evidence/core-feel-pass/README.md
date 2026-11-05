# Core feel pass: sky depth, sound that follows the world, UI sounds

Build `0.8.0-2281e9bc927a-dev`, all runs at 1280x720.

## Sky depth

- `flight-before-after-vs-reference.png`: flight before and after, beside
  `slice-overworld.png`. The after shot adds layered ink hatching and dust, a dusky
  planet limb in the bottom-left corner, rose and teal nebula washes, and a far station
  in the haze. The plum sky and the warm stars are unchanged (direction doc 1.2).
- `station-before-after-vs-reference.png`: the same near Lantern Dock, beside
  `slice-mining.png`. The planet's limb is darker than any hull, so the stations still
  stand clear of it.
- `flight-after-1280x720.png`, `station-after-1280x720.png`: the after frames at full size.
- `pan-sheet.png` (four moments of a northward cruise, `galaxy-gm motion sheet`) and
  `pan.gif` (the same four seconds, palette-reduced): the near gold dust and specks
  slide past, the hatching and far station creep, and the planet's limb barely moves.
- The world's hash is unchanged by the sky and the audio. Running the flight inputs
  for 900 ticks with HEAD's module and HEAD's module plus only the new `style.toml`,
  `audio.toml` and sounds gives the same hash (`0aa746d2…`). Shots at ticks 60, 190
  and 265 give the same hashes before and after. The manifest's `contact_speed` adds
  `engine.contact` and `bump` facts, which change the hash of runs with a bump (see the
  engine's `flight-and-collision.md`). `sound.luau` no longer records a click or panel
  cue for screen clicks. Neither changes any game rule.

## Sound that follows the world (`galaxy-gm audio mix` and `audio check`)

| What | Measured |
|---|---|
| The drive's hum, stopped / cruising / boosted | effects stem -52.6 / -32.2 / -30.8 dBFS; hum fundamental 88 / 136 / 160 Hz (pitch 0.8 to 1.45) |
| `audio/drive-check.toml` (cruise) | effects bus -33.9 and -35.7 dBFS in seconds 3 and 4 |
| `audio/drive-rest-check.toml` (at rest) | -47.9 and -47.4 dBFS |
| `audio/station-pass-check.toml`: Lantern Dock's loop | -36.7 dBFS at its rim (10 units), -43.1 once 73 units out; the loop alone is -55 at 30 units and silent past 44 |
| `drive_hum.wav` loop seam | level jump 0.32 dB, no clicks |
| Flight mix (`flight-check.toml`) | music -26.4 LUFS, ambience -37.8, effects -30.6, peak -10.3 dBFS, no limited samples |
| Every existing check (flight, mining, trade, fit, docked ×3) | passes |
| Bumps (`tests/bump.toml`) | one bump flying into rock1, still one while pressing on it, a second after backing off and flying in again |

## UI sounds (`[ui]`), measured on played bot sessions

Each session was played with `galaxy play --inputs …` and its recording mixed with
`galaxy-gm audio mix` (`recording = …`). Each click, pickup, drop and pointer move in the
inputs was paired with the UI cue the mix heard within 100 ms of it.

| Session | Actions with a cue within 100 ms | Latency |
|---|---|---|
| `tests/trade-play.inputs` | clicks 52/52 | 0 ms (on the frame) |
| `tests/fit-play.inputs` | clicks 28/28, pickups 5/5, drops 5/5 (2 accepted, 3 refused: deny) | 0 ms |
| `hover-sweep.inputs` | clicks 3/3 (press, deny on a resting Buy, press); 30 hover moves: 15 hover ticks played, 16 thinned by `hover_gap` | thinned moves have a hover sound at most 50 ms away |

The hover sweep moves the pointer once a tick across the station key, which is faster
than `hover_gap` (0.08 s), so one hover in three sounds. Then it moves once every ten
ticks, and every hover sounds. Room panels and the log open and close with the panel
notes; the plate, the status chip, the radio and the hint strip are silent (`silent` in
`ui_sounds`) as they come and go together on docking. A talk bubble's blip comes from
`[ui]` only, once per talk starting.

## Frame time (`galaxy play`, 1280x720)

| Session | fps | frame interval p95 / p99 | draw p95 / p99 |
|---|---|---|---|
| trade-play (4150 ticks) | 59.8 | 18.0 / 21.0 ms | 16.3 / 17.7 ms |
| fit-play (2340 ticks) | 59.6 | 17.7 / 27.1 ms | 15.9 / 16.7 ms |
| hover sweep (700 ticks) | 60.1 | 17.9 / 19.2 ms | 16.1 / 17.1 ms |

Before the feel pass (HEAD's module), the same trade-play run measured 59.6 fps, a frame
interval p95 of 17.8 ms and a p99 of 22.5 ms, and draw p95 / p99 of 16.1 / 17.0 ms. The
sky layers cost about 0.2 ms of draw at p95, and the frame stays at 60 fps.
