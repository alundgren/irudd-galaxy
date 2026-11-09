# Blender Bram lab: adult head and shoulders (overnight study, target not reached)

A static head-and-shoulders study of Bram, the dockmaster, made in Blender 5.3 against
the face study on the right of `game/docs/visual-upgrade/references/room-and-face.png`.
Every image here is a **Blender render**. Nothing here shows how the asset looks or
performs in the game.

**Result: provisional 2/5** for look, character and graphical quality (the coordinator's
and independent reviewer's assessment, not the owner's). The study did not reach the
target. No expression animation was made.

## Files

| Path | What it is |
| --- | --- |
| `bram_bust.blend` | Editable source (about 19 MB). Scene `BramBust`, camera `Cam_Goal_Tilt`. |
| `bram_bust.glb` | Standard glTF binary export (about 24 MB). |
| `SOURCES.md` | Source, license and hashes of the CC0 base head. |
| `validate_blend.py` | Fresh-load and privacy check for the source. |
| `scripts/` | Every script used to build, edit, paint, export and check the bust. |
| `renders/final_*` | Final evidence (below). |
| `renders/diag_*` | The rejected face diagnostic (below). |
| `renders/cp1_*` … `cp7_*` | Selected evidence from each checkpoint. |

Checkpoint `.blend` files (`checkpoints/`) and other renders stay on the machine that
made them (`.gitignore` in this folder).

## What the source contains

- **Head**: "Head (Animation) - Realistic" from Blender Studio's Human Base Meshes v1.4.1
  (CC0, author Dan Ulrich; see `SOURCES.md`). The untouched original is in the hidden
  collection `HBM_Original`. `Bram_Head` is a copy: its one Multires level applied
  (12,950 vertices), scaled 1.04, a Subdivision modifier, and a Mask modifier hiding the
  shoulders below the collar. Identity and expression edits (`scripts/sculpt_cp4.py`:
  `prepare`, `identity`, `cp5_expression`, `cp6_expression`) are region-based sculpt
  operations on its vertices.
- **Eyes**: spheres with a painted iris image (`T_Bram_Eye`, 512²), turned towards
  `Cam_Goal_Tilt`.
- **Skin**: `T_Bram_Skin`, 2048², packed, on the head's UV map. Painted from named rules in
  `scripts/paint_skin.py` (broad colour, scalp, plum accents at the lash lines, lid
  creases, nostrils, mouth line and ears, warm highlights) and baked. Hand-paintable.
- **Hair** (`scripts/build_hair.py`): a dark scalp volume with directed curl groups (grey
  mostly at the temples), tapered brows, and separate short beard and moustache hairs
  in dark, grey and silver.
- **Cap** (`scripts/build_cap.py`): low slouched crown with broad folds, curved brim, coral
  piping, brass front badge and side button.
- **Clothing**: navy jacket and cream shirt from the first checkpoint, widened at the
  shoulders, with broad folds and a `Paint_Cloth` colour layer (plum joins, warm
  shoulders). A plain raised navy collar, open at the front, with mustard on its inner
  face and edges; navy lapels with mustard edges; cream shirt collar points
  (`scripts/build_clothes_cp7.py`). The earlier funnel collar and old piping are hidden.
- **Lights**: `Bram_Rig_Neutral` (diagnostic, world `World`) and `Bram_Rig_GoalLit` (warm
  orange key, weak violet fill and rim, world `W_GoalLit`, whose violet shows to the
  camera but barely lights the head). Colour management Standard, exposure 0, EEVEE.

## Final evidence

