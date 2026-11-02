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
For a player with `engine.travel`, changing place starts the shared travel presentation;
play records `engine.place_arrived` when it lands. See [Rooms and travel](rooms-and-travel.md).

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
resumes with the same cause and endpoint, even when the player is elsewhere. Days are authored with `engine.life` and `engine.act` (below); scripts choose when
to advance their clocks and when to offer a story. The talk service keeps offered
conversations pending until acknowledged; see [Talk](talk.md).

A resource/singleton store would add a second save path just for the clock and player.
Arrays of spots and doors would require new field types. Small entities reuse schema,
capability, query and save machinery. Locations are separate from beings so surfaces
and solid objects can share the same room isolation without inventing extra being kinds.
Presentation-owned walks would be lost when switching views; recorded segments let
any later presentation reconstruct them. Room kinds, authored acts and being state names remain module concepts. The generic room and
travel presentation is described in [Rooms and travel](rooms-and-travel.md).

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

## Speaking and focus

A being may carry `engine.focus { part }`; the default required part while speaking
is `face`, and empty means no focus. Talk is not limited to beings: any anchored
speaker can use the recorded conversation service. Unseen story conversations are
kept until their final line is acknowledged. See [Talk](talk.md) for components,
geometry checks and the distinction between a pending conversation and module routines.

## Life: a day per being

`sim/src/life.rs` runs a finite authored day, using the existing place, walk and clock
services. A non-player being with a position can carry `engine.life`:

| Field | Meaning |
|---|---|
| `clock`, `first` | Clock entity and first `engine.act` entity. |
| `name`, `speed` | Sign label (empty hides it), walking speed in world units per second (default 1.5). |
| `presentation` | `states` animates visible changes; `instant` completes them immediately. |
| `current`, `phase`, `start_tick` | Engine-owned selected act and phase (`rise`, `walk`, `settle`, or empty). |
| `from_morph`, `from_angle`, `walk_spot` | Recorded source pose and visible walk endpoint. |
| `departed_*` | Trip tick, place and frozen pose used only for a departure fade. |

Each `engine.act` has positive integer `duration`, destination `place` and `spot`,
module-defined `state`, resting `morph` (0..1) and `angle` (radians), and optional `next`.
An empty next repeats the day from first. The explicit chain must be finite; references,
duration sums, poses, speeds and floors are validated with command-batch rollback and
on resume. A day may use a shared clock or its own. Clock modulo total duration selects
an act, so a script can advance several units without replaying every skipped act.
All days start at clock zero; initial loading places beings in their selected acts.

The module owns the model and its morph target: for example, morph 1 can be a seated
figure and morph 0 its neutral standing pose. The engine does not know what an activity
or pose means. At a visible change it eases to neutral over 9 ticks (0.3 s), starts an
ordinary straight walk, then eases into the new act over 9 ticks. The body keeps its
source facing while walking and takes the destination facing while settling. Position,
model morph, named mesh-part anchors, picking and focus checks use the same pose.
The neutral walk is a translated figure; skeletal gait and navigation are not supplied.

When neither end is in the player's place, the destination is applied immediately.
A departure walks to a matching door's source spot, or the source room's arrival, then
moves out of sight. An arrival starts at the destination room's arrival and walks to its
act spot. Cross-room days require arrival spots on the acts' floor layer. There is no
invisible pathfinder or doorway policy. Authors choose clear straight segments.

Leaving mid-transition finishes its act in the world on that tick. The departure pose
is saved alongside it, so the source room fades out with the figure where it was seen,
without showing the offscreen completion as a teleport. Switching from
`states` to `instant` does the same. A clock change during a visible transition cancels
the old segment and retargets from the current position and pose. Switching views or reconstructing a view
needs no notification: no view owns transition progress. A life-managed being's day
owns its resting destination; scripts author the day and clock rather than competing
with an active transition by writing its location.

