# Visual upgrade plan

Working plan for the core's rooms, beings, station exteriors and 2.5D flight.
Andreas tests each visual proof and playable increment before the next begins. This plan is a
proposal, not approval to replace the renderer or restyle the whole module at once.

## The target and the first decision

The current preferences are a friendlier, stranger technical world, saturated
paint, strong plum contours, fine drawn material marks and convincing 3D depth.
Station construction is scruffy, repaired and adapted around unfamiliar older
forms. Adult beings have distinctive faces and believable weight. Flight stays
on one plane with the lower, angled camera Andreas preferred.

[Reference guide](visual-upgrade/references.md) explains what each selected image
is meant to establish. These are generated concepts. They prove neither an asset
pipeline nor animation nor performance. The older construction is a visual
proposal, not a new explanation of the story's ancient objects.

[Direction](direction.md) remains authoritative until its amendment is agreed and
merged. Step 0 must state what changes in its low-poly, parametric-model and
lighting rules, including whether textured and rigged authored assets are allowed.
Retain its readable palette, 2.5D flight, visible speaking faces, bubbles, calm
pacing and familiar interactions. The eventual office experiment compares staged and
shoulder cameras; walking in that room is an agreed experiment, not a change to
core travel or controls. Adopt a new room presentation only after Andreas tests
and selects it. E's separate face study does not propose a second portrait UI.

Keep experiments in `prototypes/room-look/` until the look verdict is merged.
Engine work then lands as generic capabilities; the GM restyles core through
installed builds. Only Andreas merges into `main`.

## How each increment works

1. Name the one visible improvement, reference crop, test and scores expected to
   change. Put the goal beside the result at comparable framing and pixel size.
   A previous build is useful secondary evidence, never the quality target.
2. Make a small interactive asset viewer or playable build. Check it and examine
   a recording before handoff. Include a repeatable starting state and build ID.
   Reuse one viewer as capabilities accumulate; avoid another bespoke renderer
   and interaction system for each experiment.
3. Give Andreas a focused test and short clip. For an isolated asset, rotate it,
   inspect it at its intended size and change the relevant light or pose. Use the
   unprompted arrival check once a room exists. Explain unfamiliar terms and help
   identify what looks wrong.
4. Andreas plays and scores it with the [playtest sheet](visual-upgrade/playtest-template.md).
   A sentence such as "his hands float above the book" is enough evidence.
   The agent supplies measurements and a separate assessment, never invented
   owner scores. Feedback here is authoritative; copy the agreed result into the
   plan or test record before continuing.
5. Fix demonstrated defects in that increment and retest the affected part.
   Start the next increment only after Andreas accepts this one. His next test
   should compare a concrete result, not approve an unseen implementation.

Use the root merge table for engineering defects. The score thresholds below are
proposed visual rollout criteria, not a replacement merge policy. Keep this plan
about remaining work; remove a step as it lands, and keep the lasting design
choice in `direction.md`. Preserve accepted reference captures separately from
raw logs and frame dumps.

## Scorecard

This is our local review scale. The public developer accounts reviewed for this
plan describe goals, playtesting and iteration, not a shared numeric art rubric.
The following checks are our application of those lessons, not claims about
another studio's internal acceptance process.

Use whole numbers, not percentages. Rate only what was actually tested. `N/A`
means absent from this increment; `untested` means present but not checked.
Neither means a pass. Use the same reference and test conditions for comparisons.

| Score | Meaning |
| --- | --- |
| 0 | Required capability is missing or unusable. |
| 1 | Clearly wrong for the intended result. |
| 2 | Some progress, but a specific distracting problem remains. |
| 3 | Acceptable to build on in this increment. |
| 4 | Matches our target in normal play, including motion. |
| 5 | Especially successful; worth keeping as a reference for later assets. |

| Category | Plain-language question | What to look for |
| --- | --- | --- |
| Look adherence | Does this belong to our core? | Palette, contours, fine drawn detail, depth and the friendly alien tone match the agreed reference. The first impression fits the intended kind of place. Repairs look useful. Detail remains readable at actual play size. |
| Character adherence | Is this the right person? | Age, face, clothing, posture and gestures match their sheet and approved model. Recognition survives different expressions, lighting and distance. Personality is visible without inventing story facts. |
| Liveness | Does this feel inhabited and working? | People attend to tasks and each other. Machines have a purpose. Starts, stops and reactions make sense. Quiet moments remain quiet; everything does not bob in synchrony. |
| Graphical quality | Does the image hold up when inspected? | Materials differ, light explains volumes, shadows ground objects, outlines stay clean, faces have definition. No detached hatching, glaring glow, accidental intersections or distracting detail noise. |
| Motion quality | Does movement have intention and weight? | Eyes lead a glance, a hand meets its object, feet plant, turns settle and loops join cleanly. Mouth and eyelids deform without tearing. Stops and interrupted actions look plausible. |
| Performance | Does it remain responsive and smooth? | Steady pacing in flight, during talk and during room changes. Pair the human rating with actual frame-time and transition measurements below. |
| Readability and navigation | Can I tell where I am and what matters? | On arrival, the useful person, object and route are identifiable without coaching. Tug remains findable behind structures; resources differ by form as well as colour; faces and boards are readable; routes, doors and berths are clear. |
| Interaction clarity | Can I use what I see? | Clicks hit the visible object during motion. Hover and selection remain clear. Labels and bubbles stay attached and do not cover faces or the task. The board looks usable without making every prop look clickable. |

