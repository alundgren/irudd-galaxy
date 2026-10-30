# render

The Lantern Reach renderer: a small 3D scene renderer on wgpu with a cel-shaded,
ink-outlined look, plus `kit`, a procedural parametric mesh kit.

It depends on no sim crate. The app turns world state into a `Scene` each frame and
hands it to a `Renderer`, so the simulation never knows how anything is drawn.

![The demo overworld](docs/overworld.png)

![Every kit piece: stations, greenhouse stages, hulls, rocks as they are mined](docs/kit-gallery.png)

Shading stays steady while a rock is mined and turns (top), the mining beam runs from
the nose to the rock's surface (left), and silhouette ink follows the part, not the
facet (right):

![Two frames of a mined rock and their difference, before and after](docs/render-fix-shading.png)

![The mining beam, before and after](docs/render-fix-beam.png) ![Hull ink, before and after](docs/render-fix-ink.png)

## Run it

From `slices/01-lantern-reach/`:

```text
cargo run -p render --example showcase                    # window: WASD, Space mines, P pauses
cargo run -p render --example shot -- out.png             # headless PNG of the demo world
cargo run -p render --example shot -- out.png --paused    # ...with the PAUSED card
cargo run -p render --example shot -- out.png --gallery   # every kit piece, labelled
cargo run -p render --example frames -- out_dir           # consecutive frames of a rock being mined
cargo test -p render                                      # CPU tests (no window, GPU or network)
cargo test -p render -- --ignored                         # GPU tests (needs an adapter)
```

`shot` also takes `--size WxH`, `--tilt DEG`, `--zoom H`, `--center X,Y` and `--time S`.
In the showcase, Q/E or the mouse wheel zoom, T toggles the tilt and F12 saves
`showcase.png`. `showcase -- --shot out.png` flies and mines on its own for two seconds,
saves a frame through the window renderer and quits.

## Conventions

- World units, right-handed, **Z up**. The overworld play plane is `z = 0`: a sim position
  `(x, y)` is world `(x, y, 0)`. In the default camera +X is right and +Y is up the screen.
- Meshes face +X: a hull's nose points along +X, so `MeshInstance::at(mesh, pos, heading,
  scale)` points it along `heading` (radians, counter-clockwise from +X).
- Rough scale used by the kit: a Wren is 6 long, a rock 2–5 in radius, Lantern Wharf about
  31 in radius. A comfortable overworld view is 80–120 units tall.
- Colours are linear `Color`s, authored as sRGB hex (`Color::hex(0xf2c14e)`) and stored as
  `"#f2c14e"` in serde.
- The renderer never reads a clock. `Scene::time` drives shader animation (beam shimmer);
  pass a fixed value for repeatable shots.
- Overlay coordinates are logical pixels, top-left origin. `Renderer::logical_size()` is
  the viewport to pass to `Camera::world_to_screen` / `screen_to_plane`.

## Public API

```rust
use render::{Camera, Color, Renderer, Scene, MeshInstance, Material, Line, Glow,
             Panel, Rect, Text, Align, PausedCard, PointLight};
use render::kit::{self, HullParams, RockKind, RockParams, StationRecipe};

// Window (winit 0.30) or headless.
let mut r = Renderer::new_windowed(window.clone())?;     // then r.resize(w, h) on Resized
let mut r = Renderer::new_headless(1280, 720)?;           // for `galaxy shot`

// Upload once.
let wren = r.upload_mesh(&kit::hull(&HullParams::wren(), seed));
let wharf = r.upload_mesh(&kit::station(&StationRecipe::lantern_wharf()));
let sky = r.upload_backdrop(&kit::backdrop(&kit::BackdropParams::default(), seed));

