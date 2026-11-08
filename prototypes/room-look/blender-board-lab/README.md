# Blender board lab: one board corner and its first card

A small piece of the arrivals board from the selected goal image
(`game/docs/visual-upgrade/references/room-and-face.png`, crop x530–750, y240–380),
authored in Blender 5.3. Every image here is a **Blender render, not the game runtime**.
Nothing here shows engine support or performance. Scores and acceptance belong to the
coordinator and the owner. This file awards none.

## Open it

Open `board_corner.blend` in Blender 5.3 or later. Scene `BoardCorner`:

- `Cam_Reference` is fitted to 9 landmarks measured on the goal crop (about 3 px RMS
  at crop size). Render at 880×560 to compare with the crop at 4×.
- `Cam_Oblique_LeftHigh` and `Cam_Oblique_RightLow` show the real thickness.
  `Cam_Turntable` orbits the board (frames 1–96, 24 fps). The asset itself is not
  animated.
- Colour management: Standard view transform, exposure 0, look None. EEVEE.

## What is geometry, paint and light

- **Geometry** (collection `BoardCorner_Asset`): `Panel_Backing`, `Frame_Header`,
  `Rail_Top`, `Rail_Left`, `Bracket_Corner`, `Clamp_Top`, `Clip_Left`, `Plaque_01` and
  10 fasteners (raised ring, dark recess, bright metal head `M_Fastener_Head_Metal`).
  The bracket has a rounded inner corner and an upper arm that narrows towards its end.
  Parts are tapered: the front face
  is smaller than the back, so sides and undersides are real faces. Front edges have a
  narrow rounded band (1.2–1.8 mm), marked with the face attribute `edge_band`. The
  sloped sides of the bracket and block use `M_Bracket_Side_Brown`.
- **Outlines**: each raised part has an `Ink_Hull` Solidify modifier. It makes an
  inverted shell with material `M_Ink_Outline` (backface culling on, and shadow culling
  on in Blender).
- **Paint, in three packed 2048² images:**
  1. `T_BoardCorner_BaseColor`: broad colour variation and contact grime, baked from
     the node group `NG_PaintWear` (script `rebake_paint_wear.py`).
  2. `T_BoardCorner_Marks`: 118 deliberate hand-placed marks with transparency. These are
     the ship icon (overlapping planes, broken deck highlights, irregular windows,
     uneven outline), the interrupted cream highlights on upper edges, the plum
     contours, the rust patches at the lower rim, fasteners and junctions, the edge
     losses, short directional scratches, the broken cream strokes on the card's left
     and lower rim with rust beside them, and a lighter patch on the rail near the
     lamp. Every mark is one entry in the text
     `paint_marks.py`, written in goal-crop pixels or board coordinates. The helper
     `paint_marks_engine.py` casts a ray from `Cam_Reference` to find the UV spot, so
     each mark is attached to the mesh. Edit an entry and Run Script. You can also paint
     on this image by hand in Texture Paint mode, then run `flatten_albedo.py`. The
     layer stores straight (unpremultiplied) colour plus alpha. `T_BoardCorner_Marks`
     and `T_BoardCorner_BaseColor` have a fake user so a cleanup cannot delete them.
  3. `T_BoardCorner_Albedo`: the marks laid over the baked wear by
     `flatten_albedo.py`. Every painted material and the glTF export use only this
     image.
- **Light**: `Light_Lamp_Warm`, an orange point lamp above and in front of the top-left,
  where the goal's lamp hangs; `Light_Backing_Warm`, a narrow warm spot on the backing
  strip above the card; `Light_Fill_Cool`, a weak cool area light; and a dim cool world.
  The plain-colour diagnostics turn all lights off and use a white world.

## Goal comparison (checkpoint 5)

- `renders/cp5_goal_comparison.png`: full crop, then the card-and-rail region at 4×,
  then actual size with a 50% blend.
- `renders/cp5_goal_detail_zoom.png`: the bracket and the card at 8×.
- `renders/cp5_reference_camera_neutral_diagnostic.png`: plain colours.
- `renders/cp5_oblique_lit_*.png`: angled lit views.
- `renders/cp5_turntable_24fps.mp4`: 4 seconds at normal speed. A contact sheet is in
  `renders/cp5_turntable_contact_sheet.png`.

Earlier checkpoints keep their renders, and their `.blend` files are in `checkpoints/`.

Differences that remain:

- Neighbouring cards are not modelled. This section stops after the first card.
- The goal's outlines are heavier and its highlights brighter in places.
- The ship is closer to the goal's illustrated icon but still smoother and paler.
- The card's cream rim strokes read at 4× but are subtle at actual size.
- The top beam reads as a plain block in angled views.

## Standard glTF export check

