# Renderer and style

How the engine draws a module's world: models in the cel and ink style, the camera, and
the window loop. The code is `crates/mesh` (meshes and the model contract, no GPU),
`crates/render` (the renderer on wgpu), `crates/view` (the world as a scene) and
`crates/galaxy/src/play.rs` (`galaxy play` and `galaxy shot`).

```sh
galaxy play --modules tests/play/drift          # a window; Esc quits, the wheel zooms
galaxy shot --modules tests/play/drift --out a.png --ticks 60
```

## Models: the contract

A model is a glTF 2.0 file in a module, `models/<name>.gltf`, with its buffers embedded as
`data:` URIs. Its ID is `<module>.<name>`. A thing wears one through `engine.model`:

```toml
"engine.model" = { model = "example.ship", look = "example.hull", scale = 1.0, morph = 0.0 }
```

What the engine reads (`crates/mesh/src/gltf.rs`):

- The default scene's nodes with meshes, through their transforms. **Each node is one
  part.** A part's extent sizes its ink: a thin antenna keeps a hairline while a hull
  keeps its full silhouette weight.
- Triangles. `POSITION` is required; `NORMAL` is the shading normal (flat face normals when
  it is missing); `COLOR_0` times the material's base colour is the paint. Textures are
  not read: paint is per vertex, which is what the flat cel bands want.
- **Glow**, how self-lit a surface is: the material's largest emissive factor, or a
  `_GLOW` float per vertex. Glowing paint ignores the light.
- `_BITE` (a float, or a `VEC2` for now and at the morph target): a fresh cut, kept out of
  the shadow tint and hatching.
- The **first morph target**'s `POSITION` and `NORMAL`: the shape at `morph = 1`. Shape
  changes such as mining a rock are a weight, not a new mesh.
- **Axes**: glTF is +Y up with the front facing +Z. The world is +Z up and a thing at
  `angle = 0` faces +X. So glTF `(x, y, z)` is world `(z, x, y)`, and a model's front faces
  where its thing heads. One unit is one world unit.

What the engine works out at load, so modellers and generators never supply it:

- **Outline normals**: the average of the distinct face normals meeting at each position.
  The silhouette is pushed out along these, so hard corners stay closed.
- **Part spans** (minor and major extent of each node) and **facet spans** (of each
  triangle). The shader thins ink on parts and facets only a few pixels across, so a
  distant thing stays coloured instead of turning solid ink.

`galaxy-gm model <module> [<model>]` says what a model is without reading its glTF: each
part (node) by name with its triangles, size and place in the model's own frame, its glow,
bite and morph; the outline collision uses (how far it reaches front, left, back and
right); and the things whose start state wears it (`galaxy_mesh::gltf::read_parts` keeps
which vertices each node became).

`galaxy_mesh::gltf::write` writes the same format from meshes, one node per part. The
test models are made that way (`crates/galaxy/tests/drift.rs`, `make_the_models`), and
the GM's model generator will be too.

**Why glTF text with embedded buffers.** glTF is the open interchange format every tool
exports, so a model can come from a generator or from Blender. Keeping it one text file
means the module loader reads it like a script: it is hashed, stored in recordings (a
replay draws the models the session had) and hot-loaded when it changes. Binary `.glb`
would need a second file path through the loader and recordings, for a size saving that
does not matter at these polygon counts. **Why derive outline data at load** rather than
ship custom attributes: any glTF exporter works, and the rule that computes it lives in
one place.

## Style: `style.toml`

How a module wants things drawn. Every key is optional; the engine's defaults are the
style the slice settled on.

