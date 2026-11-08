# Talk

Conversations live in ordinary world components. The simulation owns starting,
advancing, interruption and seen status; the view only projects and paints them.
The code is `sim/src/talk.rs` and `view/src/talk.rs`. `galaxy-gm guide talk` gives
an authoring and playtest recipe, and `fields components` lists the schema.

## Content and progress

| Component | Fields and meaning |
|---|---|
| `engine.talk` | `speaker` is an entity; `first` starts a linked chain of lines. `anchor` is any published anchor, defaulting to `being.<speaker>`. `enabled` defaults true. `story` prioritizes an unseen moment over repeat dialogue. `active`, `current` and `seen` store engine-managed progress. `width` (320), `x` (0), `y` (-12) are logical-pixel bubble placement. `title` is the bubble's heading (empty uses the speaker's life sign name); `subtitle` is a quieter line under it, such as a place. |
| `engine.talk_line` | `text` is one nonempty line; `next` names another line or is empty at the end. A line with answers has an empty `next`. |
| `engine.talk_choice` | An answer `line` offers as a button: `text` is its label, `next` the line it leads to (empty ends the conversation), `order` its place among the line's answers (lowest first, ties by entity ID). |
| `engine.focus` | `part` names a mesh part, default `face`; empty explicitly means no focus. Beings without this component still use `face`. Non-beings have no required focus unless they opt in. |

Declare talk and line writes in the module manifest and define entities in start data,
or create them through ordinary script commands. A story event offers a new talk entity;
it does not need a player visit to keep its pending status. Disabled talks remain stored.
The speaker can be a being, a placed object or a place. An explicit anchor can point to
any publisher, including a model-less entity. Without a model, clicking within 22 logical
pixels of its anchor talks; with a model, picking uses the actual nearest mesh surface.
The speaker's location determines availability. A module can explicitly choose no focus
for a being without a face; missing required mesh parts are authoring errors.

Clicking a speaker picks its unseen story first, then repeat dialogue, then a seen story
if no repeat exists. Ties use entity ID order. Only one conversation is active at a time.
Starting another interrupts the previous one. Clicking the bubble sends
`engine.talk_next { talk, line }`; clicking a speaker sends `engine.talk_start { talk }`.
On a line with answers the bubble itself does not advance; its answer buttons send
`engine.talk_chosen { talk, line, choice }`, which continues to that choice's `next` or
ends the conversation, marking it seen.
Both are recorded inputs, validated against the current world. The line token ignores
stale advances: a double click on one displayed line advances only once. A stale answer
(not on the line showing, or not one of its answers) changes nothing and records no
`engine.talk_chosen` event, so a handler sees each answer once; the input stays in the
recording, so replays are identical. What an answer means is module logic: a handler can
record it, and keep a declined offer from returning by disabling its talk. A queued click
that outlives its room, its enabled state or the talk itself (a handler removing a
finished talk) is also a recorded no-op. Only acknowledging the last line sets `seen`. Leaving, moving the
speaker away or disabling the conversation interrupts without marking it seen. Starting
it again begins at its first line. Seen status has no clock or expiry.

Lines and the answers' next lines must form a finite graph without cycles, with existing references, nonempty text and a
current line in that chain when active. World batches validate atomically and roll back
mistakes. Start-state and resumed-world validation use the same rules. There is no new
save or recording format, and old worlds without talk retain their existing hashes.

## Presentation and picking

The view derives an internal `bubble` widget from the active line. It uses the screens'
text measurement, wrapping, anchor placement, viewport clamp, readability checks,
ordered paint layers and hit testing. Its paint uses the bubble surface of the module's `[screen]` style, with a neutral
default. No separate talk renderer or game look is introduced. Width and offsets are module-owned
presentation geometry.

The bubble is a column: the heading (larger and heavier), the subtitle (smaller and
lighter), the line, then either a small, light Continue/Done marker against the bubble's
end edge or a row of answer buttons. `[screen]` sizes them: `bubble_title` (24),
`bubble_title_weight` (650), `bubble_subtitle` (16), `bubble_text` (20, also the answer
buttons) and `bubble_marker` (16); subtitle and marker are drawn at weight 400, the line
at 500. Without answers the whole bubble is one click target: hit testing returns the
bubble for any point on it, its texts included. With answers only the buttons click.

Anchors follow the same interpolated pose as the figure. A bubble has no private timer,
position memory or Luau frame callback. Travel consumes landing clicks first and hides
interrupted talk. Painted widgets block speaker picking. `[[click]]` bot entries accept
`widget = "speaker.<full-id>"` for a model speaker and `widget = "talk.<full-talk-id>"`
for a bubble, and `widget = "choice.<full-choice-id>"` for an answer; coordinate clicks
also use the actual hit test. Model-less anchors use
coordinates. Bot targeting records the resolved input, never a future lookup.

