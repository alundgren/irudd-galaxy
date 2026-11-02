# Exit codes and errors

`galaxy`, `galaxy-gm` and their launchers share one table of exit codes, so a script or an
agent can tell what happened without reading the message.

| Code | Meaning |
|---|---|
| 0 | OK: the command did what it was asked. |
| 1 | Runtime failure: the command started but failed (I/O, no window or GPU, a drawing failure). |
| 2 | Cannot start: bad usage, an engine too old for the modules, or no installed build to run. |
| 4 | Bad input: a missing or invalid module, manifest, save, recording, inputs file or path. |
| 5 | Rejected: the input was read and judged, and turned down (a check with findings, a failing module test, a replay that ended differently). |

Code 3 is not used.

## The error object

A failed command prints one JSON object on stdout and nothing else there. `kind` tells
cases apart where the code is shared, and `error` is a message for people:

```json
{
  "kind": "engine_version",
  "error": "module demo needs engine 0.2.0 or newer, but this is engine 0.1.0. Install ...",
  "engine": "0.1.0",
  "required": "0.2.0",
  "module": "demo",
  "module_dir": "...",
  "install": "in an engine checkout at a version that is new enough, run `cargo xtask install` in engine/ ..."
}
```

Successful commands print JSON on stdout too, except `--help` and `--version`, which print
text. Messages from the build tools (`cargo xtask`) go to stderr.

## Kinds

| Kind | Code | When | Extra fields |
|---|---|---|---|
| `usage` | 2 | The command line is wrong: no command, an unknown command, missing or extra arguments. | `usage` |
| `engine_version` | 2 | A module needs a newer engine than this one (the version gate). Names both versions and how to install. Nothing is built. | `engine`, `required`, `module`, `module_dir`, `install` |
| `not_installed` | 2 | Launcher only: no installed build to run, or the build named with `--build` is not installed. | `install_root`, `install`, `builds` |
| `bad_input` | 4 | A module folder, its manifest, start state or scripts are missing or invalid; a save, recording or inputs file can't be read or doesn't fit the modules; a path is not a folder. | `path` (sometimes) |
| `findings` | 5 | `galaxy-gm check` found problems in the module. | `module`, `findings` (each with `kind`: `schema`, `script`, `start` or `look`, and `message`) |
| `tests_failed` | 5 | `galaxy-gm test` ran the module's tests and at least one failed. | `module`, `passed`, `failed`, `failures` (a line each), `tests`, `scenarios` (each with `passed` and what failed) |
| `replay_diverged` | 5 | `galaxy sim replay` ended on a different world than the recording did, or `galaxy-gm audit` or `galaxy-gm film` found a note whose world differs. | replay: `tick`, `hash`, `recorded_hash`, `builds`; audit: `notes`, `builds`; film: `out`, `notes_checked`, `builds` |
| `motion_expectations` | 5 | `galaxy-gm motion check` filmed its spec and a tool's value was out of bounds. | `spec`, `film`, `runs` (each tool run once for the same options: its `reading` and `image`), `expectations` (each with `run`, `tool`, `value`, the bounds, `got`, `pass`) |
| `no_game` | 2 | `galaxy-gm live`: no game is running (or none runs the modules a load names, or `--game` names none). Says how to start one. | `games` (when some run) |
| `several_games` | 2 | `galaxy-gm live`: several games run and none is clearly meant; `--game <pid or session>` picks one. | `games` |
| `live_protocol` | 2 | `galaxy-gm live`: the running game speaks another live protocol; names the build to run. | |
| `hot_load_refused` | 5 | `galaxy-gm live load`: a module's scripts don't load or the world doesn't fit it (nothing of that module changed), or an F8 note is open in the game (nothing loaded). | `loaded`, `failed` (each `module`, `error`), `unchanged`, `tick`, `hash` |
| `runtime` | 1 | Anything else that failed after starting: I/O, a build the launcher could not start. | `path` (sometimes) |

The table lives in code in `crates/module/src/failure.rs`; a new kind is added there and
here in the same change.
