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
of its files), `components`, `facts`, `causes`, and `state` (the random streams, the
engine services' causes and, only while bodies are in contact with contacts recorded,
`contacts`: `flight-and-collision.md`). Writing goes to a temporary file that replaces the target when
complete.

Loading rebuilds the world from the tables and checks it hashes as it did when saved, so
a save that doesn't come back exactly is refused. `sim load` needs the same set of modules
the save lists. Their versions and files may differ: every stored component must still fit
its schema, and fields a newer module added get their defaults (with a `load` cause). A save
that would lose data, such as a field a module no longer declares, is refused with a
message naming the modules whose files changed. Saves of other formats are refused too: the game is a prototype, so there are
no migrations. Saves of the slice are not supported.

Windowed resume uses `galaxy play --modules <modules> --load save.db`. It restores
through the same reader and `Session::resume`, without firing startup events again.
For play, the engine version and installed build name must match; otherwise exit 4
explains which build to restore. Uninstalled binaries have only a version identity.
Module files may have changed since the save, as when the GM changed the game between two
sittings: play carries the world over to them exactly as `sim load` and a hot-load do, or
refuses (exit 4) when the world no longer fits. Both report `resumed`: the save, its tick
and `changed_modules`. `--seed` with `--load` is a usage error.

**The continuing save.** `galaxy play --modules <modules> --continue` loads the newest
`save.db` in the sessions folder made with exactly these modules (by ID), which is where the
player stopped, since every session saves on quitting; `galaxy sim load --continue` takes
the same save headless, which is how an unattended GM checks the player's game still loads
on its change (`gm-unattended.md`). No save of the modules is exit 4. Newest is by the
save file's modified time, so a GM's own bot sessions must go to another sessions folder
(`GALAXY_SESSIONS`). The alternative, a named save slot the player picks, needs a save
browser the game doesn't have yet; the newest session is what "carry on" means today.
Bot inputs and `--ticks` use absolute world ticks; ticks already saved do not run again.
The resumed recording embeds the saved world, and F8, replay, audit and film use it.

