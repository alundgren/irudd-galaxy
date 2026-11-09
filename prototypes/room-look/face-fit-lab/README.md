# Face fit lab: registered camera, projected paint, bounded geometry fit (checkpoint 1)

A face-only diagnostic on a copy of the Bram lab's final head, against the right-hand face
study in `game/docs/visual-upgrade/references/room-and-face.png`. Every image is a
**Blender render**. Nothing here shows how the face looks or performs in the game. No
score is given here; judging the result is for the coordinator and the owner.

## Question and arms

Does a camera registered to the concept, plus paint and ink taken from the concept, make
the head read much more like the target? Does fitting the geometry to the concept's
landmarks help on top of that?

| Arm | Geometry | Look |
| --- | --- | --- |
| A | Bram lab head, unchanged | plain clay, no ink |
| B | same as A | concept projected into the head's UV paint, de-lit, 3 cel bands, plum ink shell |
| C | A plus the `Fit_Concept` shape key (bounded landmark fit) | B's method, re-projected onto C |

All three use the same fitted camera `Cam_Concept_Fit`.

## Results in numbers

Errors are in comparison-crop pixels (568×900) and as a percent of the concept's eye
separation (115 px, eye centre to eye centre). The percentages are provisional
diagnostics, not acceptance limits. One painted three-quarter view cannot fix depth.

**Camera** (`data/camera_fit.json`). Fitted only to 8 skull points: 4 eye corners, the
nose bridge, ear top, ear lobe and tragus.

- RMS 5.9 px (5.1%); worst point is the near eye's inner corner at 10.3 px (8.9%).
- Lens 56.7 mm at 0.74 m. The camera is 24.7° round to his right and looks up 3.8°.
- The lens is not pinned down. Holding it at 35, 50, 80, 120 or 200 mm gives the same
  skull RMS within 0.3 px (6.0–6.2 px), so depth and focal length come from the
  50–150 mm portrait bound, not from the image.
- A plain least-squares fit that also uses the nose and chin turns 3.1° and makes the skull
  points worse (6.5 px). We kept the skull-only camera, so that proportion differences show
  up as errors and are not hidden in the camera.

**Landmarks**: 29 marked on the concept (`renders/landmarks_concept.png`). Each mark has an
estimated uncertainty of 3 to 15 px and a role:

- anchor: skull points, used for the camera;
- form: nose and chin;
- expr: lids, pupils, mouth and lips;
- paint: brows, carried by paint only.

| Group | A (and B) RMS | C RMS |
| --- | --- | --- |
| anchor (8) | 5.9 px | 5.9 px |
| form: nose tip, nose wing, subnasale, chin (4) | 25.8 px | 14.2 px |
| expr: lids, pupils, mouth, lips (12) | 21.1 px | 13.3 px |
| paint: brows (5), reported only | 25.4 px | 25.4 px |

Per point, A → C: mouth corner near 32.5 → 12.4 px, far 27.2 → 7.7 px; stomion
39.0 → 27.0 px; nose tip 24.9 → 16.8 px; chin 30.8 → 13.0 px. The upper lids stay
13 px off in both: the model's eyes are more open than the concept's.

**Geometry fit (C)** (`data/geometry_fit.json`)

- The nose, lip and chin points on the midline move only in the face's centre plane, so
  the face stays symmetric. Mouth corners and the nose wing move in the camera's image
  plane, and the nose wing's move is mirrored onto the other wing.
- Depth moves are made expensive. Each landmark's total move is capped at 10 mm.
- 3,278 vertices moved more than 0.5 mm; the largest single move is 11.5 mm.
- No face flipped, and no face turned more than 45°.
- Moves per landmark: chin 9.7 mm down; mouth corners about 7 mm up and 6–7 mm out;
  lips and nose 5–8 mm forward and 3–7 mm up.

**Budget** (`data/budget.json`)

- Head 25,032 triangles, plus 25,032 for the ink shell; each eye 960. The Bram lab head's
  Subdivision modifier was removed (with it the head was 100,128).