Readability and interaction clarity are the two additions to the requested six.
They deserve separate scores: an attractive board can be unreadable, and a readable
board can still be hard to select. Judge visual consistency across rooms under look
adherence rather than adding another near-duplicate score.

Do not average the categories. A 5 for materials cannot cancel a 1 for motion.
For an exploratory increment, every applicable category should reach at least 3,
with no loss against its accepted predecessor unless Andreas agrees a specific
trade-off. Before broad restyling, the representative scene should reach 4 for
look, character, graphical and motion quality, and at least 3 elsewhere. Final
rollout aims for 4 in every applicable category. A visual proof must reach 4 in
its declared target categories before it becomes the basis for the next proof.
For example, a rotating head can establish appearance while acting remains N/A.
A technically correct import scoring 2 for appearance does not establish the look.
No required item may remain untested at the final review.

An invisible player, covered speaking face, unreadable required text, unreachable
interaction or crash must be fixed regardless of scores. A prototype deliberately
reproducing such a failure is evidence, not an accepted implementation.

### Arrival and attention check

Before naming the intended target, show the normal entry view with its real UI.
Ask "What draws your eye? What kind of place is this? Where would you click?"
Record the first answer, hesitation and any wrong selection before giving hints.
Then ask for the actual task and observe whether it is clear how to begin.
Repeat in the busy version and after returning to the room. A familiar tester
can confirm later improvements, but cannot reproduce a fresh first impression;
use an unfamiliar tester for that check when one is available.

Use this under look adherence, readability and interaction clarity. A lamp,
colour contrast, framing or a cable can guide attention to Bram or the board.
More clutter or more animated props can pull attention away from them. Preserve
hidden story details and player-led discovery; this check concerns finding the
ordinary room task, not revealing secrets or adding quest markers.

## What is missing today

The renderer already draws real 3D models, outlines and cel lighting. Its
[current contract](../../engine/docs/renderer-and-style.md) describes the supported models and lighting. The
present model path reads vertex colour and one morph target, not model textures
or skeletal character animation. The
[place model](../../engine/docs/places-and-beings.md) already owns beings, their
locations and recorded activity. Keep that division when adding richer motion.

The two office experiments have not established the target look. Andreas finds
both too far from the goal to give useful comparative feedback. Retain them as
interaction and control experiments. Neither is the accepted art foundation.
The immediate work is to prove individual assets and rendering capabilities,
then combine accepted results into a room.

Use the [asset and rendering check](visual-upgrade/asset-and-rendering-check.md)
throughout these proofs. An adult face needs authored anatomy and deliberate
hair; more shader noise cannot supply those. Conversely, an authored asset that
loses its paint or deformation after import exposes a runtime gap. A result in
an authoring tool and a result in the game answer different questions.

## Incremental visual proofs

The first experiments are small interactive tests. Each ends with Andreas judging
one visible result against the goal. Later rows are dependencies, not permission
to build ahead of those decisions. Split any row that cannot support focused
feedback. Add only capabilities required by the asset being tested.

