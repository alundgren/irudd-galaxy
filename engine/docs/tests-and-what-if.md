# Module tests and what-if

How a module tests itself, and how the GM finds what a module change does to a recorded
session. The code is `crates/script/src/testrun.rs` (Luau tests), `crates/sim/src/scenario.rs`
(scenario runs), `crates/gm/src/test.rs`, `crates/gm/src/bot.rs`, `crates/gm/src/perf.rs`
and `crates/gm/src/what_if.rs` (the commands), and `crates/galaxy/src/play.rs` (headless play).

## Trying it

From `engine/`, with the engine installed (`modules-and-install.md`):

```sh
galaxy-gm test tests/modules/stroll                 # 3 Luau tests and a scenario run pass
galaxy-gm test tests/failing/counter                # 2 fail on purpose: exits 5
galaxy-gm test tests/failing/counter --only quiet   # just the tests whose file or name has it

galaxy sim run --modules tests/modules --seed 7 --ticks 450 \
    --inputs tests/sessions/stroll.toml --record session.json
galaxy-gm what-if session.json --modules tests/what-if/stroll   # rules.STEP changed: tick 30
galaxy-gm what-if session.json --modules tests/modules          # the same files: no difference
```

## A module's tests

Tests live in the module's `tests/` folder and travel with it (they are module files, so a
recording stores them too, and they never run in play).

**Luau tests**, `tests/<name>.luau`, test pure Luau: rules, tables, sums. The file returns a
table of named functions, and each function is a test that passes if it returns without an
error:

```lua
local rules = require("../scripts/rules")
return {
    a_walker_reaches_the_deck_at_its_height = function()
        assert(rules.reached_deck({ layer = 0, height = 2.0 }), "2.0 high reaches the deck")
    end,
}
```

They run in the scripts' sandbox with the same step and memory budgets (`script-host.md`),
and `require` sees the same in-memory files. Each test runs in a fresh Luau state, in name
order, so tests share nothing and an endless loop fails its own test with `over_budget`.
There is no `ctx`: a test that needs the world is a scenario run. A failure names the file,
the line and the assert's message; a file that doesn't load, or doesn't return a table of
functions, is one failed `(load)` test.

**Scenario runs**, `tests/<name>.toml`, test what the module does to a world: a `seed`, a
number of `ticks`, `[[input]]` entries as in an inputs file (a plain name is the module's
own event kind) and `[[expect]]` entries on a component field (`equals`, `at_least`,
`at_most`, or `absent = true`) or a count of facts, checked after their tick (the last by
default). A script failure during the run fails it too. `saves-and-replay.md` has the
format; it is the inputs file plus expectations, so a session scripted for `galaxy sim run`
becomes a test by adding a seed, ticks and what should hold.

**`galaxy-gm test <module>`** loads the module with the modules beside it, as `check` does
(a module may count another's events), runs every Luau test and every scenario, and prints
one JSON object: each test with its result, `failures` with a line per failure, and the
counts. Any failure exits 5 with kind `tests_failed` and the same fields. A scenario that is
itself wrong (an unknown event, an expectation after the last tick) fails with that message
and the others still run. `--only <text>` keeps the tests whose file or name contains it.

**Side by side.** The Luau tests take well under a second and run first, one after another.
Scenarios and bots then share one queue on half the machine's cores, the longest first (a
bot's ticks count about ten times a scenario's, for its frames and its replay), so the
longest run, not the sum, sets how long `test` takes. Each scenario builds its own world
and Luau states in its own thread and each bot is its own `galaxy` process with its own
session folder, so nothing one writes is read by another: the results, and the report,
are the same whatever order they finish in and however many run at once. Reports keep
file order.

## What-if

