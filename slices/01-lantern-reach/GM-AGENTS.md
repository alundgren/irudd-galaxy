# GM-AGENTS.md: being the game master of a running Lantern Reach

For an AI agent (Claude Code, Codex, ...) on the same PC as the game. You read the world,
look at it with Andreas, plan how it grows, and change it only through **packs**: Luau
scripts that are validated, applied at a calm moment, and can be rolled back in-world.
Mostly you look at things together and plan; real-time changes are the exception.

Code agents changing the engine read [AGENTS.md](AGENTS.md) instead. Pack authoring rules
(the `ctx` API, determinism, legibility) are in AGENTS.md "Packs and scripts" and
[`crates/contract/api/HOOKS.md`](crates/contract/api/HOOKS.md).

## Ground rules

- **Every change is a pack.** No direct writes; the CLI can't make them.
- **Every change is seen.** Writes must come with a dock-talk line, map note or chronicle
  entry that names a person or place and cites a history fact (`fact.N`). The sandbox
  rejects a pack whose writes fire without a line (`silent_change`).
- **No timers.** Pace by activity: `G.after { dockings = 3, station = ... }`. There is no
  clock, and no pacing number belongs in Rust.
- **Shocks come with opportunities** (declared in the manifest).
- **`uses` bounds everything.** Declare every capability your pack's writes need, by
  any route: hooks, beat rewards, buoy rewards, `data/npcs.toml` (`npcs.write`). The
  host rejects a batch with a command your `uses` doesn't allow, and `pack check`
  reports it as `undeclared_capability`. The table is in HOOKS.md, "What each write
  needs". A game-master layer never registers journey beats, whatever its ID, and
  the save remembers that a pack is a game-master layer, so a reload never lifts it.
- **Facts in your own namespace.** Record kinds under your pack's whole ID
  (`gm.my_pack.noticed`), never `core.*` or another pack's. Open canon with
  `ctx.canon:open` and teach with `ctx.mechanics:teach`; they record
  `<pack-id>.canon_opened` / `<pack-id>.mechanic_taught`, which `ctx.history:record`
  refuses.
- **Changes land at calm moments**, when the player is docked or paused. Don't try to make
  them land mid-flight.
- **Ask before a big change.** Show Andreas the digest and a screenshot, say what you'd
  change and why, then propose.

## The CLI

Run from `slices/01-lantern-reach/`. Use the built binary (`target/release/galaxy` after
`play.cmd`/`play.sh`, or `target/debug/galaxy`), or `cargo run -p galaxy-app -- ...`.
Every command prints **one JSON value** on stdout (except `watch`: one JSON object per
line). Exit codes: `0` ok, `1` runtime failure, `2` usage, `4` bad input (no running game,
unknown path or ID, missing save), `5` rejected (the report is on stdout).

Every read works on the running game (`--live`) or on a save file (`--save PATH`):

| Command | What |
|---|---|
| `galaxy gm status --live` | Is a game running, which packs are staged, applied, quarantined. Start here. |
| `galaxy gm digest --live [--since fact.N]` | What changed since a fact, by place and person. Your main read. |
| `galaxy state get --live [--path P]` | The world, or a part: `player`, `stations.station.kettle_dock`, `markets.station.kettle_dock.lines.ware.ice`. |
| `galaxy state query STORE --live [--where F=V]... [--fields a,b] [--limit N]` | Records of one store: `stations`, `npcs`, `ships`, `markets`, `rocks`, `rumors`, `factions`, `regions`, `lanes`, `packs`. `F~text` matches a substring. |
| `galaxy history query --live [--kind K] [--station ID] [--ship ID] [--ware ID] [--region ID] [--since fact.N] [--limit N]` | Facts, oldest first. `--since fact.N` gives facts after N; a bare number is a tick. |
| `galaxy watch --live [--since fact.N] [--count N]` | Live only. Streams events as JSON lines until the game closes (or N items). |
| `galaxy shot --live --out P.png` | A PNG of exactly what the player sees now. Open it and look. |
| `galaxy pack check DIR` / `galaxy pack test DIR` | Fast checks / the pack's own Luau tests. No game needed. |
| `galaxy gm propose DIR --live [--wait]` | Full validation, then stage for the next calm moment. Prints the report. |
| `galaxy gm rollback CHANGE --live` | In-world rollback of `cause.N` or `stage.N`. |