| Step | Deliverable and boundary | Andreas's test and exit evidence |
| --- | --- | --- |
| 0. Define comparable targets | Crop the selected reference into a prop/material study and an adult face/clothing study. Record which details belong in geometry, painted marks, lighting and motion. Agree Bram's visual brief from current content. Keep core unchanged until the direction amendment merges. | Goal and proposed asset brief are specific enough to judge at close and intended play sizes. Generated images remain references, never runtime evidence. |
| 1. One imported prop | A small section of the posted board: painted casing, fasteners and one paper card. Actual editable mesh, normals, UV coordinates and authored image texture under plain light. Support rotation and zoom. No office, walking or dialogue. | Compare silhouette, bevels, paint and fine marks against the goal. Turn it to expose flat illustration tricks. Apply one requested correction and export/import again. Record source files, rights, conversion and missing runtime support. Look and graphical quality must reach 4 before treating this as an accepted asset. |
| 2. One adult bust | Bram's head and clothed shoulders, with coherent facial anatomy, deliberate hair and cloth folds. Check geometry under plain light before relying on ink or wear. Use the established import route. No body routine or full room. | Rotate front, side and three-quarter views; inspect close and at talk size. Judge adult appearance, identity, silhouette and clothing against the target, not the office experiments. Look, character and graphical quality reach 4 before animation work. If we cannot author this quality, resolve asset sourcing or authoring help here. |
| 3. Light and ink on accepted assets | Use that same prop and bust to establish the material response, warm light/violet fill, cast/contact shadows and controlled contours. Add one treatment at a time and keep a plain-light comparison. Authored internal marks stay attached to the model. | Move the light and camera through a repeatable range. Skin, cloth, paper and painted metal remain distinct. Lines stay stable, shadows ground the object and detail survives at play size. Look and graphical quality reach 4 in runtime. Record the cost of each added treatment. |
| 4. Deform and animate one being | Use the accepted bust for a blink, glance, facial change and short speaking motion. Then extend the approved model with the body and hands needed for one reach. Add only the import, skinning, expression and clip support those actions require. | Inspect facial identity and contours during deformation, then hand-to-card contact and interrupted movement. Clicking the moving being and projecting a speech anchor must agree with its pose. Character, graphical and motion quality reach 4. A new animated model must retain the approved appearance. |
| 5. Assemble Bram's office | Combine the accepted assets, light and motion into one room corner, then the full office loop below. Add furniture and props individually. Compare staged and shoulder views using the same scene and state. | Judge composition, liveness and usability with real UI. Test arrival, conversation, board, ledger and leave/return. Reach the representative-scene score threshold before expanding content. Choose the room presentation from this playable evidence. |
| 6. Prove the flight kit and camera | Carry the accepted treatment into one tug, station module, support and resource rock, one asset test at a time. Use the preferred lower camera and preserve one-plane flight. Solve the known hidden-tug case. | Pan, turn, approach and dock. Check recognition, material differences, stable lines, selection and complete tug occlusion. Each asset must read at flight distance before building a dense station. |
| 7. Assemble the station exterior | Build the scruffy station from the accepted kit. Add one purposeful worker task and one machine operation before increasing activity. Relevant changes follow actual simulation events. | Approach, identify destinations, dock, watch work, leave and return. Geometry, effects and motion must preserve paths and click targets. Measure the full scene before increasing density. |
| 8. Expand rooms individually | Follow the room order below using the accepted assets and motion workflow. Each being retains their own appearance and acting. | Each room has its own owner test of its short task, talk and leave/return loop. Fix failures before moving on. |
| 9. Join the journey and prove repeatability | Integrate with the landed first-hour content. Make one additional prop and animation variation through the documented workflow. Replace temporary references with accepted runtime captures. | Play flight, docking, room work and departure, quiet and busy, including repeat visits and loading. The extra asset needs no special engine code. Final scores and measured performance support acceptance. |

### Choosing rendering capabilities

No new backend is selected. The current engine already renders 3D; the question
is which missing capabilities the accepted assets need. Keep prototypes disposable.
Only propose generic engine PRs after a proof establishes their visible purpose.
Avoid a full material system, animation editor or renderer replacement on the
strength of a feature list.

If a required import or rendering feature is absent, record it explicitly. Use
one candidate runtime to establish how the same asset should look when necessary.
Compare identical source assets, camera, pose and lighting intent, allowing for
recorded colour-management differences. Do not rebuild two complete backends or
substitute different assets to hide unsupported data. Price extending the current
renderer against adopting the candidate, using the actual missing capabilities.

Before adopting another runtime for rooms, it must also prove picking, projected
speech anchors, UI composition, room transitions and recorded-state reconstruction.
Simulation, saves and module rules remain authoritative. The engine's learning
principle needs an explicit owner decision if it changes. Asset quality alone
cannot decide this integration, and 2.5D flight does not require a replacement.

### Office assembly test

Assemble the office only after its prop, being, material and motion proofs pass.
First frame Bram, desk, ledger and board in a small corner to judge composition.
Then add the entrance and enough floor space for the complete interaction loop.
No corridors, crowd, inventory redesign or new story branch is needed.

The temporary player figure tests body and camera placement, without establishing
canonical appearance. Use first-hour board and ledger content when supplied;
necessary local samples are explicitly provisional. The room test includes:

1. Enter without hints and give the arrival/attention answers. Find Bram, the
   ledger and the board. Try each camera first on a separate comparison run.
2. Move between those tasks and near walls, corners and furniture. Inspect close
   and oblique views. The camera must not enter geometry or lose the task behind
   the player's body.
3. Talk while Bram works, end and restart, and interrupt an action. Face, bubble,
   pointer target and position agree. Precise voiced lip synchronisation is not
   required for this proof.
