# AGENTS.md: the Lantern Reach slice

Read this before changing anything under `slices/01-lantern-reach/`. Brief: issue
#10. Decisions that override it: the kickoff comment on PR #11. Design:
`design/vertical-slice-lantern-reach.html` and `design/high-level-plan.html`.

## Commands

Run from this folder. In Git Bash on Windows, first `export PATH="$HOME/.cargo/bin:$PATH"`.

```sh
cargo xtask check      # fetch-luau, fmt --check, clippy -D warnings, tests. Must pass before a PR.
cargo xtask contract   # regenerate crates/contract/schemas/ and api/. Commit the result.
cargo xtask fetch-luau # pinned luau-analyze into tools/luau/ (git-ignored, SHA-256 checked)
cargo test -p galaxy-core
cargo run -p galaxy-app -- <command>   # see README.md for the CLI
```

## Rules

- Work on a branch `slice/01/<topic>` and open a PR into `slice/01-lantern-reach`.
  Never push to `slice/01-lantern-reach` or `main`. Don't merge your own PR.
- Git and `gh` only. No CI or GitHub Actions: checks run locally.
- `cargo test` stays headless: no window, GPU, audio device or network. Tests that need
  one go behind `#[ignore]`.
- Dependency versions live in `[workspace.dependencies]` in the root `Cargo.toml`; crates
  use `dep.workspace = true`. Third-party code must be well known and MIT-compatible.
- Never enable fat LTO.
- Public repo: no local absolute paths, machine names, usernames, emails or secrets in
  committed files. `OPENROUTER_API_KEY` comes from the environment only.
- Commit often. Each commit ends with the `Co-Authored-By:` trailer you were given.
- A question you'd normally ask: pick the best option, list it under "Decisions" in the
  PR description, keep going.

### Determinism

- No wall-clock reads in the sim (`std::time`, `Instant`, `SystemTime`). The sim counts
  ticks; the app maps real time onto ticks with `galaxy_core::step::FixedStep`.
- `BTreeMap`/`BTreeSet` (or `IndexMap`) for anything iterated. No `HashMap` in sim state.
- Randomness only from `ctx.rng()` / `ctx.rng_named(..)` in a system (seeded PCG32
  streams saved with the world).
- Systems are stateless (`&self`). Anything that must survive a save lives in the world.

### Pacing

There are no timers, countdowns or durations anywhere: `wait` waits for a world
condition, and pacing reads activity counters. Every pacing threshold is data (pack
TOML or scripts), never a constant in Rust. The slice's arc is compressed; the engine
must not assume that pace.

## Crate map

