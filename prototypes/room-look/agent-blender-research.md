# Agents and Blender: what works, what we got wrong, what to try next

Research for the room-look study, 2026-11-09. It asks how other people get good results
when an AI agent drives Blender, and why our Bram bust missed. It does not build an asset.
Every external claim below names its source and says whether we saw the result ourselves
or only read the author's claim.

## Coordinator assessment

This report proposes hypotheses, not a proven replacement workflow. The papers support
iterative visual feedback and expose perception failures; they do not establish that
numerical feedback alone would fix Bram. Correct anatomy terminology in code comments
does not rule out missing artistic judgment or implementation skill.

The proposed landmark tolerances and triangle budgets are experimental choices, not
validated industry acceptance limits. Camera parameters and facial geometry cannot be
uniquely recovered from one stylized view. Low landmark error can coexist with incorrect
depth, eyelids, expression and deformation. Failure to fit can also come from the fitting
algorithm or landmark choices, rather than an unsuitable base mesh.

Projected concept paint is useful as a diagnostic, but it contains baked lighting and
expression cues. A close match from one camera can hide weak geometry. Any promising
result needs held-out angles, changed illumination and a small expression test before
it can support an animated character workflow. Passing landmarks while failing visual
review does not exclude geometry as a cause. Named controls help only when their
underlying deformations are good; naming the same failed vertex edits does not fix them.

## Short answer

- **No one we found has shown an agent making a convincing stylized adult character on its
  own.** Every strong result we found is one of these: props and scenes; an agent arranging
  meshes made by someone or something else; or a human artist using AI for one step.
  This matches our result and is not a Blender limit.
- **The agents that do well get numbers back, not only pictures.** Benchmarks show that
  vision models miss small visual differences. Our board pass had a camera fitted to
  9 measured points and paint placed by goal pixel. The bust had neither. It was judged by
  eye, a checkpoint at a time.
- **Agents do better with named controls than with raw vertices.** In BlenderGym, the
  blend-target task works because each control has a name ("smile", "jaw open"). Our
  expression edits moved hand-picked rings and ellipsoids of vertices. That is the hardest
  possible form of the problem for a model.
- **A big part of the concept's look is 2D illustration: ink lines, carved creases,
  painterly grime.** The bust had no ink hull, crease paint was switched off, and it was
  lit as smooth physically based skin. The board used painted marks registered to the goal
  camera and got much closer. The bust did not reuse what worked on the board.
- **The starting head pulled toward an average young face.** We used a realistic scan head.
  A professional stylized image-to-head tutorial (see E8) warns about exactly this: fitting
  trained on normal anatomy pulls a caricature back to "normal".
- **Recommended next step:** one face-only test at conversation size. Fit the camera to the
  concept with measured landmarks. Warp the head to those landmarks. Project the concept's
  paint onto it from that camera. Render in the game's cel-and-ink style. Then compare
  numbers and ask the owner. Details are in "Smallest decisive next experiment".

## What we inspected locally

- `blender-board-lab/README.md`, its scripts list and `renders/cp5_goal_comparison.png`.
- `blender-bram-lab/README.md`, `SOURCES.md`, `scripts/` (especially `sculpt_cp4.py`,
  `topology_smile.py`, `paint_skin.py`, `face_landmarks.py`, `build_hair.py`), and the
  renders `final_goal_comparison.png`, `cp4_original_vs_bram_four_views.png`,
  `diag_face_front_and_goal_close.png`.
- `bram-study-brief.md`, `bram-checkpoint-1..3-review.md`, `overnight-reviews.md`.
- `game/docs/visual-upgrade/references/room-and-face.png` and the experiment verdict in
  `game/docs/direction.md` (section "Blender asset experiment, 2026-11-09"), plus section 1.1
  (cel shading, plum ink).
- The installed MCP server package `mcp-for-blender` 2.2.7 and its Blender add-on 1.8
  (protocol 15), read as source code. Nothing was run in Blender for this report.

The owner says the board is still not close. The provisional 4/5 is an agent score and is
not used below as evidence of success.

## Why the bust missed: diagnosis