```toml
[ink]                     # inner ink, for the whole scene
color = "#2a1a2e"
inner_px = 2.0            # line width, logical pixels
crease_degrees = 50.0     # edges sharper than this get a line
color_edge = 0.22         # a paint border this strong gets a line
depth_step = 0.55         # a depth break this big gets a line (world units)

[light]                   # for the whole scene
sun_direction = [-0.35, 0.27, 0.90]   # towards the sun; high, so tops stay lit as ships turn
band_fade_degrees = 25.0  # a face fades between tone bands over this much turn (0 to 90)
sun = "#fff1dc"
ambient = "#2c2240"
rim = "#ffb36b"
rim_strength = 0.55

[sky]                     # the backdrop, for the whole scene
top = "#4e3672"
bottom = "#2a1f48"
clouds = ["#c2577a", "#6b4aa0", "#2f8f8a", "#e8905a"]
cloud_alpha = 0.2
cloud_count = 16
stars = 260

[course]                  # the followed ship's order, for the whole scene
show = true
color = "#ffcf6b"
dot = 5.0                 # dot size, logical pixels
spacing = 16.0            # dot to dot, logical pixels
ring = 2.0                # ring line width, logical pixels
bright = 2.5              # seconds at full strength after an order
faint = 0.35              # strength after that

[looks.hull]              # a look; things wear it as `look = "<module>.hull"`
outline_px = 3.5          # silhouette width, logical pixels (0 to 16)
hatch = 0.25              # hatching in the shadow band (0 to 1)
shadow = "#9e84cc"        # multiplies paint in the shadow band
highlight = "#fff6ea"     # multiplies paint in the highlight band
bands = 3                 # 2 or 3 tone bands
```

Looks belong to their module and are named by full ID. The scene-wide tables (`ink`,
`light`, `sky`, `course`) describe one scene, so one module sets each; when two do, the last one in
module order is used and `galaxy-gm check` reports it. A broken style file, a broken model,
or a thing wearing a model or look no module has is a finding in `galaxy-gm check` and
shows in a panel at the top left while playing (a missing model draws as a small box).

**Why looks, not per-thing colours or roles.** The slice had fixed ink roles (station,
ship, rock, prop) compiled in. Those are game nouns; a look is a named bundle the module
defines, so the module decides what kinds of things there are.

## The renderer

`crates/render` draws one `Scene` per frame: a camera, lights, mesh instances, lines,
glows, a backdrop and a screen overlay. It never reads the world or a clock. A frame runs
these passes:

1. **Sky**: a vertical gradient, then soft nebula clouds and stars on a tile that repeats
   and scrolls slower than the camera (parallax), so it never competes with the play
   field. Generated from the sky style and the world's seed.
2. **Meshes**: each instance's paint is lit by the sun and quantised into two or three
   flat bands (shadow, mid, highlight). The shadow band gets sketched hatching, a rim
   light catches edges turned away, glow ignores light. Instances of the same mesh are
   drawn together.

   A flat face has one light level, so a hard band step switches a whole face at once:
   a ship turning at 180° a second flipped half its hull from lit to shadow within a
   frame. Each face therefore eases between bands over `band_fade_degrees` of turn
   (25° by default, about eight frames at full turn and 60 fps). Filmed on a ship flying a tight
   circle (`galaxy-gm motion`), the biggest one-frame brightness jump fell from 4.9 to
   1.5. The default sun is high (about 64°), so a ship's top stays lit through turns
   and only steep sides go to shadow; a lower sun shows more form on rocks but makes
   turning ships flip more. Smooth light normals were tried too: as calm, but the
   faceted look is the style.
3. **Silhouette**: the inverted hull. Back faces are drawn again, pushed out in screen
   space along the outline normal by the look's `outline_px`, in ink. Thinner on small
   parts, gone below about a pixel.
4. **Geometry buffer**: normals, paint and depth at the same four sample positions as
   colour, then the **inner ink** pass draws each sample where neighbours disagree:
   a crease sharper than `crease_degrees`, a paint border stronger than `color_edge`,
   or a depth step bigger than `depth_step` (scaled by distance).
5. **Effects**: lines (lanes, rings, beams, as ribbons facing the camera) and soft
   additive glows, depth-tested against the geometry samples, over inner ink.
6. **Resolve**: average the finished colour samples into the world raster, including ink.
7. **Downsample**: the world raster is twice the output width and height. Average each
   2x2 group in linear colour into one output pixel, giving sixteen coverage samples
   per pixel with MSAA x4. Nothing outside that pixel's footprint is mixed in.
