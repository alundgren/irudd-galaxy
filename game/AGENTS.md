# AGENTS.md: the game

Read this before changing anything under `game/`, after the root `AGENTS.md`.

These are generic GM working instructions. Learn each module's design from its own
README and files; keep its rules and names there, never in these instructions.

## What is here

- `modules/`: the runtime module collection, loaded together by `galaxy play --modules
  game/modules`. Its modules must compose into one game.
- `examples/`: standalone examples, checked, tested and played separately. Quiet
  Garden lives at `game/examples/quiet-garden`; it has its own player and scene style.
- `docs/`: the change list, design notes and the direction doc (`docs/direction.md`).
  Every change follows the direction doc; compare its screenshots side by side with the
  doc's reference images.

Nothing in `game/` is compiled.

## Work on installed tools only

Game work never builds the engine or the GM: no `cargo`, no `cargo xtask`, no `target/`
folder in a game worktree. Use the installed `galaxy` and `galaxy-gm` on PATH.

Start with `galaxy-gm guide` and `galaxy --help`. The installed guide is the source of
truth for commands, formats and workflows, versioned with the tools. Follow its topic
guides and command help rather than copying command recipes into this file. Use its
module, field and script API references when authoring; do not guess their schemas.
Read validation diagnostics as well as exit codes when an authored file is refused.
Check which modules a command loads: when proving a module is self-contained, also
check and test an isolated copy without sibling modules.

Use doctor to confirm the installed build and module compatibility. If a module needs
a newer engine, ask the owner or coordinator to provide a compatible installed build;
include the reported installation guidance. Never build it during module work or lower
the module's minimum version to bypass the gate.

## Make each change playable

Work in small increments that a player can try. For each change:

- Run the GM's module check and test commands. Include meaningful module tests and
  scenario runs that exercise the changed behaviour, not just a successful load.
- Play the changed journey in `galaxy`. Inspect a screenshot or a short bot run as
  well as the test results; a change is not finished until it has been seen and played.
  Measure the pacing or motion the change is meant to improve.
- Follow the play and notes guides for F8 notes. Read the notes inbox before choosing
  work and at status updates. Audit a note's recording to reproduce the problem,
  then record how it was handled only after verifying the result.
- Use films and the motion tools when a still image cannot explain movement. Use
  what-if to compare a module edit against recorded play, and preserve useful
  expectations as tests.
- Follow the live guide to read a running game's state, history and view, and to load
  changed modules. Verify the loaded result; a successful load alone is not a playtest.

Keep evidence with the change: what was played, test and scenario results, and the
relevant screenshots, recordings or measurements. Remove resolved items from change
lists; keep only open work. Keep generated session output out of module source.

## When the engine can't do something

First consult the installed guide, topic help and API references. If the supported
tools cannot express the required behaviour, stop that part of the work and raise a
generic engine or GM feature with the owner or coordinator. Do not edit engine code,
build tools, invent unsupported APIs or hide the missing capability in a workaround.

Describe the player-visible need, the smallest reproducing module or recording, the
installed build, what you tried and the missing generic capability. Include an
observable acceptance check and explain which module work is blocked. Keep module
names and design rules out of the proposed engine API. Continue independent work;
resume the blocked part once the feature is installed and documented in the guide.