Each row says what we saw in our files and how sure we are.

| Factor | What the files show | Verdict |
| --- | --- | --- |
| **Feedback had no numbers tied to the concept** | The board camera is fitted to 9 landmarks on the goal crop, about 3 px RMS. Bram's `Cam_Goal_Tilt` is placed by hand and "rolled 18° to match the concept's head tilt". `face_landmarks.py` measures the 3D head against itself between checkpoints, never against points on the concept. Reviews say "reads younger", "reserved", "pursed": judgments, not measurements. | **Supported.** BlenderGym (E2) reports that vision models "fail to capture subtle visual differences". An expression is a small visual difference. |
| **Representation: raw vertex regions instead of named controls** | `sculpt_cp4.py` writes edits as ellipsoid regions with grab, push and crease. `topology_smile.py` moves rings of vertices. Result: a pinched crease under the mouth corner and a stretched corner. Checkpoint 3 notes "limited precision for fine cheek, eyelid and lip edits". | **Supported** for our case. BlenderGym's blend-target task and EZBlender (E4) both rely on named, scored controls. MPFB now ships 52 named face controls (E10). |
| **Starting asset pulled toward an average face** | The CC0 "Head (Animation) - Realistic" is a scan of a young man. The four-view sheet shows the edited Bram head still close to it. The same bundle also has a stylized head, a planar head and a generic head with example head variants (E11), which we did not try. | **Supported** that the base dominated. **Hypothesis** that a stylized or planar base would do better. |
| **Look built in the wrong render style** | Bram has no ink outline anywhere in its scripts. `paint_skin.py` was run with `CREASES=False`. Wrinkles were held back on purpose ("Soft forms only, no wrinkle lines", "No new grooves"). The concept's age reads mostly through dark carved lines at the eyes, mouth and forehead. `direction.md` 1.1 calls for cel bands and plum ink. | **Supported.** The board got closest where it used deliberate painted marks; the bust skipped that step. |
| **Art knowledge in the prompts** | The edit comments are anatomically sound ("cheek mass rises and comes forward over the corner, the lower lid is pushed up by it"). | **Unresolved.** The comments describe anatomy, but do not prove sufficient artistic or implementation skill. Placement and evaluation also failed. |
| **One painted three-quarter view as the only target** | There is no front or profile of Bram. The skill pack in E7 says three-quarter concept views are "usually not metric". The pro workflow in E8 first makes a side profile, because one view gives no depth. | **Supported** as a limit on any 3D fit. It argues for fitting at the talk camera, not for full 3D likeness. |
| **Budget and order of work** | 982,704 triangles, 579,532 of them in strand hair and beard. Cap, collar and lapels were built while the face was still failing. | **Supported.** Effort went to parts that do not decide the 2/5. |
| **Tools: MCP version** | Our MCP gives `execute_blender_code`, `look`, `get_scene_info`, asset search and a paid `generate_3d`. Its craft guides (rigging, retopology, materials) ship in the package, but `get_guide` is switched off in 2.2.7 ("switched off while evals measure what the model does without them"). `look` can show views and a file, but it has no overlay, landmark or difference mode. | **Minor.** Everything the guides cover can be done with plain Python. The missing compare tooling is ours to write, as the board lab already did. |

In short: the miss comes mostly from **feedback with no numbers, editing raw vertices, an
average starting head, and skipping the illustration layer**. This does not establish a Blender limitation or rule out missing artistic judgment.

## Evidence table

"Seen" means we read the code, the paper text or the images ourselves. "Claimed" means only
the author says so.