- Materials: one painted cel material per arm, `M_Ink` and `M_Eye_Cel`.
- Images, all packed with a fake user: `T_Face_Proj_Raw`, `T_Face_Paint` and their `_C`
  copies at 1024², `T_Bram_Eye` at 512².

## Evidence (renders/)

| File | What it shows |
| --- | --- |
| `landmarks_concept.png` | the 29 concept marks, numbered, ring = estimated uncertainty |
| `cp1_A_landmark_overlay.png`, `cp1_C_landmark_overlay.png` | concept marks (crosses), model points through the fitted camera (pink rings), error lines and per-point table, over the concept and over the clay render |
| `cp1_landmarks_3d_A_vs_C.png` | where the 3D points sit on the head (front, his right profile, from below): the depth the fit assumed |
| `cp1_abc_concept_view_568x900.png`, `cp1_abc_talk_240x380.png` | concept, A, B, C at the fitted camera |
| `cp1_blend_concept_vs_BC.png` | concept and 50% blends with B and C |
| `cp1_clay_A_vs_C_front_profile_yaw.png` | neutral clay, A above C: front, profile, yaw −20°, fitted view, yaw +20° |
| `cp1_style_heldout_yaw_and_light.png` | B and C at yaw −20°, fitted view, yaw +20°, and with the key light moved to the other side |
| `cp1_raw_projection_yaw.png` | the raw projected layer, unlit; green = never seen by the camera |
| `cp1_paint_layers_uv.png` | raw layer, seen weight, B and C paint layers in UV space, de-lit concept |

Yaw −20° moves the camera 20° further round towards his right profile; +20° moves it
towards his front.

## What is painted, what is modelled, what only works from this view

- **Modelled**: the head and eyes, from the Bram lab (CC0 base head, see that lab's
  `SOURCES.md`). Arm C adds the `Fit_Concept` shape key. The eyes are turned to look at the
  fitted camera.
- **Painted**: everything else that makes B and C read as the concept is the concept's own
  pixels. That includes creases, brows, beard, the grey temple curls, the hair under the
  cap, lip colour, the dark lid lines and the nose highlight. There is no strand hair,
  no beard mesh and no cap.
- **De-lighting**: a wide blur of skin-coloured pixels estimates the concept's broad warm key
  and violet rim. That estimate is divided out at 80%. Lines narrower than the blur stay.
  The darkest marks become plum `#2a1a2e`. The cel bands then relight the head from the
  scene's lights.
- **Only true from this view**:
  - Where the camera saw nothing (green in the raw sheet), the paint is the mirrored near
    side, then flat skin or scalp colour. That covers behind the nose, the far eye corner,
    the far ear and the back of the head.
  - The mirror copies the near side's asymmetric smile onto the far side.
  - Local painted light (nose highlight, socket shadows, lower-lip shine) is still in the
    paint.

## What we saw

- **B is a large step from A at the fitted view and at talk size.** It reads as the
  concept's older, creased, smiling man. Most of that comes from the concept's pixels. A
  match from the source camera is not success on its own.
- **B holds at ±20° yaw better than expected at 240×380.** At 568×900 you can see:
  - the jaw line's paint lands on the neck as a dark stripe, because the model's chin is
    shorter;
  - pink rim colour is left on the near jaw;
  - the hair under the cap is a band across the forehead, with flat dark scalp above it;
  - the face is a painted patch, and the neck is flat colour.
- **Moving the key light fails partly.** The bands move, but the painted nose highlight and
  socket shadows stay, so the face looks lit from both sides. This is the concept's baked
  light, doubled.
- **The eyes stare.** The model's lids open about 13 px wider than the concept's upper
  lids. Paint cannot cover an eyeball. The eye image was toned down, but the lid opening is
  unchanged.
- **C barely differs from B at the fitted view.** Projected paint hides geometry from the
  camera it was projected from. At ±20° C's jaw line sits better (the neck stripe is
  mostly gone).
- **C's clay shows real defects**:
  - the lips are pushed forward into a muzzle (profile);
  - the mouth line is wavy, with pinched corners (front);
  - the nose and chin are longer.

  The landmark numbers improved while the clay got worse in places. These points cannot
  separate likeness from a crude local deformation.
