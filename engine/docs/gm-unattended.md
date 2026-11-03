# Unattended GM runs

How a GM agent runs on its own on a schedule: it reads the player's new F8 notes, changes
the game's modules, checks its work and hands the player one change to accept or roll
back. Its memory carries the player's preferences between runs. The workflow is taught by
the installed guide (`galaxy-gm guide unattended`, in `crates/module/src/help/gm.rs`); the
engine and GM supply the pieces it needs: the memory (`crates/gm/src/memory.rs`), the
continuing save (`--continue`, `crates/galaxy/src/sim.rs`) and the inbox for one game
(`notes inbox --modules`, `crates/gm/src/sessions.rs`).

## Trying it

```sh
galaxy-gm memory game/modules                          # the memory: made once, then edited
galaxy-gm notes inbox --modules game/modules --all     # this game's unanswered notes
galaxy sim load --continue --modules game/modules --state   # where the player stopped
galaxy play --modules game/modules --continue          # the player carries on
```

## How it works

**The entry point is the binary's guide.** The scheduler's prompt only says who the agent
is and where to start. The front page of `galaxy-gm guide` sends a run with nobody
watching to `galaxy-gm guide unattended`, which holds the whole workflow: what to read
first, how to pick work, the gate and the handover. The guide is versioned with the
build, so a run always gets the instructions of the tools it runs; a globally installed
skill or scheduler prompt stays a pointer to it and never copies it.

**The run** (the recipe in the guide topic):

1. *Read.* The game's own instructions (a repository's `AGENTS.md`, a README), `galaxy-gm
   doctor` for the build and the game's modules, the memory, the inbox for this game, and
   the player's game where they stopped (`galaxy sim load --continue ... --state`).
2. *Settle earlier handovers* listed in the memory: merged is done; deleted is a rollback,
   which becomes a preference, and its notes are reopened (`notes unhandle`).
3. *Understand each note from its moment:* its text and frame, `audit` (the moment drawn
   again from the replay) and `film --note` (the seconds round it).
4. *Pick work:* notes oldest first, then one step of a memory direction; small and done
   beats big and half done; a note that needs the player's decision stays unhandled and
   becomes a question in the handover. Nobody answers questions during the run.
5. *Change* the modules in a new git worktree on a branch `gm/<date>`, so the player's
   files stay as they are until they accept. The GM's own plays and bots point
   `GALAXY_SESSIONS` at a scratch folder, so their notes and saves stay out of the
   player's inbox and continuing save.
6. *The gate,* all of it passing before the handover: `check` and `test` on each changed
   module, `what-if` of the noted session on the change, a bot run of the changed journey
   that replays identically, a look at each changed moment (a shot, or a film and a motion
   tool), and the player's save loading on the change (`galaxy sim load --continue`).
7. *Hand over:* one commit per note with its evidence, `notes handle --change gm/<date>`,
   a report (what changed and why, the evidence, how to accept or roll back, questions).
   A pull request only when the game's instructions say so; the GM never merges.
8. *Remember:* edit the memory file.

**The memory** is one Markdown file a game in the per-user game data folder
(`irudd-galaxy/gm-memory/<module IDs joined by +>.md`, beside the sessions;
`GALAXY_GM_MEMORY` moves it). `galaxy-gm memory <modules>` makes it from a short template
the first time (never replacing an existing file), and prints its path and text; the GM
edits the file itself. Its sections are the player's preferences, the long-running
directions not yet in the game, and the handovers waiting for the player. The player may
read and edit it, and what they write wins. It never holds secrets: when the file holds the
OpenRouter key (the value of `OPENROUTER_API_KEY`, or anything shaped like such a key)
the command refuses with exit 4 and doesn't print it.

**The continuing save.** Every `galaxy play` session saves on quitting, so the newest
session save of a game is where the player stopped. `--continue` on `play` and `sim load`
finds it (newest `save.db` in the sessions folder whose modules have the game's IDs).
Resuming on module files the GM changed carries the world over as a hot-load does: new
fields get their defaults, and a change that would lose saved data is refused with exit 4
naming the changed modules. That is why the gate loads the player's save on the change:
a change that fails it would strand the player's game. Details in `saves-and-replay.md`.

**The reviewable unit is a git branch.** Accepting is merging it (or merging its pull
request); rolling back is deleting it (or closing the pull request), or reverting the merge
later. Git is already how this repository reviews `game/` changes, it shows the whole diff,
it works the same for a game folder with no remote (the run puts a folder that is not
under git under git first, so there is something to roll back to), and the notes'
receipts name the branch (`--change`), so the inbox says which change answered which note.

## Scheduling it

The scheduler starts an agent in the game's folder (a checkout of the game, or the folder
holding its modules) with this prompt and nothing else:

```text
You are the GM. Start with `galaxy-gm guide`.
```

With a development build, which runs only when named, the prompt names it:

```text
You are the GM. Start with `galaxy-gm --build <build> guide`, and put `--build <build>`
right after `galaxy` or `galaxy-gm` in every command.
```

- **T3 Code:** a scheduled task on the game's project, for example nightly, with that
  prompt. Each run starts a fresh thread whose working folder is the project.
- **The OS scheduler:** run an agent CLI non-interactively in the game's folder, with
  permission to run shell commands and edit files there. For example, cron on Linux or
  macOS: `0 3 * * * cd <game folder> && claude -p "You are the GM. Start with \`galaxy-gm
  guide\`."`. On Windows, a Task Scheduler task with the same command (`schtasks /Create
  /SC DAILY /ST 03:00 /TN galaxy-gm /TR "<command>"`, with the game folder as its start
  folder).

The agent needs the installed `galaxy` and `galaxy-gm` on PATH, git, and, to open a pull
request where the game's instructions ask for one, `gh` signed in. Bot runs open a window,
so schedule runs for when the player is logged in. The player reads the handover, plays it
with `galaxy play --modules <modules> --continue` (from the branch's worktree before
deciding, or after merging), and merges or deletes the branch. A session played on the
branch saves on its modules; after a rollback, a save holding data the branch added is
refused with exit 4, and `--load` of the session before the try carries on instead.

## Alternatives and why this design

- **Memory in an external memory system** (a personal memory library). It would hold the
  preferences away from the game, need its own tools and access, and travel with the
  agent rather than the game. A file with the game data needs nothing new and the player
  can open it.
- **Memory beside the modules, committed with the change.** It would be reviewed with the
  change, but a rolled-back change would lose what the run learned, and every run would
  need a commit to record a preference. The data folder keeps it whatever the player
  decides.
- **A staged module version instead of a branch** (the GM writes a copy the launcher
  could switch to). It would need its own accept, rollback and diff commands; git already
  has them, and the repository already reviews game changes that way.
- **Carrying the save over in the GM** (rewriting the player's save on the changed
  modules). It would change the player's files before they accept. Loading changed files
  at resume needs no rewrite: until they merge, the player's modules and save are as they
  left them.
- **The whole workflow in the scheduler's prompt or a skill.** It would drift from the
  installed tools; the guide changes with the build.