| # | Source and date | What is shown | Agent and interface | Prompts or code | Visual feedback | Outside assets and human work | Rig and export | Reproducible? | Weight |
|---|---|---|---|---|---|---|---|---|---|
| E1 | LL3M, arXiv 2508.08228, 2025-08-11; project page threedle.github.io/ll3m | Props and simple scenes made from Blender Python: piano, windmill, drum kit, lamp, hats restyled. No human faces. | Multi-agent: GPT-4o plans and retrieves docs, Claude 3.7 Sonnet writes code, Gemini 2.0 Flash in another role. Critic sees 5 rendered views. | Paper describes roles and a Blender API retrieval base. Repo github.com/threedle/ll3m is under an academic and evaluation license and includes an auth/login module. | Automatic critic plus user edits. The paper says vision models "may still struggle to accurately identify spatial artifacts", e.g. a watering-can handle left disconnected. | None for meshes. User corrections fix what the critic misses. | No rigging. | Partly: code is public, use is restricted, and it appears to need their service. | Strong for props, none for characters. |
| E2 | BlenderGym, arXiv 2504.01786, CVPR 2025 | 245 start-to-goal edits in 5 tasks, including 75 blend-target edits such as facial expression. | GPT-4o, Claude 3.5 Sonnet, Gemini and open models, editing Python. | Benchmark and prompts public. | Generator plus verifier, at least 3 fixed cameras per task. Main failure: "fails to capture subtle visual differences". | Starting scenes are hand-made. Ten average Blender users, limited to 8 minutes, set the human baseline. | n/a | Yes. | Strong: it measures where agents fail. |
| E3 | VIGA, arXiv 2601.11109, v1 2026-01-16, v3 2026-04-06 | Scenes rebuilt from one image: write code, run, render, compare, revise. | Training-free loop. Tested with GPT-4o, Claude 3.5 and Sonnet 4, Gemini 2.5, Qwen3-VL. | Skill library splits read-only "investigate" camera moves from editing tools. Short sliding-window memory. | Rendered views from several angles, compared with the target. | Complex objects come from external meshes (SAM-3D via `get_better_assets`). Primitives only for simple items. | n/a | Paper only; we did not run it. | Strong on loop design. Shows agents arranging assets, not authoring them. |
| E4 | EZBlender, arXiv 2601.07143, 2026-01-12, WACV 2026 workshops | A planner splits edits into parallel sub-agents. | Plan-and-react agents. | Repo named in the paper. | Scored on BlenderGym, including blend-target edits. | n/a | n/a | Not checked. | Medium: more evidence for named controls. |
| E5 | BlenderAlchemy, arXiv 2404.17672, ECCV 2024 | Procedural material edits picked by searching candidates. | GPT-4V. | Paper. | Renders each candidate and scores it with a vision model. Generates a target image when the goal is only text. | n/a | n/a | Paper. | Medium: candidate search and a visual target help. |
| E6 | Japanese blog by a Claude Code user, note.com, 2026-04-12 | Asked for an anime-style character from a three-view sheet. Got "98 spheres and cylinders" and then "a pile of circles and ovals". A snowman scene then worked. | Claude Code with the community Blender MCP. | Prompts quoted in the post. | Author looked at viewport images. | None. | None. | Yes. | Strong as an honest failure from primitives. |
| E7 | blender-game-skills, github.com/majidmanzarpour/blender-game-skills, created 2026-09-24, MIT | A Claude Code skill for turning concept art into rigged game assets through 11 gated phases. | Claude Code, headless Blender. MCP optional. | Full SKILL.md and scripts: camera-matched review renders, silhouette IoU, ten-band width profile, world-registered gate, validator, export manifest, reimport. | Every gate needs a compare sheet with numbers. Mismatches are written as numbers ("head 12 percent too tall"). | Face method: front and profile blueprints, 30 to 50 landmarks, warp to under about 1 mm, project de-lit blueprints onto UVs. | Rig and export phases with checks. | Code yes. **No result images are in the repo**, so the quality of its output is claimed, not seen. | Strong as instruction design. Unproven as output. |
| E8 | Character Creator 5 plus Headshot 3 tutorial, BlenderNation, 2026-09-10 | A human turns one caricature image into a rigged stylized character. | Paid commercial tools, with an AI side-profile generator. | Step-by-step text. | Human, working over image plates. | All the art judgment is human. The fitter "is trained on standard human anatomy, so on a caricature it will initially pull in toward normal proportions". The head mesh is locked to the image's camera so the projected texture stays aligned. | Rigged base. Sculpt goes in as a morph. | Needs paid tools. | Strong evidence on method. Not an agent result. |
| E9 | Blender Lab MCP (projects.blender.org/lab/blender_mcp) and Anthropic's "Claude for Creative Work", 2026-04-28 | Official server. About two dozen tools: run code, blend-file summaries, API and manual search, window screenshots, thumbnail renders. | Any MCP client. | Tool list read from its `readme_tools.rst`. | Screenshots only. No multi-angle view tool. | n/a | n/a | Yes. | Medium. Its stated use is explaining scenes, docs, debugging and batch scripts, not authoring characters. |
| E10 | MPFB 2.0.16 release notes, 2026-06-13; 2.0.18 on extensions.blender.org, 2026-10-10 | Parametric human generator: phenotype controls such as age, Rigify in one click, and 52 ARKit-style face controls as named blend targets. Standalone scripting examples. | Blender add-on with a Python API. | Release notes and scripting examples. | n/a | Code GPLv3. Core assets and exported characters CC0. The face-units pack is a separate community download and **its page states no license**. | Rig yes, export via Blender. | Yes, free. Listed for Blender 4.2 and newer; not checked on 5.3. | Strong candidate for named controls. Its default look is realistic-generic. |
| E11 | Blender Human Base Meshes notes (developer docs; 4.0 release notes) | Bundle lists a stylized head, a planar head, and a generic head with example head variants. | n/a | n/a | n/a | CC0. | n/a | Yes. We used only the realistic head. | Medium. |
| E12 | Image-to-3D models: TRELLIS.2 (Microsoft, MIT, 2025-12), Hunyuan3D 2.1 (Tencent), StdGEN (CVPR 2025), SAM 3D (Meta, 2025-11) | One image to a textured mesh. StdGEN splits body, clothes and hair for anime characters. | Model, not an agent. | Open weights. | n/a | TRELLIS.2 needs an NVIDIA GPU with 24 GB, tested only on Linux. Hunyuan3D 2.1's license excludes the EU, UK and South Korea. SAM 3D uses Meta's own license. | Meshes need retopology and rigging after. | Not on our Mac without new hardware or services. | Low for us now. |
| E13 | Secondary reviews and vendor blogs (MindStudio, 3daistudio, StraySpark, a reseller test of a treasure chest, 2026-10-20) | Say MCP is weak at organic characters and rigging. One test made a 1,028-triangle chest in 341 s with Claude Code. | Various. | Varies. | Varies. | Several authors sell 3D generation or MCP tools. | n/a | Rarely. | Low. Used only where primary sources agree. |

