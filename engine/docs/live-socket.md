# The live socket

How `galaxy-gm` reads a running game and hot-loads modules into it. The GM owns all of
it: the server inside `galaxy play`, the protocol and the client are the crate
`crates/live` (`galaxy-live`); `galaxy` only hosts the server in its window loop
(`crates/galaxy/src/play.rs`), and `galaxy-gm live` is the client (`crates/gm/src/live.rs`).

## Trying it

From `engine/`, with the engine installed (`modules-and-install.md`):

```sh
galaxy play --modules tests/play/life          # serves the live socket while it runs
galaxy-gm live status                          # build, modules, tick, hash, place, fps
galaxy-gm live state --entity walker           # one entity's fields
galaxy-gm live history --inputs --last 10      # what the player just did
galaxy-gm live watch --entity walker --component engine.position --field x --ticks 60
galaxy-gm live shot --out now.png              # what the window shows
galaxy-gm live load                            # hot-load modules whose files changed
```

Each command prints one JSON object with the game it read (`game`: pid, session, build and
why it was chosen) and exits with the usual codes (`exit-codes.md`).

## How it works

**Serving.** `galaxy play` starts the server unless `--no-live`; `sim run`, `shot` and
`film` never do. The server opens a local socket: a named pipe on Windows, a Unix socket
elsewhere, through the `interprocess` crate, and writes a discovery file
(`<pid>-<n>.json`) into the live folder (`irudd-galaxy/live` in the per-user data folder,
or `GALAXY_LIVE`). The file names the endpoint, a random token, the pid, the session
folder, the build and the modules with their folders. It is removed when the game closes.

**Who may connect.** Only the local user. Named pipes refuse remote clients and their
default access lets only the user (and administrators) in; the Unix socket is mode 0600
in a folder only the user can open. Every request also carries the token, which only the
user can read from the discovery file; a wrong one closes the connection. The token never
appears in replies, logs, reports or the recording: listings of games leave it out.

**Protocol.** Newline-delimited JSON, one request and one reply a line:
`{"id": 1, "token": "...", "op": "state", "args": {"entity": "walker"}}` and
`{"id": 1, "ok": true, "result": {...}}`, or an error carrying the CLI's exit code, error
kind, message and fields, so the client prints it as its own. The ops are `status`,
`state`, `history`, `watch`, `shot` and `load`; their arguments are checked (sizes, tick
ranges) before they reach the game. `PROTOCOL` is bumped on an incompatible change, and a
client that speaks another one says which build to run (`galaxy-gm --build <name>`).

**Threads.** One thread accepts connections without blocking, so it notices the server
stop. Each connection gets a thread that reads request lines (at most 64 KiB each, 16
connections at once), hands each request to the game through a channel, wakes the window
loop through winit's event-loop proxy, and writes the reply when the game has answered.
The game answers between frames, in `frame()` after the ticks and on the wake-up event,
and never waits on a client: a reply goes into a one-place channel. A shot is drawn on the
game's thread and written to PNG on another, so the frame doesn't stall while it
compresses.

**Reads.** `status`, `state`, `history`, `watch` and `shot` take the session by shared
reference (`galaxy_live::Host::session`), so a read can't change the world or its hash;
the tests compare the hash before and after a run of reads, and a watched session with an
unwatched one. `state` is the same read as `galaxy sim run --state`
(`galaxy_live::reads::state`). `history` lists facts, oldest first, with who made each
from its cause (an input, a script and the event it handled, an engine service, a
hot-load) and the script prints since. `watch` resolves its entity, component and field,
samples them now, and after every tick the game runs (`Live::after_tick`) on its cadence,
then answers once its ticks have passed; a paused game holds it, and the client gives up
after twice real time. `shot` draws the window's scene again at the last frame's moment
(the same tick fraction and presentation time), at the window's size and scale.

**Hot-loading.** `load` reads the module folders (the running modules' own folders, or
those named), passes the version gate, and with no folder named skips modules whose files
are unchanged. The game then hot-loads them through the same path `--watch` uses
(`Session::hot_load`, `saves-and-replay.md`): between two ticks, checked first (scripts
load, the world still fits; else nothing changes and the reply is `hot_load_refused`),
recorded with the new files, and followed by the same refresh of keys, sounds and the
view. So the session still replays, audits and films; a `style.toml`-only change is a
presentation-only load, as with `--watch`. After a load the game writes its recording, so
the session folder has it even if the game is killed later; its replay applies the load at
the end (`saves-and-replay.md`). While an F8 note is open a load is refused
(`hot_load_refused`): a note kept after a load on the same tick would hold a world its
replay can't reach at that note.

**Choosing a game.** The client lists the discovery files and drops those whose endpoint
no longer answers (a crashed or killed game). With one game, it is used; a load uses the
one running the modules it loads; otherwise the one playing modules in the working folder.
If that still leaves several, the command exits 2 with kind `several_games` and the list,
and `--game <pid or session>` picks one. With none, kind `no_game` says how to start one.

## Alternatives and why this design

- **TCP on localhost.** Needs nothing beyond std, but any local user's process can reach a
  port, and the tests would open network sockets. Named pipes and Unix sockets are
  local-only by nature and carry the OS's per-user access. The frozen slice made the same
  choice; this is its design, simplified to a thread per connection.
- **A non-blocking I/O loop** (the slice's). It bounds every stall precisely, but is many
  times the code. A thread per connection with blocking reads costs a parked thread per
  client, which is nothing for a development tool used by one person's agents; the game
  itself never blocks on a client either way.
- **Streaming watches.** The slice streamed events until the client hung up. A command
  here prints one JSON value, so a watch collects its samples in the game and answers
  once; agents get one report with the range and changes worked out.
- **Reading the session folder instead.** The recording on disk is written at notes and on
  close, so it lags the game, and it can't show the window. The socket answers about now.
- **Sending module files over the socket.** A load names folders instead; the game reads
  them itself, as `--watch` does, and the recording stores the exact files either way.
