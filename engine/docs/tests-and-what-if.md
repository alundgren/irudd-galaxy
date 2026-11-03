# Module tests and what-if

How a module tests itself, and how the GM finds what a module change does to a recorded
session. The code is `crates/script/src/testrun.rs` (Luau tests), `crates/sim/src/scenario.rs`
(scenario runs) and `crates/gm/src/test.rs` and `crates/gm/src/what_if.rs` (the commands).

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
- **On the recording's build.** Replay and audit run the build that recorded the session.
  What-if is new, so older builds lack it, and the question is what the change does, which
  this build answers with both runs; `baseline` reports when the build matters.

## Explicit rendered bot tests

`tests/<name>.bot.toml` opts a module into a windowed test. Ordinary `.toml` tests
stay headless scenarios; `--only` filters bot filenames too. Example (inside a module):

```toml
inputs = "tests/route.inputs"
ticks = 300
size = [1280, 720]
seed = 7
```

Inputs use the usual bot format and full event IDs; the path is relative to the
module folder. Use `.inputs` rather than `.toml` under `tests/` so it is not also
run as a scenario. Ticks must be positive; dimensions are 64..8192 physical pixels.
Resize and hot-load entries are refused because this test measures one viewport
and module set. Seed defaults to 1. Unknown declaration keys fail the test.

The GM launches the matching `galaxy` beside its own executable with `play`,
`--physical-size`, and `--no-live`, captures its JSON, then replays the recording.
Sibling modules are loaded as for scenarios. `bots` lists each result: status,
passed, requested_size, actual size, frames, frame_time (fps and interval_ms
median/p95/p99/max in milliseconds), replay.identical, hash and session folder.
The first frame has no interval; the next ten intervals are warmup, as in play.
Runs too short to measure intervals fail with an explanation. Sessions are retained
for notes, audit and film. No implicit FPS threshold is imposed.

Missing GPU/window produces status `unavailable` and a plain failure message;
it is not counted as passing. Any failed or unavailable declared test exits 5
(`tests_failed`) with the full report, just like a failed scenario. A crashed child
or missing matching executable also becomes an explained test failure. Child stdout
is captured so the GM still emits exactly one JSON value. GPU integration tests are
ignored by default; declaration and failure handling tests remain headless.