## The strongest examples, in more detail

**E2 BlenderGym: the clearest measured evidence.** The authors built 245 edit tasks with
fixed cameras and compared models with ten everyday Blender users given 8 minutes per task.
Models struggled with tasks a novice finds easy. The paper sorts failures into three kinds:
missing small visual differences, scripts that do not run, and code changes that do not
match the difference the model described. It also found that spending compute on the
verifier, the step that picks the best try, pays off. For us, a better judge of each
attempt may matter more than more attempts.

**E3 VIGA: the loop that works for scenes.** The agent keeps looking and editing apart.
It can call "investigate" to move the camera and gather views without changing anything,
then edit with code. Memory keeps only recent plans, code diffs and renders, which the
authors say prevents long-run drift. For hard objects it fetches existing meshes and does
not try to model them. The lesson: the strong systems make the agent a director and
measurer, not a sculptor.

**E1 LL3M: what code-only modelling can reach.** Clean, editable props from scripts,
with a critic looking at 5 views. Reusing the previous script, instead of regenerating,
keeps the asset stable between rounds. The gallery has no adult human face. The authors
show the critic missing a disconnected handle that a user had to point out.

**E7 blender-game-skills: the best written instructions we found.** Every constant in a
build script must name the reference view it was measured from or be marked `# inferred`.
Gates use silhouette IoU and width bands at the game's pixel size. No detail goes on until
the silhouette passes. Exports are re-imported and checked against a manifest. For faces it
asks for front and profile blueprints and landmark error under about 1 mm, then projects
the de-lit drawings onto the head. That is the same idea our board lab used. The repository
has no output images, so we cannot say how good its characters look.