Packages are named `galaxy-<name>` and live in `crates/<name>` (`core` would clash with
Rust's own `core`). Only `app` depends on everything; the sim crates never depend on
`render` or audio.

| Crate | Package | Owner | What |
|---|---|---|---|
| `crates/contract` | `galaxy-contract` | W1 | The shared interfaces. Source of truth; schemas generated from it. |
| `crates/core` | `galaxy-core` | W1 | Sim kernel, stores, tick, command buffer, history, events, counters, RNG, data loading, saves, hash. |
| `crates/app` | `galaxy-app` (bin `galaxy`) | W1 started, W6 integrates | The binary: `play` plus the JSON CLI. Thin. |
| `crates/render` | | W6 | Renderer (wgpu, 3D-ready camera). Knows nothing about the sim's internals. |
| `crates/ship` | `galaxy-ship` | W2 | Flight, control tiers, verbs and orders, mining, docking, automated craft. |
| `crates/economy` | `galaxy-economy` | W3 | The living market: pricing and saturation, activity pulses, trades, scouting, map and territory, market levers, telemetry, NPC trade choice. Register `galaxy_economy::EconomySystem` after the ship systems. |
| `crates/progress` | `galaxy-progress` | W4 | Beat runtime, standing and regard, pilot traits, hull ranks, prestige, station services, modifiers. See "Progress". |
| `crates/script` | `galaxy-script` | W5 | Luau host, sandbox, packs, ctx hooks, validator, hot reload, rollback. See "Packs and scripts". |
| `crates/gm` | `galaxy-gm` | W5 | Game-master bridge: live socket, `--live` reads, digest, `gm propose` (sandbox run in child processes), calm-moment staging, inbox, rollback. Agents acting as game master read [GM-AGENTS.md](GM-AGENTS.md). |
| `crates/audio` | `galaxy-audio` | W6 | Sound cues synthesised offline (fundsp) and played through kira; the anti-fatigue rules. See `crates/audio/README.md`. |
| `crates/mind` | | W5 | Fast-model layer on OpenRouter, behind a trait. |
| `xtask` | `xtask` | W1 | `cargo xtask check`, `cargo xtask contract`, `cargo xtask fetch-luau`. |
| `packs/core` | | W4 (+W3 data) | The authored backbone: `manifest.toml`, `data/*.toml`, later scripts. |
| `packs/gm-examples` | | W5 | Game-master example packs; `bad_script/` holds packs `pack check` must reject, `bad_sandbox/` packs only the sandbox catches. |

Contract changes go through W1: change `crates/contract`, run `cargo xtask contract`,
and the schema diff in the PR is what gets reviewed.

## The one write path

Nothing mutates `WorldState` directly except `galaxy_core::apply::apply`. Everything
else (native systems, scripts, fast-model choices, the CLI) submits a
`CommandBatch { cause, commands }`:

- A `Cause` says why: `Verb { actor, verb }`, `Script { pack, revision }`,
  `Mind { npc, decision }`, `System { name }` or `Rollback { of }`, plus a one-line
  `summary` and the fact IDs it `cites`.
- Batches apply at the **end of the tick**, in order: first those submitted between
  ticks (`Sim::submit`), then each system's in system order.
- A batch is **atomic**: if a command fails or a runtime invariant breaks, every command
  in it is undone and the batch is reported in `TickReport::rejected`. Its events and
  facts never happen. Undo restores the captured values without the forward checks, so
  a batch may pass through a dangling reference that only the invariants catch.
- Numbers must be finite: a command holding NaN or infinity is rejected, the
  `numbers_finite` invariant guards the world, authored data with one is a load error,
  and `Save` refuses to write one (JSON can't carry it). Integer adjustments use checked
  maths and build their inverse before changing anything.
- Money is whole centicredits (1 cr = 100, `contract::money`): `Player.credits`, every
  price, limits and credit deltas. Data files write decimal credits and the loader
  converts them. A trade prices each unit, rounds it to a centicredit and pays the sum.
- Each applied command returns an `UndoRecord`. For journaled causes (`Script`, `Mind`)
  the engine keeps them, and `Sim::queue_rollback(causes, summary)` inverts them as new
  batches with a `Rollback` cause, one batch per cause (`Rollback { of: [cause] }`).
  Player verbs and systems aren't journaled: what the player earned stays earned.
- Each cause is rolled back at most once. A committed `Rollback` batch marks its `of`
  causes in `KernelState::rolled_back` (saved); `queue_rollback` skips those and any
  with a rollback already pending, and a batch rolling back a marked cause is rejected.
  A rejected rollback can be queued again.
- Commands are `Reversible` (exact inverse) or `CompensateOnly` (`Emit`, `RecordFact`:
  history can't be unsaid, so a rollback must compensate in-world).
- Batches are worked out from the tick's starting world, so a batch checks what it
  relies on when it applies: `Expect*` commands (`ExpectDocked`, `ExpectShip`,
  `ExpectStanding`, `ExpectFitting`, `ExpectHullRecord`) change nothing and fail if the
  committed world no longer matches; `UpdateOrders` and `UpdateMarketLine` are
  compare-and-set. Never write a whole record from the planning snapshot (prestige
  changes only the hull, `SetShipHull`). Rollback batches leave expectations out.

## How a tick works

```
Sim::step(input)                      (does nothing while paused)
  for each system, in order:
      system.step(ctx)                ctx.world = state at the end of last tick
                                      ctx.events = events committed last tick
                                      ctx.input, ctx.history, ctx.rng(), ctx.submit(batch)
  world.meta.tick += 1
  for each batch: apply all commands -> check invariants -> commit or undo
      commit: record cause, journal undo, for each Emit: append a fact,
              bump counters, publish an EventEnvelope { tick, fact, cause, event }
```

Systems never see each other's writes inside a tick. Code outside the tick (the app,
the game-master bridge) reads events with `Sim::subscribe` / `Sim::drain_events`.

## How to add things

**A system** (your workstream's per-tick logic): implement `galaxy_core::System`
(`name()` + `step(&self, ctx)`), read `ctx.world`, submit batches. Register it with
`Sim::add_system`. Workstream crates depend on `galaxy-core`, so they can't be listed in
`galaxy_core::default_systems()` (the kernel's own, none so far): export a
`systems(..)` function from your crate and add it to `Game::open` in
`crates/app/src/game.rs`, the one composition `play`, `sim run`, `shot` and the tests
use. The name keys its RNG stream; don't rename casually.
`placeholder::PlaceholderFlight` only drives the kernel's own tests.

**Ships** (`galaxy-ship`): `ShipSystem` runs every ship each tick (the flagship from
`PlayerInput`, everyone else from orders) and `RegrowthSystem` regrows rocks from region
activity counters. Each ship's tick is one batch (motion, activity, an `UpdateOrders`
compare-and-set on its orders, verb effects, events and facts), so a mine or trade the
kernel rejects never advances the order; it retries next tick. Every batch from a ship
with orders carries the check, even when the queue looks unchanged. The player's clicks
and presses are written ahead of that batch (`ShipActivity::held_clicks`, `held_press`)
and replayed if it is rejected. Within a tick one ship trades per market line and rock
units are claimed in turn: the player's flagship first, then the others in ID order,
rotated by one each tick so nobody waits for ever. Buy and sell orders
go through the `TradeDesk` trait (`trade` plus a `ceiling` for floors, all money in
centicredits). The game's desk is `galaxy_ship::EconomyDesk` (`crates/ship/src/market.rs`):
it fits the runner's "up to" requests to `galaxy_economy::plan_trade` (a buy is clamped
to the stock, the free hold and what the credits afford), maps the economy's errors one
for one onto `TradeError`, passes the economy's commands through unchanged (they open
with `ExpectDocked` and change the line with a compare-and-set `UpdateMarketLine`), and
takes its ceiling from the top of the line's price band. The app's `Game::open`
registers the kernel's systems, `galaxy_ship::systems(Box::new(EconomyDesk))`,
`galaxy_economy::EconomySystem`, the script host (core, then gm-examples, then the game master's applied packs), then
`galaxy_progress::ProgressSystem`, and pumps `galaxy_mind::MindHost` after each step. `FlatDesk` (fixed prices) and closures are for tests.
Control tiers are per-save flags named in `ship_rules.toml`: read them with
`Controls::of(world)`, set them with `controls::set_capability`. Loops: `controls::{save_loop, run_loop, assign_loop}` build
the batches; `assign_loop` takes the desk to refuse floors above the ceiling. Every
tunable is data (`hulls.toml`, `rocks.toml`, `ship_rules.toml`).

**A command**: add a variant to `contract::command::Command` (Box large payloads to
keep the enum small), add its arm in `galaxy_core::apply::apply` that validates inputs,
makes the change and returns the exact inverse (`UndoRecord::Invert`) or a
`Compensate` note, mark it in `Command::reversibility` if it's compensate-only, add its
`op` to `Command::op`, give it a row in `contract::needs::COMMAND_NEEDS` (what a pack must
declare to cause it, or `Never`), add a round-trip case to `undo_records_invert_commands`
in `crates/core/tests/kernel.rs`, run `cargo xtask contract`.

**An event**: add a variant to `contract::events::Event` with stable-ID fields, a
matching `EventKind` (with its `as_str` name, which is also its fact kind), and fill in
`Event::kind`, `ship`, `subjects`, `amount` and `counter_bumps`. Emit it with
`Command::Emit { event }` inside the batch that makes the change.

**An activity counter**: add a `CounterKind` variant and the bumps in
`Event::counter_bumps`. Counters only count player-owned ships (`Owner::Player`) and
player events, in the player scope plus the station and region where it happened.

**A store** (a new kind of world record): add an ID type with `string_id!` in
`contract/src/ids.rs`, the record type in `contract/src/world.rs` with an `id` field
and a `record!` entry, a `Store<Id, T>` field on `WorldState` (its default must be
empty so older saves still load: `#[serde(default)]`), a `PutX` command and its apply
arm, checks in `invariants::{KeysMatchIds, ReferencesResolve}`, and, if it's authored, a
`data::XFile` plus loading in `galaxy_core::load`. Bump `WORLD_FORMAT_VERSION` only for
incompatible changes.

**A fact kind**: no code needed. Record one with
`Command::RecordFact { fact: Box::new(FactDraft { kind, subjects, amount, note }) }`.
A pack records kinds only in its own namespace, its whole ID
(`gm.kettle_growth.stage_built`; see "Capabilities, one table"). To index a new kind of
subject, add a field to `history::Subjects` and a column plus index in the save's
`history` table (`crates/core/src/save.rs`).

**An invariant**: implement `galaxy_core::invariants::Invariant` and add it to
`invariants::builtin()` (or `Sim::add_invariant` for one run). It runs after every
batch; keep it cheap.

**Authored data**: TOML under `packs/core/data/`, one file per type in
`contract::data::DATA_FILES`, schemas in `crates/contract/schemas/data-*.json`. Unknown
keys and dangling IDs are load errors. The current numbers are provisional.

## Packs and scripts

The live layer is Luau, run by `galaxy-script`. The hook reference is generated:
`crates/contract/api/HOOKS.md`, with types in `crates/contract/api/galaxy.d.luau`.
The worked example is `packs/gm-examples/kettle_growth`.

**A pack** is a folder: `manifest.toml` (schema `crates/contract/schemas/manifest.json`),
`scripts/*.luau`, optional `lib/*.luau` (with `lib = true` in the manifest),
`data/*.toml` (only `npcs.toml` installs so far, and it needs `npcs.write`) and
optional `tests/*.luau`. `require`
loads only from the script roots (`scripts/`, and `lib/` if declared; never `tests/`),
and every check covers exactly those files (`Pack::loadable`). Its revision is a BLAKE3 hash of its files; every write a script makes
is caused by `Cause::Script { pack, revision }`, so a revision can be rolled back. The
manifest lists `uses` (capabilities such as `events.sell`, `stations.write`), each
`[[script]]` with its `handlers` (`on.<event>`, `after.<trigger>`) and typed `stores`,
`[[shock]]`/`[[opportunity]]` pairs, the `[rollback]` line and `[limits]`.

**A script** returns `G.script { id, on = { ... }, after = { ... } }`. Handlers get a
`ctx` built from `uses`: anything undeclared is absent. Rules the host enforces:

- Every write takes the call's one cause: `local why = ctx:cause { summary = ..., cites = { ev.fact } }`
  (a second `ctx:cause` is an error). A handler is atomic: its writes are one batch,
  checked against the kernel's invariants before it's submitted, and an error, a write
  that would be rejected, or running over the step, memory or write quota discards all
  of it. The check (`TickCtx::preview`) runs on the world the kernel will apply the
  batch to: the batches queued ahead of it this tick, and every invariant of the sim.
- Only **script faults** abort: a Luau error, running over the step, memory or write
  quota, an invalid argument, an undeclared hook, or a write the check rejects on the
  world the call read as well. Repeated aborts (`limits.quarantine_after`) quarantine
  the script revision for good (`script_quarantined` fact; reloading the same revision
  doesn't clear it). A write that is valid on the world the call read but loses to a
  change made earlier in the tick (an undock, moved cargo, a compare-and-set or
  `Expect*` check, a record gone) is **contention**: the host tells them apart by
  checking the rejected batch again on that world (`host::check`). Contention never
  counts. A beat transition or trigger tries again next tick; an `on.` handler runs
  again for the same event next tick, up to `limits.contention_retries` times (default
  3), then is dropped with a `script_contention_dropped` fact. Pending retries live in
  `ScriptHealth::retries`, so save and load replay exactly. A retry reads the world as
  it is then: check what the handler relies on instead of letting a hook raise.
- Scripts hold no state in Luau: each call runs the script and its modules afresh, so
  locals and upvalues start over. Remember things in a declared store
  (`ctx.store:set(store, key, value, why)`) or read them back from history.
- Determinism: `tostring`/`string.format` never show addresses, `pairs`/`next` refuse
  tables keyed by tables or functions, and a generic `for` names exactly one
  `pairs(...)`, `ipairs(...)`, `string.gmatch(...)` or `utf8.codes(...)` call, or
  `next, t`, between `in` and `do` (checked on a tokenizer that follows Luau's lexer;
  `for k, v in t` and `pairs(a) and t` are check errors). Scripts are always strict:
  the first line is `--!strict`, and `!nocheck`/`!nonstrict` anywhere is a check error.
- Bounded: each handler call, beat callback and `G.test` predicate runs in a fresh
  Luau state (so memory outcomes don't depend on earlier calls; the pack's bytecode is
  compiled once and shared), native helpers that do O(n) work, world views such as
  `ctx.beats:field` included, charge the step budget (`crate::meter`), and script text leaving the VM (abort messages, `print`) is cut to
  1 KiB with a marker. Known gap: comparing two long strings (`a < b`) is one VM
  opcode and isn't metered; it is bounded only by the memory limit, which caps string
  length.
- Pace by activity only: `G.after { dockings = 3, station = ... }`, `ctx.counters`. There
  is no `os`, timer, `math.random` (use `ctx.rng`) or `loadstring`.
- `ctx.talk:offer`, `ctx.map:note`, `ctx.chronicle:write` need a person or a place and
  `cites` with existing fact IDs.
- Reads see the world as the tick started, plus the writes of handler calls that ran
  earlier this tick (not the current call's own writes).
- A fact a call records exists only once the batch commits: say the line that cites it
  from a later call (an `on.beat_started` handler for an `on_start` callback's facts,
  the next docking for a handler's).
- **Answers at the dock**: a dock-talk line may offer `choices` (`{ id, text }`, at
  most 4, needs a `speaker`). The player's pick (or a standing question such as
  `choice.what_changed`) comes in as `PlayerInput::replies`; the progress system turns
  each into a `reply` event (`on.reply`: `npc`, `station`, `choice`) only while the
  player is docked where that person is. In play the talk panel shows the open
  choices as buttons (keys 1 to 9, `app::hooks::open_choices`), and the dock screen's
  "Ask what changed" sends `choice.what_changed` to the dockmaster: the core pack's
  `talk` script answers (Ilse's b17 homecoming in `act4`) and records
  `core.asked_what_changed`. Ambient talk declares `talk.pick` and calls `ctx.talk:request` with ordered,
  authored candidates filled from cited facts. `MindHost` picks an ID using Jev or
  candidate one as the complete default, and emits `talk_picked` with a
  `Cause::Mind` and `mind.decision` fact. `ctx.talk:selected` rechecks the current
  docking, pack revision, speaker and citation before the script says/retires it.
  Requests and results are saved events; unresolved requests resume on load.
  `galaxy shot --keys` has `:reply:N` and `:reply:CHOICE`.
- `ctx.space` launches scripted craft owned by a person (`put_craft`: a teacher's
  tender), lays out live rock fields (`put_field`: a rumored vein) and rests a field's
  regrowth (`set_regrow`); `ctx.space:craft` reads any non-player craft. Market base
  levers (`set_base` in centicredits, `scale_base`) move a line's base price, so
  standing tender floors see the new ceiling; one that moves it records
  `market_base_changed` (station, ware, amount = the new base). A change that must
  wait for a calm moment calls `ctx.beats:expect_docked(station, why)`: the call's
  writes apply only if the player is still docked there.

**Capabilities, one table**: `uses` bounds everything a pack can change, by every
route, not only the `ctx` hooks. `galaxy_contract::needs::COMMAND_NEEDS` maps every
`Command` to the capabilities a pack needs to cause it (`Expect*` checks need nothing,
the pack's own stores and triggers need nothing, engine-only commands are "never", and
a few depend on their payload: an `emit` by event, a `record_fact` by kind, a `say` by
channel, a buoy by its reward). The script host checks **every** batch a pack causes
against it before submitting it (handler and trigger calls, beat transitions with their
rewards and callbacks, data installs): a command the manifest doesn't allow rejects the
whole batch as a fault (`undeclared_capability`, naming the command and what it lacks).
That is defence in depth on top of the per-hook gating. A survey-buoy reward is
checked when the pack places it (`put_survey_buoy`); the pilot's claim later pays
exactly that. `galaxy pack check` applies the same table to what the pack's data would
install (`npcs.toml` needs `npcs.write`) and its beats' rewards would write (standing
needs `standing.write`). **A new command needs a row** (`every_command_has_a_row`
fails without one), and a new route a pack writes through must call
`host::declared` with its `Route` before it submits.

- **Routes.** A batch is checked by where it came from (`needs::Route`): `Hooks` (a
  handler, trigger or beat callback), `Beat` (the beat runtime's own transition: the
  beat's state, `beat_started` / `quest_step` / `beat_completed`, its rewards) or
  `Data`. A beat transition is checked in two parts, so a callback can't borrow the
  runtime's right to write journey events. `put_beat`, `set_fitting`, non-`mech.`
  flags and the beat events are beat-runtime only.
- **Provenance.** A pack records facts only in its own namespace, `<pack-id>.*` (the
  whole ID: `core.probe` can't write `core.*`, `gm.a` can't write `gm.b.*`). Engine
  events (docking, trades, mining ...) never come from a pack. A fact or event that
  reports a checked change must come right after it in the same batch:
  `<pack-id>.canon_opened` after the `set_canon_status` that opens its node
  (`ctx.canon:open`, gated by `opens`), `<pack-id>.mechanic_taught` after its `mech.*`
  flag (`ctx.mechanics:teach`), `market_base_changed` after its `set_market_base`,
  `deliver` after its `adjust_cargo`, `standing_changed` after its deed's change.
  `ctx.history:record` refuses the protected names (`needs::PROTECTED_FACTS`). So no
  beat precondition can be met by a forged fact (`crates/script/tests/provenance.rs`).
- **Layers.** A pack's layer comes from the path that loads it, never from its ID or
  from it being new. Only the authored backbone (`Game` loads the packs root's `core/`
  with `ScriptHost::load`) is core. Game-master layers (`gm-examples`, applied, staged
  and sandboxed packs: `ScriptHost::load_layer`, `CheckOptions::game_master`) may not
  register or drive journey beats or pay their rewards, whatever their ID, and may not
  take `core`'s namespace or the ID of a pack the world records or the host runs as
  core (`gm_core_id`, `CheckOptions::core_ids`), so they never replace the core VM. A
  reload keeps the folder's layer: a game-master folder whose manifest names a new ID
  is a new game-master layer, never one already loaded (`reload_id_taken`). Each
  pack's layer is recorded in the world (`PackState::layer`, by
  `ScriptHost::record_layers` from `install` and the bridge's apply, as a system batch
  no rollback undoes) and never changes. Every later load reads it: the check
  (`check_pack`) and the host's run (the beat route is refused for a layer the world
  or the host knows), whichever path loads the pack (`reload`, the dev watcher,
  `play`, `sim run`, the bridge, the sandbox).

**Checks**: `galaxy pack check DIR` runs manifest, strict typecheck (`luau-analyze`
against a per-pack `galaxy.d.luau`), hooks, references, invariants and a smoke run, and
prints a `ValidationReport` with codes and file:line locations. Dev hot reload
(`ScriptHost::watch` + `poll_reload` between ticks) runs the same checks and keeps the
last good revision if they fail. `galaxy pack test DIR` runs `tests/*.luau`.

**Rollback**: `galaxy_script::rollback(sim, causes, line)` inverts the causes' writes,
retires their pack revisions, switches their packs off (`PackState::off`: no revision
of the pack runs again in the save, from any source, until the game master applies a new
one, whose apply batch switches it back on) and has the pack's `[rollback]` speaker say so, citing
the change's facts, in dock talk and the chronicle. The retirement is its own batch and
the host stops a revision as soon as it's queued (`TickCtx::queued`), so a rolled-back
revision runs nothing in the tick its change is undone; the lines are checked against
the world the rollback leaves and dropped (never the retirement) if they can't be said.

**The game-master bridge** (`galaxy-gm`): `Bridge::open` loads the packs a save's
game-master folder (`<save>.gm/state.json`) says run into the game's one `ScriptHost`, before
the game attaches it (`Game::open_gm`; `Game::open_with` takes the same hook, and the
sandbox child loads its packs there too). A revision of a pack the game already runs replaces
it, so gm packs run in the same tick order as `core` and `gm-examples`. The app (`galaxy
play`, `galaxy sim run`) calls `Bridge::serve` / `apply_staged` / `after_tick`
between ticks, never inside one. A staged pack applies at a calm moment (docked or
paused, or on load): snapshot, and only once that is written (and the world hasn't moved
since) the fast checks again, then one batch caused by the pack revision that installs
its data and records `gm_change_applied`; that batch's cause is the change ID. A failed
snapshot leaves the pack staged. `state.json` is intent, the save is proof:
`bridge::reconcile` matches it to the save's facts and queued batches on open, so a crash
between apply and save re-applies the pack instead of losing or doubling it. Reads on the
live socket run on a read thread from a copy of the state; the socket's I/O thread is
non-blocking and enforces `live::Limits`. The sandbox step runs `galaxy sim sandbox` as child processes on forks
with `bot::ActivityBot` flying the region's stations, so its length is dockings, never
time. Its thresholds are `SandboxRules`.

**Adding a hook function**: add a `FnDef` to `contract::api::FUNCTIONS` (and a
capability to `USES` if new), add its `method(...)` in `crates/script/src/ctx.rs`, run
`cargo xtask contract`. W4's hooks (`beats.*`, `orders.*`, `mechanics.*`,
`market.levers`, `standing.*`, `traits.read`, `pacing.*`) live in
`crates/script/src/ctx/progress.rs`. Runtime recipe and canon hooks live in `ctx/live.rs`. The reserved capabilities left (
`mind.write`, `telemetry.read`) are the extension points for the other
workstreams: flip `available`, add the functions, and declaring them stops being an
error.

## Progress (W4)

`galaxy-progress` is native Rust; the journey itself is Luau in `packs/core`.

- **Register** `galaxy_progress::ProgressSystem` after the ship and economy systems,
  and load `packs/core` into the `ScriptHost` (it runs the beats). The system turns
  `PlayerInput::services` (fit an upgrade, build a tender, prestige) into one atomic
  batch each, and after anything happens pays faction deeds, earns traits and the
  current hull's next rank. Check a purchase first with `services::check` / `offers`.
- **Beats**: `G.beat { id, act, goal, requires, steps, done, rewards, on_* }` in
  `G.script { beats = { ... } }` (needs `beats.write`). Requirements are data built
  with `G.*` helpers (`G.done`, `G.counter`, `G.fact`, `G.cargo`, `G.test(fn)` ...),
  never time; numbers can be `G.tunable(id)` from `pacing.toml`. State lives in
  `world.progress.beats` by stable ID, so beats survive save, load and hot reload.
  Facts: `beat_started`, `quest_step` (the next step; `quest` is the beat ID),
  `beat_completed`. The HUD's line is `galaxy_progress::beats::hud_goal`. The
  journey is `packs/core/scripts/` (one script per act or situation: `act1` to `act4`,
  `dispute`, `rumor`, `shortage`, `tender`, `fragment`, plus `talk` for ambient dock
  talk); `lib/ids.luau` maps the spec's IDs onto the data's, `lib/lines.luau` holds every
  core NPC line; `data/canon.toml` holds the canon graph and `lib/canon.luau` delegates openings to `ctx.canon`. Worked examples:
  `packs/core/scripts/act1.luau` (b01-b05). A transition (with its
  callback's writes) is checked like a handler batch, against the world the kernel
  will apply it to; only one that will commit is visible to later beats that tick, so
  a successor never unlocks on a completion the kernel would reject. One that loses to
  contention is checked again next tick and never counts toward quarantine. A live `G.test`
  costs about 0.3 ms a tick (a fresh state per call): list data requirements
  ahead of it, since a list stops at the first that fails, and prefer data
  (`G.near(spot, within)` for "went somewhere") where there is one.
- **Data** (all `packs/core/data/`): `upgrades.toml` (upgrades and tenders, costs,
  sellers, unlock requirements, granted `cap.*` flags, modifiers), `controls.toml`
  (capabilities, tiers, reserved slots), `traits.toml`, `[[rank]]` in `hulls.toml`,
  `[[regard]]` / `[[deed]]` / `[standing]` in `factions.toml`, and `pacing.toml`
  (tunables and `[prestige]`). The core loader checks every reference.
- **Effects**: traits, fittings and ranks only ever add up to the fixed `Modifier`
  list; read a ship's totals with `galaxy_progress::modifiers::for_ship` (the fold
  lives in `galaxy_contract::modifiers`, so the kernel's hold invariant, the ship and
  the economy read it without depending on progress). Free hold is always
  `modifiers::hold_capacity(world, ship)`, never the hull's `hold_capacity` alone.
- **Standing** is hidden; show `standing::tier` (regard). Change it only through
  `standing::Deeds` (or `ctx.standing:deed`), which clamps, applies trait bonuses and
  writes `standing_changed` / `regard_changed`. Each deed carries an `ExpectStanding`
  band (the standings it has the same effect from), and `Deeds::after` counts standing
  already written in the batch (a script call's earlier deeds, beat rewards) or queued
  this tick (the native deeds).
- **Purchases** assert what they buy is still unowned, so a request repeated in one tick
  fails without charging. Prestige carries a stripe due that tick into the frozen
  record. Berthed hulls (`Player::is_berthed`) can't be ordered, given loops or flown.
- **Predicates** (`G.test`) read position and depletion through `ctx.beats:pilot()`
  (`position`), `ctx.beats:distance`, `ctx.beats:field` and `ctx.beats:rock`; keep
  their thresholds in `pacing.toml`.
- **Journey tests**: `crates/script/tests/journey_core.rs` plays each act headless with
  only `packs/core`, through `crates/script/tests/common/pilot.rs` (the real systems,
  the script host, the fast-model fake; input, trades, services, replies and orders).
  Its `Setup` seeds focused later-act fixtures: beats done, fittings, flags,
  counters, and lower pacing tunables for long waits. A beat marked done that way has
  no `beat_completed` fact, so a line citing one stays unsaid. The app's
  `harness` instead plays earlier beats through `Game`; `beat jump` saves that real
  history. `crates/app/tests/harness.rs` runs all 17 beats with and without the GM
  examples, plus golden replay. `journey_fixes.rs` holds
  the review regressions. `crates/app/tests/journey.rs` plays the same journey
  through the app's `Game` and `Session` at the keys (Act 1 into b07, a reply).

## IDs

Namespaced, dot-separated `[a-z0-9_]` segments: `station.kettle_dock`, `ware.ice`,
`rock.shallows.ice.03`. Live-created things are namespaced by their pack
(`gm.sixth_station`). Each kind has a newtype (`StationId`, `WareId`, ...) that is
ordered, cheap to clone and borrows as `&str`. Constants:
`const KETTLE: StationId = StationId::from_static("station.kettle_dock");` (checked at
compile time). History facts and causes use sequence IDs, `fact.42` and `cause.42`.

## Saves

One SQLite file per save (`galaxy_core::Save`). Tables: `meta`, `world` (world and
kernel state as JSON), `history` (append-only, indexed by kind, station, ship, other,
cause), `causes`, `journal`, `snapshots`. `Save::write` replaces the world row and
syncs the logs: it keeps the stored rows that match the state and replaces everything
from the first difference, so writing a restored snapshot drops the discarded timeline.
`Save::snapshot` / `load_snapshot` are the hard-restore path. A snapshot is complete
whether or not its state was written: it shares the matching prefix of each log with the
save's tables and holds the rest itself, and `write` moves shared rows into a snapshot
before dropping them, so every listed snapshot stays loadable (checked by a digest).
Fork = `galaxy_core::fork(from, to)` (a file copy). `state_hash` is BLAKE3 over the
canonical JSON of everything a save holds: same build, seed and inputs give the same
hash, and save -> load -> continue gives the same hash as an uninterrupted run.
