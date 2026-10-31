# Irudd Galaxy

A calm solo top-down space mining and trading sandbox.

## Layout

- `engine/`: the engine, the game binary `galaxy` and the GM `galaxy-gm` (everything compiled)
- `game/`: the game as modules, never compiled; `game/modules/core` is the base game
- `slices/`: vertical slices, each self-contained; `01-lantern-reach` is frozen as a reference
- `prototypes/`: throwaway experiments
- `research/`: research pages
- `design/`: design pages; start at [design/index.html](design/index.html)
- `assets/`: art and audio, stored with Git LFS

Rule: `engine/` and `game/` never depend on `slices/` or `prototypes/`.

Agents (and people working with them): start with [AGENTS.md](AGENTS.md).
