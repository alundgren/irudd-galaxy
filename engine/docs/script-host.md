# Script host

How module scripts run. The code is `crates/script`: `vm.rs` (the sandbox and budgets),
`meter.rs` (what native helpers charge), `api.rs` (the `ctx` API, declared once) and
`host.rs` (loading and calling a module's scripts). The sandbox, the meter and the portable
maths are carried over from the slice.

## What a script is

A module lists its entry scripts in its manifest. Each returns a table of handlers:

```lua
return {
    on = {
        ["engine.start"] = function(event, ctx) ... end,
        pressed = function(event, ctx) ... end,   -- this module's own event kind
    },
    cadence = function(ctx) ... end,             -- once per second of game time
    screen = function(ctx, ui) ... end,          -- pure widget tree, on changed reads
}
```

Handlers run on events (facts recorded on the previous tick, and inputs on this one) and
on the slow cadence. Nothing runs every frame: per-frame work (motion, the floor height,
later flight and collision) is engine services in Rust. A script may `require("./name")`
the module's other `.luau` files.

## ctx

A handler reads and writes the world only through `ctx`: `get`, `has`, `query`, `put`,
`patch`, `remove`, `record`, `random`, `random_int`, plus `tick` and `module`. Reads see
the world as it was when the call started. Writes are buffered as commands and checked
when they are made, against the component schemas and the module's capabilities, so a
mistake fails at the line that made it. When the handler returns, its commands apply as
one batch with the handler as the cause; if it fails, none apply and the engine records an
`engine.script_failed` fact (module, script, handler, code, message and place).

The API is declared once, in `api.rs`. The host binds exactly those names (a test checks
it), and `galaxy-gm types` prints the Luau type file made from the same declaration, which
every install ships as `galaxy.d.luau`.

## Screen functions

`screen` uses a separate read-only context (`module`, `get`, `query`) and the six `ui`
constructors. Its reads invalidate the cached tree when their data changes. It has no
simulation writes, random stream or tick access, and shares the fresh VM and budgets.
Returning nil hides the screen. `screens.md` describes layout, anchors, clicks and replay.
`galaxy-gm types` includes `ScreenCtx`, `Ui`, `Widget` and the optional script function.

## The sandbox

- Only `string`, `table`, `math` (without `random`), `bit32`, `utf8`, `buffer` and `vector`
  are loaded. No `os`, `io`, `debug`, `loadstring`, `getfenv` or `collectgarbage`.
- Libraries and globals are frozen, and every chunk runs in its own read-only environment.
- **A fresh Luau state for every call.** Locals and upvalues carry nothing from one call to
  the next, so the world is a script's only memory: a save or a replay runs exactly the
  same. The module's bytecode is compiled once and shared, so a fresh state costs little
  more than its libraries.
- **No addresses.** `tostring`, `string.format` and `print` show tables and functions as
  their type name. Iteration never follows heap addresses (next section).
- **Portable maths.** `math.sin`, `math.pow`, `vector.dot` and the like are `libm` and plain
  Rust, and the compiler is told they aren't builtins, so the platform's C library is never
  reached. The same script gives the same bits on every OS.

## Iteration order

A table's keys come back in slot order. For strings, numbers and booleans the slot comes
from the value, so the order is the same on every run. For keys that are tables or
functions it comes from the heap address, which changes from run to run, so a script
walking such a table would write a different world on replay. Every way of walking a
table goes through one check that refuses such tables:

- `pairs` and `next` are the engine's own (`meter.rs`). They check the keys when a walk
  starts and each key as it comes back.
- **Generalized iteration** (`for k, v in t do`) is walked by the Luau VM itself, not by a
  function, so it would skip that check. Before a module file is compiled, the host
  rewrites each generic `for` so its iterator goes through an engine function:
  `for k, v in t do` compiles as `for k, v in __galaxy_iterate(t) do` (`iteration.rs`).
  For a plain table that function does what `pairs(t)` does, in the same order the VM
  would use. Anything else (iterator functions such as `pairs(t)` or `string.gmatch`,
  tables with `__iter` or `__call`) passes through unchanged. The rewrite only adds text
  on the loop's own lines, so error lines stay right, and scripts can't use the name
  `__galaxy_iterate`.
- The refusal is a runtime error at the line of the loop, `pairs` or `next` call, and it
  is the same on every run and every OS. A script keys by strings or numbers instead (an
  entity's ID, for example).

**What is guaranteed:** no generic `for` loop in a loaded file walks a table natively.
Every one gets its iterator from `__galaxy_iterate`, or the file doesn't load.

The rewrite reads tokens, not a full parse. It skips strings, comments and interpolated
strings by Luau's own lexical rules (escapes such as `\z`, which carries a string across
a line break, long brackets with their level, numbers), and finds the loop's `do` as the
first one outside brackets and function literals. A misplaced insertion can't compile.
When it can't place one (an if-expression inside a function literal in the loop's
iterator), the file fails to load at that loop's line and asks for the iterator in
parentheses.

