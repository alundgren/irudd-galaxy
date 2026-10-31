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

## The sandbox

- Only `string`, `table`, `math` (without `random`), `bit32`, `utf8`, `buffer` and `vector`
  are loaded. No `os`, `io`, `debug`, `loadstring`, `getfenv` or `collectgarbage`.
- Libraries and globals are frozen, and every chunk runs in its own read-only environment.
- **A fresh Luau state for every call.** Locals and upvalues carry nothing from one call to
  the next, so the world is a script's only memory: a save or a replay runs exactly the
  same. The module's bytecode is compiled once and shared, so a fresh state costs little
  more than its libraries.
- **No addresses.** `tostring`, `string.format` and `print` show tables and functions as
  their type name, and `pairs` and `next` refuse tables keyed by tables or functions, whose
  order follows heap addresses.
- **Portable maths.** `math.sin`, `math.pow`, `vector.dot` and the like are `libm` and plain
  Rust, and the compiler is told they aren't builtins, so the platform's C library is never
  reached. The same script gives the same bits on every OS.

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
- **Wall-clock time limits.** Simple, but a call could pass on one machine and fail on
  another, which breaks replays. Step counting is deterministic.
- **Another language (plain Lua, JavaScript, WebAssembly).** Luau has a sandbox mode, a
  type checker (`luau-analyze`, shipped with each install) and is small to embed; mlua
  builds it from source, so no system library is needed.