4. Read and select the board and ledger with full UI. Switch cameras during the
   same task and conversation; location, activity and selection survive.
5. Leave and return. Score the views separately and name one correction. Choose
   staged, shoulder or a specific further camera test from the evidence.

The existing flight geometry test reaches the berth in about 16.4 simulation
seconds. A separate stress view completely hides the tug behind the hub.
Reproduce it with `prototypes/room-look/check-flight.py` before changing the
camera treatment. These findings prove neither target art nor steady performance.

### Room order within step 8

1. **Fitting cradle and Orrin.** Ship, sockets, rack and glass tag must remain clear
   while Orrin examines the tag. The first-hour track owns the blueprint event.
2. **Mags's office.** Give her charts deliberate organisation and the gap around
   the ring, without turning it into a glowing clue marker. Check her brimmed hat,
   red pencil and brisk, precise gestures against her sheet.
3. **Cold Kettle and Hessa.** Prototype the stove, suspended kettle and practical
   barge interior while its content is developed. Integrate only once the
   first-hour track supplies the room and visit. Hessa's age, teal knit cap, heavy
   coat, weathered face and working habits must survive the new treatment.
4. **Exchange and Tova, then Sefa.** Test frequent trade, selecting moving beings,
   resource/material clarity and the counter at full UI width. Sefa's scarf,
   pushed sleeves and patient attention to work must differ from Tova's approved
   performance. Her existing sheet controls her identity and story.
5. **Galley and Ines.** Stove, serving, seating and conversation, first quiet and
   then with a small group. Keep the cook and food readable through steam and
   activity. This is the main busy-room performance test.

Each numbered item stops for Andreas's test. Tova and Sefa also get separate
acceptance. For beings without sheets, agree a small appearance and gesture
reference using current content. Do not create a new backstory to justify a model.
The player's origin and choices remain as their sheet defines them.

## Performance evidence

Aim to preserve smooth 60 Hz play on the agreed reference configuration. A frame
at 60 Hz lasts about 16.7 ms. The reported roughly 14 ms galley result is context,
not a GPU budget: existing draw timing can include waiting for presentation.
The agent must identify what each timing actually measures before claiming spare
rendering capacity.

For each increment use the same build route, seed, save/start state, output size,
display scale, quality settings, refresh rate and test configuration as its
baseline. Begin at the design size of 1280×720 logical pixels and record physical
output too. Also inspect Andreas's normal playing resolution. Measure a steady
20-second segment after warm-up, with no build running alongside it. Repeat when
results are noisy, not as a substitute for a controlled comparison.

Record median, p95 and p99 frame interval, the count of intervals over 33.3 ms,
and sim/scene/draw times where available. p95 means 95% of frames are this fast or
faster. Record room-entry and reload duration separately. Add GPU time only if
it is genuinely measured; mark unsupported measurements unavailable.

Proposed starting steady-play targets are p95 at or below 18 ms, p99 at or below
25 ms and no recurring 33 ms cadence. These are initial targets for agreement at
step 0, not established engine guarantees. Keep transition and asset-loading
spikes in their own results rather than hiding them in a steady average. Establish
entry/load limits from the baseline and Andreas's tolerance before changing them.
When an asset or effect changes, exercise it specifically, including its
activation and interruption, instead of relying only on the busiest benchmark.
For example, check a new wing animation on both turns and a new particle effect
while it starts and stops at the intended output sizes. A busy scene can pass
while a particular asset or combination still fails. A confirmed pacing
regression is investigated before accepting the increment.

For isolated proofs, measure only the asset and treatment under test. Passing a
single-object viewer does not establish a room budget. As scenes become available,
use an ordinary flight pass, a dense station approach, one close talk, a board
interaction and the busy galley. `engine/docs/tests-and-what-if.md`
describes the existing performance scenes and their timing limitations.
Headless films establish appearance and repeatability, not real-time 60 fps.
Agents run windowed measurements only in a verified isolated graphical session,
or provide the build for Andreas to run himself. Mark unavailable checks untested.
Validation stays local, follows desktop isolation and uses `cargo xtask` when
engine work requires builds. This documentation change requires no build.

## Ownership and handoff

This track owns visual assets, recipes, materials, lighting, staging, camera,
animation and the generic renderer capabilities needed to present them. The
first-hour track owns dialogue, scripts, maps/fog, docking rules, room existence
and board entries. Agree object anchors and interaction requirements together.
Do not replace their content with prototype placeholders during integration.

Before each core restyle, read the current room and being files again and combine
with the first-hour changes. Keep engine capability PRs separate from game restyles
except required mechanical compatibility fixes. Engine PRs contain no game nouns.
Use the repo's independent code review and local checks before handing a PR back.
This plan does not authorize a merge or a production deployment.