**E8 Character Creator plus Headshot: how a human does our exact task.** Start from one
caricature image. Make a side view. Push the fitting curves out to the real edges of the
caricature, because the fitter drifts toward normal anatomy. Sculpt primary forms over the
image plates with wide soft falloff. Keep the head locked to the image's camera so the
projected texture stays aligned. Sculpt finer forms as a morph on a rigged base. This
paid tool chain is not for us, but the order of steps is: register to the image first,
then push past "normal" on purpose, then project paint, then refine.

**E6 the honest failure.** Primitives plus a toon shader did not make a character from a
three-view sheet. The author's own next ideas were direct mesh editing and starting from
an existing model. We already tried both, and both helped. The remaining gap is
measurement and the look.

## Is there a strong, comparable autonomous character success?

No, not one we could verify. We looked for agent-made adult heads with a clear expression,
in a stylized look, with the prompts, iterations and exports shown. We found:

- research systems that make props and scenes (E1, E3), or tune named controls on
  existing models (E2, E4);
- skills and tutorials that describe character pipelines, with no result images (E7);
- vendor demos where an image-to-3D service makes the character and the agent places it.
  We did not count these, because the hard part is done by the paid model and the demos
  hide the selection and clean-up;
- human-led tool chains for stylized heads (E8, and Blender Studio's stylized character
  course, where the artist explores many rough heads and throws most away).

The search does not establish a typical quality level for autonomous character work.
Our 2/5 result is evidence about this attempt only. The question is which parts to
hand to measurement, named controls, existing assets and the owner's eye.

## What agents do well, and where people supply the rest

Agents do well at:

- writing repeatable build scripts that can be re-run from a clean start;
- hard-surface props from measured parts (our board, LL3M);
- placing, scaling, lighting and wiring materials when they get numbers back;
- turning a number-backed difference into a code change ("corner 3.5 mm below centre");
- validation, export, re-import and file hygiene (our technical checks all passed).

People or outside assets supply:

- the base anatomy (scan meshes, base meshes, parametric humans);
- the look, through painted textures and projection from concept art;
- judgment of likeness and expression. Benchmarks show vision models miss small
  differences, and our reviews could only say "reads younger";
- complex organic geometry, through image-to-3D models or sculptors.

## Supported conclusions and hypotheses

Supported by sources and our own files:

1. Agents improve with feedback that has numbers and fixed cameras (E2, E3, E7, our board).
2. Vision-only self-critique misses small visual differences (E1, E2). Expression edits
   are small differences.
3. Named controls are easier for models than raw geometry (E2, E4).
4. Strong systems bring in existing meshes for complex organic forms (E3, E8, our
   checkpoint 4 jump).
5. One three-quarter painting does not pin down a 3D face (E7, E8).
6. Our bust skipped the board's working technique: a registered camera and paint placed
   from goal pixels.

Hypotheses we have not tested:

- H1. A camera fitted to concept landmarks, plus projected concept paint and plum ink,
  will move Bram's talk-size read more than any further vertex editing.
- H2. Warping toward concept landmarks will fix age and expression better than ellipsoid
  edits, because the target positions are measured, not guessed.
- H3. A stylized or planar base head will drift toward average less than the scan head.
- H4. MPFB's 52 named face controls would let an agent tune the half smile by numbers.
  Its realistic default look may still fight the concept.
- H5. At conversation size, most of the remaining gap is in surface paint and lines, not
  in 3D form.

## Recommendations for our workflow

Use these for the next character attempt in Blender 5.3 with MCP 2.2.7 and add-on 1.8.

1. **Register before you edit.** Mark 20 to 30 landmarks on the concept face: eye corners,
   pupils, brow peaks, nose tip and wings, mouth corners, upper and lower lip middle,
   chin, ear top, hairline, cap brim. Fit the talk camera to them and report the error in
   pixels, as the board lab did.
2. **Edit toward measured targets.** Warp the head so its landmarks project onto the
   concept's landmarks, with smooth falloff, and store the result as a named blend target.
   Report error per landmark after every round.
