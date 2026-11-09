# Bram's arrivals desk, concept first: work log

Started 2026-11-09 05:47 UTC, deadline 11:45 UTC. Owner away; brief relayed by the parallel room-look
session. My angle: start every piece from the concept image itself. The room-look session builds a
geometry-driven style kit; we exchange findings, not files.

Study folder: a working folder outside the repo; the scripts and this log are copied into `prototypes/room-look/concept-first/`.
Blender 5.3.0 RC, background processes only (`-b --factory-startup --disable-autoexec`). The open
Blender window on this PC belongs to the room-look session and was never touched.

Source of truth: `concept/room.png` and `concept/portrait.png`, cropped from the owner's
`concept-reference.png` (room panel pixels x 8..1091, y 6..899; portrait x 1103..1663, y 6..899).

Words used below:
- **Projection**: shine the concept image into the 3D scene from the camera that "painted" it, like a
  slide projector. From that camera the scene looks exactly like the concept.
- **Unseen**: surfaces the projector can't reach (hidden behind something, or facing away). The concept
  says nothing about them; anything there is invented. Rendered magenta.
- **Palette sampling**: each object's colour is the median concept colour over the pixels where that
  object is visible from the concept camera.

---

## Piece 0: camera match and layout blockout (05:50-06:00 UTC)

**Goal.** One 3D layout whose silhouettes sit on the concept when seen through one camera, so every
later piece can borrow colour, detail and position straight from the concept.

**What I did.**
1. Read positions off a gridded copy of the concept (`concept/room-grid.png`, `scripts/grid.py`).
2. Camera. The board's vertical edges stay parallel, so the camera has no tilt; the framing uses lens
   shift instead (`shift_y -0.077` puts the horizon at concept row 363). 35 mm lens on a 36 mm sensor.
   The horizon height came from the desk top: its front and back edges at rows ~795 and ~690.
3. Scale. The painting is not one consistent real-world scale: Bram's head and the mug are big for the
   board. I fixed scale by Bram (face ~0.23 m wide), which gives a counter 1.05 m high, a standing
   viewer's eye at 1.58 m, the back wall 2.29 m away. Board ~0.84 m wide, lamp shade ~0.35 m.
4. Blockout (`scripts/layout.py`). Every element is a pixel rectangle plus one depth guess; helpers
   turn pixels into metres through the camera (wall items: a depth plane; desk items: where their
   base row hits the desk top). The left wall turns 40Â° toward the viewer at the teal pillar, which
   is why the porthole reads as an upright oval.
5. Palette: object-ID render (Raw colour, no anti-aliasing) -> median concept colour per object
   (`tools.palette_from_id`). Output `palette.json`.
6. Projection material: computes, per pixel, where that point lands in the concept camera and looks
   the colour up there. A shadow-casting light at the projector marks unseen surfaces magenta.

**Evidence** (`pieces/00-layout/`): `overlay-edges.png` (blockout outlines in cyan over the concept),
`sheet.png` (rows projected / palette / lit, columns concept, 25Â° left, 25Â° right, 50Â° right, higher),
`projected-orbits.png`, `unseen.json`.

| View | Unseen share of the image |
|---|---|
| concept camera | 0.15% (edge pixels) |
| 25Â° left | 4.4% |
| 25Â° right | 11.2% |
| 50Â° right | 20.8% |
| 25Â° higher | 13.8% |

**What worked.** Outlines land on the concept within a few pixels for the board, lamp, sign, cases and
head. With projection, the scene holds up to about 25Â° of orbit with only small invented areas: the
painted ink, rivets and wear stay where they belong because they sit on geometry at the right depth.

**What failed / limits.**
- Bug 1: the projector copied the camera's transform before Blender had computed it, which gave
  nonsense stripes. Fix: update the view layer first.
- Bug 2: the unseen mask first showed 0% because EEVEE's bounce light from the bright projected
  surfaces lit hidden areas. Fix: threshold 0.5 against direct light of ~5-20.
- Projection carries the painter's lighting (lamp hotspot on the board, shadows). Moving a light
  will expose that. Piece 1's job.
- Blockout shapes are crude (Bram is ellipsoids and rods). They only need to be right where the
  concept sees them; unseen sides are the fill list.

