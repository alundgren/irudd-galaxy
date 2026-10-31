# AGENTS.md: the game

Read this before changing anything under `game/`, after the root `AGENTS.md`.

These are the GM's instructions: how to build modules with `galaxy-gm` and play them with
`galaxy`. They are generic. Everything about one particular module lives in that module's
own folder, never here. This file is a stub and grows as the GM does.

## What is here

- `modules/`: one folder per module. A module is a folder with a `manifest.toml` (`id`,
  `version`, `min_engine_version`) and, as the engine grows, data, scripts, screens,
  assets and tests.
- `docs/`: the change list and design notes.

Nothing in `game/` is compiled.

## Work on installed tools only

Game work never builds the engine or the GM: no `cargo`, no `cargo xtask`, no `target/`
folder in a game worktree. Use the installed `galaxy` and `galaxy-gm` on PATH (see
`engine/docs/modules-and-install.md` for installing them).

```sh
galaxy-gm doctor                      # the installed engine and the modules here
galaxy-gm check game/modules/<id>     # check a module
galaxy play --modules game/modules    # play (not built yet)
```

Every command prints JSON. The exit codes and error kinds are in
`engine/docs/exit-codes.md`. Exit code 2 with kind `engine_version` means a module needs
a newer engine than the one installed: install it as the message says, or ask for it.
Never work around it.

## When the engine can't do something

A capability the engine or GM lacks becomes a generic engine or GM feature in its own
change, never a workaround in a module. Raise it with the owner or the coordinator.
