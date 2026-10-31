# AGENTS.md

Rules for every agent working in this repo, whether one agent on a small change or an
orchestrator running many. Folders add their own (`engine/AGENTS.md`, `game/AGENTS.md`);
where a folder's rules are stricter, they win. A task's brief can override anything here
except "Public repo" below.

## Where things stand

| Folder | Phase |
|---|---|
| `game/` (with `engine/`, which serves it) | prototype |
| `slices/01-lantern-reach/` | prototype, frozen |

Phases: **prototype** (slices: a test of an idea), **production** (`game/`, building the
real game), **released** (`game/` once players have a public build). `prototypes/` are
throwaway: they only have to run. `engine/` has the phase of `game/`. Change the phase
when the game moves on. The phase decides what blocks a merge (below). Nothing else in
this file changes with it.

The slice `slices/01-lantern-reach/` is frozen as a reference: no changes, except to keep
it building through its own `cargo xtask`. It is deleted later, with the owner's OK.

## The two folders

- **`engine/`** is one Cargo workspace holding everything compiled: the engine crates, the
  game binary `galaxy`, the GM binary `galaxy-gm`, `xtask`, and `engine/docs` with one
  "how it works" note per system. Rules: `engine/AGENTS.md`.
- **`game/`** is never compiled. It holds the game as modules: `game/modules/core` is the
  base game, written through the GM. `game/AGENTS.md` is the GM's instructions;
  `game/docs` the change list and design notes.

Game work uses the installed `galaxy` and `galaxy-gm` and never builds them. Engines
install as versioned builds side by side, and a module names the oldest engine it runs on
(`engine/docs/modules-and-install.md`).

A PR changes `engine/` or `game/`; try very hard never to change both. A breaking engine
change carries its mechanical core fix in the same PR and says so; anything beyond a
mechanical fix goes through the GM in its own PR.

**The boundary rule.** The engine boundary (no game nouns in the engine, and the other
engine rules) serves the game. When a boundary rule would block a feature the owner wants,
or make the game worse, stop and raise it with the owner. Never bend it silently, and
never keep it silently at the game's expense.

## Principles

- **Playable increments.** Every piece of work ends in something a player can try. Build
  the thinnest loop that plays end to end first, then widen it. Before calling work done,
  look at the result yourself: a screenshot, a bot run or a short play, not only tests.
- **Player feedback first.** Playtest feedback outranks planned work. On a long job,
  whoever coordinates watches the feedback channel the brief names (often a PR's
  comments, which send no notification) from the first install on, and checks it at
  every merge and status update.
- **Measure what the player feels.** Pacing, frame time, load time: whatever the work is
  about is a number a test or bot reports from the first build, so drift shows early.
- **Effort in proportion.** Review, hardening and polish scale with what a mistake costs
  in the folder's phase.
- **Docs shrink as work lands.** Change lists, known-issue pages and notes list only what
  is still open. The change that fixes an item deletes it from the doc in the same PR,
  with no "done" markers or "fixed" sections. A doc that no longer has open items is
  deleted.

## What blocks a merge

| | prototype | production | released |
|---|---|---|---|
| Crash in normal play, a blocked journey or progression, a failing `check` | blocks | blocks | blocks |
| Broken contract between crates, including sim determinism | blocks | blocks | blocks |
| A secret reaching a log, save, report, crash dump or post on a path the game uses | blocks | blocks | blocks |
| Saves from the current build losing data or failing to load | blocks | blocks | blocks |
| Saves from older builds | may be refused with a clear message; no migrations needed | saves from the last tagged build load, with a test on a stored save | saves from every released version load |
| Packs, mods and scripts | written by us and our agents: guard against mistakes (bad data, runaway scripts, wrong namespace), not attackers | same | once players share packs, treat them as hostile: sandbox escapes block |
| Performance | only if it makes the game unplayable | past the scene's budget | past the budget on the minimum target machine |

Use the table and agreed requirements to distinguish real defects from speculative
hardening, edge-case limits, contrived attacks and style preferences. A real defect in
the work gets fixed and validated before the work is called done, however small it is.
A concern that is not a real defect is dropped. There is no category of accepted defects
to leave behind as "non-blocking findings", and no follow-up issue queue for review
findings. Filing an issue is not a substitute for finishing the work.