**How a miss fails closed.** The scanner isn't trusted on its own. After compiling, the
host reads the bytecode the Luau compiler produced (`bytecode.rs`). It finds every
instruction that starts a generic loop and checks the instruction right before it: a call
whose three results seed the loop, of a function loaded from the global
`__galaxy_iterate`. That global can only be the engine's function: the globals are frozen,
each chunk's environment is read-only, and a local of that name compiles to a different
load. The name can't appear anywhere in a script's text, even in a comment, so every use
of it in the compiled code is one the rewrite added. A loop that fails the check stops the
file from loading, with the file and the loop's line and a note that this is an engine
bug. So a future scanner miss is a load error, never an order that changes between runs.
The reader follows Luau's loader for the bytecode versions it knows. Bytecode it can't
read (another version after a Luau upgrade, an unknown opcode or constant) also stops the
file from loading, so an upgrade that changes the format shows up as every script failing
to load, not as a skipped check.

This all runs once per module load, when the bytecode is compiled, and costs nothing per
tick.

## Budgets

Each call has a step budget (loop back-edges and calls, counted by a VM interrupt, plus
what native helpers charge for work in proportion to their input), a memory allowance
(the collector is stopped, so garbage counts) and a limit on the commands it writes. Over
any of them the call is aborted: `pcall` can't catch it, and its writes are discarded.
Because the state is fresh and the counts are steps and bytes, not time, whether a call
runs out is the same on every run and every machine. The limits are in `vm.rs`
(`LIMITS`).

## Alternatives and why this design

- **One long-lived Luau state per module.** Cheaper per call, but scripts could keep state
  in upvalues that no save holds, and memory limits would depend on what earlier calls
  left behind. Fresh states make scripts pure functions of the world.
- **Per-frame script hooks.** Convenient, but slow, and hard to keep deterministic and
  within budget. Events plus a slow cadence cover game rules; per-frame work belongs in
  engine services.
- **Hand-written bindings plus a hand-written type file** (what the slice had). They
  drifted. One declaration for both keeps them in step.
- **Iteration order: a deterministic order instead of a refusal.** Sorting keys that are
  tables has no stable rule, and making heap addresses the same on every run would mean
  owning the allocator and the collector's timing. **Refusing every `for k, v in t do`**
  would be simpler than the rewrite, but walking a table keyed by strings is ordinary
  code. Patching the bytecode instead of the source would avoid the token scan, but
  rewriting jumps and registers in a format that changes between Luau versions is far
  riskier than reading it. The host only reads the bytecode, to confirm the rewrite, and
  refuses what it can't read. Luau's own parser would be the best cross-check, but its
  C API offers only the compiler.
- **Wall-clock time limits.** Simple, but a call could pass on one machine and fail on
  another, which breaks replays. Step counting is deterministic.
- **Another language (plain Lua, JavaScript, WebAssembly).** Luau has a sandbox mode, a
  type checker (`luau-analyze`, shipped with each install) and is small to embed; mlua
  builds it from source, so no system library is needed.