- **First C attempt (rejected, not kept as a file).** Matching every point exactly, with
  free sideways moves, bent the nose and mouth towards his left in the front view. That
  is what forcing one view's points to zero does.

## Hypotheses, as far as this checkpoint goes

- **Registered camera plus projected paint and ink makes the fitted view much closer.**
  Supported for that view and for ±20° at talk size. Not supported under changed light,
  and untested in motion.
- **Landmark-driven geometry fitting helps on its own.** Weak. It fixes the jaw
  line under paint at held-out angles, and the numbers improve. But with one view and
  Gaussian moves it also makes visible clay defects (muzzle, wavy mouth). Depth stays
  inferred.
- **The remaining gap is lids and expression geometry**, then paint that still carries
  light.

## Not done in this checkpoint

- No blink or smile test. B/C did not hold up under the moved light, and the lids do not
  match. Animating projected lid paint over eyes that open too wide would mostly show
  that known mismatch, so the static result comes first, as the brief asked.
- No named expression controls. No topology-preserving lid or smile deformation exists
  here yet.
- No glTF export or reimport. The Bram lab already proved that export path for the same
  base head.
- No game runtime test.

## Suggested next step (for the owner to choose)

1. **Repaint, don't project.** Treat `T_Face_Paint` as an underpainting. Hand-author
   view-independent marks (creases, brows, lid lines) in the marks layer, and remove the
   painted highlights and socket shadows. Then re-test the moved light and ±20°.
2. **Fix the eye opening as geometry.** A lid-closing deformation that follows the lid loops,
   stored as a shape key (not region pushes), measured on the lid landmarks.
3. **Get a second view before more geometry fitting.** A side view drawn by an artist,
   even a rough one, would pin the depth that this fit could only guess.

## Files

| Path | What it is |
| --- | --- |
| `face_fit.blend` | editable source, opens on arm B through `Cam_Concept_Fit` (scene `FaceFit`) |
| `build_all.sh` | rebuilds the `.blend`, data and every image from the Bram lab source (which it only reads) |
| `scripts/make_face_file.py` | copies the head and eyes out of the Bram lab file, removes everything else |
| `scripts/landmarks.py` | concept marks and the rules that find the matching 3D points |
| `scripts/camfit.py`, `scripts/fit_camera.py` | camera model and fit; writes `data/camera_fit.json`, `data/landmarks_model.json` |
| `scripts/project_paint.py` | visibility-masked projection into `UVMap_HBM`, de-light, mirror fill; writes the paint layers |
| `scripts/fit_geometry.py` | bounded landmark fit into the `Fit_Concept` shape key; writes `data/geometry_fit.json`, `data/camera_fit_C.json` |
| `scripts/build_look.py` | cel, raw, eye and ink materials and the `Ink_Hull` Solidify |
| `scripts/render_checkpoint.py`, `scripts/landmark_views.py` | renders into `renders/work/` (not in git) |
| `scripts/overlay.py`, `scripts/make_sheets.py`, `scripts/draw.py`, `scripts/pngio.py` | numpy-only sheets and overlays, run through `scripts/run_py.py` |
| `scripts/set_arm.py` | puts arm A, B or C on the head and saves |
| `scripts/report_budget.py` | triangles, materials, images → `data/budget.json` |
| `scripts/clean_for_repo.py`, `validate_blend.py` | file-browser cleanup (from the board lab) and the open/pack/privacy check |

To edit paint by hand, paint on `T_Face_Paint` in Texture Paint mode with the
`UVMap_HBM` map. `T_Face_Proj_Raw` is the untouched projection; keep it for comparison.
Re-running `project_paint.py` overwrites both.

Checks run for this checkpoint:

- `build_all.sh` from an empty `renders/` and `data/` reproduced the numbers above.
- `validate_blend.py` passed: window and screens present, scene and camera right, images
  packed and protected, shape keys and modifiers as described, no private folder text
  in the decompressed file.
- The Bram lab's `bram_bust.blend` had the same SHA-256 before and after.