`--live` alone talks to the newest running game; `--live PATH` to the one playing that
save. With `--save PATH` instead, `propose` stages the pack for the next time the save is
loaded (`galaxy play` or `galaxy sim run`), and `rollback` runs one hands-off tick and
writes the save. While a game is playing a save, the save-file write commands refuse
(`game_running`); use `--live`.

## 1. Find the running game

```sh
galaxy gm status --live
```

Exit `4` with `"no running game answered; is galaxy play open?"` means none is running:
ask Andreas to start one (`play.cmd`), or work on a save file. Otherwise you get:

```json
{
  "live": true, "calm": true, "tick": 136,
  "save": ".../saves/lantern-1793323926.sqlite",
  "gm_dir": ".../saves/lantern-1793323926.gm",
  "staged": [], "applied": [], "quarantined": [], "script_trouble": [],
  "inbox": { "dir": ".../saves/gm-inbox", "waiting": [], "incomplete": [], "validating": null }
}
```

How `--live` finds it: the game writes `live.json` into the save's game-master folder
(`<save>.gm/`) and a copy into `.live/` in the per-user save folder
(`%APPDATA%\Irudd\Lantern Reach\data\saves` on Windows, the game's folder under
`~/Library/Application Support` on macOS, the XDG data folder on Linux, or
`$GALAXY_SAVES`). You never need to read those files: the CLI does.

## 2. Read what happened: the digest

(Values in the examples below are illustrative; the shapes are exact.)

```sh
galaxy gm digest --live                # everything (grouped and capped)
galaxy gm digest --live --since fact.42   # since your last read
```

```json
{
  "since": "fact.42", "until": "fact.97", "tick": 18210, "facts": 55,
  "player": { "credits": "212.40", "ship": "ship.player", "region": "region.lantern_shallows",
              "docked_at": "station.kettle_dock", "hold": { "ware.ice": 0 } },
  "places": [
    { "id": "station.kettle_dock", "name": "Kettle Dock", "kind": "station",
      "at": "region.lantern_shallows", "facts": 12,
      "summary": ["dock x3 (fact.51, fact.70, fact.95)", "sell x3, 60 ware.ice (fact.52, fact.71, fact.96)"],
      "kinds": [ { "kind": "sell", "count": 3, "amount": 60, "wares": { "ware.ice": 60 },
                   "first": "fact.52", "last": "fact.96", "cites": ["fact.52", "fact.71", "fact.96"] } ],
      "talk": ["talk.4"] }
  ],
  "people": [ { "id": "npc.odo_fenn", "name": "Odo Fenn", "kind": "person",
                "at": "station.kettle_dock", "facts": 0, "summary": [], "kinds": [], "talk": ["talk.4"] } ],
  "elsewhere": [ { "kind": "session_start", "count": 1, "first": "fact.43", "last": "fact.43", "cites": ["fact.43"] } ],
  "changes": [ { "cause": "cause.88", "tick": 18001, "kind": "script", "pack": "gm.kettle_growth",
                 "revision": "ec0f6ab691be7f57", "summary": "Odo names what the next greenhouse ring needs",
                 "cites": ["fact.95"], "recorded": [], "rolled_back": false } ],
  "talk": [ { "id": "talk.4", "tick": 18001, "channel": "dock_talk", "speaker": "npc.odo_fenn",
              "place": "station.kettle_dock", "text": "Odo's greenhouse needs ...", "cites": ["fact.95"] } ]
}
```

- `places`: one group per station (then regions for facts with no station), per fact
  kind, with the newest fact IDs in `cites` (up to 8). Cite these in your lines.
- `people`: per named person: their facts and the lines they said.
- `changes`: what scripts, fast models and rollbacks did in the span, by cause.
- Keep `until` and pass it as `--since` next time.

