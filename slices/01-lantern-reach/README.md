# The Lantern Reach (slice 01)

The first vertical slice of Irudd Galaxy: a calm, solo, top-down space mining and
trading game in Rust. Brief: issue #10, with the decisions in the kickoff comment on
PR #11. Design: `design/vertical-slice-lantern-reach.html` at the repository root.

Status: Act 1 is playable in a window. Under it: the shared contract, the headless sim
kernel with saves, ships (arcade flight, mining and regrowth, docking, lanes, control
tiers, orders, the tender), the living market and map (`crates/economy`) and the
cel-shaded renderer (`crates/render`). Beats, scripts and dock talk aren't wired in yet:
the goal line is a placeholder that follows Act 1 (ice to Kettle Dock, then iron ore at
the Wharf).

![Mining ice in the Lantern Shallows](docs/screenshots/mining.png)

## Play

```sh
./play.sh            # macOS and Linux
play.cmd             # Windows
```

Both build in release (thin LTO) and run `galaxy play`, which continues your latest save
or starts a new game. Arguments pass through: `play.cmd --new` starts over (the old save
stays), `--seed N` picks the new game's seed, `--save PATH` plays a particular file.

| Key | Does |
|---|---|
| **W A S D** | Fly. The ship stops when you let go. |
| **Space** | The one thing in reach: dock in a ring, undock, ride a lantern current, or hold it by a rock to mine. A hint at the bottom says which. |
| **P** or **Esc** | Pause. The whole galaxy stops under the PAUSED card. |
| **M** | The map: regions, lanes, who holds what, and the prices you've seen, dated by your dockings ("?" where you haven't been). |
| **Mouse wheel**, **-** / **=** | Zoom. |
| **Click** | Fly, mine or dock by clicking, once you've earned those controls (Act 2). Shift queues. Until then clicks do nothing. |

Docked, the dock screen lists the station's market: the price of the next unit, the
stock and what's in your hold. **W/S** (or the arrows) pick a line, **E** trades one,
**Shift+E** sells all of it or buys as many as fit and you can afford; the buttons do
the same. Each price moves a little with every unit, so selling all at once earns less
per unit than spreading it out. **Space** or the Undock button leaves.

Saves are written at calm moments (when you dock, when you pause, when you quit) to a
per-user folder: `%APPDATA%\Irudd\Lantern Reach\data\saves` on Windows,
a `saves` folder for the game under `~/Library/Application Support` on macOS, the XDG
data folder on Linux. Set `GALAXY_SAVES` to use another folder. The game is silent for now.

A first loop: undock with Space, fly right into the ice field, hold Space beside a rock
until your hold fills, fly down and right to Kettle Dock (the marker at the screen edge
points the way), drift into its ring, press Space, and sell.

![The dock screen at Kettle Dock](docs/screenshots/dock.png)

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
| `play [--save PATH] [--new] [--seed N] [--packs DIR]` | The game in a window (see Play). Prints the save and tick when it closes. |
| `shot --save PATH --out P.png [--map] [--size WxH] [--zoom H] [--keys SCRIPT] [--write]` | Renders a save headless with the same scene and panels as play (needs a GPU adapter, no window). `--keys` flies it first with scripted input, e.g. `space,wait*3,d*200,space*240` (undock, fly right, mine); `:pause`, `:map`, `:next`, `:trade` and `:tradeall` work too (`crates/app/src/script.rs`). `--write` keeps the scripted run. |
| `play --beat N`, `beat jump`, `pack check`, `gm propose`, `gm rollback` | Not implemented yet (exit 3). |

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
crates/render     the cel-shaded wgpu renderer and the parametric mesh kit (knows no sim)
crates/audio      a silent kira wrapper; a missing audio device never stops the game
crates/app        the galaxy binary: play, shot and the JSON CLI
                  session (pause, dock trades), input, view (world to scene), ui, saves
packs/core        the authored backbone: manifest.toml and data/*.toml
xtask             cargo xtask check | contract
```

Coding agents: read [AGENTS.md](AGENTS.md).
