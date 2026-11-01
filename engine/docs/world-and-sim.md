# World and sim

How the engine holds a game world and moves it forward. The code is `crates/api` (the
contract: IDs, schemas, commands, causes, capabilities, randomness, portable maths) and
`crates/sim` (`world.rs`, `services.rs`, `session.rs`).

## What a world is

A world is entities with components, plus a history.

- An **entity** is an ID such as `example.front`. It exists while it has at least one
  component.
- A **component** is a set of typed fields of a component type, such as `example.door
  { open = true, pushes = 2 }`. Modules declare their component types in their manifest
  (`modules-and-install.md`); the engine knows nothing about them except their fields.
  Field types are `number`, `int`, `bool`, `text` and `id`.
- A **fact** is something that happened: a kind (a declared event kind), a tick, a payload
  and a cause. Facts are the history, and each fact is also an **event**: handlers for its
  kind run on the next tick.
- A **cause** says why something changed: a module's start state, an input, a script
  handler (which module, file, handler and the fact it ran for), an engine service or a
  hot-load. Every component remembers the cause and tick of its last change, and every fact
  its cause, so "why is this so?" always has an answer.

The engine owns only the component types its own services act on:

| Type | Fields | Who writes it |
|---|---|---|
| `engine.position` | `x`, `y`, `layer`, `height`, `angle` | modules place and turn things and change `layer`; the motion service moves `x`, `y`; the ground service sets `height` |
| `engine.motion` | `vx`, `vy` (units per second) | modules |
| `engine.surface` | `layer`, a rectangle, `height`, `rise_x`, `rise_y` | modules |
| `engine.model` | `model`, `look`, `scale`, `morph` | modules; the view draws it (`renderer-and-style.md`) |
| `engine.camera` | `tilt`, `view`, `min_view`, `max_view`, `fov` | modules; the camera follows the thing that has it |
| `engine.flight` | `speed`, `accel`, `brake`, `turn`, `inertia`, `boost`, `stop_time`, `slowdown` | modules; the flight service reads it (`flight-and-collision.md`) |
| `engine.helm` | `turn`, `thrust`, `brake`, `boost`, `stopping` | modules set the hands; the flight service keeps `stopping` |
| `engine.steer` | `mode`, `target`, `x`, `y`, `range`, `done` | modules give orders; the flight service sets `done`, moves `target` along a route and clears `mode` when the helm takes over |
| `engine.waypoint` | `next` | modules |
| `engine.body` | `radius`, `mass`, `fixed`, `drag` | modules; the collide service moves `x`, `y` and `vx`, `vy` to push things apart |

Places, spots, doors, locations, beings, walks and clocks are also ordinary engine
components; [places-and-beings.md](places-and-beings.md) describes their fields and
commands. A player is a being marked `player`, with a location like any other being.
Place kinds and being state names stay module content.

## 2.5D

The world is a flat 2D simulation with height and floor layers, drawn later as 3D models.
There is no vertical physics. A thing stands on a floor layer (0 the ground, 1 a deck
above it, and so on). `engine.surface` rectangles describe the floors: a flat deck has no
rise, a ramp or hill side has some. Each tick the ground service sets every position's
`height` to the highest surface on its layer under its point, or 0 where there is none.
Only surfaces in the same place as the entity contribute height. Going up a ramp lifts you; stepping onto a deck or climbing a ladder is a script patching
`layer`, which the module needs the `engine.position` capability for.

## IDs and capabilities

Everything a module names is namespaced by the module: `<module>.<name>`. A module may
write its own names plainly (`door`); everything else uses full IDs. `engine` is the
engine's namespace; no module may take it.

Capabilities are granted per component type. A module writes its own component types; it
writes another module's, or the engine's, only if its manifest lists them in `writes`. It
creates entities only in its own namespace, but may put its components on any entity. It
records only its own event kinds. Reading is free. Inputs, hot-loads and engine services
act as the engine. Modules are written by us and our agents, so this guards against
mistakes, not attackers.

## One command set

World writes go through commands, in a batch with one cause:

- `put` sets a whole component (missing fields get their defaults) and creates the entity
  if it is new;
- `patch` changes some fields of an existing component;
- `remove` removes one component, or the whole entity;
- `record` records a fact;
- `move`, `walk` and `advance_clock` expand to component writes in order, with the
  same cause and capability checks. Their state is saved like any other components.

`World::apply` checks each command against the registry (the schema and the actor's
capabilities) and against the world (a patch needs the component), and applies the batch
whole or undoes it. Place references and cycles are checked on the complete batch;
module start states are checked together after loading. Scripts, start states and inputs
all go through it. Engine services,
which run every tick, change their component fields directly under their own cause.

## The tick

The sim runs at a fixed 30 ticks per second of game time and reads no clock. Each tick:

1. the tick number goes up;
2. this tick's inputs record their events;
3. every fact not handled yet (those recorded during the last tick, and this tick's
   inputs) runs its handlers, in sequence order: modules in ID order, scripts in manifest
   order. A handler's writes apply as soon as it returns, so the next handler sees them.
   Facts recorded now wait for the next tick. Inputs are handled before the services, so
   a key acts on the tick it arrives;
4. engine services run: flight, motion, walking, collide, then ground (`flight-and-collision.md`);
5. every 30th tick (once a second), each script's `cadence` handler runs.

Per-frame work runs in Rust services, never in Luau: scripts run only on events and the
slow cadence (`script-host.md`).

## Determinism

The same modules, seed and inputs give the same world, bit for bit, on every OS:

- every map is ordered by key; nothing iterates a hash map;
- randomness comes from seeded PCG32 streams, one per user (`script/<module>`), derived
  from the world seed and the stream's name, so adding a user never shifts another's
  numbers. The streams are part of the world;
- maths uses `libm` or plain Rust, never the platform's C library (`crates/api/src/geom.rs`;
  scripts get the same through the sandbox);
- the world's hash is BLAKE3 over its canonical JSON (ordered maps, floats as their
  shortest round-trip decimal). Module file hashes in causes are over text with `\n` line
  endings, so a CRLF checkout hashes the same.

`crates/galaxy/tests/sim.rs` holds the hash a scripted session must end with. It runs in
`check` on every OS, so a platform difference fails it.

## Alternatives and why this design

- **Typed Rust stores for each kind of thing** (what the slice had: stations, ships, wares
  as Rust types with about eighty commands). Fast and checked by the compiler, but every
  game change was a Rust change and a slow build. Modules declaring their own component
  types keeps the game out of the engine.
- **An archetype or sparse-set ECS** (components in packed arrays by type). Much faster
  for many entities and per-frame systems. The headless spine has few entities and runs
  game rules on a slow cadence, so ordered maps keep it simple, deterministic and easy to
  save and hash. Storage can change behind `World` when the renderer and flight bring
  per-frame work at scale.
- **Free-form scripts writing anything**. Simple, but nothing would say who changed what,
  saves couldn't be checked against schemas, and one module could silently break
  another. One small command set with causes and per-type capabilities costs little and
  makes every change explainable.
- **Variable time steps**. Smoother for rendering, but runs would not repeat. A fixed tick
  plus render interpolation (later) gives both.