A merge blocker is work for the author, not automatically a decision for the owner.
Fix it within the agreed scope and validate it. Ask the owner only when resolution needs
a product or scope decision, missing access, or an action not already authorized. State
that decision precisely; keep independent authorized work moving.

## Public repo

- No local absolute paths, machine names, usernames, emails or secrets in committed files,
  issues, PRs or comments. Keys come from the environment only.
- Git and `gh` directly. No CI: GitHub Actions are only for releases, and checks run
  locally.
- Only the owner merges into `main`, or an agent the owner's brief explicitly authorizes
  to merge (for example the coordinator of an overnight run).

## Multi-agent work

### Order

1. One agent builds the spine: the shared contract and the thinnest playable loop.
   Install it for playtesting.
2. Then fan out, but only onto parts whose interfaces have landed. When two crates
   depend on each other, land the lower one first.
3. One owner per file. Shared files (content tables, golden files, `AGENTS.md`) are
   edited by one agent, or batched into an integration pass.

A long job works on an integration branch and opens PRs into it. Batch content and
tuning changes into fewer PRs.

### Reviews

Every code PR gets a first review from an agent that didn't write it. Write review
instructions from this template, filling in the brackets:

```text
Review PR #<n>: <one line on what it does>. <Folder> is in phase <phase>. Apply the
"What blocks a merge" table in the root AGENTS.md and be proportionate for a
single-player game in that phase.
Verdict: APPROVE (no real defects remain) or CHANGES REQUESTED (real defects to fix,
each with a reproduction or failing test). Drop speculative concerns and preferences;
do not turn them into findings or follow-up issues.
For each defect, name the broken requirement and explain how supported use or a
plausible authoring mistake reaches it. A synthetic reproduction can prove a real bug;
unusual syntax alone does not prove an attack. Apply the phase's threat model, and do
not expand the review into a search for deliberate bypasses.
The author reported `check` passed at <sha>. If HEAD is that commit, don't rerun it:
run the tests for what you probe (`cargo xtask cargo test -p <crate>`).
```

A re-check covers only the findings it lists and the fixer's "new behaviour" list. It is
not a new review: no "final sweep", no attack lists. After two re-checks the coordinator
ends the delegated review loop, assesses remaining findings against the merge table,
and owns completing the remaining real fixes with appropriate verification and the
required `check`. The round limit stops review churn, not the work of fixing defects.
Merge only with authorization; otherwise hand back the ready PR.

### Fixes

- One job, one purpose. Merging the integration branch into a PR is its own step, with
  its own check, never part of a fix.
- Fix real defects, including minor ones. Drop concerns that are not defects. Do not
  defer a real fix into an issue or hand it back to the owner as code triage.
- Give constraints, not designs: "the save format stays compatible", not "use
  compare-and-set".
- Keep fixes proportional to the demonstrated failure and agreed requirements. Do not
  add a new validation subsystem just to satisfy an expanding set of review probes.
- Before pushing, the fixer reviews its own diff, runs `check`, and posts on the PR
  "`check` passed at `<sha>`" and "New behaviour this fix introduces: ...".

### Builds

`cargo xtask check` is the merge gate. Run it from `engine/` for the engine; the slice has
its own copy of xtask in its folder, and both share the folders, slots and permit below.
Run it before asking for review, and again before merging if the branch changed since.
A PR that only changes `game/` or docs builds nothing. Reviewers skip it when the commit
matches a reported pass. Never enable fat LTO; thin LTO or none.

Cold builds are the slowest part of a review, and parallel builds starve the machine.
`cargo xtask` handles both by default; nothing needs setting up:
- **Shared folders.** It builds outside the worktree, under a per-user cache folder
  (`irudd-galaxy/target` in the OS cache folder; `cargo xtask target-dir` prints the
  one in use). Set `GALAXY_TARGET_ROOT` to an absolute folder to put them elsewhere,
  such as a fast drive. It moves the shared folders; it can't turn them off.