**Hearing a resume.** A resumed world is the saved one exactly, so nothing in it says the
game stopped, and presentation that follows new facts (sound cues, `audio.md`) plays no
earlier cue again. So resuming (`play --load`, `play --continue`, `sim load`) tells the
scripts: the first tick run after the save starts with the input `engine.resumed` (source
`resume`, no payload), before any other input on that tick. Handlers see it like any
event; a module whose music should still be playing records its music event again, and
the cue plays on that tick. `Session::announce_resume` queues it, and `resume_save` (both
commands' shared path) calls it. A load that runs no tick (`sim load --ticks 0`) adds
nothing. It is recorded like any input, so a replay, audit, film or what-if of the resumed
session applies it from the recording; a replay starts with `Session::resume` alone and
never announces a second time.

This defines what a resumed world equals. Before, a save resumed for n ticks ended on the
same hash as a run that never stopped. Now it ends on the hash of a run that never stopped
and was given that one input on the tick after the save (an `[[input]]` with name
`engine.resumed` and source `resume` in its inputs file). The input is always there, so the
hash moves by its fact even when no module handles it; the tests in
`crates/galaxy/tests/sim.rs` and `places.rs` compare a resumed run with such a direct run,
and the generic module `tests/modules/resume` shows a cue coming back
(`tune.luau`: `play` marks it playing, `engine.resumed` records the theme again).

## Recordings

A recording is one JSON file that holds everything needed to run a session again:

- `build`: the engine version, and for an installed build its commit and build name. The
  launcher runs exactly that build for `galaxy sim replay` and `galaxy-gm audit`;
- `modules`: the exact files of every module at the start;
- `seed` and `start`: the whole world the session started from;
- `events`, in order: inputs (`tick`, `source`, `name`, `payload`), hot-loads (`tick`,
  `module` and its exact new files), notes (`tick`, `text`, the world's `hash` then, and
  from a played session the `camera`, a moment, and the `frame` file that was seen) and
  zooms (`tick`, and the `view` height the wheel left; one a tick), and widget presentation
  snapshots (`tick`, `state`: scroll offsets and an active drag). Both are
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
tick, and compares the end hash with the recorded one. A world hot-load made after the last
tick ran (recorded for the tick that never came, as when the recording is written right
after a load or the window closes before the next tick) is applied at the end, since the
recorded end already holds it: after a note on the last tick (taken before the load), and
before a film draws the last tick (`Replay::finish`, which every reader shares). It reports the world's hash at
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
  recording.json.handled/0001.json   the GM's answer to note 1, when handled
  audit/0001.png      the same moment drawn again by galaxy-gm audit
  shots/tick-88.png   what the window showed when galaxy-gm live shot asked
```

The note editor wraps text inside a clipped viewport and scrolls to keep the end caret
visible while typing or deleting. Wheel and Page Up/Down scroll through the full note;
End returns to the caret. A separate 32px track and minimum 44px thumb also support
dragging and track clicks, without covering the note text. The editor
uses the same headless font measurement as drawing. This editing view is not part of
the saved note image: F8 preserves the game frame before opening the editor.

**F8** pauses the game on the tick it is on and opens a note box over the frame that was
on screen. Enter keeps the note, Esc drops it; the game carries on from the same tick
either way, and keys held when F8 was pressed are let go. A kept note stores its text, the
tick, the world's hash, and a **moment**: how far between ticks the frame was drawn, the
presentation time, the frame's size and scale, the player's zoom, how long the course had
been showing, and the camera. The frame itself goes to `notes/`, written as a PNG on a
background thread so Enter doesn't freeze the game while it compresses (`play` waits for
those writes before it quits). A bot run takes notes the same way from `[[note]]` entries
in its `--inputs` file, on the first frame at or after their tick; like F8, a note pauses
the game, so its time is left out of the frame-time report.

**`galaxy-gm notes`** lists the notes of every session in the sessions folder, newest
first (or of one session or recording): number, tick, time and text, the frame that was
seen and, once audited, the frame drawn again. It shows 20 unless `--all`.

**`galaxy-gm notes inbox`** uses that same session reader but keeps only unhandled notes,
newest sessions and newest notes first. Each entry keeps its original note number, tick,
time in seconds, text and frame path, so `audit` and `film --note` address the same moment.
The default cap of 20 is applied after filtering; `--all` removes it. `--handled` lists
answered notes instead, including their reason and optional change reference. The ordinary
`notes` command remains the complete history, with `handled: null` or the answer per note.
No sessions yet means an empty inbox; unreadable recordings are reported as `unreadable`
when listing a folder, as for `notes`, rather than silently disappearing. `--modules <dir>`
(on `notes` too) keeps only the sessions that played exactly the modules in `<dir>`, one
game when the sessions folder holds several; `sessions_of_other_modules` counts the rest.

```sh
galaxy-gm notes inbox
galaxy-gm notes handle <session> --note 1 --reason "Adjusted the panel spacing" --change <commit>
galaxy-gm notes inbox --handled
galaxy-gm notes unhandle <session> --note 1
```

Handling requires a nonempty reason describing what answered the note. `--change` is
optional free text: a commit, PR or module version. Repeating handle replaces that note's
answer; unhandle removes it and is harmless when already unhandled. Unknown note numbers
are `bad_input` (exit 4), invalid arguments `usage` (2), and storage I/O failures `runtime`
(1). Each command prints one JSON value. Damaged or unsupported receipts are `bad_input`,
not silently treated as answered; unhandle can remove a damaged receipt.

**Storage:** `crates/gm/src/sessions/handled.rs` writes format-1 JSON receipts under
`<recording filename>.handled/`, beside the recording, named by note number (`0001.json`).
Each holds the original note event, reason and optional change. Matching the event prevents
a recording replaced at the same path from hiding a different note. Appending notes to a
running session preserves existing marks. A temporary file in the same folder replaces
one receipt atomically; separate note files avoid competing updates to one shared index.
Keep this folder when moving or backing up a session. New sessions and engine reinstalls
do not touch it; it lives with the game data, including when `GALAXY_SESSIONS` moves that
data. No absolute session identity or engine version is needed to find the marks.

An engine-repository checklist would tie player feedback to a checkout and installation;
putting answers into the recording would let the playing process overwrite them. A global
inbox database would need its own session identities and relocation rules. Per-note
receipts reuse the existing session layout and reader and let an unattended GM use the
same commands later, without changing the recorded play or its replay format.

**`galaxy-gm audit <recording>`** runs on the build that made the recording (the launcher
picks it, as for replay). It replays from the start and, at each note, compares the
world's hash with the note's; draws the moment again from the replay, between the same
two ticks and with the same camera, into `audit/`; and compares that picture with the
frame that was seen (mean difference per channel, and the share of pixels that differ).
On the machine that played, the pictures are identical (except a course caught in its
fifth of a second fading out, which is drawn gone); on another GPU they should still
visibly match. A world that differs exits 5 with kind `replay_diverged`.

The report also says how many pictures it compared (`pictures_compared`) and, in a
sentence (`pictures`), why not all of them. The usual reason is a recording copied out of
its session folder: the frames the player saw stay behind in `notes/`, so only the world
hashes were checked. Notes from `galaxy sim run` have no frame at all, so they never
count as compared.

So a note says exactly where the player was and what they saw, and an agent can stand in
the same moment, look at the state, and try a fix against it.

To see how the moments round a note *moved*, `galaxy-gm film <recording> --note <n>` films
the seconds before and after it, frame by frame as the window drew them, for the motion
tools (`motion-tools.md`).

Audio is presentation only: recorded facts and place state drive sounds without any
audio state in the world hash. `audio.toml` travels with module text; binary sound
assets stay in the module folders and are unnecessary for headless replay. The
[audio note](audio.md) explains offline mixing and its scripted-input command.

## Inputs files and scenario tests

`--inputs <file>` takes a TOML file of `[[input]]`, `[[note]]`, `[[hot_load]]`,
`[[zoom]]` and `[[resize]]` entries by tick (`crates/sim/src/scenario.rs` shows the
format; `tests/sessions/stroll.toml` is an example). A hot-load names a module folder; it
is read when the run starts. A zoom turns the wheel `steps` once its tick has run; it is
for the view only (`play`, `shot` and `film` use it, `sim run` has nothing to zoom), and
`play` records it like the player's own wheel. A resize sets the play window's inner size
in logical pixels once its tick has run, so a bot can measure resizing
(`resize_interval_ms` in play's frame times, `renderer-and-style.md`). Only `play` has a
window to resize; the other commands ignore it, and it is not recorded, because a film or
audit draws at the size it is given.

A scenario test is a module's `tests/<name>.toml`: a seed, a number of ticks, inputs, and
`[[expect]]` entries on component fields or fact counts, checked after their tick. The
runner (`run_module_tests`) runs each and reports the expectations that failed and any
script failures. `galaxy-gm test <module>` runs them with the module's Luau tests, and
`galaxy-gm what-if` runs a recording again on changed modules to find the first tick that
differs: both are in [tests-and-what-if.md](tests-and-what-if.md).

## Hot-loading

A changed module can replace the running one between two ticks, from `--watch` (a file in
a module folder changed), an inputs file, or `galaxy-gm live load` while `play` runs
(`live-socket.md`). The engine rebuilds the schemas and every
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
- **Hearing a resume: a recorded input.** A fact written into the world when the save
  loads (as a hot-load writes `engine.hot_loaded`) would put an event between two ticks
  that no inputs file can give, so no uninterrupted run could reach a resumed world. The
  audio replaying the last music cue still in effect would keep the hash, but only for
  music, and only for cues a table can read back from history: a script's choice made from
  world state (a channel that changed by distance) can't be told from an old fact. An
  input on the first tick lets every script react, and keeps a simple equality: resuming
  is that one input.
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
frame size and interpolation fraction, like model poses. Hover pointers, scrolling and transient drag state use presentation snapshots;
each note keeps its exact last drawn snapshot.
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

Room travel stores its endpoints and start tick in `engine.travel`. Play submits the
declared `engine.place_arrived` input when the cut lands, including an early click;
replay consumes that exact input. F8 and film rebuild the cut from the world tick and
existing interpolation metadata. Room markers are imported into the initial world;
saves keep them as ordinary spot and position components. Existing room-less recordings
and save format 1 remain compatible. See [Rooms and travel](rooms-and-travel.md).

## Conversations

`engine.talk` stores current line, active status and whether the final line was
acknowledged; `engine.talk_line` holds the authored text chain. Start and advance are
recorded engine inputs. Leaving interrupts without marking a story seen. Save/load
and replay retain pending and active conversations through the ordinary component
path, without a format change. F8, audit and film rebuild the bubble through the same
scene path and existing moment metadata. See [Talk](talk.md).

Life days and transition phases are ordinary engine.life and engine.act components.
A saved rise, walk or settle retains its clock, target act, source morph/facing, phase
start and walk endpoint. Resume validates these references. F8, audit and film compute
the same fractional pose and sign without a private timer. Every increase of an
existing clock records engine.clock_advanced under the writing command cause; replay
regenerates it from the recorded input and script. No storage format changes.
Audit and `film --note` share the exact-moment scene path. A drawn note film exports
`note.png` at the note's original physical size, scale and fractional tick, alongside
its regularly sampled motion frames.

A hot-load whose only changed file is `style.toml` carries `presentation_only = true`
in its recorded hot-load event. It replaces the embedded style without a world cause
or `engine.hot_loaded` fact, so changing screen paint cannot change the simulation.
Events without this flag retain the original world-hot-load semantics.
Replay keeps both kinds in one acceptance-ordered queue: world reloads run before
their recorded tick, and style reloads can run after theirs, but never pass an earlier
world reload. A style snapshot made after a code edit therefore sees that code when
replayed, even when both edits arrived between the same two ticks. Audit and film use
this same queue, including its notification to rebuild the view's module assets.

`galaxy-gm look` replays embedded modules headlessly using the shared screen/mesh
picker. `--note` restores the exact moment and checks its world hash; `--tick` draws
an endpoint at the requested viewport. It reports both builds. The optional pointer
in presentation version 1 defaults to absent in older recordings. Hover labels derive
from that pointer, the screen tree and posed geometry; no hover state enters saves,
world hashes or recorded simulation inputs.
