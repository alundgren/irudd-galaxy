# AGENTS.md: the Lantern Reach slice

Read this before changing anything under `slices/01-lantern-reach/`. Brief: issue
#10. Decisions that override it: the kickoff comment on PR #11. Design:
`design/vertical-slice-lantern-reach.html` and `design/high-level-plan.html`.

## Commands

Run from this folder. In Git Bash on Windows, first `export PATH="$HOME/.cargo/bin:$PATH"`.

```sh
cargo xtask check      # fmt --check, clippy -D warnings, tests. Must pass before a PR.
cargo xtask contract   # regenerate crates/contract/schemas/. Commit the result.
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
| `crates/economy` | | W3 | Pricing and saturation maths, map, scouting, territory. |
| `crates/progress` | | W4 | Beat runtime, standing, traits, prestige. |
| `crates/script`, `gm` | | W5 | Luau host, sandbox, packs, validator, rollback, game-master bridge. |
| `crates/mind` | | W5 | Fast-model layer on OpenRouter, behind a trait. |
| `xtask` | `xtask` | W1 | `cargo xtask check`, `cargo xtask contract`. |
| `packs/core` | | W4 (+W3 data) | The authored backbone: `manifest.toml`, `data/*.toml`, later scripts. |

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
`systems(..)` function from your crate and add it to `game()` in `crates/app/src/main.rs`
so the app and `sim run` use it. The name keys its RNG stream; don't rename casually.
`placeholder::PlaceholderFlight` only drives the kernel's own tests.

**Ships** (`galaxy-ship`): `ShipSystem` runs every ship each tick (the flagship from
`PlayerInput`, everyone else from orders) and `RegrowthSystem` regrows rocks from region
activity counters. Each ship's tick is one batch (motion, activity, an `UpdateOrders`
compare-and-set on its orders, verb effects, events and facts), so a mine or trade the
kernel rejects never advances the order; it retries next tick. Within a tick one ship
trades per market line and rock units are claimed in ship-ID order. Buy and sell orders
go through the `TradeDesk` trait (`trade` plus a `ceiling` for floors): the economy
crate implements it (a closure works too) and the app passes it to
`galaxy_ship::systems(desk)`; `FlatDesk` is the stand-in. Control tiers are per-save
flags named in `ship_rules.toml`: read them with `Controls::of(world)`, set them with
`controls::set_capability`. Loops: `controls::{save_loop, run_loop, assign_loop}` build
the batches; `assign_loop` takes the desk to refuse floors above the ceiling. Every
tunable is data (`hulls.toml`, `rocks.toml`, `ship_rules.toml`).

**A command**: add a variant to `contract::command::Command` (Box large payloads to
keep the enum small), add its arm in `galaxy_core::apply::apply` that validates inputs,
makes the change and returns the exact inverse (`UndoRecord::Invert`) or a
`Compensate` note, mark it in `Command::reversibility` if it's compensate-only, add a
round-trip case to `undo_records_invert_commands` in `crates/core/tests/kernel.rs`,
run `cargo xtask contract`.

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
Pack kinds are namespaced by pack (`gm.kettle.stage_up`). To index a new kind of
subject, add a field to `history::Subjects` and a column plus index in the save's
`history` table (`crates/core/src/save.rs`).

**An invariant**: implement `galaxy_core::invariants::Invariant` and add it to
`invariants::builtin()` (or `Sim::add_invariant` for one run). It runs after every
batch; keep it cheap.

**Authored data**: TOML under `packs/core/data/`, one file per type in
`contract::data::DATA_FILES`, schemas in `crates/contract/schemas/data-*.json`. Unknown
keys and dangling IDs are load errors. The current numbers are provisional.

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
