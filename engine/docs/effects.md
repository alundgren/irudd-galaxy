# Effects

What a module can ask the renderer for beyond its models: beams between things, particles
(sparks, chips, exhaust, puffs), soft halos round self-lit paint, and camera feel (a
kick, a widening with speed, a look ahead). All of it is drawn only. The code is
`crates/view/src/effects.rs` (from the world to the scene), `crates/render/src/effects.rs`
(the style tables) and `crates/render/src/renderer/fx.rs` with `shaders/fx.wgsl`
(drawing). The test module is `tests/play/effects`.

```sh
galaxy play --modules tests/play/effects          # 2 mines, 1 docks, W flies
galaxy play --modules tests/play/effects --inputs tests/play/effects-inputs.toml --ticks 720
```

## What a module writes

Components, which scripts put and patch like any other (`writes` in the manifest), and
tables in `style.toml`. `galaxy-gm fields components` and `galaxy-gm fields style` list
every field and key with its range and default.

```toml
# start.toml
"engine.beam" = { look = "m.cutter", x = 2.3, height = 0.2 }   # target set by a script
"engine.emitter" = { recipe = "m.exhaust", x = -2.0, angle = 180.0, by = "thrust" }
"engine.burst" = {}                                            # recipe and tick by a script
"engine.camera" = { view = 36.0, speed_zoom = 0.25, lead = 0.3 }

# style.toml
[looks.hull]
halo = 1.0                   # a halo round each patch of self-lit paint
[beams.cutter]
color = "#7ff0d8"
impact = ["m.sparks", "m.chips"]
[particles.sparks]
shape = "spark"              # dot, spark, puff or chip
blend = "add"                # add (a glow) or paint (inked, covers)
rate = 50.0                  # a second, from an emitter or a beam's landing
count = 12                   # in one burst
```

