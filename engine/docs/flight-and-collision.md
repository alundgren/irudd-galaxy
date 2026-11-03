# Flight and collision

How things fly, take orders and bump into each other. The code is in `crates/sim`:
`flight.rs` (the flight service), `collide.rs` (the collide service) and `shape.rs`
(solid outlines). Both are engine services: Rust, every tick, deterministic. Luau only
sets the controls and gives orders, on events.

```sh
galaxy play --modules tests/play/drift      # W/S thrust, A/D turn, Shift boost
                                            # 1 dock, 2 mine the nearest rock, 3 fly the route
galaxy play --modules tests/play/ambient --inputs tests/play/ambient-inputs.toml --ticks 900
                                            # a spinning rock, a hold through six cuts
```

## Flying by hand

A thing with `engine.flight`, `engine.position` and `engine.motion` flies. Its
`engine.helm` holds the hands on the controls: `turn` (-1 to 1, positive turns left),
`thrust` (-1 to 1 along the nose), `brake` and `boost`. A module sets the helm from keys
(`[keys]` in its manifest, handled by a script); the flight service flies it every tick:

- **Tank controls.** `turn` swings the nose at `turn` degrees per second (180 by default);
  `thrust` pushes along it toward top `speed` at `accel`.
- **Inertia.** `inertia` (0 to 1) decides how much of the old velocity survives a change.
  When the nose swings while thrusting, the sideways speed swings round into the new
  heading rather than vanishing (grip), and what inertia keeps carries on sideways for a
  moment: the ship drifts a little through a turn, then tracks. From rest, thrust goes
  straight along the nose, so a ship leaves a dock in a straight line.
- **Letting go.** With `stop_time` above 0, letting go of thrust starts the auto-brake:
  speed falls in a straight line to zero over `stop_time` seconds from any speed, so the
  ship coasts briefly and stops where the player expects. `brake` brakes harder; `boost`
  multiplies top speed and thrust by `boost`.

These are the slice's flight rules and its flight lab's inertia curve, carried over in
`f64` with `libm`. The defaults are tuned for the drift test module's scale; a module
sets its own numbers.

Inputs are handled before the services in a tick, so a key's handler sets the helm before
that tick's flight: a key acts on the tick it arrives (at most one tick, 33 ms, after the
press).

## Orders

`engine.steer` gives the flight service an order to fly instead of the helm:

| `mode` | What it does | Done when |
|---|---|---|
| `arrive` | flies to `target` and stops with `range` between the outlines (or to the point `x`, `y` when `target` is empty) | stopped there |
| `hold` | the same, then turns to face the target: mining, with `range = 0` the ship sits on the rock's drawn surface. Once there it keeps holding (below) | stopped and facing it |
| `dock` | flies onto the target's point and turns to its `angle` | stopped on it, facing that way |
| `follow` | passes each `engine.waypoint` from `target` along `next` without stopping, and stops on the last | stopped on the last |

An order steers straight for its point: the wanted speed is the most it can still brake
from before the point (accounting for a tick of travel, so it never overshoots), reached
at `accel` and shed at `brake`; the nose turns toward the way it goes at the turn rate,
and it loses some speed while flying away from its nose (`slowdown`). When it gets there
the engine sets `done` and records `engine.arrived` (`entity`, `mode`, `target`), and the
helm has the ship again: W flies straight off a dock. Touching the helm while an order is
under way sets it aside (the engine clears `mode`): the player always has the controls.
While the followed ship has an order, the view draws its course: still dots to where it
is going and a ring round the end (`renderer-and-style.md`).

`arrive` and `hold` stop where the two outlines meet along the line between their
centres, by the same test collision uses ([`touching`](../crates/sim/src/shape.rs)), plus
`range`: so arriving never counts as overlapping, the target is never pushed, and from
any side the ship stops at the target's drawn surface. The ship's own outline is
measured nose toward the target, as it will be at the end, so a ship that starts facing
away doesn't swing its nose into the rock as it turns.

An order that must face a way (`hold`, `dock`) turns only once the ship has stopped on
its point: while it is still moving the nose points the way it goes. So a dock arrives
after the turn, not when the ship first sits on the berth. On drift's scripted dock
(`tests/play/films/inputs/dock.toml`) the ship stops on the berth at about 2.62 s, then
turns 1.8 radians to the berth's heading at its turn rate (180 degrees a second), and
`engine.arrived` comes at 3.30 s. `galaxy-gm motion track <film> --to berth` shows it
under `approach` (`stopped_at`, `turned_after_stop`, `arrived_at`).

