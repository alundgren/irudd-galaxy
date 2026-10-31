# Modules and install

How the engine finds modules, how it decides whether it can run them, and how engines are
installed so that game work never builds one. The code is `crates/module` (one crate both
binaries and the launcher use), `crates/launcher` and `xtask/src/install.rs`.

## Why it works this way

The game is an engine that runs modules, and the modules (the base game among them) are
never compiled. People and agents working on modules use an installed engine: they never
build it, so a module worktree needs no Rust toolchain and starts no build. That needs
three things:

1. Engines installed as versioned builds, kept side by side, so a module can rely on the
   version it was written for while newer engines arrive.
2. Modules that say which engine they need, and an engine that refuses a module it is too
   old for, with a message that says how to install a new enough one, before doing
   anything else.
3. One `galaxy` and one `galaxy-gm` on PATH that pick the right installed build by
   themselves.

The alternatives were one installed engine replaced on every install (a module written for
an older engine breaks without warning), and building the engine in each worktree (slow,
and it ties game work to a toolchain). Symlinks for the commands on PATH would need
Developer Mode on Windows, so the launchers are copied executables.

## A module

A module is a folder with a `manifest.toml`:

```toml
id = "example"                 # lowercase letters, digits, - and _; starts with a letter
version = "0.1.0"              # the module's own version
min_engine_version = "0.2.0"   # the oldest engine that can run it

scripts = ["scripts/main.luau"]   # entry scripts (script-host.md)
events = ["opened", "pressed"]    # event kinds it records or takes as input
writes = ["engine.position"]      # other modules' or the engine's component types it writes

[components.door]                 # a component type, example.door
open = "bool"                     # a field and its type
pushes = { type = "int", default = 0 }
```

Field types are `number`, `int`, `bool`, `text` and `id` (an entity ID, or `""` for
none). Component types, event kinds and fields have one-part names; everything a module
names is `<module>.<name>` to everyone else (`world-and-sim.md`). Versions are
`MAJOR.MINOR.PATCH` (`MAJOR.MINOR` means `.0`), compared number by number. Unknown fields
are an error, so typos are caught.

Beside the manifest:

```text
start.toml         optional: the entities a new world starts with
scripts/*.luau     scripts
tests/*.toml       scenario tests (saves-and-replay.md)
```

`start.toml` lists entities by name with their components:

```toml
[entities.front]                       # example.front
door = { open = false }                # this module's component type
"engine.position" = { x = 1.0, y = 2.0 }
```

## The module loader

`crates/module/src/module.rs` reads a module folder into memory once: its `.toml` and
`.luau` files (names starting with `.` are skipped), with line endings made `\n`. The
engine runs, records and hashes only that copy, so a recording can store exactly the files
a session ran. Other files (notes, later assets) are not read yet.

## The version gate

Every command that takes modules runs the gate before any work:

1. Read only `min_engine_version` from each manifest. A manifest written for a newer engine
   can have fields this engine does not know; reading the minimum alone means it is still
   refused for the right reason.
2. Take the highest minimum. If it is newer than this engine, stop with exit code 2 and
   kind `engine_version`, naming both versions and how to install (`exit-codes.md`).
3. Then read each manifest in full; a bad one is exit 4, kind `bad_input`.

The gate only reads files. It never builds or installs; it tells the person what to run.

Which modules a command is about:

| Command | Modules |
|---|---|
| `galaxy-gm check <path>` | the module at `<path>` (the folder or its `manifest.toml`) |
| `galaxy play --modules <dir>`, `galaxy sim run --modules <dir>`, `galaxy sim load <save> --modules <dir>` | `<dir>` if it is a module, else every module folder in it |
| `doctor` (both binaries) | walking up from the working folder: the first folder that is a module, holds modules, or holds `game/modules` |

`play` is not built yet: after the gate passes it exits 1 with kind `not_built`. `check`
loads the module with the modules beside it (manifest, schemas, scripts, start state);
findings exit 5 with kind `findings`. `doctor`
reports this engine, the installed builds, the launchers and the modules it found. The
`sim` commands are in `saves-and-replay.md`.

## The install layout

One per-user folder: `irudd-galaxy/engine` in `%LOCALAPPDATA%` (Windows),
`~/Library/Application Support` (macOS) or `~/.local/share` (Linux).
`GALAXY_INSTALL_ROOT`, an absolute folder, moves it.

```text
<root>/
  bin/                       the launcher folder: put it on PATH
    galaxy, galaxy-gm        launchers (.exe on Windows)
    launcher.toml            which build they came from
  builds/
    0.1.0-<commit>/          one folder per build: version plus 12-character commit
      galaxy, galaxy-gm
      tools/luau/luau-analyze
      galaxy.d.luau          Luau types for module scripts, from the ctx declaration
      build.toml             version, commit, dev, install time
    0.1.0-<commit>-dev/      a --dev build
```

## Installing

From `engine/`:

```sh
cargo xtask install        # a clean engine/ only
cargo xtask install --dev  # a branch build with uncommitted changes
```

It refuses when `engine/` has uncommitted changes, unless `--dev`. It builds `galaxy`,
`galaxy-gm` and the launcher once, with the release profile (thin LTO), in the leased slot
while holding the build permit. The built `galaxy-gm` writes the Luau type file
(`galaxy-gm types`). It assembles the build folder beside the others and moves it into place. Earlier builds stay; installing the same build again replaces it. Then it
prints where the launcher folder is and how to put it on PATH. It never edits PATH.

An engine release is a version bump in `engine/Cargo.toml` merged to `main`, then installed
from `main`. Installing other commits of the same version is allowed; the launcher prefers
the most recently installed one.

A `--dev` build is never chosen by itself. Run it by name:
`galaxy-gm --build 0.1.0-<commit>-dev doctor`.

## The launchers

`galaxy` and `galaxy-gm` in the launcher folder are copies of one small program. It reads
its own name to know which tool it stands for, picks a build, and runs that build's
executable with the same arguments and the same working folder, passing its exit code back.

It picks:

1. `--build <name>` as the first argument: exactly that build. It is taken off the
   arguments.
2. For `galaxy sim replay <recording>`: the build the recording names, if it names one (a
   recording made by an installed build does), so a replay runs on the engine that made
   it. If that build is not installed it exits 2 with kind `not_installed`.
3. Otherwise the newest non-dev build whose version is at least the highest
   `min_engine_version` of the modules the arguments name (the table above). If none is
   new enough, the newest one, whose own gate refuses with the message above.
4. No non-dev build installed: exit 2, kind `not_installed`.

Installing copies the launchers in unless the installed launchers come from a newer engine
version, so installing an older build never replaces newer launchers. A `--dev` install
only adds launchers when there are none. On Windows a running launcher can't be
overwritten, so the old file is renamed aside and removed on a later install.

## Checking on Linux from Windows

`cargo xtask linux-check [--distro <name>]` takes this machine's build permit, runs
`cargo xtask check` inside WSL on the same checkout, and gives the permit back when WSL
finishes. WSL builds in its own shared folder, which stays warm between runs.