For detail, follow up with `history query` (`--kind sell --station station.kettle_dock`)
or `state get --path ...`.

## 3. Look at it together

```sh
galaxy shot --live --out shots/now.png
```

```json
{ "out": ".../shots/now.png", "width": 1600, "height": 950, "tick": 143, "paused": false,
  "map": false, "docked_at": "station.lantern_wharf", "position": { "x": -200.0, "y": 150.0 },
  "goal": "Bring ice to Kettle Dock" }
```

Open the PNG (you can read images) and describe what you see with Andreas: the station, the
dock screen, the goal line, the map if it is open. It is the window's current frame, panels
included. For a save file without a running game: `galaxy shot --save PATH --out P.png`
(add `--map` for the map, `--keys` to fly it first; see README).

To follow the game as it plays:

```sh
galaxy watch --live --since fact.97
```

Each line is one object with a `type`:

| `type` | Fields | When |
|---|---|---|
| `watching` | `ack: { tick, calm }` | First line. |
| `fact` | `fact: { id, tick, kind, cause, subjects, amount?, payload }` | Every new history fact (dock, sell, mine, a pack's own kinds ...). |
| `talk` | `line: { id, tick, line: { channel, speaker?, place?, text, cites, about } }` | A line was said. |
| `calm` | `calm, paused, docked_at, tick` | The player became calm (docked or paused) or left it. |
| `gm` | `what: staged / applied / change / quarantined / rolled_back / inbox_claimed / inbox_result`, `stage`, `pack`, `revision`, `change?` | Game-master changes. |
| `script` | `event: { kind: aborted / quarantined, pack, script, ... }` | A handler was aborted or quarantined. |
| `lagged` | `dropped` | You read too slowly; items were dropped (the game never waits for you). |

## 4. Write and check a pack

A pack is a folder (see AGENTS.md "Packs and scripts"). Copy
`packs/gm-examples/kettle_growth` as a starting point, give it a new `id` (`gm.<name>`),
and keep it out of `packs/core`. Then:

```sh
galaxy pack check my_pack           # manifest, strict typecheck, hooks, references, invariants, smoke
galaxy pack test my_pack            # tests/*.luau
```

`pack check` needs `luau-analyze` for the strict typecheck (`cargo xtask fetch-luau`);
`--no-typecheck` skips it with a warning. Fix every error; the report gives a `code`, a
`message` and a `location: { file, line }`.

## 5. Propose it

```sh
galaxy gm propose my_pack --live --wait
```

The full pipeline, as one report:

1. `manifest`, `types`, `hooks`, `references`, `invariants`, `smoke`: the fast checks,
   against the running world.
2. `canon`: validates manifest `opens` and `asserts` against `core/data/canon.toml`, including opener rights, revealed prerequisites and beat gates. This is structured validation; free prose still needs editorial review.
3. `sandbox`: the game writes a fork of its current state; two `galaxy sim sandbox`
   child processes fly an activity bot round the stations of the player's region for 6
   dockings (`--dockings N`), one with your pack and one without. Compared: prices inside
   their bands (`price_out_of_band`), runaway producers or stock (`runaway_producer`,
   `runaway_stock`), handlers over budget or quarantined (`over_budget`, `quarantined`,
   `handler_aborted`), batches the kernel rejected (`invariant_hit`), and dead ends where
   the loop can't go on with the pack (`dead_end`). The live game never waits for this.
4. `legibility`: from the same runs, your writes came with lines (`silent_change` is an
   error; `nothing_fired` is a warning when the pack did nothing in that much activity).
5. `stage`: the copy is stored by revision in `<save>.gm/packs/` and staged. It applies at
   the next calm moment: a snapshot first, then one batch caused by your pack revision.
   It applies only once the snapshot is written and the world is still the one the
   snapshot holds; if the snapshot fails, the pack stays staged and is tried again at
   the next calm moment (`gm status` shows `"snapshot": "writing"` or `"failed: ..."`
   on the staged entry).

```json
{
  "pack": "gm.kettle_growth", "revision": "ec0f6ab691be7f57",
  "status": { "status": "applied", "change": "cause.8" },
  "steps": [
    { "step": "manifest", "outcome": "passed", "diagnostics": [] },
    { "step": "sandbox", "outcome": "passed", "diagnostics": [
      { "severity": "info", "code": "sandbox_run", "message": "forked the save and flew the activity bot for 6 dockings ..." } ] },
    { "step": "legibility", "outcome": "passed", "diagnostics": [
      { "severity": "info", "code": "lines_fired", "message": "0 writes, 1 dock talk, 0 map notes, 0 chronicle lines in 6 dockings" } ] },
    { "step": "stage", "outcome": "passed", "diagnostics": [
      { "severity": "info", "code": "staged", "message": "staged as stage.1; the player is at a calm moment, so it applies now" },
      { "severity": "info", "code": "applied", "message": "stage.1 applied at a calm moment; roll it back with `galaxy gm rollback cause.8 --live`" } ] }
  ]
}
```

- `status`: `accepted` (staged, not applied yet), `applied` with the `change` ID (with
  `--wait`), or `rejected` (exit 5; the stage step is `skipped`).
- Without `--wait`, the command returns as soon as the pack is staged. `--wait` waits
  (up to `--timeout`, 600 s) for the player to dock or pause; exit 1 `timeout` if they
  don't, and the pack stays staged.
- The **change ID** is the cause of the batch that applied the pack. Everything the
  revision ever writes is rolled back together with it.
- Proposing a new revision of a pack that already runs replaces it at the calm moment.

## 6. Watch it land

```sh
galaxy gm status --live
```

```json
{ "applied": [ { "stage": "stage.1", "pack": "gm.kettle_growth", "revision": "ec0f6ab691be7f57",
                 "name": "Kettle Dock grows from deliveries", "state": "running", "change": "cause.8",
                 "snapshot": 1, "tick": 448, "source": "cli", "dir": ".../packs/gm.kettle_growth/ec0f6ab691be7f57",
                 "quarantined_scripts": [] } ],
  "staged": [], "quarantined": [], "script_trouble": [] }
```

`state` is `applying` (its batch is queued but not committed yet, e.g. still paused), `running`,
`superseded` (a newer revision runs), `rolled_back`, `off` (another change of the same pack
was rolled back) or `not_loaded`. A script that keeps
aborting is quarantined (listed under `quarantined` and `script_trouble`); roll its change
back and fix it. Then read the digest again and take a screenshot when the player is
where the change shows.

## 7. Roll it back

```sh
galaxy gm rollback cause.8 --live      # or stage.1
```

```json
{ "change": "cause.8", "pack": "gm.kettle_growth", "revision": "ec0f6ab691be7f57",
  "causes": ["cause.8", "cause.12"],
  "outcome": { "plan": { "batches": 2, "compensate": ["cause.8: `gm_change_applied` fact was recorded"] },
               "retired": [["gm.kettle_growth", "ec0f6ab691be7f57"]], "cites": ["fact.3"],
               "line": { "channel": "dock_talk", "place": "station.kettle_dock",
                         "text": "We've taken the new ring down to re-seal it. Your ice is still counted, every bit." } },
  "applies": "at the end of the next tick" }
```

Every write the revision made is inverted, the revision is retired (it never runs again,
and proposing the same revision again is quarantined: change it first), the whole pack is
switched off in this save (no revision of it runs, not even the bundled `gm-examples`
copy, after a reload too, until you propose and apply a new revision), and the pack's
`[rollback]` speaker says the line in dock talk and the chronicle. What the player earned
stays earned. Facts and chronicle lines can't be unsaid. A hard restore to the snapshot
is a developer tool (`Save::load_snapshot`), not your normal path.

## Worked example: Kettle Dock grows

`packs/gm-examples/kettle_growth` grows Kettle Dock from the ice and plate delivered there,
and Odo Fenn says so.

```sh
galaxy gm status --live                       # a game is running; note "calm"
galaxy gm digest --live                       # what has the player done at Kettle Dock?
galaxy history query --live --kind sell --station station.kettle_dock
galaxy shot --live --out shots/before.png     # look at Kettle Dock with Andreas
galaxy pack check packs/gm-examples/kettle_growth
galaxy pack test packs/gm-examples/kettle_growth
galaxy gm propose packs/gm-examples/kettle_growth --live --wait
#   -> status "applied", change "cause.8"; Odo Fenn appears at Kettle Dock
galaxy watch --live --count 20                # fly with Andreas; Odo talks after three dockings
galaxy gm digest --live --since fact.3        # Odo's line, citing the docking it followed
galaxy shot --live --out shots/after.png
galaxy gm rollback cause.8 --live             # if it misbehaves: the ring comes down, Odo says why
```

If the game stops between applying a pack and writing the save, nothing is lost or
doubled: on the next load the bridge checks `state.json` against the save, and a pack
whose apply the save doesn't hold is staged again under the same `stage.N` (it applies
at that load, which is a calm moment).