## Visibility, combinations and limits

`galaxy-gm check` stages every authored talk, including disabled ones, in its speaker's
place, and every line that talk can reach, through `next` and every answer. A line with
answers is laid out with its buttons, so a bubble too narrow for them fails. It rebuilds the module's conditional screens, room
camera, figure poses, anchor and actual bubble dimensions together. Thus a longer second
line, room panel or clamped bubble can fail even when each screen alone passes. Existing
room checks still test every room and floor spot, including empty rooms.

Focus checks use the glTF mesh part's triangles, after model scale, facing, morph and
world pose. All focus vertices must project inside the viewport and camera depth range.
For every logical pixel covered by a front-facing focus triangle, a camera ray tests the
nearest focus surface against all visible posed meshes, including other parts of its own
model. A hat in front of the face fails even when the anchor projects correctly. The same
pixel is tested against clipped painted screen surfaces, including the speech bubble.
A missing mesh part or no front-facing focus pixels fails. Explicit no-focus speakers
still receive bubble readability and anchor checks.

Active straight walks are checked at every remaining tick and half-tick pose through
completion, with all moving entities evaluated together. Poses are streamed rather than
retained as a film in memory. The life check additionally stages every authored day arrangement and transitions
between them, including module-owned morph poses and simultaneous walkers. It does not
invent idle sway, future event-handler states or arbitrary script branches. Authored variants need their own check, and recorded play remains necessary.
Visibility is geometric at the checked viewport (1280x720), not a GPU lighting or
subjective expression judgement. Subpixel occlusion and model ink are not measured.

## While playing

