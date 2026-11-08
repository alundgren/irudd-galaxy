# World edits

How a test, an agent or the owner puts a world in the state they want without playing
there. There are two ways:

- **an edit**: a field set, a component or thing added or removed, a thing moved to
  another place, from outside the module's rules;
- **a debug event**: one of a module's own events, marked debug-only, which its scripts
  handle like any input, so the change keeps the module's rules.

Use an edit by default. Use a debug event whenever the change has rules the module
enforces (fitting a part, the next stage of a build, anything with costs, limits or
follow-on state the scripts keep together).

The code is `crates/sim/src/edit.rs` (the vocabulary, the engine-kept state and the edited
count), `Session::check_edit`, `Session::edit` and the debug checks in
`crates/sim/src/session.rs`, `DueEdits` in `crates/sim/src/scenario.rs` (edits waiting for
their tick), the `edit` and `debug` ops in `crates/live`, and `galaxy-gm live edit` and
`live debug` in `crates/gm/src/live.rs`.

## Trying it

From `engine/`, with the engine installed (`modules-and-install.md`):

```sh
galaxy-gm test tests/edits/purse              # earn.toml sets its start state, then buys
galaxy play --modules tests/play/drift        # in another shell, while it runs:
galaxy-gm live edit "set ship engine.position x=20 y=10" "remove rock4"
galaxy-gm live state --entity ship
galaxy-gm live history --kind edited          # engine.edited, by edit from live
galaxy-gm live status                         # edited: how many changes from outside
```

## One vocabulary

An edit names one entity and does exactly one thing. The same keys are an `[[edit]]` entry
in a scenario, in an inputs file and in a `live edit --file`; `live edit` also takes them as
a line of text.

| Edit | TOML | Text |
|---|---|---|
| set some fields of a component the thing has | `component`, `set = { coins = 500 }` | `set <entity> <component> coins=500` |
| put a whole component, adding or replacing it (the other fields at their defaults); on a new ID it makes the thing | `component`, `put = { lit = true }` | `put <entity> <component> [lit=true]` |
| remove a component, or the whole thing | `remove = true` [, `component`] | `remove <entity> [<component>]` |
| move a located thing to a place at once (a teleport) | `place = "..."` [, `spot = "..."`] | `place <entity> <place> [<spot>]` |

Moving within a place is a `set` of `engine.position`. Explored memory is ordinary
state: `put <ship> engine.explored` clears it (the explorer marks round itself again on
the next tick), and `set <ship> engine.explored cells="0:0+4;1:0+4"` sets it, in the
canonical text `map-and-fog.md` describes; a malformed text is refused with the reason. Text values read as TOML (`99`,
`true`, `"two words"`, `[1, 2]`); a bare word is text. Within one list, a later edit sees
what earlier ones put: `put` a new thing's location, then `place` it.