3. **Use named controls for expression.** Build or import named blend targets (smile left,
   cheek raise left, lower-lid raise left, brow raise right) and tune their values. Do not
   move rings of vertices by hand.
4. **Put the look in paint and ink, as on the board.** Project the concept's face paint
   from the fitted camera, keep a hand-paintable marks layer, add the ink hull, and render
   with cel bands and plum ink as in `direction.md` 1.1. Keep the neutral clay check only
   as a diagnostic.
5. **Judge at the size the player sees.** Compare at 240×380 and 570×900 with a sheet
   showing concept, render, 50% blend and landmark overlay. Ask the owner, not an agent
   score.
6. **Order the work.** Face first. Hair, beard and costume only after the face passes. Hair
   and beard as a few shells or cards with painted texture, not strands.
7. **Write the tools as lab scripts and call them through MCP.** `look` is for quick looks.
   Compare sheets and numbers come from our scripts, which also run in background Blender.

### Stop doing these

- Stop editing faces by pushing ellipsoid regions or vertex rings and judging by eye.
- Stop holding back creases and lines. The concept's age is in its lines.
- Stop judging the bust in smooth realistic lighting with no ink. That is not the game's
  style.
- Stop placing the talk camera by hand. Fit it to concept landmarks and report the error.
- Stop building cap, collar and hair detail before the face passes at talk size.
- Stop using strand hair for a talk-size portrait (580k of 983k triangles).
- Stop counting an agent reviewer's x/5 as progress. Use measured error plus the owner's
  side-by-side verdict.
- Stop assuming the realistic scan head is neutral. It pulls toward its own face.

## Tool contract for the implementer

Each tool is a script in the lab folder that runs in a live session or with
`blender -b ... --python <script> -- <args>`. Every tool writes a JSON result and its images
to the lab's `renders/` folder.

| Tool | Input | Output | Fails when |
|---|---|---|---|
| `mark_concept_landmarks` | concept crop, landmark list | `concept_landmarks.json` in crop pixels, plus an overlay image the owner can check | fewer than 20 landmarks, or the owner rejects the overlay |
| `fit_talk_camera` | concept landmarks, matching mesh vertex ids | camera, focal length, RMS and worst error in pixels | RMS above 1.5% of the crop's eye-to-eye distance |
| `warp_to_landmarks` | target pixels, stiffness, region mask | named blend target `Fit_Concept`, error per landmark | any mesh triangle flips, or error grows |
| `set_face_controls` | `{control_name: value}` | values applied, landmark error before and after | unknown control name |
| `project_concept_paint` | concept crop, fitted camera, UV map, mask | painted layer, and a mask of what the camera cannot see | painted area leaks onto the back of the head |
| `render_compare` | sizes, style `game_cel` or `clay` | sheet: concept, render, 50% blend, landmark overlay; RMS error; silhouette IoU | missing camera or concept |
| `validate_budget` | asset collection | triangle count per part, textures, materials | head over 30k triangles, or total over 60k |

## Sample implementer brief

```text
Goal: Bram's face at conversation size, matching the right-hand face study in
game/docs/visual-upgrade/references/room-and-face.png. Face only: no cap, collar or hair
detail beyond flat placeholder shells.

Start from a copy of the checkpoint 7 head in prototypes/room-look/blender-bram-lab
(keep the original untouched). Work in a new lab folder.

Order:
1. Mark at least 20 landmarks on the concept crop. Save them and an overlay. Stop and show
   the overlay before going on.
2. Fit the talk camera to those landmarks. Report RMS and worst error in pixels.
3. Warp the head toward the landmarks as a named blend target. Report error per landmark.
   Repeat until RMS is under the limit or three rounds stop helping.
4. Project the concept's face paint from the fitted camera. Add the ink hull and cel
   lighting from direction.md 1.1. Crease lines are allowed and expected.
5. Render the compare sheet at 240x380 and 570x900, plus front, side and 20-degree yaw
   views in clay.

Every change is a script you can re-run from the copied head. Every number in a script
says where it was measured, or is marked inferred. Describe differences as numbers, not
adjectives. Do not score your own result out of 5. Hand back the sheets, the numbers and
a list of what you inferred.

Out of scope: purchases, accounts, paid generation, new add-ons, hair strands, animation,
engine changes.
```

