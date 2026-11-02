# The model generator

How the GM makes a module's models from recipes in its data. The code is `crates/recipe`
(recipes to meshes to glTF, no GPU), on `crates/mesh` (the mesh builder and the glTF
writer); `galaxy-gm model make` (`crates/gm/src/model.rs`) runs it on a module folder.

```sh
galaxy-gm model make tests/play/yard           # recipes/*.toml -> models/*.gltf
galaxy-gm model make tests/play/yard --check   # are the committed models what the recipes make?
galaxy-gm fields recipe                        # every shape, key, range and default
galaxy play --modules tests/play/yard          # fly round what it made
```

## Recipes

A recipe is `recipes/<name>.toml` in a module and makes `models/<name>.gltf`, so the model
is `<module>.<name>` like any other. It is a list of parts:

```toml
about = "a small station"
seed = 3                       # random variation: rock lumps and chips

[[part]]
name = "core"                  # the glTF node, so the part galaxy-gm model and the ink see
shape = "tiers"                # a shape, with its own keys
radius = 4.0
height = 5.0

[[part]]
name = "arms"
shape = "truss"
length = 7.0
width = 0.8
angle = 30.0                   # placed round the centre, turned to face out
distance = 9.5
copies = 3                     # three of them, 120 degrees apart

[[part]]
name = "thrusters"
use = "thruster"               # another recipe of this module, as a preset
at = [-2.0, 0.55, 0.0]
mirror = true                  # and its left-right twin, in the same part

[[marker]]
name = "spot.dock"             # an empty named node: a room's camera, a spot, any point
angle = 90.0
distance = 11.5
turn = 180.0
```

- **Shapes** are generic: primitives (`box`, `cylinder`, `lathe`, `ring`, `sphere`,
  `plate`, `strut`) and the bigger pieces the slice's station and hull kits were made of
  (`tiers`, `spoked_ring`, `truss`, `mast`, `tanks`, `panel`, `dome`, `beacon`, `block`,
  `scaffold`, `spire`, `petals`, `cabins`, `body`, `wing`, `fin`, `nozzle`, `rock`). Each
  declares its keys with ranges and defaults in one table (`crates/recipe/src/shapes.rs`),
  which is what the recipe is read against and what `galaxy-gm fields recipe` prints.
- **Placement**: `at`, or `angle` and `distance` round the model's centre with the part
  turned to face outwards; then `turn`, `pitch`, `roll` and `scale`. `copies` repeats a
  part round the centre and `mirror` adds its left-right copy. Axes are the model's: x
  its front, y its left, z up.
- **Paint** is by palette slot. `recipes/palette.toml` holds `slot = "#rrggbb"` lines, a
  recipe's `[palette]` adds or replaces slots, and a shape paints with up to five roles:
  `paint` (slot `main`), `trim`, `accent`, `lights` (fully self-lit) and `glass` (a little
  self-lit). A role can name another slot or a literal colour. A used recipe is painted
  with its user's slots first, so a fitted-part preset takes the hull's colours, and a
  `use` part can repaint it with `palette = { ... }`.
- **Rocks** are the one shape with a morph target: with `mined` above 0, the target is the
  same rock chipped by flat cuts, which carry a bite, and shrunk. `engine.model` `morph`
  then runs it from whole to mined (`renderer-and-style.md`).
- **Markers** are empty named nodes after the parts, with a pose: `turn` and `pitch` say
  where they look, so a `camera` marker looks down at the room and a `spot.<name>` faces
  the way a being stands (`rooms-and-travel.md`).

Each `[[part]]` becomes exactly one glTF node, named by `name` (default: its shape or
used recipe, `.2`, `.3` after the first). That is the model contract's part: the renderer
sizes ink per node, and `galaxy-gm model` and `galaxy-gm motion flicker` name parts by it.

**A bad recipe is refused, naming the field.** Every key is read against its table:
an unknown key, a missing one, a value out of range, a slot the palette lacks, a shape
that doesn't exist, a recipe that uses itself, or a part name used twice. The refusal is
one JSON object, exit 4, with `recipe` (the file) and `field` (`part[2].radius`,
`palette.trim`, `marker[0].pitch`), and nothing is written: `model make` makes every
recipe before it writes any. Limits keep a recipe inside a module file: 64 parts, 32
copies a part, 100,000 triangles and the loader's file size.

**What a recipe makes is the module's business.** The engine and GM know shapes, not
things: no shape is one particular station, ship or rock, and a hull's features (wings,
fins, engines, canopies, cargo pods) are generic parts placed by the recipe. Presets are
recipes too: the hull a module offers in three sizes is three recipes, or one used three
times with `scale`.

## The same bytes on every OS

A recipe gives a byte-identical file on Windows, Linux and macOS, so the committed models
are checked like code and a recording or hot-load hashes the same everywhere:

- **Portable maths.** Trigonometry goes through `libm` (the mesh builder's lathe and
  torus, the shapes' rotations), and glam is built with its `libm` feature, so nothing
  calls the platform's C library. Everything else is plain arithmetic and square roots,
  which IEEE 754 rounds the same everywhere; Rust never fuses a multiply and an add on
  its own. The generator avoids glam calls that use fused multiply-add on ARM (`lerp`,
  `mul_add`).
- **Seeded variation.** Lumps and chips come from the recipe's `seed` through the
  generator's own SplitMix64, forked per part, so adding a part doesn't move the others.
- **Fixed order.** Parts and markers are written in recipe order, accessors and buffer
  views in a fixed order, JSON keys sorted, floats in serde_json's shortest round-trip
  form, lines ending in `\n`.

`crates/recipe/tests/recipes.rs` pins the SHA-256 of every model in the yard test module
(`tests/play/yard`) and checks the committed files are what the recipes make. It runs in
`cargo xtask check` on Windows and in `cargo xtask linux-check`, so a platform difference
fails one of them. Apple Silicon is expected to match: glam's NEON code does the same
non-fused operations for the matrix and vector maths the generator uses.

## Alternatives, and why this design

- **Shape code per thing (the slice).** The slice built its five stations, three hulls,
  fitted-part looks and rocks in Rust, with named variants such as one hull class per
  ship. That puts game nouns in the engine, and the GM couldn't add a ship without a
  build. Here the code is a vocabulary of generic shapes and the things are data.
- **Only primitives.** Boxes, lathes and rings alone can make anything, but a station core
  with a ring of lit windows is dozens of parts; the bigger shapes carry the slice's
  proportions, which were tuned on screen, so a recipe stays short enough to read.
- **Free-form expressions or scripts in recipes.** Parameters as formulas, or Luau that
  builds meshes, would be more flexible, but slower to check, harder to refuse with a
  named field, and a second way to be non-deterministic. Copies, mirror, presets and the
  seed cover the variations so far.
- **Generating at load.** The engine could build models from recipes each time it starts.
  Committing the glTF keeps one model contract (hand-made and generated files load the
  same way), lets Blender files sit beside generated ones, and keeps recordings and
  hot-loads unchanged; `--check` and the hash test keep the files honest.
- **Binary glTF or quantised positions.** Not needed: the text files stay well under the
  loader's size limit, and the hash is of the exact text.