`check` judges the states it stages; a script can still cover a face at runtime (patching
a model, moving a prop, opening a screen over the speaker). So every scene also runs the
same focus test (`View::live_focus` calls the check's `focus`) for each line on screen,
with the frame's camera, size, interpolated poses and painted screens. A covered or
off-screen focus becomes a problem such as ``talk `talk.third_talk`, line `talk.third_1`:
speaker `talk.third` focus `face` is covered by geometry of `talk.third` ``. It goes the
same way as live screen readability: the problems panel while it lasts, play's report
(every problem seen in the session, first seen first) and stderr with its tick. It reads
the world and never writes it, so replays and hashes are unchanged; because it is part of
the scene, F8, audit and film draw the same panel and notes still audit pixel-identically.
`check` turns it off while it stages lines, as it runs the same test itself.

Each view keeps the most recent focus verdict together with the exact inputs to that
pixel test (`view/src/talk/focus.rs`). A frame still builds its visible posed meshes and
screen hit geometry. The snapshot compares float bits, not hashes or world ticks: the
camera (including projection), logical viewport, speaker and named focus vertex range,
every visible mesh's ID, vertex positions, indices and transforms, and the complete
ordered screen hit hierarchy (kind, diagnostic path, rectangle, clip and hidden flag).
Only an identical snapshot reuses the verdict. Missing parts and models are checked
before reuse. Both success and failure are cached; the talk/line prefix is rebuilt for
the current line.

This is not a once-per-line check: camera shake or zoom, a fractional movement or morph,
a changed or hot-loaded mesh, an arriving or departing prop, and opening, scrolling,
clipping or hiding a screen all change the snapshot on the very frame they change the
visibility test's inputs. Changes to fields the test never reads, such as paint colour,
need no retest.

Below that, the view keeps the screen-independent half of the last 8 tests: per speaker,
part and set of poses, whether the part is on screen, and in pixel order each pixel that
shows it with the first mesh in front of it, as far as the test got. A test that differs
only in its screens (`check` stages every line of a conversation over the same poses, each
with its own bubble) replays those pixels against the new screens and casts rays only past
where the last test stopped. A first test casts no more rays than the plain loop, which
stops at the first covered pixel; it still runs on a miss, so a changed pose pays its cost.
A ray skips the speaker's and the other meshes' triangles when it misses their padded
bounds. Neither cache is saved or recorded; a fresh audit or film recomputes the same
verdict from its frame's inputs. Tests compare reused verdicts with forced-fresh pixel
tests across geometry, camera, viewport, interpolation, morph, asset, UI clipping,
hierarchy and paint-order changes.

For headless runtime measurements, set `GALAXY_FOCUS_MODULE` to a module folder,
`GALAXY_FOCUS_INPUTS` to an inputs/clicks route, `GALAXY_FOCUS_BEFORE` to its pre-talk tick,
and `GALAXY_FOCUS_TICK` to a tick holding an active line (seed 1, 1280x720). From `engine/`:

```sh
cargo xtask cargo test -p galaxy-view --lib talk_cost_probe --release -- --ignored --nocapture
```

The probe resolves clicks and arrivals headlessly, then measures 600 repeated held-world
scenes after five warmup frames: before talk, active talk, and a measurement control with
live focus disabled. It also reports screen layout, geometry preparation and focus costs,
the number of full pixel evaluations and Luau rebuilds, and the world hashes. Its isolated
focus samples include the first cold query. Optional `GALAXY_FOCUS_INPUT_EXPORT` writes
the resolved inputs so installed builds can replay and film exactly the same route.

## Reconstruction and evidence

F8's world tick plus existing camera, size, scale and interpolation metadata reproduce
the active line and its anchor. Audit and film use the same scene/screen path. The
playable fixture is `tests/play/talk`; its Room B cover button covers the third
speaker's face at runtime, which check can't see and the problems panel flags.
`tests/play/offer` styles its bubble and offers two answers: taking shows the next line,
declining ends the talk and its handler disables the offer.
`tests/play/talk-covered` intentionally covers a speaker's face in a room the player has
not entered. It must fail check.

From the repository root, select the installed dev build after each executable:

```sh
galaxy-gm --build <name> check engine/tests/play/talk/demo
galaxy-gm --build <name> check engine/tests/play/talk-covered/demo
galaxy --build <name> play --modules engine/tests/play/talk --inputs engine/tests/play/talk-inputs.toml --ticks 380
galaxy --build <name> sim replay <session>/recording.json
galaxy-gm --build <name> audit <session>/recording.json
galaxy-gm --build <name> film <session>/recording.json --from 195 --to 375 --size 1280x720 --out <film>
galaxy-gm --build <name> motion track <film> --thing talk.second
galaxy-gm --build <name> motion strip <film> --screen "300,240;650,240"
galaxy-gm --build <name> motion sheet <film> --count 12
galaxy-gm --build <name> motion flow <film> --screen --region 300,180,420,220
galaxy-gm --build <name> motion flicker <film> --screen
galaxy-gm --build <name> check engine/tests/play/offer/demo
galaxy-gm --build <name> check engine/tests/play/sign-panel/demo
galaxy --build <name> play --modules engine/tests/play/offer --inputs engine/tests/play/offer-inputs.toml --ticks 70
galaxy --build <name> play --modules engine/tests/play/offer --inputs engine/tests/play/offer-decline-inputs.toml --ticks 50
```

For answers, `tests/play/offer-inputs.toml` (70 ticks) takes the offer and hears the
repeat talk, and `tests/play/offer-decline-inputs.toml` (50 ticks) declines it; both leave
notes that audit redraws identically. `crates/view/tests/offer.rs` covers sizes, the one
click target, answer order, the stale double answer, branches, replay, a mid-talk redraw,
save/load and authoring errors.

The bot speaks to a non-being anchor, leaves an unfinished story, returns to hear it,
finishes it, asks again, moves a speaker during a line, changes rooms, and deliberately
covers then uncovers a face: the problems panel shows the covered focus in note 9 and is
gone again in note 10, and play's report lists it. Headless tests cover those state transitions, save/replay,
fresh-view reconstruction, interpolated bubble attachment, malformed chains, alternate
focus, viewport, panel/bubble/mesh coverage and active-walk combinations. They use no GPU.
Manual play, audit and film provide the pixel evidence.

## Alternatives and why

Module-only conversation counters could implement dialogue, but would duplicate pending
and interruption semantics and give the checker no finite list of lines to exercise.
Linked line entities reuse the world's existing scalar schema, references, capability
checks and persistence without introducing a second content store. Speaker-specific
bubbles would exclude signs and other anchored objects. Answers are entities pointing at
their line, like lines pointing at the next, because the world has no list field; a
per-line list would need a new field type. Answer buttons in a room panel would separate
the answer from the question. Rich text spans in one label would need a markup parser
and per-span measurement; a column of plain texts reuses the screen layout and checks. Separate overlay paint would
bake another look into the engine and make later styling harder.

An anchor-only visibility test misses hats and panels covering part of a face. Bounds-only
occlusion also rejects empty spaces between model parts; per-pixel triangle rays measure
actual foreground geometry headlessly. Screenshots alone cannot prove attachment during
movement, so recorded motion and fractional-frame headless assertions complement the
geometry check. Conversation state held by the view would break save/load, audit and
film; ordinary world components avoid that extra reconstruction contract.

Life signs share the screen path and merge the speaking being's name into its bubble.
The figure, its morphed head anchor and focus geometry use the same recorded phase.
See [Places and beings](places-and-beings.md) for the day contract and check bounds.

A screen's `ui.hover` declaration on the speaker anchor supplies its hover label,
including on the bubble sharing that anchor. The screen engine keeps tooltip text
clear of speech and reports overlap when no candidate fits. Each staged line also
contributes hover observations to `check`; `look --note` returns the bubble path or
speaker and nearest mesh part using the same frame geometry. The talk and life
fixtures include declarations. See [Screens](screens.md) for authoring and picking.
