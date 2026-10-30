# Irudd Galaxy

A calm solo top-down space mining and trading sandbox.

## Layout

- `game/`: the real game (engine, scripting, content, tools)
- `slices/`: vertical slices, each self-contained
- `prototypes/`: throwaway experiments
- `research/`: research pages
- `design/`: design pages; start at [design/index.html](design/index.html)
- `assets/`: art and audio, stored with Git LFS

Rule: `game/` never depends on `slices/` or `prototypes/`.

Agents (and people working with them): start with [AGENTS.md](AGENTS.md).
