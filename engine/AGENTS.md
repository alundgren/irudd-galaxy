# AGENTS.md: the engine

Read this before changing anything under `engine/`, after the root `AGENTS.md` (phase,
what blocks a merge, the boundary rule, builds, platforms).

`engine/` is one Cargo workspace holding everything compiled: the engine crates, the game
binary `galaxy`, the GM binary `galaxy-gm`, their launcher and `xtask`. The game itself
lives in `game/` as modules and is never compiled.

## Commands

Run from this folder, on any OS. Windows shell notes are under "Platforms" in the root
`AGENTS.md`.

```sh
cargo xtask check                     # fmt --check, clippy -D warnings, tests. Must pass before a PR.
cargo xtask linux-check               # the same check in WSL, holding this machine's build permit (Windows)
cargo xtask cargo test -p galaxy-module   # any cargo command, in the shared target folder
cargo xtask build [--release]         # galaxy, galaxy-gm and the launcher
cargo xtask run [--gm] -- <args>      # build, then run galaxy (or galaxy-gm) from the slot
cargo xtask install [--dev]           # install a versioned build and the launchers
cargo xtask fetch-luau                # pinned luau-analyze into tools/luau/ (git-ignored)
cargo xtask target-dir                # the leased slot builds go to
cargo xtask loc --base origin/main    # code lines per crate, and the change since main
```

**Seeing motion.** One screenshot can't show how something moves. `galaxy film` (or
`galaxy-gm film` for a played session) films it frame by frame, and `galaxy-gm motion`
answers questions about the film; `docs/motion-tools.md` says which tool fits which
question.

This xtask is a copy of the slice's. It uses the same shared folder, slots and build
permit, so engine and slice builds queue for each other. Never plain `cargo build` or
`cargo test`, never cold or uncached builds, no build measurements.

## Rules

- **The engine knows no game.** It builds and passes its tests without `game/`. Engine and
  GM source never refer to anything a `game/` module defines: no `core.` IDs and no code
  for one particular ship, station or ware. Ordinary words such as `deck` or `target` are
  fine. Tests use generic modules made in the test, under `tests/modules` (headless) or
  under `tests/play` (drawn and played). Reviewers reject an engine PR that names
  something from `game/`.
- **The boundary serves the game.** See the boundary rule in the root `AGENTS.md`: when a
  rule here would block a feature the owner wants or make the game worse, stop and raise
  it.
- **Learning is a goal.** We write the engine's systems ourselves and use libraries only
  for plumbing (windowing, GPU, audio output, font shaping, file formats, the Luau VM).
- **One note per system.** `docs/` has a "how it works" note for each engine system: how
  it works, the alternatives and why this design. A PR that changes a system updates its
  note. Current notes: `docs/world-and-sim.md`, `docs/saves-and-replay.md`,
  `docs/script-host.md`, `docs/modules-and-install.md`, `docs/renderer-and-style.md`,
  `docs/flight-and-collision.md`, `docs/motion-tools.md`, `docs/exit-codes.md`,
  `docs/screens.md`, `docs/places-and-beings.md`.
- **Exit codes and error kinds** are one table for both binaries, kept in
  `crates/module/src/failure.rs` and `docs/exit-codes.md` together.
- **Determinism.** The simulation is deterministic: fixed tick, seeded randomness,
  portable maths (`libm` or plain Rust, never `f32::sin` and friends), recorded inputs.
  `crates/galaxy/tests/sim.rs` holds the world hash a scripted session must end with on
  every OS; change it only on purpose, and say so in the PR.
- **Per-frame work never runs in Luau.** Scripts run on events and the slow cadence;
  per-frame work is engine services in Rust.
- **Tests stay headless**: no window, GPU, audio device or network. Tests that need one go
  behind `#[ignore]`.
- **Dependencies** live in `[workspace.dependencies]` in `Cargo.toml`; crates use
  `dep.workspace = true`. Third-party code must be well known and MIT-compatible.
- **Never fat LTO.** The release profile uses thin LTO.
- **Bin targets are named `engine-*`** (`engine-galaxy`, `engine-galaxy-gm`,
  `engine-launcher`, `engine-xtask`) while the slice shares the build slots. Cargo names
  binaries without a hash, so a name the slice also uses would let one workspace silently
  overwrite the other's program. `cargo xtask install` copies them as `galaxy` and
  `galaxy-gm`. A new bin or example takes a name no slice bin or example uses. The names
  go back once the slice is deleted.

## Releases and installs

- An engine release is a version bump in `Cargo.toml` (`[workspace.package] version`)
  merged to `main`, then `cargo xtask install` from `main`.
- Module work uses installed builds only. When a module needs a newer engine, it raises
  its `min_engine_version` in the same PR that starts using the new feature, after that
  engine is released.
- `cargo xtask install --dev` installs a branch build that runs only when named
  (`galaxy-gm --build <name> ...`).

## Platforms

Each engine change is checked on Linux as well as Windows: `cargo xtask linux-check` runs
the check in WSL with WSL's own warm build folder, only while holding the Windows build
permit, so it never runs beside another heavy build. Docker works too, with a persistent
mounted target folder. The PR names every platform it did not check.