Names: an inputs file writes full IDs. A scenario writes names as its expectations do (the
module's own names plainly, anything else in full). `live edit` takes them as `live state`
does (a full ID or a unique last part: `ship`, `walker`); a new thing for `put` is named in
full.

A scenario that sets its start state, and pays in through the module's rule:

```toml
seed = 1
ticks = 4

[[edit]]                      # tick 0, the default: before the first tick
entity = "buyer"
component = "wallet"
set = { coins = 150, work = 9 }

[[input]]
tick = 1
name = "buy"
[[expect]]
tick = 1
facts = "refused"             # one job short
equals = 1

[[edit]]
tick = 1                      # once tick 1 has run
entity = "buyer"
component = "wallet"
set = { work = 10 }

[[input]]
tick = 3
name = "grant"                # debug = ["grant"] in the manifest
source = "debug"
payload = { coins = 5000 }    # the script caps it at 1000
```

## Debug events

A module lists debug-only events in its manifest, from its `events`:

```toml
events = ["buy", "grant"]
debug = ["grant"]
```

Only an input with source `debug` sends one: `[[input]] source = "debug"` in a scenario or
inputs file, or `galaxy-gm live debug <event> [field=value ...]` against a running game,
which queues it for the next tick (marked `via` live). `check_input` refuses a debug event
from any other source, and source `debug` for an event that isn't one. A manifest that
binds a debug event to a key is refused; a screen button names its event while the game
runs, so a press that names a debug event does nothing. The engine adds nothing else: the
scripts handle the event as they handle any input, and it is recorded and replayed as one.

## How it works

**When.** An edit is made between two ticks: an entry on tick `t` once tick `t` has run and
before `t + 1`, so tick 0 is the start state, and a scenario's expectations on tick `t` see
it. A live edit is made between the frames it arrives in. A session resumed from a save
skips inputs-file edits before the save's tick, as it skips inputs.

**One batch.** `Session::edit` turns the edits into the world's own commands (`patch`,
`put`, `remove`, `move`) and applies them as one batch with the cause `edit` and its
source (`scenario`, `inputs`, `live`), whole or not at all. The engine actor may write any
component, so an edit reaches anything a module declares, but the batch is checked as every
batch is: field types, and afterwards the same consistency checks a resumed save runs, for
places, spots, doors and walks, talks and lives. An edit that would leave a dangling
reference (removing a place its spots or a being still name, a talk's first line) is
refused with the check's message, and nothing changes.

**State the engine keeps.** Some engine components are the engine's to keep consistent,
and an edit may not write them; the refusal says what to use instead (`KEPT` in
`edit.rs`):

| Component | Kept | Instead |
|---|---|---|
| `engine.walk` | all of it | an input or a debug event starts a walk; `place` teleports |
| `engine.travel` | all of it | an input or a debug event goes through a door; `place` teleports |
| `engine.location` | all of it, removal included, once a thing has one | `place`, which moves the position, location and any walk together |
| `engine.talk` | `current`, `seen`, `active` | an input or a debug event starts or answers a talk; the other fields can be set |
| `engine.life` | `current`, `phase`, `start_tick` and the saved pose fields | change the acts or advance the clock |

`put` of one of these on a thing that has it is refused too (it would reset the kept
fields), and so is removing a thing in the middle of a walk or a trip, or `place` during a
trip.

**Refusals.** Before the batch, `check_edit` refuses, by name, an entity ID that isn't full
or isn't in a loaded module's namespace, an entity that doesn't exist (except for `put`), a
component type no module declares, a `set` or `remove` of a component the thing doesn't
have, a field the type doesn't declare, a value of the wrong type and engine-kept state. A
list with one bad edit changes nothing; the error says which edit. An inputs file's edits
are also checked for component types and fields when the run starts, before anything
plays. In a scenario, a refused edit is a scenario error (the test fails with the message,
the others still run); in `live edit` it is exit 4, `bad_input`.

**Scripts hear it.** The batch records an `engine.edited` fact (`source`, and the
`entities` and `components` the edits named), an event on the next tick, as
`engine.hot_loaded` is. It always fires. Scripts that keep values derived from others (a
count, a cache, a "done" flag) handle it and re-derive them. The engine never runs handlers
for an edit by itself: an edit sets exactly the state it names. What a module author has to
know:

- an edit is outside the rules: a script's checks (prices, capabilities, the order of
  steps) don't run for it. Edit what the rules read (the counter, not only its reward), or
  give the module a debug event for the change;
- raising an `engine.clock` records `engine.clock_advanced` as any write does;
- a teleported body is where it was put on the next tick; no travel or glide runs for it.

**Marked as edited.** `edit::edits(world)` counts the world's `edit` causes and its inputs
from source `debug`. It is read from the causes, which every save keeps, so a save made
after an edit is marked with no new field, and an unedited world's hash is unchanged.
`live status`, `galaxy sim run`, `sim load`, `sim replay` and `galaxy play` report it as
`edited`, and `galaxy-gm recording <rec>` counts the recording's edits and debug events
(the start world's included). An edited save loads like any other.

**Recorded.** With a recording on, the edit goes into it as an `edit` event (`tick`,
`source`, the edits with full IDs), and a debug event as an input with source `debug`. A
replay (and so audit, film, look, what-if and the recording views) keeps edits and
hot-loads in one queue in the order they were made, and makes each edit through the same
`Session::edit` once its tick has run, so the world, its facts and its hash come back the
same. Notes, edits and inputs keep the order they happened in within a tick: a replay
brings the world to each note in turn (`Replay::at_note`), making only the edits taken
before it, so audit, look, film and what-if see the world each note was taken on, even
with an F8 note either side of a live edit. A recording written right after a live
edit replays to it. Recordings and saves
without edits read, replay and hash exactly as before; the recording and save format
numbers are unchanged.

## Alternatives and why this design

- **Only debug events.** Every change would keep the module's rules, but every module would
  have to write a handler for each thing a test might set, and a test could only set what a
  module chose to expose. Edits use the module's components and fields as the vocabulary,
  with no game nouns in the engine; debug events cover the changes whose rules matter.
- **Starting a scenario from a save** (a cached fixture). A save is exact, but it is binary,
  made by playing there, and goes stale whenever the start state or the rules change. A few
  `[[edit]]` lines say what the test needs and keep working as the world changes around
  them.
- **Silent edits** (no fact). Simpler, but a module with a derived value would be left
  inconsistent with no way to notice. One fact a batch costs nothing in a world that wasn't
  edited, and lets the module decide.
- **Raw writes to everything.** Walks, trips and talk progress are tied to positions,
  places and other things the engine moves together; a raw write would break that in ways
  the checks can't always see. The kept state is refused instead, with the command that
  keeps it whole.
- **An edited flag in the save.** A new field would change the save format and every
  hash; the causes already say it.
- **Recording the commands instead of the edits.** The commands are what is applied, but
  the edits are what someone wrote; replaying them through the same path keeps one check
  and one meaning, and a recording reads as the edit was given.
