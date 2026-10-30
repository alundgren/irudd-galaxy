# The Lantern Reach (slice 01)

The first vertical slice of Irudd Galaxy: a calm, solo, top-down space mining and
trading game in Rust. Brief: issue #10, with the decisions in the kickoff comment on
PR #11. Design: `design/vertical-slice-lantern-reach.html` at the repository root.

Status: Act 1 is playable in a window. Under it: the shared contract, the headless sim
kernel with saves, ships (arcade flight, mining and regrowth, docking, lanes, control
tiers, orders, the tender), the living market and map (`crates/economy`) and the
cel-shaded renderer (`crates/render`), the Luau script host (`crates/script`: sandboxed
packs, the hook API, `galaxy pack check`, hot reload, rollback by cause), the fast-model
layer (`crates/mind`) and progress (`crates/progress`): the beat runtime with b01 to b05
in the core pack, standing, traits, hull ranks, prestige and station services. All of
it runs in `galaxy play` and `galaxy sim run` as one composition
(`crates/app/src/game.rs`): the goal line is the active beat's, people speak authored
lines that cite what happened, and the hullwright sells upgrades on the dock screen.

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
| **N** | Mute or unmute the sound (remembered between games). |
| **Mouse wheel**, **-** / **=** | Zoom. |
| **Click** | Fly, mine or dock by clicking, once you've earned those controls (Act 2). Shift queues. Until then clicks do nothing. |

Docked, the dock screen lists the station's market: the price of the next unit, the
stock and what's in your hold. **W/S** (or the arrows) pick a line, **E** trades one,
**Shift+E** sells all of it or buys as many as fit and you can afford; the buttons do
the same. Each price moves a little with every unit, so selling all at once earns less
per unit than spreading it out. **Space** or the Undock button leaves. A trade you ask
for goes ahead of the flagship's own queued orders, which wait their turn.

