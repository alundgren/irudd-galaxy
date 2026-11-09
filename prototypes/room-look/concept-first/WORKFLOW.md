# Concept-first recipe: from one concept painting to a lit, moving 3D room

This is the repeatable part of the Bram study. Every step is a script in `scripts/`; the numbers in
brackets are what Bram's arrivals desk used. Evidence and failures are in `LOG.md`.

The idea in one paragraph: the concept painting already contains the colours, ink, wear and detail
we want. Instead of repainting it, put a camera where the painter "stood", build simple geometry
that lines up with the painting through that camera, and shine the painting onto the geometry like
a slide projector. Then take the painter's light *out* of that paint, so real lights can move. What
the painting never showed (hidden or turned-away surfaces) is measured and filled deliberately.

```
concept.png ─► 1 camera match ─► 2 blockout from pixels ─► 3 ID render + palette
                                                                  │
          6 relight + animate ◄─ 5 bake to UVs + fill unseen ◄─ 4 delight + measure painter's light
```

## 0. Inputs

- One concept panel cropped to its frame (`concept/room.png`, 1083×893).
- Blender 5.3 in background mode only; Python 3 with numpy and Pillow for image maths.
- Run everything from this folder: `python -I scripts/pieceN.py` (or `scripts/room.py` for the whole order). Blender is called by
  `tools.blender()` with `-b --factory-startup --disable-autoexec`.

## 1. Match the camera (`cf.py`: `CAM_*`)

1. Grid the concept (`scripts/grid.py`) and read pixel positions.
2. Verticals parallel → camera without tilt, framing by lens shift (`shift_y`).
   Horizon row from a horizontal surface whose front and back edges you can see (desk top).
3. Choose a lens (35 mm) and **fix scale by a character's head**, not the props; painters
   exaggerate heads and hero props. Bram's face ~0.23 m gave: eye 1.58 m, wall 2.29 m, counter 1.05 m.

Check: render the blockout through the camera and draw its edges over the concept
(`tools.edge_overlay`). Outlines should land within a few pixels.

## 2. Blockout from pixels (`layout.py`)