## Smallest decisive next experiment

**Question:** if the head is registered to the concept and carries the concept's paint and
ink, does Bram read as the concept's Bram at conversation size? Or is the gap in 3D form?

**Setup:** the brief above. One head, one camera, one session. No new installs. It reuses
the board lab's raycast-to-UV paint method and its comparison sheets.

**Arms:** A is the current checkpoint 7 head with the new camera and look, but no warp.
B is A plus the landmark warp. Showing both separates "the look fixed it" from "the form
fixed it".

**Success, all of these:**

1. Talk-camera landmark RMS under 1.5% of the eye-to-eye distance in the crop. No
   landmark off by more than 4%.
2. Shown blind at 240×380, the owner picks the new head over checkpoint 7 as "Bram from the
   concept" and calls it mature with a warm half smile.
3. At ±20° yaw from the talk camera, the projected paint does not visibly smear or double.
4. The head stays under 30k triangles. Export and re-import match, as the board check did.

**Failure, and what it tells us:**

- Landmark error stays high after the warp: investigate camera fit, landmark selection and the deformation algorithm before rejecting the base head. Next,
  try the stylized or planar base (H3) or MPFB with its named face controls (H4), after
  the owner approves the install and the face-units license is checked.
- Numbers pass but the owner still rejects it at talk size: the metrics missed important
  visual differences. Geometry, materials, expression and registration all remain possible causes. Next, raise
  with the owner whether talk faces should be painted 2.5D layers or need a human sculptor.
  Either one changes the game's direction and is the owner's call.
- A beats B clearly: most of the gap was paint and ink (H5). Put effort into the
  painting tools, not sculpting.
- Paint smears at ±20°: projection alone cannot carry it. Paint needs a second view or hand
  repainting in the marks layer.

## Decisions for the owner

These are not needed for the experiment above. They come up only if it fails in the
ways described.

- **Installing MPFB plus its face-units pack.** Free and GPLv3. Core assets are CC0, but
  the face-units pack page states no license. Recommended only if the warp test fails on
  form.
- **Image-to-3D.** TRELLIS.2 needs an NVIDIA 24 GB GPU. Hunyuan3D 2.1's license excludes the
  EU and UK. Our MCP's `generate_3d` uses paid credits. Not recommended for now.

## Sources

- LL3M: https://arxiv.org/abs/2508.08228, https://threedle.github.io/ll3m/,
  https://github.com/threedle/ll3m
- BlenderGym: https://arxiv.org/abs/2504.01786
- VIGA: https://arxiv.org/abs/2601.11109
- EZBlender: https://arxiv.org/abs/2601.07143
- BlenderAlchemy: https://arxiv.org/abs/2404.17672
- Honest failure blog (Japanese): https://note.com/fit_liger8646/n/n5f33b25ccea9
- blender-game-skills: https://github.com/majidmanzarpour/blender-game-skills
- Stylized image-to-head tutorial:
  https://www.blendernation.com/2026/09/10/headshot-3-stylized-image-to-3d-building-a-bodybuilder-caricature-in-cc5-and-blender/
- Blender Lab MCP: https://projects.blender.org/lab/blender_mcp
- Claude for Creative Work: https://www.anthropic.com/news/claude-for-creative-work
- MPFB: https://static.makehumancommunity.org/mpfb/releases/release_2016.html,
  https://extensions.blender.org/add-ons/mpfb/,
  https://static.makehumancommunity.org/assets/assetpacks/faceunits01.html
- Human Base Meshes:
  https://developer.blender.org/docs/features/asset_system/asset_bundles/human_base_meshes/,
  https://developer.blender.org/docs/release_notes/4.0/asset_bundles/
- Blender Studio stylized character course:
  https://studio.blender.org/training/stylized-character-workflow/
- TRELLIS.2: https://huggingface.co/microsoft/TRELLIS.2-4B
- StdGEN: https://stdgen.github.io
- Community MCP (installed version): https://github.com/ahujasid/blender-mcp