Below the market are the station's people. The hullwright (Orrin, at the Wharf) lists
what he can fit now, with the price and what it does, or what stands in the way ("Mining
lens costs 600 cr. You have 160 cr."); **Fit** buys it. Tamsin at Kettle Dock builds the
tender once you've earned it, and Orrin's prestige offer opens a card that says what
stays and what resets before you trade up. At a dockmaster, **Ask what changed** gets
up to three things that happened since you were last there, each about a real event.

The talk panel at the bottom left shows what people said since you last docked, with
who and where. It's empty, and hidden, until someone has something to say.

![Orrin's list on the Wharf's dock screen](docs/screenshots/dock-services.png)

The packs: a new game installs `core` (the journey) and `gm-examples` (the game-master
examples). `--no-gm` (or `GALAXY_NO_GM=1`) starts a game with `core` only and runs any
save without them. With `OPENROUTER_API_KEY` set, a fast model picks which authored line
someone says; without it people say their most specific line, and the log says so once.
`--hot-reload` reloads packs as you edit them (development).

The window won't shrink below 960x540. Below 1280x720 the panels scale down to fit
rather than clipping, and a market longer than the screen scrolls with the selection.

Saves are written at calm moments (when you dock, when you pause, when you quit) to a
per-user folder: `%APPDATA%\Irudd\Lantern Reach\data\saves` on Windows,
a `saves` folder for the game under `~/Library/Application Support` on macOS, the XDG
data folder on Linux. Set `GALAXY_SAVES` to use another folder.

Sound is on at a modest volume: a few soft cues for docking, undocking, ore coming
aboard, a full hold and a trade going through. There's nothing while you fly and no
beam tone while you mine. **N** mutes. The volumes (`master`, `ui`, `world`, from 0.0
to 1.0) and the mute live in `sound.toml` in the save folder, which is written the first
time you play. Without an audio device the game just runs silent. The rules the
sounds follow are in [crates/audio/README.md](crates/audio/README.md).

A first loop: undock with Space, fly right into the ice field, hold Space beside a rock
until your hold fills, fly down and right to Kettle Dock (the marker at the screen edge
points the way), drift into its ring, press Space, and sell.

![The dock screen at Lantern Wharf](docs/screenshots/dock.png)

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
| `new --save PATH [--seed N] [--packs DIR] [--no-gm]` | New save from the core pack, installing `gm-examples` unless `--no-gm`. Seed defaults to 1. |
| `sim run --save PATH --ticks N [--dry-run] [--packs DIR] [--no-gm]` | Fast-forward the whole game (scripts, beats, progress; the seeded fake mind, never the network) with hands-off input; prints tick, hash and the packs that ran; writes back unless `--dry-run`. Game-master packs applied to the save run in the same script host, and staged ones apply first. |
| `sim sandbox --save PATH [--pack DIR]... [--dockings N] [--packs DIR]` | One sandbox run (what `gm propose` starts as a child process): the whole game with these packs loaded beside core and gm-examples, the activity bot flying N dockings; prints telemetry and what each pack did. Never writes. |
| `state get (--save PATH \| --live [SAVE]) [--path P]` | The world state, or part of it: dotted (`stations.station.kettle_dock.build`, IDs stay whole) or slash-separated. |
| `state query STORE (--save PATH \| --live) [--where F=V]... [--fields a,b] [--limit N]` | Records of one store matching filters (`F~text` for a substring). |
| `history query (--save PATH \| --live) [--kind K] [--station ID] [--ship ID] [--ware ID] [--region ID] [--since fact.N\|T] [--until T] [--limit N]` | History facts, oldest first. `--since fact.N`: after that fact; a number: from that tick. |
| `watch --live [SAVE] [--since fact.N] [--count N]` | Streams the running game's facts, talk lines, game-master and calm-moment events, one JSON object per line. |
| `gm digest (--save PATH \| --live) [--since fact.N]` | What changed since a fact, grouped by place and person, citing fact IDs. |
| `gm propose DIR (--save PATH \| --live) [--wait] [--dockings N] [--no-typecheck] [--packs DIR]` | Full validation (fast checks, a sandbox run with and without the pack in child processes, legibility), then stage: live, it applies at the next calm moment (docked or paused); on a save, at the next load. The sandbox children get the packs folder: `--packs`, else the running game's own (live), else the default lookup. Prints the report; exit 5 if rejected. |
| `gm status (--save PATH \| --live)` | Applied, staged and quarantined packs, with change IDs. |
| `gm rollback CHANGE (--save PATH \| --live) [--packs DIR]` | In-world rollback of `cause.N` or `stage.N`: inverts the pack revision's writes, retires it, switches its pack off in the save (no revision of it runs again, gm-examples' copy included, until a new one is applied), and its speaker says why. |
| `api dump [--name NAME]` | The contract's JSON Schemas. |
| `play [--save PATH] [--new] [--seed N] [--packs DIR] [--no-gm] [--hot-reload]` | The game in a window (see Play). Prints the save, tick and packs when it closes. |
| `shot --live [SAVE] --out P.png` | What the running game's window shows right now. |
| `shot --save PATH --out P.png [--map] [--size WxH] [--zoom H] [--keys SCRIPT] [--write]` | Renders a save headless with the same scene and panels as play (needs a GPU adapter, no window). `--keys` flies it first with scripted input, e.g. `space,wait*3,d*200,space*240` (undock, fly right, mine); `:pause`, `:map`, `:next`, `:trade`, `:tradeall`, `:buy:upgrade.mining_lens`, `:ask`, `:click:X:Y`, `:qclick:X:Y` and `:mapclick:REGION` work too (`crates/app/src/script.rs`). `--write` keeps the scripted run. |
| `mind try [--packs DIR]` | Live TypeSafe Jev calls on OpenRouter, with latency and cost: a trader choice and a dock-talk pick among authored lines, plus a gpt-oss line if `talk.free_phrasing` is on. Needs `OPENROUTER_API_KEY` in the environment (exit 4 without it). |
| `pack check DIR [--save PATH] [--analyzer PATH] [--no-typecheck]` | Validate a pack: manifest, strict typecheck, hooks, references, invariants, smoke run. Prints the validation report. |
| `pack test DIR` | Run the pack's `tests/*.luau` unit tests. |
| `audio render --out DIR` | Render every take of every sound cue to a WAV in DIR and print its measurements (peak, RMS, spectral centroid, length). Exit 5 if one is outside the design limits. |
| `play --beat N`, `beat jump` | Not implemented yet (exit 3). |