## The inbox (no CLI, no socket)

1. Copy a pack folder into `gm-inbox/` next to the save (the `inbox.dir` in `gm status`),
   say `gm-inbox/my_pack/`.
2. When every file is there, create an empty file `READY` in it
   (`gm-inbox/my_pack/READY`). Write it **last**: until it exists the game leaves the
   folder alone (`gm status` lists it under `inbox.incomplete`), so a half-copied pack is
   never taken. Folders whose name starts with `.` are never looked at, so you can also
   copy into `gm-inbox/.my_pack/`, add `READY`, and rename it to `my_pack/`.

At its next calm moment the running game claims it (moves it into `<save>.gm/inbox/` and
removes `READY`, which isn't part of the pack), validates it in full like `propose`,
writes `gm-inbox/<folder>.result.json` (`{ "entry", "claimed", "stage"?, "report" }`)
and, if accepted, stages it for the calm moment after. One entry at a time, in name
order.

## The socket, for tools that speak it directly

Newline-delimited JSON over a local socket: a named pipe `\\.\pipe\galaxy-<key>` on
Windows, a Unix socket elsewhere. `live.json` gives `endpoint: { kind, name }`, `token`,
`pid`, `save`, `protocol` (1). Only the local user can connect (default pipe security
with remote clients rejected; a 0600 socket), and every request carries the token.

```json
{"id": 1, "token": "<from live.json>", "op": "state.get", "args": {"path": "player"}}
{"id": 1, "ok": true, "result": {"name": "Pilot", "credits": 4000, "...": "..."}}
{"id": 2, "ok": false, "error": {"code": 4, "kind": "bad_input", "message": "no `nope` in the world state"}}
```

Ops: `hello`, `state.get {path?}`, `state.query {store, where[], fields[], limit?}`,
`history.query {kind?, station?, ship?, ware?, region?, npc?, since?, until?, after?, limit?}`,
`gm.digest {since?}`, `gm.status`, `gm.fork {to}` (writes the current state to a new save
file), `gm.stage {dir, report}` (a copy already in `<save>.gm/packs/`, with its accepted
report), `gm.rollback {change}`, `shot {out}`, `watch {since?}`. `watch` answers `ok`, then
sends `{"stream": <id>, "item": {...}}` lines. The game answers between frames and never
waits for a slow client.

Limits, so no client can hold the game up:

- One request at a time per connection, a line of at most 1 MiB. Paths, filters and
  fields are at most 1024 bytes, and a `state.query` takes at most 64 filters or fields
  (`bad_input` otherwise). A reply is at most 32 MiB: ask for less (`path`, `fields`,
  `limit`) if you get the error.
- At most 16 connections and 8 watchers; over that the answer is `busy` (exit 1). Reads
  wait in a short queue; when it's full they are `busy` too: try again.
- The first line (with the token) must arrive within 10 s of connecting, and any line
  within 10 s of its first byte. A connection with nothing in flight that stays quiet
  for 15 minutes is closed: reconnect.
- A client that stops reading for 30 s is disconnected (a watcher's stream ends with
  it). Read your stream promptly; while you are slow, items drop and a `lagged` item
  says how many.