**Holding on.** A `hold` that got there stays `done` and keeps holding until the helm or
a new order takes over: as its target is cut smaller (morph), turns or is nudged, the
ship stays on the new surface and keeps facing it, with no new `engine.arrived` and no
course drawn again (the view draws a course only while an order is not done). At the end
of every tick, after the cadence, `settle_holds` moves each holding ship onto its goal
when that is within a tick's flight at top speed, so a cut a handler or the cadence
makes on any tick never shows a gap; further off (the target was pushed away), flight
steers back as for any order. Settling comes after collision, so the ships it moves get
collision's push-apart passes again (below): two ships holding one rock side by side
stay apart as it shrinks, a little off the surface where they meet, rather than settling
into each other. The view eases a morph over a tick like a move, so between
ticks the shrinking rock and the ship settling onto it stay together. Six cuts of a
rock in `tests/play/ambient` keep the surface gap under 0.05 units at every tick
(`crates/view/tests/ambient.rs`). Issuing `hold` again after each cut closed the gap too,
but each new order brightened the course for `bright` seconds.

A dock point must be clear of solid outlines; a ship can't stop inside something solid.
Where a ship docks and what docking shows (stations, places, interiors) is the module's
and, later, the place model's: the engine only flies there.

## Spin

`engine.spin { rate }` turns a thing steadily, `rate` degrees a second (positive is
counter-clockwise), for slow rocks and wrecks. The spin service runs after motion and
before collide, in plain `f64` (`angle + rate * tick`, wrapped into `-PI..=PI`), so a
replay turns the same bits on every OS and a body's outline (which is turned with the
thing) turns with it. Things with `engine.flight` are skipped: flight owns their nose.
The view interpolates the angle between ticks, so a 12 degree a second rock turns
smoothly at any frame rate. Parts of a model that turn or pulse only on screen (a
station's ring, its lamps) are the model's, not the world's: see
`renderer-and-style.md`.

## Collision

`engine.body` makes a thing solid. Everything that looks solid can be: ships, rocks and
stations bump and push, and nothing takes damage.

**Outlines.** A body's outline is a circle of `radius`, or, with radius 0, the footprint of
its model: how far the model reaches in each of 128 directions on the sim plane (its
support function), turned with the thing, scaled and morphed like the drawing. That traces
the model's convex outline as drawn, so a mining ship touches the rock where the rock is
drawn, and a rock mined smaller (morph) gets smaller to bump into too. The profiles are
made from the module's glTF files when modules load (`Shapes`), with plain arithmetic, so
replays collide identically.

**Each tick, after motion:**

1. Bodies are swept along x; pairs in the same place whose spans meet are tested.
2. Two outlines overlap when, along every sampled direction, their reaches add up to more
   than the gap between their centres. The direction with the least overlap is where
   they push apart (separating axes on the sampled outlines).
3. The overlap is split by mass: a light thing moves more than a heavy one. A `fixed`
   body, an active walker, or one without `engine.motion`, never moves. Their velocities lose what was
   carrying them into each other, with a little bounce (0.2), so a ship slides along a
   rock instead of sticking, and a rock it rams drifts off.
4. Four passes settle a pile-up.
5. Things that don't fly lose `drag` of their speed per second, so a pushed rock comes to
   rest.

When `settle_holds` sets holders down at the end of the tick, `separate` runs steps 1 to 4
again for positions only (no bounce or drag), testing just the pairs with a moved body
in them; a body they push joins the moved ones. No tick ends with solid bodies inside
each other, and a tick where no hold settled is untouched.

Nothing traps a pilot: a rock is pushable unless the module fixes it, and a ship pressed
against a fixed body keeps every direction that leads away from it.

## Alternatives, and why this design

- **A physics library** (rigid bodies, solvers). It would hide the part worth learning
  and bring float behaviour we don't control across OSes. The game needs pushing, not
  stacking or spinning.
- **Circles only.** Simple and exact, but a ship mining an oblong rock would stop short of
  its surface on one side and sink in on the other. The change list asks for no visible
  gap.
- **Exact convex polygons.** Exact SAT needs the hull's edges and is fiddly with morphing;
  sampled reaches morph by interpolation and cost the same for every shape. Sampled
  outlines stand slightly proud of long flat edges (where two neighbouring sample lines
  meet beyond the edge), by about 1% of the edge's length at 128 directions; 32 left
  visible gaps beside a rock's flat faces.
- **Spin as a field of `engine.motion`.** One component for all movement reads well,
  but every existing world stores every field of a component, so a new field would have
  changed every world's hash and saved state. A component of its own leaves worlds that
  don't spin as they were.
- **Re-issuing `hold` after each cut, or a new order mode for it.** Re-issuing flashed
  the course each time; a separate mode would make every module choose. Holding on is
  what `hold` means.
- **Orders as Luau.** Steering is per-tick work; scripts give orders on events and hear
  `engine.arrived`, and the engine flies.

`engine.location.place` isolates coordinate spaces: bodies in different places never
collide, even across a parent/child relationship. Missing or empty location keeps the
original shared plane. Active walks run before collision and are kinematic straight
segments; see `places-and-beings.md`. Ground lookup is likewise local to each place.