**`galaxy-gm what-if <recording> --modules <dir>`** runs two replays side by side, one tick
at a time: the recording as it was, and the recording with the given modules' files in
place of the recorded ones (modules it didn't have are added). After each tick it compares
the two worlds and stops at the first that differs. There it lists what differs: entities
or components only one run has, each field with its recorded and changed value, and the
facts recorded that tick by only one run, or, when both recorded the same facts in another
order (the order they're handled in next tick), the first position where they disagree;
then anything else (random streams that were drawn a different number of times, the
causes). `report` says it in a few lines:

```text
first difference after tick 30 (0:01.0): 2 in entities and components, 0 in facts
stroll.w1 stroll.walker.steps: recorded 1, changed 2
stroll.w2 stroll.walker.steps: recorded 1, changed 2
```

Details that decide the answer:

- **The start.** A session that started fresh (`play`, `sim run`) starts again from the
  changed modules' start states with the recorded seed, so a changed start value or a new
  field default shows as a difference at tick 0. A session that started from a save resumes
  that world with the changed modules, as `sim load` would (new fields get defaults; a
  change that would lose data is refused with exit 4), so a new default shows at the tick
  the recording starts on. `from` is that tick, and the comparison runs from it.
- **Hot-loads.** Recorded hot-loads of a changed module are skipped and listed: the changed
  files are used throughout. Hot-loads of other modules apply as recorded.
- **Builds.** Both runs are this build, so a difference comes from the module change alone.
  `baseline` says whether the plain replay still reaches every recorded hash it passed; if
  it doesn't (the recording is from another build), differences are against that replay.
- **A run that can't go on**, such as a recorded input the changed modules no longer
  declare, is the difference: `error` says why, at that tick.

It exits 0 whether or not the world differs: it answers a question, it doesn't judge.

**Comparing cheaply.** The world's hash serializes the whole world, facts and causes
included, and those only grow, so hashing every tick of a long session would cost the
square of its length. The two worlds were equal at the previous tick, so each tick compares
the entities and only the facts and causes added since.

## Alternatives and why this design

- **Luau tests with a fake `ctx`.** Handlers could be called with a stub world, but the
  stub would be a second, simpler world to keep in step with the real one. Scenario runs
  already test handlers against the real sim, cheaply and headless, so Luau tests stay for
  pure logic (the slice's `pack test` made the same split).
- **A separate test format.** Scenario runs reuse the inputs file, so a session scripted to
  look at something becomes a test by adding expectations, and `galaxy-gm fields inputs`
  documents both.
- **What-if from per-tick hashes in the recording.** Storing a hash each tick would let
  what-if run one replay, but it makes every recording bigger and only tells *that* the
  world differs, not what. Running the recording beside the change gives both worlds, so
  the report can name the fields.
- **What-if to the end.** After the first difference most of the world drifts (cause ids
  shift, scripts see other values), so later differences mostly echo the first. The first
  tick and what differs there is what points at the cause; a longer look is `galaxy sim run
  --state` on the changed modules.
- **Scenarios one after another.** They ran serially while the bots ran side by side, so
  `test` took the scenarios' sum plus the bots' (about 20 minutes for the core module).
  One queue for both lets a long scenario run beside the bots. Splitting the queue in two
  (scenarios on threads, bots on processes) would leave one pool idle while the other
  works.
- **Half the cores, not the build permit.** Other agents build and test on the same
  machine. Taking `cargo xtask`'s build permit would make every `test` wait behind every
  build, and builds behind tests, though a test compiles nothing; half the cores leaves the
  other half to a build, which `CARGO_BUILD_JOBS` and the permit already bound.
- **On the recording's build.** Replay and audit run the build that recorded the session.
  What-if is new, so older builds lack it, and the question is what the change does, which
  this build answers with both runs; `baseline` reports when the build matters.

## Bot tests

`tests/<name>.bot.toml` declares a bot: a route of clicks, drags, keys, zooms and notes
played through `galaxy play`, checked to the end and replayed. Ordinary `.toml` tests stay
headless scenarios. `--only` filters bot file names too, and a whole file name
(`--only trade.bot.toml`) runs that file alone, never `mixed-trade.bot.toml` beside it.
Example (inside a module):

```toml
inputs = "tests/route.inputs"
ticks = 300
size = [1280, 720]
scale = 1.25
seed = 7
```

Inputs use the usual bot format and full event IDs; the path is relative to the module
folder. Use `.inputs` rather than `.toml` under `tests/` so it is not also run as a
scenario. Ticks must be positive; dimensions are 64..8192 physical pixels. `scale` is the
UI scale, 0.5 to 4 (1 by default): the screen is `size / scale` logical pixels, the space
screens lay out in and bot clicks name points in. Resize and hot-load entries are refused
because a bot plays one viewport and module set. Seed defaults to 1. Unknown declaration
keys fail the test.

**Always headless.** `galaxy-gm test` launches the matching `galaxy` beside its own
executable with `play --headless`, `--physical-size` and `--scale`, captures its JSON, then
replays the recording. A bot never opens a window and never measures frame times: those
are the performance scenes' (below). Bots run side by side with the scenarios, longest
first, on half the machine's cores; each report is still in declaration order. Sibling
modules are loaded as for scenarios. `bots` lists each result: status, passed, requested
and actual size, scale, frames, wall_ms and speed (game seconds a wall second), resting_clicks, problems (every
model, look, readability or covered-speaker problem shown, in the order first seen),
note_problems (each note's tick and the problems its frame showed), replay.identical, hash
and the session folder, retained for notes, audit and film. `notes` says how much game time
the bots played and how long it took.

A bot whose notes need a GPU to draw their frames, on a machine without one, produces
status `unavailable` and a plain failure message; it is not counted as passing. Any failed
or unavailable declared test exits 5 (`tests_failed`) with the full report, just like a
failed scenario. A crashed child or missing matching executable also becomes an explained
test failure. Child stdout is captured so the GM still emits exactly one JSON value. GPU
integration tests are ignored by default; declaration and failure handling tests remain
headless, and `crates/galaxy/tests/headless.rs` plays a generic route headless: a click on
a button riding a moving marker, a resting button, keys.

### How a headless run plays

`galaxy play --headless` (`crates/galaxy/src/play.rs`) is the window's session loop with
no window, audio device or live socket. A window frame runs the ticks real time owes, then
draws; headless, each tick runs as soon as the last frame is built. Both go through the
same two steps:

- **A tick** (`App::tick`): the inputs due, and the bot's pointer gestures and clicks,
  each resolved against the screen as it stands: the scene is laid out again at the end of
  the last tick (`alpha` 1) at the screen's logical size, then the widget, anchor, target
  or speaker is found with the same hit tests a pointer uses. A resting button is reported,
  not clicked. Keys are their bound events down on their tick and up on the next.
- **A frame** (`App::present`): the zooms due, the scene, its UI sounds, presentation
  snapshot and problems, then the notes due, kept over that frame. A window also draws it;
  headless draws only a note's frame, so a run without notes needs no GPU.

Headless, a frame follows every tick, drawn at the tick's end, and presentation time is
the world's (tick / 30), never the wall clock. The screen is `--size` at `--scale`, as a
window of that size at that scale lays out (a window takes `--scale` in place of the
display's). So a headless run is the same however fast the machine is, and two runs of a
route give the same recording.

**What is the same as a window, and what is not.** Played headless and in a window, a route
gives the same world hash at every tick, the same recorded inputs (tick, name, source,
payload), the same notes (text and world hash) and the same UI-sound cues. Compared on the
core module's `fit` (44694 ticks) and `trade` (8594 ticks) routes against recordings played
in a window at 1280x720 and 125%. What differs is frame timing, which no two window runs
share either:

- A window draws about two frames a tick, at real time, so its presentation snapshots
  (widget motion, timed by the screen clock) and each note's moment (`alpha`, `time`, the
  camera and motion state it was drawn with) come from whichever frames it happened to
  draw. Headless draws each tick's end.
- The screen clock of a headless frame is the tick's end; a window frame's is part way
  through the next tick. A screen change timed by the clock, such as a panel opening once
  the view's glide ends, is therefore seen, and its UI sound stamped, one tick earlier
  headless: 11 of 267 cues on `fit` and 11 of 148 on `trade`, every other cue on the same
  tick. Where a screen leaves widgets out while the view glides, the same widget's path
  index can differ for that tick too.
- A window frame that runs late runs two ticks before drawing. A note due on the first is
  then taken on the second: one tick late (`trade`'s note at 7466 was kept at 7467 in the
  window). Headless never runs two ticks between frames.

**Speed.** A tick of the core module costs under a millisecond; a frame about five, about
90% of it the screens whose reads changed running their Luau again, each in a fresh VM. So
a headless bot runs about four to seven times faster than a window, more side by side.

## Performance scenes

`tests/<name>.perf.toml` declares a moment to measure: where to start, how long to hold it,
and at what size. `galaxy-gm perf <module>` plays each in a real window at real speed and
reports the frame times that window measured. `galaxy-gm test` never runs them. Example:

```toml
inputs = "tests/route.inputs"   # optional: none holds the module's start
at = 7200                       # the tick the window starts on (0 by default)
seconds = 30                    # how long the window plays, 1 to 600
hold = true                     # optional: the route stops at `at`
size = [1280, 720]
scale = 1.25
seed = 1
```

**How it runs** (`crates/gm/src/perf.rs`). With `at` above 0, the route plays headless and
fast to that tick and is saved there (`galaxy play --headless --save`). A window then loads
the save and plays `seconds` at real speed, the route's later inputs still playing, so a
scene can go on through a busy stretch. With `hold`, the window gets none of the route
after `at`: it holds that moment (an open talk, a menu), which the route would soon have
left. Scenes run one at a time. Each reports
`frame_time`: fps, then interval_ms (frame to frame), sim_ms, scene_ms and draw_ms, each as
median, p95, p99 and max after the first ten frames, as `galaxy play` reports them.

**Compared with the last run.** The last result of each scene is kept in the per-user data
folder (`irudd-galaxy/perf/<module>/<name>.perf.json`; `GALAXY_PERF` moves it), never in the
module, because the numbers are this machine's. The report shows the previous result
(build, when, frame times) and `change`: this run minus the last, in milliseconds, positive
slower. A different size, scale or GPU since the last run is said in `change_note`. Each
run replaces the kept result.

**No budgets.** Nothing fails on a number; `perf_failed` (exit 5) means a scene could not
play: no window or GPU, a wrong declaration, a run that stopped early.

**When to run it**: once per finished PR, on a quiet machine with no builds running.
Anything else using the CPU or GPU moves the numbers more than most changes do.

## Alternatives and why this design (bots and performance)

- **Resolving bot actions without a screen.** A click could be turned into its event from
  the widget tree alone. But where a click lands depends on layout, anchors, the camera,
  covering widgets and resting buttons, which is the view. Laying out the same scene a
  window would, and hit-testing it the same way, is what makes the result the window's.
- **Sparse frames.** Building only the frames a bot action, note or zoom needs (plus one a
  game second) ran about 17 times faster than a window with the same world and inputs, but
  it stamped UI sounds of panels the world opened at the next frame, up to a second late,
  so the recording no longer matched a window's. A frame a tick keeps it.
- **A window for every bot, or on request.** Real time made the core routes take over an
  hour, each window opened on the owner's screen, and frame times measured while other
  bots and builds ran said little. Frame times belong to a few chosen moments, measured
  once on a quiet machine: the performance scenes.
- **Budgets.** A threshold would fail on whatever else the machine was doing, and the
  numbers differ from machine to machine. Showing the change since the last run on the
  same machine says what a PR did, which is the question.
- **Results in the repo.** The numbers are one machine's, so a committed baseline would be
  wrong on every other one.
- **The display's scale.** Without a declared scale a bot laid out at whatever the
  display's scaling was, so the same route clicked different layouts on different machines.
  The scale is the bot's, and a window takes it too.
