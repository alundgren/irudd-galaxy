# The Lantern Reach (slice 01)

The first vertical slice of Irudd Galaxy: a calm, solo, top-down space mining and
trading game in Rust. Brief: issue #10, with the decisions in the kickoff comment on
PR #11. Design: `design/vertical-slice-lantern-reach.html` at the repository root.

Status: the foundation is in (shared contract, headless sim kernel, saves, JSON CLI),
plus the Luau script host (sandboxed packs, the hook API, `galaxy pack check`, hot
reload, rollback by cause). There is no window or flight yet; `galaxy play` says "not
implemented yet".

## Build and check

You need the pinned toolchain (`rust-toolchain.toml`, installed by rustup on first
use). On Windows, the MSVC Build Tools. Run everything from this folder.

```sh
cargo build                 # everything
cargo test                  # headless tests (no window, GPU or network)
cargo xtask check           # fetch-luau + fmt + clippy -D warnings + tests: run before every PR
cargo xtask contract        # regenerate crates/contract/schemas/ and api/ after a contract change
cargo xtask fetch-luau      # pinned luau-analyze (strict typecheck) into tools/luau/
```

There is no CI. Checks run locally.

## The `galaxy` CLI

Every command prints one JSON value on stdout.

```sh
cargo run -p galaxy-app -- new --save saves/a.sqlite --seed 7
cargo run -p galaxy-app -- sim run --save saves/a.sqlite --ticks 3000
cargo run -p galaxy-app -- state get --save saves/a.sqlite --path stations/station.kettle_dock
cargo run -p galaxy-app -- history query --save saves/a.sqlite --kind dock --limit 20
cargo run -p galaxy-app -- api dump --name world
```

Or build once and call `target/debug/galaxy` directly.

| Command | What it does |
|---|---|
| `new --save PATH [--seed N] [--packs DIR]` | New save from the core pack. Seed defaults to 1. |
| `sim run --save PATH --ticks N [--dry-run]` | Fast-forward headless with hands-off input; prints tick and hash; writes back unless `--dry-run`. |
| `state get --save PATH [--path a/b/c]` | The world state, or a slash-separated part of it. |
| `history query --save PATH [--kind K] [--station ID] [--ship ID] [--ware ID] [--region ID] [--since T] [--until T] [--limit N]` | History facts, oldest first. |
| `api dump [--name NAME]` | The contract's JSON Schemas. |
| `pack check DIR [--save PATH] [--analyzer PATH] [--no-typecheck]` | Validate a pack: manifest, strict typecheck, hooks, references, invariants, smoke run. Prints the validation report. |
| `pack test DIR` | Run the pack's `tests/*.luau` unit tests. |
| `play`, `beat jump`, `gm propose`, `gm rollback`, `shot` | Not implemented yet (exit 3). |

Exit codes: `0` ok, `1` runtime failure, `2` usage error, `3` not implemented yet,
`4` bad input (missing or existing save, invalid data, unknown path or ID), `5` rejected
(`pack check` rejected the pack or `pack test` had failures; the report is on stdout).

```sh
cargo run -p galaxy-app -- pack check packs/gm-examples/kettle_growth
cargo run -p galaxy-app -- pack check packs/gm-examples/bad_script/os_time   # exit 5
cargo run -p galaxy-app -- pack test packs/gm-examples/kettle_growth
```

`pack check` needs `luau-analyze` for the strict typecheck (`cargo xtask fetch-luau`,
or `--analyzer`, or `$GALAXY_LUAU_ANALYZE`); without it, it rejects unless you pass
`--no-typecheck`.

The CLI finds `packs/` through `--packs`, then `$GALAXY_PACKS`, then the nearest
`packs/core` above the working directory or the executable. Saves go wherever you
point `--save`; `saves/` here is git-ignored.

## Layout

```
crates/contract   shared interfaces (IDs, world schema, events, verbs, commands, history, hook API)
crates/contract/schemas   generated JSON Schemas (commit them)
crates/contract/api       generated galaxy.d.luau and HOOKS.md: the Luau hook API (commit them)
crates/core       headless sim kernel, data loading, saves, determinism hash
crates/economy    the living market: prices, pulses, trades, scouting, map, levers, telemetry
crates/script     the Luau script host: sandbox, packs, ctx hooks, checks, hot reload, rollback
crates/progress   beats, standing and regard, traits, hull ranks, prestige, station services
crates/app        the galaxy binary
packs/core        the authored backbone: manifest.toml, data/*.toml and scripts/ (the journey's beats)
packs/gm-examples game-master example packs: kettle_growth, and bad_script/ (must be rejected)
tools/            git-ignored: luau-analyze from cargo xtask fetch-luau
xtask             cargo xtask check | contract | fetch-luau
```

Coding agents: read [AGENTS.md](AGENTS.md).
