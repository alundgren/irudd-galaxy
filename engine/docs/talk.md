# Talk

Conversations live in ordinary world components. The simulation owns starting,
advancing, interruption and seen status; the view only projects and paints them.
The code is `sim/src/talk.rs` and `view/src/talk.rs`. `galaxy-gm guide talk` gives
an authoring and playtest recipe, and `fields components` lists the schema.

## Content and progress

| Component | Fields and meaning |
|---|---|
| `engine.talk` | `speaker` is an entity; `first` starts a linked chain of lines. `anchor` is any published anchor, defaulting to `being.<speaker>`. `enabled` defaults true. `story` prioritizes an unseen moment over repeat dialogue. `active`, `current` and `seen` store engine-managed progress. `width` (320), `x` (0), `y` (-12) are logical-pixel bubble placement. |
| `engine.talk_line` | `text` is one nonempty line; `next` names another line or is empty at the end. |
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
Both are recorded inputs, validated against the current world. The line token ignores
stale advances: a double click on one displayed line advances only once. A queued click
that outlives its room or enabled state is also a recorded no-op. Only acknowledging the last line sets `seen`. Leaving, moving the
speaker away or disabling the conversation interrupts without marking it seen. Starting
it again begins at its first line. Seen status has no clock or expiry.

Lines must form a finite, nonempty chain with existing references, nonempty text and a
current line in that chain when active. World batches validate atomically and roll back
mistakes. Start-state and resumed-world validation use the same rules. There is no new
save or recording format, and old worlds without talk retain their existing hashes.

## Presentation and picking

The view derives an internal `bubble` widget from the active line. It uses the screens'
text measurement, wrapping, anchor placement, viewport clamp, readability checks,
ordered paint layers and hit testing. Its paint is the same neutral panel surface as
screens; no talk-specific palette, font, game look or style file is introduced. The later
screen-style item can change this same paint path. Width and offsets are module-owned
presentation geometry. The line and Continue/Done affordance form one click target.

Anchors follow the same interpolated pose as the figure. A bubble has no private timer,
position memory or Luau frame callback. Travel consumes landing clicks first and hides
interrupted talk. Painted widgets block speaker picking. `[[click]]` bot entries accept
`widget = "speaker.<full-id>"` for a model speaker and `widget = "talk.<full-talk-id>"`
for a bubble; coordinate clicks also use the actual hit test. Model-less anchors use
coordinates. Bot targeting records the resolved input, never a future lookup.

## Visibility, combinations and limits

`galaxy-gm check` stages every authored talk, including disabled ones, in its speaker's
place, and every line of that talk. It rebuilds the module's conditional screens, room
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
retained as a film in memory. This covers the existing walk service; it does not invent
later life schedules, idle sway, future event-handler states or arbitrary script
branches. Authored variants need their own check, and recorded play remains necessary.
Visibility is geometric at the checked viewport (1280x720), not a GPU lighting or
subjective expression judgement. Subpixel occlusion and model ink are not measured.

## Reconstruction and evidence

F8's world tick plus existing camera, size, scale and interpolation metadata reproduce
the active line and its anchor. Audit and film use the same scene/screen path. The
playable fixture is `tests/play/talk`; `tests/play/talk-covered` intentionally covers a
speaker's face in a room the player has not entered. It must fail check.

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
```

The bot speaks to a non-being anchor, leaves an unfinished story, returns to hear it,
finishes it, asks again, moves a speaker during a line, changes rooms, and deliberately
covers then uncovers a face. Headless tests cover those state transitions, save/replay,
fresh-view reconstruction, interpolated bubble attachment, malformed chains, alternate
focus, viewport, panel/bubble/mesh coverage and active-walk combinations. They use no GPU.
Manual play, audit and film provide the pixel evidence.

## Alternatives and why

Module-only conversation counters could implement dialogue, but would duplicate pending
and interruption semantics and give the checker no finite list of lines to exercise.
Linked line entities reuse the world's existing scalar schema, references, capability
checks and persistence without introducing a second content store. Speaker-specific
bubbles would exclude signs and other anchored objects. Separate overlay paint would
bake another look into the engine and make later styling harder.

An anchor-only visibility test misses hats and panels covering part of a face. Bounds-only
occlusion also rejects empty spaces between model parts; per-pixel triangle rays measure
actual foreground geometry headlessly. Screenshots alone cannot prove attachment during
movement, so recorded motion and fractional-frame headless assertions complement the
geometry check. Conversation state held by the view would break save/load, audit and
film; ordinary world components avoid that extra reconstruction contract.
