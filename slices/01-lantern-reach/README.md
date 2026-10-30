# The Lantern Reach (slice 01)

The first vertical slice of Irudd Galaxy: a calm, solo, top-down space mining and
trading game in Rust. Brief: issue #10, with the decisions in the kickoff comment on
PR #11. Design: `design/vertical-slice-lantern-reach.html` at the repository root.

Status: the foundation is in (shared contract, headless sim kernel, saves, JSON CLI),
plus headless ships: arcade flight, the context action, mining and regrowth, docking,
lanes, control tiers, the order runner and the automated tender (`crates/ship`). There
is no window yet; `galaxy play` says "not implemented yet".

## Build and check

You need the pinned toolchain (`rust-toolchain.toml`, installed by rustup on first
use). On Windows, the MSVC Build Tools. Run everything from this folder.

```sh
cargo build                 # everything
cargo test                  # headless tests (no window, GPU or network)
cargo xtask check           # fmt + clippy -D warnings + tests: run before every PR
cargo xtask contract        # regenerate crates/contract/schemas/ after a contract change
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
| `mind try [--packs DIR]` | Live TypeSafe Jev calls on OpenRouter, with latency and cost: a trader choice and a dock-talk pick among authored lines, plus a gpt-oss line if `talk.free_phrasing` is on. Needs `OPENROUTER_API_KEY` in the environment (exit 4 without it). |
| `play`, `beat jump`, `pack check`, `gm propose`, `gm rollback`, `shot` | Not implemented yet (exit 3). |

Exit codes: `0` ok, `1` runtime failure, `2` usage error, `3` not implemented yet,
`4` bad input (missing or existing save, invalid data, unknown path or ID).

The CLI finds `packs/` through `--packs`, then `$GALAXY_PACKS`, then the nearest
`packs/core` above the working directory or the executable. Saves go wherever you
point `--save`; `saves/` here is git-ignored.

## Layout

```
crates/contract   shared interfaces (IDs, world schema, events, verbs, commands, history)
crates/contract/schemas   generated JSON Schemas (commit them)
crates/core       headless sim kernel, data loading, saves, determinism hash
crates/ship       flight, control tiers, orders and loops, mining, docking, lanes, tenders
crates/economy    the living market: prices, pulses, trades, scouting, map, levers, telemetry
crates/app        the galaxy binary
crates/mind       the fast-model layer: OpenRouter behind a trait, menus, budgets, write-back
packs/core        the authored backbone: manifest.toml and data/*.toml
xtask             cargo xtask check | contract
```

Live fast-model tests are ignored by default:
`cargo test -p galaxy-mind --test live -- --ignored --nocapture` (needs the key).

Coding agents: read [AGENTS.md](AGENTS.md).
