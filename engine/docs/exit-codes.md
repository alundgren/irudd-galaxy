# Exit codes and errors

`galaxy`, `galaxy-gm` and their launchers share one table of exit codes, so a script or an
agent can tell what happened without reading the message.

| Code | Meaning |
|---|---|
| 0 | OK: the command did what it was asked. |
| 1 | Runtime failure: the command started but failed (I/O, a command that is not built yet). |
| 2 | Cannot start: bad usage, an engine too old for the modules, or no installed build to run. |
| 4 | Bad input: a missing or invalid module, manifest, save, recording, inputs file or path. |
| 5 | Rejected: the input was read and judged, and turned down (a check with findings, a replay that ended differently). |

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
| `findings` | 5 | `galaxy-gm check` found problems in the module. | `module`, `findings` (each with `kind`: `schema`, `script`, `start` or `id_in_engine`, and `message`) |
| `replay_diverged` | 5 | `galaxy sim replay` ended on a different world than the recording did. | `tick`, `hash`, `recorded_hash`, `recorded_build`, `this_build` |
| `not_built` | 1 | The command exists in the plan but not yet in this engine. It still ran the version gate first, and the modules passed. | `command`, `engine`, `modules` |
| `runtime` | 1 | Anything else that failed after starting: I/O, a build the launcher could not start. | `path` (sometimes) |

The table lives in code in `crates/module/src/failure.rs`; a new kind is added there and
here in the same change.