- **Slots.** Each run leases one of a small pool of folders (`slot-1` to `slot-4`) and
  holds it until it exits, so one agent uses a slot at a time. It takes the slot its
  worktree used last, else its branch's, else the most recently used free one. A new
  branch or worktree therefore starts with the dependencies compiled and rebuilds only
  our own crates. `GALAXY_TARGET_SLOTS` changes the pool size; each slot is several GB.
- **One heavy build at a time.** Commands that compile (`build`, `run`'s build, `check`,
  `install`, `linux-check`, and `cargo xtask cargo` with `build`, `test`, `clippy` and the
  like) first wait for a machine-wide build permit and print who holds it. `run` gives the
  permit back once the game is built but keeps its slot while the game runs.
  `GALAXY_BUILD_PERMITS=2` allows two at once and sets `CARGO_BUILD_JOBS` to
  cores / permits; `0` turns the wait off.
- Locks are OS file locks: a crashed or killed run releases its slot and permit.

Notes:
- Run targeted commands through it, `cargo xtask cargo test -p <crate>`, never plain
  `cargo`: that builds cold into the worktree's `target/` and skips the queue.
- Parallel agents queue for the permit: a second build waits for the first instead of
  both running at half speed.
- `GALAXY_TARGET_KEY=<name>` pins a folder of that name under the root instead of a
  slot (still one run at a time).
- There is no opt-out. `cargo xtask` ignores `CARGO_TARGET_DIR`, and a
  `GALAXY_TARGET_ROOT` that isn't an absolute folder falls back to the default.
- Never turn off, bypass or opt out of the shared build cache (the shared folders or the
  machine's compiler cache), and never run cold or uncached builds, for any reason: no
  fresh or emptied target folders, no deleting slots, no `cargo clean`. No build
  measurements.
- `cargo xtask loc [--base <ref>]` counts code lines per crate.

## Platforms

Development happens on Windows, Linux and macOS. Everything in this file applies to all
three unless it sits under a platform heading.

**All platforms**

- Run checks through `cargo xtask`, so the commands are the same everywhere.
- Text files are LF in every checkout (`.gitattributes`). Code and tests still must not
  depend on line endings: read files as text and accept `\r\n`.
- No hard-coded path separators. File names must be unique ignoring case (Windows and
  macOS), and must match case exactly where they're referenced (Linux).
- The merge gate runs on the author's platform. A change to a platform layer also gets
  `check` on a second platform, and the PR says which. Platform layers are windowing, GPU,
  audio, sockets, file paths and process spawning. A platform that wasn't checked is named
  as unchecked.
- Read-only reviews use `git checkout --detach origin/<branch>`: a branch checked out in
  another worktree can't be checked out again.

**Windows**

- In Git Bash, cargo may not be on `PATH`: `export PATH="$HOME/.cargo/bin:$PATH"`.
- In Windows PowerShell 5.1, don't wrap commands in `bash -lc '...'`, because quoting
  breaks across the two shells. Run `cargo`, `git` and `gh` directly. There's no `&&`, and
  brace patterns like `crates/{a,b}` don't expand, so list the paths:
  `rg -n pat crates/app crates/gm`.
- WSL is the second-platform check for Linux: `cargo xtask linux-check` in `engine/` runs
  the check there while holding this machine's build permit.
- Optional: put worktrees and the target root on a Dev Drive, or exclude the target root
  from Defender scanning.

**Linux**

- Packages and the desktop setup: "Playing on Linux" in `slices/01-lantern-reach/README.md`.
- Without a desktop, windowed runs work under Xvfb or Weston with lavapipe.

**macOS**

- `cargo xtask check` passes on Apple Silicon.
- On a MacBook keyboard, F3 (the beat overlay) needs Fn + F3; plain F3 is Mission Control.
- Sim maths must give the same bits on every OS, or replays recorded on one platform
  diverge on another. Use `galaxy_contract::geom` (`sin`, `cos`, `atan2`,
  `Vec2::length`), not `f32::sin` and friends: those call the platform's C library.
- A Linux check can run in Docker (`rust:1-bookworm`, plus `pkg-config` and
  `libasound2-dev`). Mount folders under your home folder; `/tmp` may not be shared with
  the Docker VM.