- **Beams.** `engine.beam` runs from its thing (offset by `x`, `y`, `height` in the
  thing's own frame, so a nose stays a nose as the ship turns) to the drawn surface of
  `target`, while `target` names a thing in the same place. Empty is no beam.
- **Emitters.** `engine.emitter` sprays a recipe from its thing, or from `target`, at
  `rate` times the recipe's rate. `by = "thrust"` scales that by how hard the thing
  pushes ahead (the helm's thrust, or speeding up along its nose under an order),
  `by = "speed"` by its share of top speed, and empty sprays always.
- **Bursts.** `engine.burst` makes one burst of the recipe's `count` on `tick`. A script
  sets `tick = ctx.tick` on an event: a dock, a sale, a hit. One burst per thing at a time.
  A thing that needs two at once uses a second thing.
- **Halos.** A look's `halo` puts a soft additive glow round every patch of self-lit
  paint on the models that wear it: windows, lanterns, an engine's nozzle. A patch is a
  connected run of glowing triangles in one model part, found when the model loads. A
  part that pulses pulses its halo; a part that spins carries it round.
- **Camera feel**, on `engine.camera`: `speed_zoom` widens the view by that share at the
  followed thing's top speed, `lead` aims that many seconds of its travel ahead, and
  `shake` (a share of the view) kicks and shakes the camera for `shake_seconds` from
  `shake_tick`. Like bursts, a script sets `shake_tick = ctx.tick` on the event.

Missing recipes and beam looks show in the problems panel, as missing models do.

## Drawn only, and the same every time

The world never reads an effect, so recordings, saves and world hashes are the same
with or without them, and a replay can't diverge over one. A test steps one session
while a view draws it every tick, and checks the hash against a session nobody draws.

Each frame works out every live particle afresh, as a function of:

- the tick it was born on, its index on that tick, the thing, the recipe and the world's
  seed, which seed its dice (splitmix64), so its lifetime, direction, speed and turn are
  the same on every run;
- where its emitter was at that moment, from a short history of each spraying thing's
  pose, one entry a tick;
- the moment drawn (`tick - 1 + alpha`, as for poses).

So a particle born behind a ship stays where it was born while the ship flies on: a
trail, not a cloud that rides along. The history is filled by `View::before_step`, which
every path that draws a session already calls once a tick (play, shot, film, audit,
look). A film or an F8 note therefore draws the same sparks the player saw. Two views of
one session give identical particle lists, and a test checks it.

Positions move with drag analytically: speed `v0 e^(-k t)`, distance `v0 (1 - e^(-k t)) / k`.
Colour and size run from their start to their end values over the life, and opacity
fades out towards the end.

**Why not simulate particles step by step.** A stepped simulation keeps state per
particle. A note's redraw or a film from the middle of a session would need that state,
saved or replayed, and a frame-rate-dependent step would draw differently at 60 and 144
Hz. A closed form needs only the emitter's past poses, which are cheap (about 120 small
entries per spraying thing), and every frame is exact at any rate.

**Why not on the GPU.** The counts here (hundreds, at most 4096) cost well under a
millisecond on the CPU, and a CPU list is what the headless tests and the film's probes
can read.

## The cap

A scene draws at most 4096 particles (`MAX_PARTICLES`); the rest are dropped and the
problems panel says so. Recipes are bounded too (count 256, rate 400 a second, life 4
seconds), halos stop at 512 a scene and 64 lamps a model. A runaway module at the cap
still holds 60 fps at 1280x720 (scene build 0.6 ms median).

## Drawing

Particles join the effects pass, between lines and glows, as camera-facing quads lifted
towards the camera by half their size, so they don't clip into the surface they leave.
They are depth-tested against the models.

- **dot**: a soft round falloff.
- **spark**: a streak along its velocity on screen, 45 ms of its travel long, brightest
  at its head with a white core.
- **puff**: a round cel disc with an ink rim.
- **chip**: a small turned square with an ink rim.

`add` writes premultiplied colour with zero alpha (it brightens what is beneath);
`paint` covers. Puffs and chips are meant to be painted, so they read as part of the
cel-and-ink world.

Beams are the renderer's beam lines: a hot white core and a coloured glow whose strength
is the style's `glow`, drawn over what they cross. The end must therefore sit on the
target's surface: the view casts a ray from the beam's start towards the middle of the
target's bounds, against its mesh as drawn this frame (scaled, turned and morphed, so a
rock being cut is hit where it is now), and falls back to the target's outline reach when
it misses. Halos sit at both ends, the landing's twice the size of the source's. Flicker
is two slow sines out of step per thing, by game time.

Halos are the renderer's glows: additive and local. There is no full-screen bloom,
which would blur the ink lines the look depends on.

## Camera feel

The follow camera's aim and view height take three offsets, all worked out from the
world and the pose history, never from frame-to-frame state:

- **Speed.** Velocity is measured over the last 8 ticks of the history (continuous
  between ticks), so a single tick's bump doesn't jolt the camera. `speed_zoom` widens
  the view by `smoothstep(speed / top speed)` of itself. As the ship slows to arrive, the
  view eases back in.
- **Lead.** The aim moves `lead` seconds of that velocity ahead, at most 30% of the view,
  and eases back onto the ship as it stops.
- **Shake.** Two damped waves (6 and 11 a second) out of step on each axis, seeded by
  the shake's tick, fading as the square of the time left. The first instant is a kick
  one way. Eye and aim move together, so framing and tilt don't change.

**Reduced motion.** `galaxy play --reduced-motion` (and `galaxy shot`) turns all three
off. It is the player's setting, not the world's: notes keep it in their moment, so a
redraw matches what the player saw. Room cameras and glides never take camera feel.

## Measured

`tests/play/effects-inputs.toml` flies, turns, mines and docks at 1280x720 (a release build
on the development PC). Frame interval median 16.67 ms, p95 18.3 ms (60 fps), scene build 0.08 ms
median. With three emitters held at the particle cap: interval median 16.67 ms, p95
18.3 ms, scene build 0.6 ms median.