**Reusable lesson.** For a framed conversation camera, projecting the concept onto a matched blockout
gives the exact concept look for free, and the unseen mask turns "how far can the camera move" into
a number and a picture of what still has to be made.

---

## Piece 1: lamp corner, delit and relit, with sway and flicker (06:00-06:20 UTC)

**Goal.** Back wall, arrivals board, arches, pillar and the lamp, carrying the concept's own paint, lit
by real lights that can move: the lamp sways and flickers, and nothing painted stays stuck.

**Steps** (`scripts/piece1.py` runs them all; `python -I scripts/piece1.py [delight,bake,still,tests,anim]`)
1. **Delight** (`scripts/delight.py`). Inside each object's mask (from piece 0's ID render), blur the
   brightness while staying inside the mask (sigma 28 px), divide it out and rescale to the object's
   median brightness. Slow painted light (the lamp's glow, falloff) goes; fast detail (ink lines,
   rivets, labels, wear) stays. `concept-delit.png`, `delit-compare.png`.
2. **Measure the painter's light** (`painter-light.json`, `style.json`):
   - shadow ratio = the blurred colour where the painted light is weakest divided by where it is
     strongest, averaged over wall, arches, board frame, desk and lamp shade: **0.24 / 0.25 / 0.38**
     (R/G/B). The painter's shadows keep blue best: the violet shadows, measured.
   - lamp colour = the back wall's colour within 160 px of the bulb divided by its colour more than
     320 px away: **1.00 / 0.86 / 0.76** (warm).
   - ink = median of the 1-5% darkest pixels: near-black warm brown (7/4/4 of 255). The concept's
     outlines are darker and less plum than the brief's words suggest; plum shows in the
     midtone lines, not the darkest ink.
3. **Bake** (`scripts/bake_concept.py`). For each object: UVs (wall-mounted objects: flat front
   projection that gives the front face the whole texture, back faces folded away; lamp: smart UV);
   Cycles EMIT bake of the projected delit concept; DIFFUSE-direct bake with a single light at the
   projector, which gives a "how squarely did the concept see this texel" map. Hidden texels get a
   smooth push-pull fill. Board slots the concept mostly hid (rows 4-5, left column, behind Bram's arm)
   copy the best-seen slot of their column. 28 s on the GPU for 25 objects.
4. **Relight** (`scripts/relight.py`, EEVEE). Light model, both ends measured:
   `final = albedo Ã— ( shadow_ratio Ã— (1 âˆ’ min(t,1)) + light_hue_at_half_strength Ã— t )`, where
   `t` = light arriving / lit_level, capped at 1.4. Unlit = the painter's violet shadow, lit = the
   delit concept colour. Lights: lamp spot under the shade rim (measured warm colour), the painter's
   implicit key (warm sun from upper left/front), a weak porthole fill (porthole palette colour).
   Freestyle silhouette/crease lines in the measured ink colour.
5. **Animate**: lamp, bulb and spot parented to a pivot at the wall mount; sway 3.5Â° plus a 1.2Â° wobble
   over 3 s; flicker: dips to 35% (frames 30-33) and 60% (55-56) plus 5% noise, driving spot power,
   bulb emission and the shade's painted glow together.

**Evidence** (`pieces/01-lamp-corner/`): `final-compare.png` (concept, relit concept view, relit 25Â°
right), `anim-concept.gif`, `anim-side.gif`, `strip-*.png`, `frames-key.png` (rest, sway, dip, dip),
`tests-sheet.png` (delit vs raw paint: rest, swung 16Â°, lamp off, porthole off, swung seen from the side),
`lamp-off-delit-vs-raw.png`.

| Measure (concept camera) | Value |
|---|---|
| Overall brightness vs concept, target objects | 0.77 |
| Colour balance vs concept (R/G/B ratio) | 0.80 / 0.76 / 0.76 (balanced, a bit dark) |
| Saturation (mean maxâˆ’min), concept / render | 0.202 / 0.164 |
| Board brightness: lamp on / 35% dip / off | 0.74 / 0.70 / 0.57 (concept 0.65) |
| Painted light stuck with lamp OFF (near/far brightness) | raw paint 1.096, delit 1.050 (concept itself 1.14) |

**What worked.**
- From the concept camera the corner reads as the concept: the same ink, rivets, labels, pipes and
  board, but lit by lights that move. The board's light pool shifts with the sway; the board dims
  visibly in the flicker dips; the glow inside the shade goes out with the lamp.
- Seen from 25Â° right, the same textures hold, because they live on geometry at the right depth.
- Two numbers that used to be taste (shadow colour, lamp colour) are now measured from the concept.

**What failed / limits** (in order of how much they cost):
- **EXR bake round trip double-encoded the colour** (lifted blacks, a foggy look). Found by rendering the
  baked albedo with no lighting and checking 5 texels against the delit concept (`scripts/diag_bake.py`):
  texture 117 â†’ render 176, exactly one extra sRGB encode. Fix: 8-bit sRGB PNG textures. Earlier
  light tuning had been chasing this bug.
- **Wrong light-colour measurement first**: concept Ã· delit is the same for every channel because
  delighting keeps hue. Replaced by near-vs-far on one material.
- **Brightness cap hid the flicker**: the 32 W lamp put the board at the cap, so a 65% power dip showed
  as 2%. Rebalanced (13 W lamp, weaker key) so the lamp is the board's main light, as in the concept.
- Delighting removes only about half the stuck light (excess 9.6% â†’ 5.0%). The rest sits in small
  regions the 28 px blur treats as detail.
- Unseen areas are smooth smudges: the wall where Bram sits, the board frame's interior on the left.
  In the full room Bram covers them from the concept camera; from other angles they show. This is
  the concept-first method's main open problem.
- Still 20-25% darker and a little less saturated than the concept overall. The painter pushes
  saturation in lit areas; this model doesn't (the style-kit session's "lift colour" may be the fix).