Life runs after event and cadence handlers, so the clock and player destination for the
tick are settled first. Walk integration remains in the normal service order. The
engine never increments a clock. Each increase of an existing clock (advance_clock,
put or patch) appends `engine.clock_advanced { entity, amount, value }` with the writing
command's cause, including multiple increases in one handler. Failed batches undo both
the writes and their facts. Clock initialization is start state; zero additions do not
record an increase. Direct decreases select an earlier day position, without an advance
fact. The existing place golden changes because the recorded clock fact is new.

## Signs and exhaustive authored-day checks

`view/src/life.rs` adds ordinary anchored text widgets to the screen path. A resting
sign says the being's name and activity; while moving it shows the name. An active
speaker's name moves into the bubble so two panels do not overlap. There is no new
palette or font. A module's room or hall screen can query the same location, being and
life fields for a directory of who is where; the generic fixture does this. The module's `[screen]` style supplies the sign surface and text colour, or the neutral
default when absent.

`galaxy-gm check` enumerates distinct selections over the least common repeat period
of the days sharing each clock, and the product of independent clocks. It stages every
resting arrangement and every transition between arrangements in each place, including
simultaneously moving beings. Each pose first runs ordinary screen readability with
talk inactive, so name signs are checked against panels even for beings without a talk.
Every authored talk line then uses screen readability and the ordinary triangle
focus check at each tick and half-tick, including rising and settling morphs. This also
checks jumps across acts and the day wrap, not only adjacent destinations. A clear
resting scene can therefore fail for a crossing figure, a raised head under a panel,
or a sign or bubble crossing a panel. Results name the place, transition, widget paths
and talk when active. Active walks staged by the talk checker also include silent signs.

The checker refuses a clock period over 100000 units, over 64 distinct arrangements,
or a transition longer than 18000 ticks, with an explicit finding; it never marks a
partial exploration as passed. These are authoring-check bounds, not runtime limits.
The check covers authored days with the current module geometry and screens. Arbitrary
future script branches, retargeting during a walk and changed assets require their own
variants or recorded runs. The command checks at 1280x720; other window sizes need
recorded runs too. Geometric focus limits are those in [Talk](talk.md).

## Trying life and alternatives

From the repository root, select the dev build after each executable:

```sh
galaxy-gm --build <name> guide life
galaxy-gm --build <name> check engine/tests/play/life/demo
galaxy --build <name> play --modules engine/tests/play/life --inputs engine/tests/play/life-inputs.toml --ticks 380 --size 1280x720
galaxy --build <name> sim replay <session>/recording.json
galaxy-gm --build <name> audit <session>/recording.json
galaxy-gm --build <name> film <session>/recording.json --from 10 --to 125 --size 1280x720 --out life-film
```

Click the being, advance the clock, leave during its walk and return. Advance again and
switch presentation during departure. The last bot walk has a name label without talk.
`view/tests/life.rs` covers day selection, offscreen and cross-room changes, pose and
bubble continuity, interruption, save/load/replay, invalid authoring rollback, and a
scene that passes at rest but hides focus along its walk. All these tests are headless.

A script-only schedule would duplicate phase and offscreen completion rules and hide
future poses from the checker. A per-frame Lua animation would violate the engine's
cadence contract. Linked scalar components reuse capabilities, validation and save
storage; absolute recorded phases let film and audit reconstruct mid-walk frames.
Morph targets keep poses in module assets instead of hard-coding game looks. A full
skeleton, navigation mesh or generalized behaviour tree would exceed this day's thin
loop. The finite day can later issue more elaborate engine-owned motion without moving
schedule meaning into the renderer.

The life demo declares `ui.hover` on `being.life.walker` and labels its controls.
Picking uses the interpolated, morphed mesh; the label follows the same head anchor
as the name sign and speech. The shared screen path owns paint and placement, with
no changes to life state or its hash. `tests/play/hovers-inputs.toml` follows the
moving speaker and takes eight F8-path notes for replay, audit and `look`.
See [Screens](screens.md) and [Motion tools](motion-tools.md).