8. **Overlay**: screen-space panels (rounded, bordered, shadowed) and text, in layers; the
   screen system builds on this.

Text uses glyphon (cosmic-text shaping) with one bundled font (Fredoka, SIL OFL), so a
shot looks the same on every machine. MSAA x4 where all the colour, geometry and depth
formats support it; otherwise those passes share the same single-sample fallback.

**Why ink before resolve.** A single-sample geometry buffer made inner lines jump a
whole pixel as the camera moved, even though the surrounding colour was antialiased.
The ink shader now reads the matching sample in each neighbouring pixel and blends
into that colour sample. Geometry interpolates depth, normal and paint at the sample
position too, so the plane test does not compare pixel-centre depths at covered edges.
Only the final colour is resolved. Resolving geometry first would blend unrelated
normals, packed depth bytes and paint at boundaries, inventing creases and surfaces.
Sample coverage keeps the existing crease, paint, depth and feature-size rules.
Effects remain in front of ink and use the same depth coverage.

**Why a denser world raster too.** Four samples removed the repeated inner-ink flicker,
but a long silhouette could still pop as its coverage changed; disabling inner ink
did not remove it. Drawing the world at twice each dimension adds subpixel coverage
to colour, silhouettes, geometry, ink and effects together. The box downsample keeps
flat faces flat and strokes sharp. A spatial edge filter was tried, but softened
strokes while still leaving a small flicker run. Temporal accumulation would need
history and disocclusion handling. Neither is needed for this path.

All style sizing still uses the output viewport: silhouette width, feature-size fades,
subpixel feature culling and hatch spacing keep their native-pixel meaning. Only the
ink stencil offsets scale with the denser raster. The overlay is drawn after the
downsample at native resolution. Resize rebuilds both sets of targets and bindings.
If doubling would exceed the device's texture dimension limit, the world raster uses
native dimensions and the final pass copies it. Without MSAA the denser raster still
provides four coverage samples per output pixel. The denser raster costs four times
the world target storage and raster work; frame pacing is checked with the real bot
window, not inferred from offline film speed. This changes presentation only, with no
world or replay state.

**Why inverted hull plus a post pass.** The hull gives a clean, width-controlled
silhouette that reads at any zoom; the post pass finds inner edges a hull can't (creases,
paint borders, one part in front of another). Post-process edges alone wobble on
silhouettes and can't be sized per look; hulls alone miss every inner line.

## The view: world to scene

`crates/view` reads the world and never writes it, so drawing can't affect a replay.

- Every entity with `engine.position` and `engine.model` is drawn at `(x, y, height)`,
  turned by `angle` about +Z, scaled by `scale`, morphed by `morph`, in its look.