// Every frame.
let mut scene = Scene::new(Camera::top_down(center, 100.0).with_tilt(20.0));
scene.backdrop = Some(sky);
scene.meshes.push(MeshInstance::at(wren, ship_pos, heading, 1.0));
scene.lines.push(Line::path(points, 1.0, color).dashed(3.5, 2.5).wobbly(0.5));
scene.lines.push(Line::beam(from, to, 1.6, Color::hex(0x7ff0d8)));
scene.overlay.texts.push(Text::new("Bring ice to Kettle Dock", pos, 24.0).weight(600));
scene.overlay.paused = paused.then(PausedCard::default);
r.render(&scene)?;                                       // window: draw and present
let png = r.render_to_rgba(&scene)?.encode_png()?;       // any renderer: read back
```

| Item | What it is |
|---|---|
| `Renderer` | `new_windowed(Arc<Window>)`, `new_headless(w, h)`, `resize`, `set_scale_factor`, `logical_size`, `upload_mesh` / `update_mesh` / `remove_mesh`, `upload_backdrop`, `measure_text`, `render`, `render_to_rgba` |
| `RgbaImage` | `width`, `height`, `pixels` (sRGB RGBA8), `pixel`, `encode_png`, `save_png` |
| `Scene` | `camera`, `lights`, `clear_color`, `backdrop`, `meshes`, `lines`, `glows`, `overlay`, `time` |
| `Camera` | `top_down(center, view_height)`, `.with_tilt(deg)`, `perspective(eye, target, fov)`, `view`, `projection`, `view_proj`, `world_to_screen`, `screen_ray`, `screen_to_plane`, `world_per_pixel` |
| `Lights` | key `sun_direction` / `sun_color`, `ambient`, `rim`, up to 8 `PointLight`s |
| `Material` | `tint`, `shadow`, `highlight`, `bands` (2 or 3), `outline`, `outline_px`, `emissive`, `hatch` (serde) |
| `Line` | `path`, `circle`, `beam`, `.dashed(dash, gap)`, `.wobbly(amplitude)`, width in world units |
| `Overlay` | `panels: Vec<Panel>`, `texts: Vec<Text>`, `paused: Option<PausedCard>` |
| `MeshBuilder`, `MeshData` | build your own meshes (`cuboid`, `lathe`, `frustum`, `torus`, `extrude`, `quad`, `tri`, `tri_shaded`, `part`); `MeshData::raycast` |
| `palette` | the warm swatches (`INK`, `PARCHMENT`, `CORAL`, `MUSTARD`, `TEAL`, `LANTERN`, ...) |
| `demo` | sample code: a `DemoWorld` mapped to a `Scene`, and `gallery()` |

### The kit

All generators are CPU-side, deterministic for a seed and unit tested.

- `kit::hull(&HullParams, seed)`: `HullParams::wren()`, `tandem()`, `heron()` or any
  params (length, width, height, nose, wing span and sweep, engines, cargo pods, fin, mast,
  facets, colours, and a `mark`). Out-of-range values are clamped. Every hull has a dorsal
  stripe and a tail pennant. The Wren adds cheek pods, the Tandem a box cab, the Heron
  canards and a wide mast dish. `mark` is `lantern_kettle`, `toll_key`, `open_lamp`, or
  empty; the Wren preset carries the kettle.
- `kit::rock(&RockParams { kind, radius, remaining }, seed)`: ice, iron or glassite. As
  `remaining` drops the rock shrinks (`kit::rock_scale`), gets planar chips with a warm
  bitten face and inked cracks, and glassite crystals wear down and break off; at 0 it is
  a few pebbles with warm caps. The same seed keeps the same rock, and chips, cracks and
  crystals grow in gradually, so one step changes it only a little. Re-mesh with
  `update_mesh` when `remaining` crosses a visible step (the demo and the game use 1/20),
  and in between scale the instance by `rock_scale(now) / rock_scale(meshed)` so it
  shrinks smoothly.
- `kit::station(&StationRecipe)`: a station is a serde recipe of parts, a palette and
  decals (`stripe`, `chevron`, `sigil`, `number`). `StationRecipe::validate()` reports
  every problem with its part index; `validate_art` checks the recipe against
  `assets/art-bible.toml`. `kit::station_lights` gives lantern and beacon positions for
  glows and point lights; `kit::docking_ring_center` finds the ring. Presets:
  `lantern_wharf()`, `kettle_dock(stage)` (greenhouse stages 0–3), `cinder_hook()`,
  `glass_drift_observatory()`, `brassmouth()`.
- `kit::backdrop(&BackdropParams, seed)`: a quiet gradient, a few nebula clusters and sparse
  stars on a repeating tile that scrolls with a small parallax.

A recipe a game master could write (missing fields use defaults):

```json
{
  "name": "Sixth Station",
  "palette": { "hull": "#f0d9b5", "accent": "#c2577a" },
  "parts": [
    { "part": "hub", "radius": 5, "height": 4 },
    { "part": "arm", "length": 8, "width": 2, "at": { "angle": 45, "distance": 5 } },
    { "part": "greenhouse", "radius": 4, "stage": 2,
      "at": { "angle": 45, "distance": 17 }, "color": "#8fbf7a" },
    { "part": "docking_ring", "radius": 10 }
  ]
}
```

Part kinds: `hub`, `docking_ring`, `arm`, `greenhouse`, `antenna`, `tank`, `sail`,
`lantern`, `lens`, `cargo_pod`, `crane`, `solar_petal`, `lamp_mast`, `dome`, `habitat_ring`,
`beacon`, `signage`, `antenna_cluster`, `docking_arm`, `banner`, `arch`, `spire`, `block`,
`scaffold`.
`at` is `{ angle (deg), distance, z, turn (deg), scale }`; in a part's frame +X points away
from the station centre.

## How it draws

One MSAA (4x) world pass with a depth buffer, an inner-ink pass, then an overlay pass:

1. Backdrop: a vertical gradient with a soft vignette, then instanced soft nebula puffs and
   stars on repeating tiles.
2. Meshes: cel shading with 2–3 hard tone bands from a half-Lambert key light, warm violet
   shadows (never gray), a hard rim light, stepped point-light pools, hatching in the shadow
   band, and self-lit paint (vertex alpha) for windows, lanterns and engines. Band edges
   are hard steps smoothed over one pixel; hatching is a fixed pixel spacing anchored to
   the object, so it moves with it rather than swimming as the camera pans. Most kit parts
   are flat shaded; a rock's skin carries smooth normals (`MeshBuilder::tri_shaded`), so
   its bands follow the lumps as it turns instead of whole facets flipping band.
3. Ink outlines: an inverted hull. Back faces are pushed out along normals averaged across
   hard edges, so outlines don't crack at corners. Weight follows the size of the part a
   facet belongs to (`MeshBuilder::part`; every primitive is one): a part a dozen pixels
   thick keeps the art-bible weight, however finely it is faceted; a thinner one thins to
   a hairline, and a part under about a pixel is not drawn. Works the same with a
   perspective camera. This is the silhouette only.
4. Inner ink: a second, non-multisampled pass writes an octahedral normal and a 16-bit
   view depth into an `Rgba16Float` target (8-bit normals wobbled enough to flicker a crease
   near the art bible's angle as a ship turned) and the unlit paint into an `Rgba8Unorm`
   one. Paint alpha stores how many pixels the facet's own minor axis covers. A fullscreen pass then draws a line where
   neighbouring surface pixels crease harder than the art bible's angle, where the paint
   changes, or where depth breaks the local plane by a step that is large for that view
   depth. A continuous slope is not an edge, and features only a few pixels across get no
   inner line. Pixels with no surface neighbour are left to the hull outline.
   `textureLoad` and core-WebGPU formats keep the pass the same on DX12, Vulkan and Metal.
   Widths come from `assets/art-bible.toml`.
5. Lines, beams and glows: drawn after inner ink, depth-tested against the mesh, so a beam
   or glow in front of a crease stays visible. Camera-facing ribbons with soft ink-edged
   sides, dashes and hand-drawn wobble; additive beams and glows. A glow sits its radius,
   and a beam half its width, towards the camera along the line of sight (same pixels), so
   the surface it lands on can't cut it. Beams have soft ends and a shimmer measured in
   beam widths, which reads the same at every zoom. To land a beam on a mesh, end it at
   `MeshData::raycast` (the demo and the game aim at the rock's centre).
6. Overlay: rounded SDF panels with an ink border and soft shadow, glyphon text, and the
   PAUSED card (dimmed world, big centred card) drawn above everything.

## Assets and licences

- `assets/fonts/Fredoka-Variable.ttf`: Fredoka by Milena Brandão and Hafontia, SIL Open Font
  License 1.1 (`assets/fonts/OFL.txt`), from Google Fonts. It is the only font loaded, so
  shots look the same on every machine.
- Dependencies: wgpu, winit, glyphon (cosmic-text), glam, bytemuck, pollster, png, serde.
  All MIT or Apache-2.0 (or Zlib).