- `renders/final_goal_comparison.png`: concept crop, the bust in the neutral light, the
  bust in the goal light (570×900, `Cam_Goal_Tilt`, from Bram's right, rolled 18° to
  match the concept's head tilt; the tilt is a camera roll).
- `renders/final_talk_size_row.png`: the same at 240×380.
- `renders/final_dressed_neutral_sheet.png`: neutral light, front, side, three-quarter.
- `renders/final_turntable_24fps.mp4`: a 4-second camera orbit for 3D inspection, neutral
  light; 8 frames in `final_turntable_24fps_contact_sheet.png`.
- `renders/final_export_reimport_comparison.png`: source, reimported GLB and their
  difference (×4), for the goal view in both lights, three-quarter and side.

## What still misses the target

- Expression: reserved and slightly pursed from the goal view; the warm half smile with
  the cheek rolled up into the lower lid is not there, and he reads younger than the
  concept. The scripted region edits and the topology diagnostic did not achieve the
  target.
- Hair: grey temple hair reads as grey patches; the moustache is a narrow band.
- Cap: lower and folded, but taller at the back than the concept's and softer-sided.
- Clothing: plain collar and flat lapels; no teal panels, patches, buttons, wear or cloth
  texture; the back of the collar is a plain flat band.
- Light and finish: orange hotspots on the cheek and nose in the goal light; the navy
  jacket reads lilac there; surfaces are broad and smooth.

## Face diagnostic (rejected, kept as evidence)

`scripts/topology_smile.py` moved connected rings of vertices on the base mesh's own
topology: the mouth corners where the inner mouth (face set 7) meets the outer face, the
cheeks grown from the ring outside the lips, and the lower-lid margins where the lid face
sets meet the eye sockets. It relaxed the pucker slightly but added a pinched vertical
crease under his left mouth corner (front view) and a sharp, stretched corner (goal view),
and the gain at talk size was small. The final source uses the cp7 face. The diagnostic
source is kept locally as `checkpoints/bram_bust_diag_topology.blend`; its renders are
`renders/diag_face_goal_view_570.png`, `diag_face_talk_size_240.png` and
`diag_face_front_and_goal_close.png` (concept, cp7 clay, diagnostic clay).

## Export

`bram_bust.glb`: Blender's glTF 2.0 exporter, GLB, modifiers applied, the 27 meshes that
render, no cameras, lights or animation. 982,704 triangles: beard hairs 321,300, scalp
hair 258,232, cap crown 100,480, head 100,128 (Subdivision level 2), shirt 73,350, jacket
53,822, the rest under 25,000 each. Two embedded PNG images: `T_Bram_Skin` 2048² (16 MiB
as uncompressed RGBA8, about 21 MiB with mipmaps) and `T_Bram_Eye` 512². 17 materials,
all base colour factors except skin and eyes. Extension used: `KHR_materials_specular`.
The jacket's two material sections carry `COLOR_0` (the `Paint_Cloth` layer), which glTF
multiplies into the base colour as in the source. Everything is exported double-sided.

Reimport check (`scripts/reimport_check.py`, same cameras, lights, worlds, Standard
view, exposure 0), mean absolute difference per channel: goal view neutral 0.11/255,
goal view goal light 0.25/255, three-quarter 0.12/255, side 0.09/255. The visible
differences are thin lines at the mustard rims of the collar and lapels, where the
Solidify material offsets put the lining material on the edges.

Only in the source, not in the GLB: the lights, worlds (including the camera-only violet
background), colour management, cameras and turntable, the hidden original base mesh,
the scripts and the generation steps, and the Subdivision, Solidify and Mask modifiers as
live modifiers (they are applied in the export).

This proves that the tested content survives a standard export and reimport in Blender.
It says nothing about the game renderer, which currently reads vertex colour and one
morph target, not textures. At about a million triangles, mostly hair, this export is not
a real-time asset: the hair and beard would need cards or baked textures first.

## Reproduce and check

Open `bram_bust.blend` in Blender 5.3 (GUI), add `scripts/` to `sys.path` in the Python
console, show the `HBM_Original` collection in the viewport, then run in order:
`import sculpt_cp4; sculpt_cp4.prepare(); sculpt_cp4.identity(); sculpt_cp4.cp5_expression(); sculpt_cp4.cp6_expression()`,
then `build_hair.py`, `build_cap.py`, `paint_skin.py` with `CREASES=False`,
`HAIR_PAINT=True`, `ACCENTS=True` in its namespace, and `build_clothes_cp7.py`. The
jacket and shirt meshes and their materials come from the first checkpoint
(`build_form_mesh.py` with `torso_form`/`shirt_form`, then `dress_clothes.py`).

Background checks (none of them saves the `.blend`):

```
blender -b --disable-autoexec bram_bust.blend --python-exit-code 1 --python validate_blend.py
blender -b bram_bust.blend --python-exit-code 1 --python scripts/export_glb.py -- <abs path>/bram_bust.glb
blender -b bram_bust.blend --python-exit-code 1 --python scripts/reimport_check.py -- <abs path>/bram_bust.glb <abs path>/renders
blender -b bram_bust.blend --python-exit-code 1 --python scripts/turntable.py -- <abs path>/renders/final_turntable_24fps.mp4
blender -b --factory-startup --python scripts/strip_png_metadata.py -- <png> ...
```

`validate_blend.py` fails unless the file has a window manager and screens, opens on
`BramBust` with a goal camera, holds the required meshes, the skin and eye images are
packed with a fake user, the head uses the skin image and UV map, no appended mesh still
records its source file, and no private folder text appears anywhere in the decompressed
file. `strip_png_metadata.py` removes the `.blend` path Blender writes into render PNGs.

## How the study went

- cp1–cp3: a head built from blended primitives, then edited; rejected for inflated,
  tubular forms.
- cp4: the CC0 base head; anatomy clearly better, identity generic.
- cp5–cp6: expression and brow corrections, salt-and-pepper beard as separate hairs.
- cp7: cap, collar and lapels, goal light, painted accents.
- Final: cp7 face, the cape collar replaced by a plain raised collar, export and checks.