Export: `board_corner.glb` (1.72 MB). It was made with Blender's glTF 2.0 exporter: GLB,
modifiers applied, no lights, cameras or animation. It holds 18 meshes, 15 materials and
7,300 triangles, including the outline shells. Buffers are embedded, with no external
files. The one embedded PNG, `T_BoardCorner_Albedo`, is 1.31 MB; on the GPU, a 2048²
RGBA8 texture is 16 MiB uncompressed, about 21 MiB with mipmaps. Extensions used:
`KHR_materials_specular`. `M_Ink_Outline` exports with `doubleSided: false`. All other
materials are double-sided.

`reimport_check.py` runs Blender in the background without saving:
`blender -b board_corner.blend --python reimport_check.py -- <out_dir> <prefix>`. It
copies the authored scene, unlinks the authored asset and imports the GLB into the
copy. Both are rendered with the same camera, lights, world, Standard view, exposure 0
and 880×560 size. A checksum confirmed the authored `.blend` stayed unchanged.

| View (checkpoint 5) | Default import, mean abs diff | Outline shadow culling restored |
| --- | --- | --- |
| Reference, plain colours | 0.00/255, max 2 | n/a |
| Reference, lit | 38.0/255 (35% of pixels > 24) | 0.00/255, max 6 |
| Oblique left high, lit | 19.3/255 | 0.00/255, max 2 |
| Oblique right low, lit | 24.8/255 | 0.00/255, max 9 |

Images: `renders/cp5_export_reimport_comparison.png` (columns: source, default import,
shadow culling restored, difference), plus `renders/cp5_export_src_*.png`,
`renders/cp5_export_glb_*.png` and `renders/cp5_export_glb_hullnoshadow_*.png`.
Checkpoints 3 and 4 keep their results under `cp3_` and `cp4_`.

**The outline shell and shadows.** In the source, `M_Ink_Outline` has Blender's EEVEE
material setting `use_backface_culling_shadow = True`. Its inward-facing shell is then
skipped in the shadow pass. glTF 2.0 has no field for this setting, and the GLB carries
only `doubleSided: false`. After a default import the setting is off, so the shell casts
a shadow onto its own part (the default-import column above). The "restored" column is
the same import with only one change: `reimport_check.py` sets
`use_backface_culling_shadow = True` on the imported outline material. Tested behaviour:
with that one setting, the lit renders match the source (max 9/255). This is a Blender
EEVEE setting, not a portable glTF flag. A runtime needs its own equivalent: culling
back faces of `M_Ink_Outline` in its shadow pass, or not casting shadows from meshes
that use that material. It also has to respect `doubleSided: false` in the colour
pass.

**Effects that exist only in the Blender source** (not counted as exported results):

- The shadow culling setting described above.
- The procedural `NG_PaintWear` network, the separate marks layer and the mark scripts
  do not export. The GLB carries only the flattened `T_BoardCorner_Albedo`.
- Lights, world, colour management and the turntable camera are scene settings. They
  are not in the GLB.
- Cycles ignores backface culling for camera rays, so this outline method only works in
  EEVEE and in rasterising engines.

## Repository hygiene

Blender's Shading workspace has a file-browser panel that remembers the last folder of a
"Save As", which can be a local home path. Its folder field is a fixed-size buffer and
keeps old text after the end marker. Blender renders also stamp the `.blend` path into
PNG metadata; stamps are off in this scene and the PNGs here were rewritten without
metadata.

Before committing, run on each `.blend`:

```
blender -b --factory-startup --disable-autoexec <file.blend> --python-exit-code 1 --python clean_for_repo.py
blender -b --disable-autoexec <file.blend> --python-exit-code 1 --python validate_blend.py [-- checkpoint]
```

`clean_for_repo.py` overwrites those file-browser fields, keeps a fake user on every
packed `T_BoardCorner_*` image, and saves normally, so the window, workspaces and active
scene stay in the file. (An earlier version used `bpy.data.libraries.write`. That wrote
a library-style file without a window manager or screens and dropped the fake users;
Blender then reported "Library file, loading empty scene".) `validate_blend.py`
reloads the file and fails with exit status 1 unless: the file holds a window-manager
block and screens; it opens on scene `BoardCorner` with camera `Cam_Reference`; the 18
asset objects are present; the paint layers are packed 2048² images with a fake user
and relative paths; the marks layer is not empty; and no private folder text appears in
images, scripts, file browsers or anywhere in the decompressed file. `checkpoint` mode
accepts earlier checkpoints that predate the marks and albedo layers.

The saved viewport looks through `Cam_Reference` in Material Preview, so opening the
file shows the asset straight away.

## Not done yet

- Any runtime test in the game engine, including whether the outline shells and the
  albedo texture look the same there.