- **Interpolation.** The sim steps at 30 ticks a second; the screen runs at the display's
  rate. Before each tick the view keeps the poses, and each frame draws `alpha` of the
  way from the last tick to the current one (the fixed step's leftover time), turning the
  short way round. Motion is smooth at 60 or 144 Hz with no change to the sim.
- **The camera** follows the first entity, in ID order, with `engine.camera`. `tilt`
  leans it back from straight down (30 by default shows the sides of things), `view` is
  the world units it shows from the bottom of the screen to the top, and `fov` above 0
  makes it a perspective camera with the same framing at the target. The wheel zooms
  between `min_view` and `max_view`. Zoom is the player's, not the world's, so it is not
  a sim input: the world never sees it. A played session's recording keeps it beside the
  inputs as presentation only, so a film of the session shows the zoom the player had,
  and an inputs file can turn the wheel with `[[zoom]]` (`saves-and-replay.md`).

- **The course** (`crates/view/src/course.rs`). While the thing the camera follows has
  an order under way (`engine.steer`, `flight-and-collision.md`), round dots run from it
  to where it is going, through the waypoints left on a route, and a ring marks where it
  ends, just outside what the target wears. The leg being flown is brightest; later legs
  are fainter with a small ring on each waypoint.
  - **Still dots.** The dot pattern is measured from where the route stops, so the dots
    stay put in the world and the ship flies over them; the dots nearest the ship fade
    rather than pop. (Measured from the ship, the whole pattern slid along as it flew,
    like a chain being reeled in, which pulled the eye.)
  - **Calm over time.** Full strength for `bright` seconds after an order, then it
    settles to `faint`. On arrival the ring closes in a little and fades, once; when the
    player takes the controls the whole course fades in a fifth of a second, so it is
    clear the order is gone. Nothing loops.
  - **Crisp at any zoom.** Sizes are logical pixels; dots are drawn round in the shader
    with a one-pixel soft edge and a thin ink rim that lifts them off the backdrop.
  - Only the followed thing shows a course, so other pilots' orders don't clutter the
    view; `[course]` recolours or hides it.

Cameras for places other than flight (stations, interiors, moving between them) wait for
the place model.

## Playing: `galaxy play` and `galaxy shot`

`play` opens a window (1280x720 logical pixels unless `--size`) and runs the session in
real time:

- Each frame asks the fixed step how many ticks are due (at most 5, so a stall never
  fast-forwards the world), runs them, builds the scene and draws it.
- Keys a module binds (`[keys]` in its manifest, `modules-and-install.md`) become its
  events as inputs on the next tick, from the source `keyboard`. Key repeat is ignored,
  and losing focus lets go of every held key, so none sticks down. Escape quits.
- `--watch` hot-loads a changed module between ticks and reloads its models and style.
- Every session records itself into a session folder, and F8 takes a note with the frame
  that was shown (`saves-and-replay.md`). `--record` and `--save` also write the
  recording and the save to those paths when the window closes; `--frames <n>` quits
  after n frames.
- **The bot**: `--inputs <file>` plays scripted inputs on their ticks through the same
  loop the keyboard feeds, and `--ticks <n>` quits when the world reaches tick n. A bot
  run is a real play session, window and all, so its frame times are the player's.

When it quits it prints **frame times**, the numbers to watch for drift: `interval_ms`
(frame to frame, what the player feels), `sim_ms`, `scene_ms` and `draw_ms` (handing the
frame to the GPU, which waits for the display at vsync), each as median, p95, p99 and
max, skipping the first ten frames. Times from a software renderer or a virtual display
are for information only; the 60 fps target is for the owner's PC.

`shot` runs the same session headless for `--ticks` (30 by default) with optional
`--inputs`, draws the last tick at `--size` pixels and writes a PNG. It draws one still
moment (`alpha` 1, time 0), so it can't show motion; `galaxy film` draws every frame on a
fixed clock for that (`motion-tools.md`). It reports the camera,
the models drawn and the scene build time. It needs a GPU (or a software one such as
lavapipe or WARP); tests that render are `#[ignore]`d so the check stays headless.

**Why the window loop lives in the game binary and the view in a library.** The GM draws
too (a recording's notes again, films of it), so everything between the world and the
renderer is a library both binaries share; only the window and keyboard are the game's.
A view can also be made with no renderer (`View::detached`): it builds the same scenes,
whose meshes draw nothing, so a film's probes need no GPU.

## Module screens

`View` builds module widget trees through the read-only screen host. Rust lays out text
with the headless shaper in `galaxy-render::text`, the same font/metrics as GPU drawing.
`Overlay.widgets` holds ordered panel/text layers between the base and the three engine
chrome layers. GPU storage and glyphon renderers grow with the stream, preserving every
widget's paint order. The fixed chrome layers remain for notes and diagnostic panels.
Named `engine.anchor` publishers project interpolated positions into this layout each
frame. Left clicks hit-test the tree and record the chosen event as source `ui`; bot
`[[click]]` entries use the same path. See `screens.md` for the API and readability rules.

Screen widget layers can carry a logical-pixel clip rectangle. The overlay pass scales
and intersects it with the physical viewport and applies the same scissor to panels
and glyphs; empty clips draw nothing. Nested scrolling intersects ancestor clips before
submitting layers. Headless layout and hit tests use those same logical rectangles.
