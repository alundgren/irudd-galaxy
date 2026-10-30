# AGENTS.md

Rules for every agent working in this repo, whether one agent on a small change or an
orchestrator running many. Folders add their own (`slices/01-lantern-reach/AGENTS.md`);
where a folder's rules are stricter, they win. A task's brief can override anything here
except "Public repo" below.

## Where things stand

| Folder | Phase |
|---|---|
| `slices/01-lantern-reach/` | prototype |

Phases: **prototype** (slices: a test of an idea), **production** (`game/`, building the
real game), **released** (`game/` once players have a public build). `prototypes/` are
throwaway: they only have to run. When `game/` starts, add it here, and change its phase
when it moves on. The phase decides what blocks a merge (below). Nothing else in this file
changes with it.

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

Everything else blocks in no phase: hardening, edge-case limits, contrived ways to smuggle
data out, style. File it as a follow-up issue, unless the reviewer shows a realistic
player hitting it.

## Public repo

- No local absolute paths, machine names, usernames, emails or secrets in committed files,
  issues, PRs or comments. Keys come from the environment only.
- Git and `gh` directly. No CI: GitHub Actions are only for releases, and checks run
  locally.
- Only the owner merges into `main`.

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
Verdict: APPROVE, APPROVE WITH NOTES (non-blocking findings, for a follow-up issue) or
CHANGES REQUESTED (blocking findings only, each with a reproduction or failing test).
The author reported `check` passed at <sha>. If HEAD is that commit, don't rerun it:
run the tests for what you probe (`cargo xtask cargo test -p <crate>`).
```

A re-check covers only the findings it lists and the fixer's "new behaviour" list. It is
not a new review: no "final sweep", no attack lists. After two re-checks the coordinator
decides: merge with a follow-up issue, or stop and say why.

### Fixes

- One job, one purpose. Merging the integration branch into a PR is its own step, with
  its own check, never part of a fix.
- Fix the blocking findings first. Minor ones go into a smaller second pass or the
  follow-up issue.
- Give constraints, not designs: "the save format stays compatible", not "use
  compare-and-set".
- Before pushing, the fixer reviews its own diff, runs `check`, and posts on the PR
  "`check` passed at `<sha>`" and "New behaviour this fix introduces: ...".

### Builds

`cargo xtask check` is the merge gate. Run it before asking for review, and again before
merging if the branch changed since. Reviewers skip it when the commit matches a reported
pass. Never enable fat LTO; thin LTO or none.

Cold builds are the slowest part of a review. Set `GALAXY_TARGET_ROOT` once per machine
to a folder outside the repo. `cargo xtask` then builds into `<root>/<branch>`, so every
worktree on a branch (builder, reviewer, fixer) shares one warm build. Notes:
- Run targeted commands through it: `cargo xtask cargo test -p <crate>`.
- `cargo xtask target-dir` prints the folder.
- Two agents on the same branch at once wait on cargo's lock, so run them in turn.
- Folders under the root can be deleted any time; they rebuild.
- If `GALAXY_TARGET_ROOT` is unset, the normal `target/` is used, and `cargo xtask` prints
  a note saying so. Before a multi-agent run, check that it's set: an unset root is the
  cold-build cost this section exists to avoid.

**Fanning out: borrow a slot.** Stable cargo has no compiled-dependency cache shared
between folders, so a new folder per branch means a full cold build (about 8 minutes on
16 cores) for every worker. When a job fans out onto several branches, give each agent a
long-lived folder instead: `GALAXY_TARGET_KEY=slot-1`, `slot-2`, and so on.
- Slots outlive branches and runs. The dependencies stay compiled; only our own crates
  rebuild where the branch differs.
- One agent per slot at a time. A reviewer or fixer borrows the slot its builder used,
  after the builder is done.
- Cap each build at its share of the cores, `CARGO_BUILD_JOBS` = cores / parallel agents,
  or parallel builds starve the machine.
- Keep the pool small (about four). Each slot is several GB.

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
- WSL is the second-platform check for Linux.
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
