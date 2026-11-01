# Saves and replay

How a world is saved and loaded, how a session is recorded, and how a recording is run
again. The code is `crates/sim` (`save.rs`, `recording.rs`, `scenario.rs`) and the `sim`
commands in `crates/galaxy`.

## Trying it

From `engine/`, with the engine installed (`modules-and-install.md`):

```sh
galaxy sim run --modules tests/modules --seed 7 --ticks 450 \
    --inputs tests/sessions/stroll.toml --save end.db --record session.json
galaxy sim replay session.json          # identical: true, the same hash
galaxy sim run --modules tests/modules --ticks 120 --save half.db
galaxy sim load half.db --modules tests/modules --ticks 80 --state
galaxy sim run --modules tests/modules --watch --ticks 900   # edit a script meanwhile
```

Every command prints one JSON object: the tick, the world's hash, the modules, script
failures, hot-loads, what scripts printed, and with `--state` the world: a line an
entity (where it is, its speed, the model it wears, its components by name) and the facts
counted by kind, so it stays a screenful. `--entity <id>` and `--component <name>` print
those fields in full; `--all` prints every field, the facts and causes.

## Saves

A save is one SQLite file. Tables: `meta` (format, engine, build, seed, tick, hash),
`modules` (each module the world ran with: ID, version, minimum engine version and the hash
of its files), `components`, `facts`, `causes`, and `state` (the random streams and the
engine services' causes). Writing goes to a temporary file that replaces the target when
complete.

Loading rebuilds the world from the tables and checks it hashes as it did when saved, so
a save that doesn't come back exactly is refused. `sim load` needs the same set of modules
the save lists. Their versions may differ: every stored component must still fit its
schema, and fields a newer module added get their defaults (with a `load` cause). A save
that would lose data, such as a field a module no longer declares, is refused with a
message. Saves of other formats are refused too: the game is a prototype, so there are
no migrations. Saves of the slice are not supported.

## Recordings

A recording is one JSON file that holds everything needed to run a session again:

- `build`: the engine version, and for an installed build its commit and build name. The
  launcher runs exactly that build for `galaxy sim replay` and `galaxy-gm audit`;
- `modules`: the exact files of every module at the start;
- `seed` and `start`: the whole world the session started from;
- `events`, in order: inputs (`tick`, `source`, `name`, `payload`), hot-loads (`tick`,
  `module` and its exact new files), notes (`tick`, `text`, the world's `hash` then, and
  from a played session the `camera`, a moment, and the `frame` file that was seen) and
  zooms (`tick`, and the `view` height the wheel left; one a tick). Zooms are
  presentation only: a replay of the world skips them, and a film of the session
  (`galaxy-gm film`, `motion-tools.md`) applies them;
- `end`: the last tick and the world's hash there.

It is laid out a record a line: the format, build, seed and end first, then each event on
a line of its own, then the module files and the start world, each on one long line. A
search for `"type":"note"` finds every note. Recordings written compact on one line, as
earlier builds did, read the same; the format number is unchanged.

`galaxy-gm recording <recording>` reads one without scripts: `summary` (build, modules,
ticks, counts of each kind of event), `timeline`, `inputs` (`--name` keeps the inputs
whose name contains a text), `orders` (it replays the recording and lists each change of
an entity's `engine.steer` and each arrival) and `zooms`, a line each, capped at 40
unless `--all`, with `--from`/`--to` ticks.

`timeline` answers "what did the player do?" in one list, with times as `m:ss.s` and the
tick. It replays the recording and tells an order the player gave from one that changed
by itself: an order that changes on a tick an input came is the player's, named with that
input (a mine key reads `order drift.ship: hold drift.rock4 (from drift.order_mine
down)`); one that changes with no input is `auto` (a waypoint step, a script), and a run
of those for one thing is one line. Each player order is marked `superseded at ...
before arriving` or `arrived at ...`. Arrivals, notes and zoom bursts (wheel clicks
within a second of each other) take a line each, and other keys a line a run (inputs
within two seconds of each other).

An input is anything from outside the sim: a key, a click, a scenario step. Its `name` is
a declared event kind and its `source` says where it came from, so flight controls and
widget clicks fit the same shape later.

## Replay

`galaxy sim replay <recording>` builds the modules from the recorded files (never the
module folders), starts from the recorded world, applies each input and hot-load on its
tick, and compares the end hash with the recorded one. It reports the world's hash at
every note, so an audit can jump to a noted moment. A different end exits 5 with kind
`replay_diverged`. `--state` prints the world at the end, as for `sim run`: a line an
entity, `--entity` and `--component` for fields, `--all` for everything.

**Which build.** The launcher runs a recording's own build for `sim replay` and `audit`
when it is installed; `--build <name>` before the command runs another. `galaxy-gm film`
draws with the build that runs it, or with the recording's build when given
`--as-recorded` (it runs that build's executable to draw, then brings the film up to its
own format). A replay, audit or film reports `builds`: the build that recorded the
session, the one working on it now, whether they are the same, and a note. The world can
replay identically on a newer build while it draws differently (a drawing defect fixed
since), so a film or audit on another build says plainly that it may not show what the
player saw, and gives the command that uses the recording's build.

## Played sessions and F8 notes

Every `galaxy play` records itself into a folder of its own under the sessions folder
(`irudd-galaxy/sessions` in the per-user data folder, or `GALAXY_SESSIONS`):

```text
<sessions>/2026-11-01-142233-drift/
  recording.json      written at every note and when the window closes
  save.db             the world when the window closed
  notes/0001.png      the frame that was shown when each note was taken
  audit/0001.png      the same moment drawn again by galaxy-gm audit
```

**F8** pauses the game on the tick it is on and opens a note box over the frame that was
on screen. Enter keeps the note, Esc drops it; the game carries on from the same tick
either way, and keys held when F8 was pressed are let go. A kept note stores its text, the
tick, the world's hash, and a **moment**: how far between ticks the frame was drawn, the
presentation time, the frame's size and scale, the player's zoom, how long the course had
been showing, and the camera. The frame itself goes to `notes/`. A bot run takes notes the
same way from `[[note]]` entries in its `--inputs` file, on the first frame at or after
their tick.

**`galaxy-gm notes`** lists the notes of every session in the sessions folder, newest
first (or of one session or recording): number, tick, time and text, the frame that was
seen and, once audited, the frame drawn again. It shows 20 unless `--all`.

**`galaxy-gm audit <recording>`** runs on the build that made the recording (the launcher
picks it, as for replay). It replays from the start and, at each note, compares the
world's hash with the note's; draws the moment again from the replay, between the same
two ticks and with the same camera, into `audit/`; and compares that picture with the
frame that was seen (mean difference per channel, and the share of pixels that differ).
On the machine that played, the pictures are identical (except a course caught in its
fifth of a second fading out, which is drawn gone); on another GPU they should still
visibly match. A world that differs exits 5 with kind `replay_diverged`.

So a note says exactly where the player was and what they saw, and an agent can stand in
the same moment, look at the state, and try a fix against it.

To see how the moments round a note *moved*, `galaxy-gm film <recording> --note <n>` films
the seconds before and after it, frame by frame as the window drew them, for the motion
tools (`motion-tools.md`).

## Inputs files and scenario tests

`--inputs <file>` takes a TOML file of `[[input]]`, `[[note]]`, `[[hot_load]]` and
`[[zoom]]` entries by tick (`crates/sim/src/scenario.rs` shows the format;
`tests/sessions/stroll.toml` is an example). A hot-load names a module folder; it is read
when the run starts. A zoom turns the wheel `steps` once its tick has run; it is for the
view only (`play`, `shot` and `film` use it, `sim run` has nothing to zoom), and `play`
records it like the player's own wheel.

A scenario test is a module's `tests/<name>.toml`: a seed, a number of ticks, inputs, and
`[[expect]]` entries on component fields or fact counts, checked after their tick. The
runner (`run_module_tests`) runs each and reports the expectations that failed and any
script failures. `galaxy-gm` will run it for modules; today the engine's own tests run the
test module's scenarios.

## Hot-loading

A changed module can replace the running one between two ticks, from `--watch` (a file in
a module folder changed) or an inputs file. The engine rebuilds the schemas and every
module's scripts, checks the world still fits (as for a save: new fields get defaults, a
change that would lose data is refused and nothing changes), swaps them in, and records an
`engine.hot_loaded` fact with a `hot_load` cause naming the module, its version and its
files' hash. Scripts keep no state between calls, so swapping them loses nothing. A
recording stores the new files, so the replay loads exactly what the session did.

## Alternatives and why this design

- **Saves as one serialized blob.** Simpler, but tools and agents could only read it by
  loading the engine. Tables are queryable with plain SQL.
- **Replay from saves every few ticks (snapshots).** Robust to non-determinism, but large,
  and it hides non-determinism instead of finding it. Because the sim is deterministic, a
  recording only needs the start, the inputs and the module files, and the end hash proves
  the replay matched.
- **Recording module folders by path.** Smaller, but a replay would run whatever the
  folder holds now. Storing the exact files makes a recording self-contained, which is
  what lets agents audit a playtest later.
- **A binary recording format.** Smaller and faster, but opaque. JSON is readable by
  people and agents, and recordings are small while modules are text. Binary assets will
  need a different encoding when they arrive.

## Screens in notes and films

Button clicks record ordinary event inputs with source `ui`, including the button's
payload. Replaying applies the chosen event without hit-testing a new viewport. Bot
inputs can name a widget by full path or unique id; the view resolves its current
centre before the tick and records the same event as a pointer click. No selector or
coordinate is added to the recording format. Screen
functions are pure reads of the replayed world; their open state must be module state,
changed by recorded events. A fresh audit or film view therefore rebuilds the same trees
from the embedded scripts and current components. Named anchors use the recorded camera,
frame size and interpolation fraction, like model poses. No additional presentation
record is needed in Screens I: it has no hover, scroll or transient pointer rendering.
See `screens.md` for the bot click and note example.

## Places, clocks and in-progress walks

Places, beings, the player location, spots, doors and clocks use normal component rows;
the SQLite and recording format numbers stay unchanged. An active `engine.walk` stores
its place, original and destination coordinates, spots, start tick, speed and completion
state. Loading or starting a recording halfway through it needs no camera or hidden
script state: the Rust walking service resumes from those fields. The game clock is a
separate recorded integer and advances only through module commands.

Resume validates place references and active walk requirements after schema conformance.
Malformed content is refused rather than silently losing a being or walk. The dedicated
place scenario and executable tests compare a mid-walk save, its resumed endpoint and
replays with a direct run on both Windows and Linux; see `places-and-beings.md`.