- The lamp shade's back half and the arm are fill colour, because the concept only shows one side.

**Reusable lessons.**
- Delight per object mask, not over the whole image; otherwise light bleeds across object edges.
- Measure light colours by comparing one material near and far from the light, never concept Ã· delit.
- Painted glow is light, not paint: mask it (bright texels of the lamp) and drive it with the same
  value as the real light.
- Check the bake path with an unlit albedo render before tuning any lights.
- Balance lights so the light you animate is the surface's main light, below the brightness cap.

---

## Piece 2: filling what the concept never showed (06:20-06:31 UTC)

**Goal.** Piece 1 left smooth smudges wherever the concept was hidden (behind Bram, inside the board
frame, the lamp's far side, above the frame). The owner wants a room you can walk around, so these
are the concept-first method's main weakness. Fill them with the concept's own paint, honestly.

**What I did** (`scripts/fill_by_example.py`, `scripts/piece2.py`)
1. **Patch mode** (first try): greedy image quilting in texture space. Each tile that is at least 25% unseen
   copies the seen patch of the same object whose top and left overlap best match what's already
   there, recoloured to the smooth fill's local mean, feathered, never over seen texels.
2. **Detail mode** (kept): the same search, but only the fine detail of the chosen patch (colour
   minus a 5 px blur, × 0.8) is added on top of the smooth colour continuation.
3. Bake fix found on the way: texels outside the concept camera's frame counted as seen (the
   visibility light shines everywhere; the camera doesn't) and baked black. They now count as unseen.
4. Concept annotation fix: the caption box "E / Ink, depth, old technology" was baked onto the wall.
   Annotation rectangles are now blacked out before baking, so they're treated as unseen
   (`ANNOTATIONS` in `piece1.py`). Applied from the next re-bake on.

**Evidence** (`pieces/02-unseen-fill/`): `wall-patchmode-texture.png` (patch mode, rejected),
`wall-smooth-vs-example.png` (smooth vs patch mode, texture space), `views-sheet.png` (smooth vs detail
from concept, 25° left, 25° right, 50° right, higher), `yaw-25-smooth-vs-detail.png` (enlarged).

**What worked.** Detail mode makes hidden wall read as the same riveted, worn, painted metal: from 25°
left, the wall beside and above the board now has rivet lines, seams and grain instead of a smudge.
Nothing is invented at the feature level; colour still follows the smooth continuation.

**What failed / limits.**
- Patch mode **rejected**: it tiled the wall with copies of the busiest seen features (pipe clamps)
  and showed tile seams, far busier than the concept's mostly plain panels. Copying whole patches
  hallucinates structure the painter never drew.
- Detail mode still repeats small motifs (hook-like marks left of the board) and shows a few
  straight tile edges at close range.
- Objects with no fully seen window (the lamp arms) get no detail; they stay smooth.
- Features in hidden areas (a pipe continuing behind Bram, the lamp's back, a door) aren't
  synthesised. That needs deliberate modelling or a second concept view.
- Slow in plain numpy: 400 s for the lamp corner (wall texture 2048², 1746 tiles).

**Reusable lesson.** Fill unseen surfaces in two layers: colour by smooth continuation, texture by
copying fine detail from the same object's seen paint. Never copy whole patches. Mask concept
annotations and anything outside the camera frame as unseen before baking.

---

## Piece 3: the arrivals board as a split-flap display (06:25-06:35 UTC)

**Goal.** Make the board itself alive the way an arrivals board is: slots that flip to the next
arrival, using only the concept's own slot paint, under the moving lamp.

**What I did** (`scripts/splitflap.py`, job `board` in `scripts/relight.py`, `scripts/piece3.py`)
- Each baked slot becomes a split-flap unit: a static top half (current card), a static bottom half
  (current card), and per flip one flap hinged on the middle line. The flap's front is the old card's
  top half; its back is the new card's bottom half, laid out upside down so it reads upright after
  the 180° fall. Visibility of every half is keyframed so each shows exactly when a real board would.
- Cards = the concept's own slots: slot (row i, column j) shows rows i+1, i+2, i+3 of column j in turn,
  so the board scrolls up and each new "arrival" is another real painted ship silhouette.
- Every slot flutters through 3 cards, 5 frames apart, 4 frames per flip (eased: slow start, slap
  down), cascading 4 frames per row and 2 per column. The lamp keeps swaying and flickering, so the
  flaps turn through real light.

**Evidence** (`pieces/03-board/`): `anim-board.gif` (close 24° camera), `anim-concept.gif`,
`board-key-frames.png` (frames 18-60), `flip-row0.png` (frames 18-23 enlarged),
`late-frames.png` (settled: 56, 60, 80, 96), `strip-*.png`.

**What worked.** Flaps swing toward the viewer in perspective; the back of each flap shows the
next card upright; settled frames show complete new cards with the thin middle seam a real split-flap
has. The flaps catch the lamp differently as they turn. All content is concept paint.

**What failed / limits.**
- The ship silhouettes from different rows are similar, so each update is subtle at a glance.
- Freestyle draws thin lines along flap edges and the middle seam; slightly busy up close.
- Hidden slots (rows 4-5 left, behind Bram) show copies of the top slot, so two cards repeat.
- Flaps have no thickness and cast no contact shadow on the card below.

**Reusable lesson.** Repeated parts in a concept (slots, tiles, labels) are a content library: cut
them per instance, and motion can recombine them without inventing new paint.

---

## Piece 4: the porthole as parallax layers, plus two bake fixes (06:35-07:25 UTC)

**Goal.** The porthole should read as a window onto space: when the camera moves, the planet,
tower and platform move against each other and against the room; the tower's windows blink.

**What I did** (`scripts/porthole_layers.py`, `scripts/piece4.py`, job `porthole` in `relight.py`)
- Geometry: boolean hole in the left wall, an open tube with flipped normals (the wall's depth),
  a parametric torus bezel. All three are baked from the concept like any other object.
- Space content: hand-traced polygons over the concept's porthole (tower, left platform), each cut
  out to its own transparent layer. Everything else is the sky plate.
- Sky plate: the concept's sky padded 800 px on every side. Near the paint, a push-pull continuation;
  further out, a fade (blur 60) to the painter's deep sky colour; stars sprinkled at the density
  measured in the painted sky (pixels above threshold 0.25: 2.8%).
- Layers are planes parallel to the concept image plane at 12 m (platform), 25 m (tower) and 90 m
  (sky). Parallel to the image plane means each vertex's UV is exact from the concept camera, so
  layers can move freely and still match the painting at frame 1. The platform drifts 0.9 m; the
  tower's warm pixels blink with a per-pixel noise phase driven by an object property.
- Freestyle ignores the space collection (no ink outline around layer planes).
- Two cameras: the concept framing dollying 0.8 m right and back; and a window camera 1 m from the
  porthole along the concept's own line of sight through it, swaying 0.25 m.

**Two bake fixes found by this piece's dolly**
- *Ghost lamp*: the painter's key light cast a hard shadow of the lamp onto the wall, which read as
  a second lamp once the camera moved. The key now shades but casts no shadow.
- *Bram on the wall*: background texels next to Bram's silhouette baked Bram's jacket paint, because
  the blockout never matches the painted outline. Fix: a guard band, concept pixels within 12 px of a
  foreground mask (Bram, plant, chair, desk props) count as unseen for background objects and get
  filled (`guard.png` from the delight step, 5th argument of `bake_concept.py`).

**Evidence** (`pieces/04-porthole/`): `anim-dolly.gif`, `anim-window.gif`, `strip-*.png`,
`window-frames.png`, `layers/layers-preview.png`; after the fixes, `anim-dolly/0024.png` (one lamp, no
jacket paint on the wall).

**What worked.** Parallax between planet, tower and room reads immediately; the window camera shows
the tower against the planet from a new angle without visible plate edges; blinking is subtle.
The guard band removed the silhouette fragments with no visible cost.

**What failed / limits.**
- Colour segmentation of the porthole content failed (planet, tower and sky share hues); hand
  tracing 3 polygons took a few minutes and is the reliable route.
- First sky attempts: EXTEND streaks at the plate edges; then 6.5% stars (too many); then a
  row-profile continuation smeared the planet into a band. The padded plate with distance fade fixed all three.
- Aimed along the porthole's axis, the window camera mostly saw invented sky; aiming along the
  concept's sight line keeps it on painted content.
- The dolly past ~0.5 m leaves the painting's frame: the right side of the room is plain filled
  wall and desk. Projection-baked rooms want camera moves that stay near the painted frame.

**Reusable lesson.** Anything seen through an opening is a stack of image-plane-parallel layers cut
from the concept; distance gives the parallax for free. And: every background bake needs a guard
band around foreground silhouettes.

---

## Piece 5: the whole room at once, and a matte for the stand-ins (07:00-10:35 UTC)

**Goal.** Run every object the concept camera sees through the same recipe (desk, props, plant,
chair, Bram), then animate all the parts of pieces 1-4 together: lamp sway and flicker, the
split-flap board, porthole parallax with blinking tower, and Bram breathing. Camera: a slow push-in,
as if leaning toward the desk.

**What I did** (`scripts/piece5.py`, `scripts/matte.py`, `scripts/room.py`, job `room` in `relight.py`)
- All 46 blockout objects baked in one pass; foreground stand-ins (Bram's head, cap, torso and arms,
  props, plant, chair) get example detail fill like the walls.
- `relight.py` split into reusable setups (`setup_space`, `setup_board`, `setup_breathing`) so one job
  composes them; 144 frames, two cameras (push-in: 0.55 m forward, 0.30 m right, eased; 25° side).
- Ink lines only on geometry that matches the paint (architecture, board, lamp, desk).
- The stand-ins never match the painted silhouettes, which the first whole-room render showed at
  once: a halo disc round Bram's head, Freestyle ovals round his torso and boxes round the mug.
  Fixed in four steps, each found by looking at the result:
  1. *Difference matte*: render the background alone from the concept camera, compare with the delit
     concept; where the concept looks like the background, the stand-in is see-through (alpha texture).
  2. *Stand-ins larger than the paint*: Bram's epaulette lay outside his torso blob and was lost;
     head, cap and torso grew ~10% and the matte trims the excess.
  3. *Painted ID for delighting*: the wall inside the stand-in's outline was delit as if it were Bram,
     leaving an ellipse seam; rejected pixels now take the label of the background next to them.
  4. *Decide inside, not just near the outline*: the board between Bram's raised arm and his head lies
     deep inside the union of stand-ins and was kept as "Bram". Found by rendering the torso alone over a
     flat background (`probe_alpha.py`): an opaque ellipse with board paint inside it.
  Smaller ones on the way: alpha weighted by "how squarely seen" made grazing rims opaque; stand-ins
  shadowed the wall in the visibility bake; they cast lamp shadows with their own wrong outline.

**Evidence** (`pieces/05-room/`): `anim-push.gif`, `anim-side.gif`, `strip-push.png`,
`strip-side.png`, `check-push-ends-final.png` (concept | frame 1 | frame 144),
`zoom-bram-f1-v*.png` (the fix sequence, v1-v8), `probe/both.png` (the isolated torso),
`../01-lamp-corner/matte-check.png`.

**What worked.** From the concept camera, frame 1 reads as the painting, relit: Bram, the desk set,
the board and the porthole hold, and the push-in to Bram's face keeps the painted face (the camera
moves toward the projection, not across it). Everything moves at once from one script. Steps 1-3
removed the head halo, the Freestyle ovals and boxes, the lost epaulette and the smear at his shoulder.

**What failed / limits.**
- **Ghost ellipse right of Bram: fixed at 11:05 by a hand-traced exclusion polygon; a faint arc remains
  on the left.** One 11-point polygon in `matte.py` (`EXCLUDE`, traced in a few minutes from a 2x crop)
  marks the board between Bram's face, fist and sleeve as background. After one rebake the left-column
  slots count as seen again (slot 30/40/50: 15/0/0% → 72/44/34%), and the lighter patch with doubled
  slots is gone from frame 1 and from the end of the push-in (`check-push-ends-v9.png`,
  `zoom-bram-f1-v9.png`; v8 is before). Still visible: a fainter arc of the torso stand-in over the
  porthole beside his left shoulder, the same problem outside the traced region. How it was found:
  step 4's automatic fix did not take. The matte confirms itself:
  where it keeps a region as Bram, the next bake guards and fills the background there, so the
  background never matches the concept in that region and the region stays "Bram". On top of that, the
  background render does not match the delit concept even on fully visible board slots (distance 0.32
  at concept pixel 600,300 versus 0.07 on plain wall); the board slots' copy-from-sibling step is the
  suspect. Visible as a lighter rounded patch with doubled board slots, worst at the end of the push-in.
  Diagnosed at 10:40 (`pieces/05-room/diag-slots.png`, delit concept | background render): (a) the
  delit concept has a dark diagonal wedge over the left slot column, left by the stand-in masks in
  delighting; (b) in the background render the left-column slots look alike and brighter than the
  delit concept: paint the concept did show was replaced, either by the copy-from-sibling step or by
  the guard from a matte that had kept those slots as Bram. Confirmed from `bake.json`: left-column slots
  30/40/50 count as 15%/0%/0% seen (the guard), so they are copied from slot 10, a different ship, and
  the next matte keeps them as Bram again. Tried: comparing blurred colours (so a sibling slot with a
  different ship still reads as board); not enough, because the wedge in the delit concept also makes
  those slots darker than their sibling. It is a loop through delight, matte and guard; next step is to
  seed the matte from a source that is not baked from itself (a hand-traced character outline, as for
  the porthole, or the partner's modelled Bram rendered from the concept camera). The traced polygon
  above is that step, done for the one region that showed. `export/scene-export.blend` predates it (10:30).
- The 25° side camera breaks: Bram reads as a stretched cut-out, the porthole shows only deep sky
  (a flat violet disc), and the wall above the painted frame is plain fill (`strip-side.png`).
- The whole frame is dimmer and less saturated than the concept (piece 1: brightness 0.77, saturation
  0.16 vs 0.20); the light model loses some of the painter's warmth.
- Sideways moves past ~0.5 m leave the painting: the right side of the room beyond the frame is plain
  fill. A modelled room (the partner's track) holds there; this one does not.
- Bram is a painted cut-out on blobs: he breathes, but cannot turn his head or blink without a second
  view or modelled face.
- Each full rebake is ~25 min (Cycles bakes are fast; the plain-numpy example fill is the slow part).

**Reusable lesson.** For characters and props, a projected room needs a *painted matte*, not the
blockout's outline: make stand-ins bigger than the paint, let a difference against the background
decide what is foreground everywhere inside them, and use that matte for alpha, for the background's
guard and for delighting. Test every cut-out alone over a flat background.
