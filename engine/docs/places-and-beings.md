# Places and beings

The place model is world state, independent of cameras, screens and travel effects.
`crates/sim/src/places.rs` implements its commands, reference checks and walking service.
The engine has one concept, a being, for anyone or anything met in a place. What a being
is, its state names, and kinds of place are module content.

## The components

All IDs below refer to ordinary entities. Empty optional IDs are stored as null, read
by Luau as `""`. `galaxy-gm fields components` lists the types and defaults.

| Component | Meaning |
|---|---|
| `engine.place` | `parent` forms the place tree; empty means a root. Optional `enter` points to the place entered by default. Optional `arrival` names a spot in this place. `width` and `depth` describe its floor in world units (nonnegative; 0 means unspecified). |
| `engine.spot` | A `name`, unique within its `place`, and local floor `x`, `y`, `layer`. Spots are entities, so no list or map field type is needed. |
| `engine.door` | A directed connection `from` one place `to` another, with optional source `spot` and destination `arrival`. Declare two doors for a two-way connection. |
| `engine.location` | An entity's containing `place` and optional occupied `spot`. Used by beings, bodies, surfaces and other placed things. Empty place, or no location component, means the original shared plane. |
| `engine.being` | Module-defined `state`, optional `home` place and a `player` marker. A being must also have a location in a place. At most one being is marked player; worlds without a player remain valid. |
| `engine.walk` | An `active` segment with `place`, source and destination spot IDs (`from`, `to`), saved `from_x`, `from_y`, `to_x`, `to_y`, `layer`, `start_tick`, `speed`, and completion `state`. Source spot can be empty. |
| `engine.clock` | A nonnegative integer `value` in module-defined units. A module can have several explicitly named clocks. |

Place parent and enter chains must be acyclic. References must exist and spots must
belong to the place using them. Checks run after a complete command batch, after all
module start states load, and when a saved world resumes. Thus authoring can use forward
references and change several connected entities atomically. Removing referenced content
requires repairing its references in the same batch. A bad batch rolls back completely.

Locations separate physical coordinate spaces. Bodies in different places never collide,
including parent and child places. Ground height uses only surfaces in the body's own
place and floor layer. Existing modules with no locations keep their shared-plane
behavior. Floors, spots and positions are 2.5D; there is no vertical physics. Floor size
is descriptive, not an invisible collision wall.

## Script operations

The normal `ctx.get` and `ctx.query` read these components. There is no singleton API:
find the player among `engine.being` entities by its marker; keep a clock's entity ID in
module content. `galaxy-gm types` includes these additions:

```lua
ctx.move("visitor", "room", "door_spot", "visiting")
ctx.move("player", "block") -- follows enter; uses the final place's arrival
ctx.walk("visitor", "table_spot", 1.5, "resting")
ctx.advance_clock("day", 1)
```

`move(entity, place, spot?, state?)` changes location at once, follows `enter` to its
last place, and places the entity at the selected spot (the final place's arrival when
omitted, local origin on layer 0 if neither exists). It creates a position if needed,
preserves facing, cancels a walk and zeros motion and steering when those exist. It
leaves state unchanged when omitted. A door describes connectivity; it is not an access
restriction on this story-move operation. Scripts decide which door a player may use.
No cut, camera transition or presentation-arrival event is implied by a move.

`walk(entity, spot, speed, state?)` starts a straight segment from the current position
to a spot in the same place and floor layer. Speed must be finite and positive, in world
units per second. It clears the occupied spot while walking and captures the endpoint
coordinates, so later spot edits do not bend an in-progress segment. Another walk starts
from the current position. The optional state is applied on completion; omitted state
captures the being's state at the start. Walks also work on located entities without a
being when state is omitted. Flight and walking cannot control the same entity.

`advance_clock(entity, amount)` adds a nonnegative whole amount, rejecting overflow.
The engine never advances these clocks by itself. Elapsed sim ticks drive walks; world
clock values drive whatever rules the module chooses.

These are buffered commands, like `put` and `patch`: writes apply in order after the
handler returns, before the next handler reads. Two clock advances add sequentially;
a move followed by a walk sees the new place. Reads within the handler still see its
starting snapshot. All expanded writes keep the handler's cause and component
capabilities. Moves need location and position writes, plus being for a state change,
walk to cancel an existing walk, and motion/steer when present. Walks need location,
position and walk writes, plus being for an explicit completion state and motion/steer
when present. Clock advances need clock writes. Granting walk permits its service to
apply the captured being state on completion, just as motion permits its service to
update position each tick.

## Walking in Rust

After motion, before collision and ground, the Rust service computes distance along the
segment from `(tick - start_tick) * TICK_SECONDS * speed`, using portable `libm::hypot`
for length. It derives each position from the original segment, not accumulated velocity.
At the end it sets the exact endpoint, the occupied spot and captured state, clears
`active`, and records `engine.walk_finished { entity, place, spot }` once. Handlers see
that event next tick. A zero-length walk finishes on the next service pass.

Active walkers bypass velocity integration and collision displacement; their bodies
can push movable bodies. This is authored straight walking, not navigation or obstacle
avoidance: modules choose clear segments. Collision resumes normal handling after
completion. Changing places uses a move; future corridor navigation can issue segments
and moves without replacing this model. No view needs to be present for a walk to finish.

## Saves, replay and alternatives

Everything is in ordinary components, including the active walk and clock. SQLite save
format 1 and recordings need no new storage or presentation state. The same saved walk
resumes with the same cause and endpoint, even when the player is elsewhere. Pending
story moments and daily routines remain module rules; the engine provides the recorded
state they can build on.

A resource/singleton store would add a second save path just for the clock and player.
Arrays of spots and doors would require new field types. Small entities reuse schema,
capability, query and save machinery. Locations are separate from beings so surfaces
and solid objects can share the same room isolation without inventing extra being kinds.
Presentation-owned walks would be lost when switching views; recorded segments let
any later presentation reconstruct them. The prototype's cut, room kinds, routines and
poses are deliberately not engine concepts.

## Headless proof

Run from `engine/`, using a build that includes this feature:

```sh
galaxy-gm check tests/modules/places/demo
galaxy sim run --modules tests/modules/places --seed 7 --inputs tests/modules/places/inputs.toml --ticks 33 --save walk.db --record walk.json --state --all
galaxy sim load walk.db --modules tests/modules/places --ticks 0
galaxy sim replay walk.json
galaxy sim load walk.db --modules tests/modules/places --ticks 62 --state --all
galaxy sim run --modules tests/modules/places --seed 7 --inputs tests/modules/places/inputs.toml --ticks 95 --record end.json --state --all
galaxy sim replay end.json
```

At tick 2 the visitor moves to the other room, the player enters the first room, and
the clock reaches 5. At tick 3 a three-unit walk starts at speed 1; tick 33 is one unit
along, and tick 93 finishes. The player stays in the other room. Both collision probes
stay at `(20, 0)` in separate places, on floors of heights 2 and 5. The scenario checks
these states and one completion event. `crates/galaxy/tests/places.rs` runs the executable
entry points, compares save/load/replay hashes and fixes a cross-platform golden hash.
`crates/sim/tests/places.rs` also checks authoring failures, rollback, capabilities,
retargeting, cancellation, hot-load, zero-distance completion and same-place collision.

Run the scenario assertions during engine development with
`cargo xtask cargo test -p galaxy-sim --test places`.