Exit codes: `0` ok, `1` runtime failure, `2` usage error, `3` not implemented yet,
`4` bad input (missing or existing save, invalid data, unknown path or ID, no running game
for `--live`), `5` rejected (`pack check` or `gm propose` rejected the pack, `pack test`
had failures, or `audio render` found a cue outside its limits; the report is on stdout).

```sh
cargo run -p galaxy-app -- pack check packs/gm-examples/kettle_growth
cargo run -p galaxy-app -- pack check packs/gm-examples/bad_script/os_time   # exit 5
cargo run -p galaxy-app -- pack test packs/gm-examples/kettle_growth
```

`pack check` needs `luau-analyze` for the strict typecheck (`cargo xtask fetch-luau`,
or `--analyzer`, or `$GALAXY_LUAU_ANALYZE`); without it, it rejects unless you pass
`--no-typecheck`.

`--live` talks to the running `galaxy play` over a local socket (a named pipe on Windows,
a Unix socket elsewhere): the newest game, or the one playing `--live SAVE`. Only the local
user can connect. A game master's guide, with the JSON shapes, is
[GM-AGENTS.md](GM-AGENTS.md). Each save keeps its game-master state in `<save>.gm/` next
to it, and a running game picks up pack folders dropped into `gm-inbox/` there once
they hold a `READY` file (write it last).

The CLI finds `packs/` through `--packs`, then `$GALAXY_PACKS`, then the nearest
`packs/core` above the working directory or the executable. Saves go wherever you
point `--save`; `saves/` here is git-ignored.

## Layout

```
crates/contract   shared interfaces (IDs, world schema, events, verbs, commands, history, hook API)
crates/contract/schemas   generated JSON Schemas (commit them)
crates/contract/api       generated galaxy.d.luau and HOOKS.md: the Luau hook API (commit them)
crates/core       headless sim kernel, data loading, saves, determinism hash
crates/ship       flight, control tiers, orders and loops, mining, docking, lanes, tenders
crates/economy    the living market: prices, pulses, trades, scouting, map, levers, telemetry
crates/script     the Luau script host: sandbox, packs, ctx hooks, checks, hot reload, rollback
crates/progress   beats, standing and regard, traits, hull ranks, prestige, station services
crates/render     the cel-shaded wgpu renderer and the parametric mesh kit (knows no sim)
crates/audio      five soft cues synthesised offline (fundsp) and played through kira; never required
crates/mind       the fast-model layer: OpenRouter behind a trait, menus, budgets, write-back
crates/gm         the game-master bridge: live socket, digest, propose with a sandbox run, staging, rollback
crates/app        the galaxy binary: play, shot and the JSON CLI
                  session (pause, dock trades), input, view (world to scene), ui, saves
packs/core        the authored backbone: manifest.toml, data/*.toml and scripts/ (the journey's beats)
packs/gm-examples game-master example packs: kettle_growth; bad_script/ and bad_sandbox/ (must be rejected)
tools/            git-ignored: luau-analyze from cargo xtask fetch-luau
xtask             cargo xtask check | contract | fetch-luau
```

Live fast-model tests are ignored by default:
`cargo test -p galaxy-mind --test live -- --ignored --nocapture` (needs the key).

Coding agents: read [AGENTS.md](AGENTS.md). Game-master agents: [GM-AGENTS.md](GM-AGENTS.md).