Each element = a pixel rectangle plus one depth guess. Helpers turn pixels into metres through the
camera: `wall_box` (rectangle on a depth plane), `desk_item` (base row hits the desk top → how far back it
stands), `flat_on_desk`, `blob` (ellipsoid matching a pixel ellipse), `rod`. Keep thickness behind the
surface it belongs to (a wall's thickness behind its face, or it hides what is mounted on it).

## 3. ID render and palette (`piece0.py`, `tools.palette_from_id`)

Render each object in a unique flat colour (Raw view transform, no anti-aliasing). From that:
per-object masks in concept pixels, and each object's median concept colour (`palette.json`).

## 4. Delight and measure the painter's light (`delight.py`)

- Per object mask: blur brightness while staying inside the mask (sigma ~28 px at 1083 px wide),
  divide it out, rescale to the object's median brightness. Painted light gradients go; ink, rivets,
  labels and wear stay. Never delight across object edges.
- Measure, from the same blur:
  - **shadow ratio** = blurred colour where painted light is weakest / where strongest, averaged over
    large surfaces → the painter's shadow colour as a multiplier [0.24, 0.25, 0.38: violet].
  - **light colour** = one material near the light / the same material far from it [lamp 1.00, 0.86, 0.76].
    (Not concept ÷ delit: delighting keeps hue, so that ratio is grey.)
  - **ink** = median of the 1-5% darkest pixels.

## 5. Bake to UV textures and fill what was never seen (`bake_concept.py`, `fill_by_example.py`)

- UVs: wall-mounted objects get a flat front projection (front face gets the whole texture, back
  faces folded to one texel); round or free objects get smart UV.
- Before baking, mask what is not the room: annotation boxes in the concept (captions, labels) are
  written black so they count as unseen.
- **Guard band**: for background objects, concept pixels within ~12 px of a foreground silhouette
  (characters, plants, props) count as unseen. A blockout never matches the painted outline, and
  without the band the character's paint is baked onto the wall behind them (`guard.png`).
- **Foreground matte** (`matte.py`, second bake): blockouts of characters and props are stand-ins,
  drawn ~10% *larger* than their painted silhouette (paint outside a blockout is lost; the excess is
  trimmed). After a first bake, render the background objects alone from the concept camera and compare
  with the delit concept everywhere inside the stand-ins (plus 14 px): where the concept looks like the
  background, the foreground is transparent. Decide *inside* too, not just near outlines (the board seen
  between Bram's arm and head lies deep inside the union of his stand-ins); close holes under ~9 px so
  chance colour matches don't punch through. Then: delight again with a "painted ID" (stand-in pixels the
  matte rejects take the label of the background next to them, or the stand-in's outline becomes a seam
  in the wall's delit paint), and bake again: foreground objects get an alpha texture (decided wherever
  the concept saw the texel at all, grazing rims included); the background's guard shrinks to 4 px
  around the painted silhouette; foreground stand-ins neither shadow the background in the visibility
  bake nor cast shadows in the relit scene. `room.py` runs the whole order.
- **Test every cut-out in isolation**: render the projected object alone over a flat background. A
  torso-only render showed in one frame what whole-room renders hid for hours (`probe_alpha.py`).
- Cycles EMIT bake of the projected delit concept → colour per texel.
- Cycles DIFFUSE-direct bake with one shadow-casting light at the projector → how squarely the concept saw
  each texel. Texels outside the camera frame also count as unseen.
- Unseen texels: smooth push-pull colour continuation, then **detail-only example fill** (copy fine grain
  from seen paint of the same object, never whole patches; whole patches hallucinate pipes and clamps).
- Repeated parts (board slots): copy the best-seen sibling into ones the concept hid.
- Save albedo as **8-bit sRGB PNG** (an EXR round trip double-encoded colour here).

Check: render the albedo unlit through the concept camera (`relight.py … albedo`) and compare
texels with the delit concept (`diag_bake.py`) before touching any light.

## 6. Relight and animate (`relight.py`)

Light model (EEVEE, Shader to RGB of a white diffuse = light arriving):
`final = albedo × ( shadow_ratio × (1 − min(t,1)) + light_hue_at_half_strength × t )`,
`t = light / lit_level`, capped at 1.4. Unlit = the painter's shadow colour; lit = the delit concept.

- Lights: the painted light sources (lamp: spot under the rim, measured colour); the painter's implicit
  key (warm sun from the upper left); weak fills from windows (palette colour).
- Calibrate with `compare.py` (per-object colour vs concept at the concept view), then check
  `light_response.py` (does the surface you animate respond?) and `stuck_light.py`
  (lamp off: how much painted light is left behind?).
- Painted glow is light: mask the glowing texels and drive them with the same value as the real light.
- The response saturates quickly: t runs from shadow colour to lit over a factor of ~4 in light
  (lit_level), so tune lights in small steps. Mounted in another room, the board went from dim
  (1.5 W) to blown out (3.5 W); 2.5 W matched the concept (partner's measurement, delta E 2.4). A light
  linked only to the projected asset tunes it without touching the rest of the room.
- Freestyle silhouettes and creases in the measured ink colour, **only on geometry that matches the
  paint** (architecture, board, lamp, desk). Rough foreground stand-ins draw their own wrong outline
  over the painted ink (an oval around Bram, boxes around the mug).
- Motion: parent a moving prop with its lights to a pivot at its real mount point.
- The painter's implicit key light shades but **casts no shadow**: its hard shadow of a prop reads as a
  second copy of the prop once the camera moves.

## 7. Bring parts to life with the concept's own paint

- **Openings** (windows, portholes): trace the distinct things seen through it as polygons
  (colour segmentation fails when they share hues), cut each to a transparent layer, and put the
  layers on planes parallel to the concept image plane at plausible distances (12 / 25 / 90 m).
  Parallel planes keep exact UVs from the concept camera; distance gives parallax for free. The
  sky becomes a padded plate: continuation near the paint, fading to the painter's deepest sky
  colour, stars at the density measured in the painted sky. Exclude these planes from ink lines.
  (`porthole_layers.py`, `relight.py` `setup_space`)
  When the opening is reused in another room, the layers are laid out along the *concept camera's*
  sight line through the opening; the new room's camera sees only sky until the stack is swung about
  the opening's centre onto its own sight line (found by the partner session mounting this porthole).
  Store the stack under a "view axis" empty at the opening, not in world positions.
- **Repeated parts** (board slots, tiles, labels) are a content library: cut each instance from the
  bake and recombine them in motion (split-flap: static halves plus a hinged flap per flip, visibility
  keyframed). (`splitflap.py`, `relight.py` `setup_board`)
- **Small life**: lamp sway and flicker (painted glow follows the light), window blinks via a noise
  phase, a character's breathing as a small scale and tilt about the hips (`setup_breathing`).

## Camera moves this holds up to

Projection paint is right from the concept camera and degrades with distance from it. Measured here:
4-14% of visible pixels unseen at 25° yaw or pitch, 21% at 50°. A sideways dolly of ~0.5 m stays on
the painting; at 0.8 m the frame's edge comes in and the room beyond it is plain filled surface.
Choose room layout and cameras around the projection camera (seats, character angles), and paint a
second concept view for any angle that must hold beyond that.

## What this recipe does not solve (yet)

- **The difference matte can confirm its own mistakes.** Where it wrongly keeps background as
  foreground, the next bake guards that background, fills or copies it, and the next matte sees a
  mismatch there again (Bram kept the board slots right of his arm: a ghost ellipse). Seed the matte
  from something not baked from itself: a hand-traced outline (minutes per character, as for the
  porthole), or a modelled character rendered from the concept camera. Verified: one 11-point traced
  exclusion polygon (`matte.py` `EXCLUDE`) removed the ghost right of Bram in one rebake. A full traced
  outline per character is the reliable version.

- Surfaces the concept never showed get colour and grain, not designed features. Anything that must
  read from behind (a lamp's back, Bram's far side) needs either a second concept view or modelled
  detail.
- Characters: projection gives the painted face from the front only; see LOG.md for the head.
