# Screens

Screens are pure Luau functions returning engine-owned widget trees. An entry script
listed in `manifest.toml` may return `screen = function(ctx, ui) ... end` alongside its
event handlers. `nil` hides that screen. All screens run in module ID order, then manifest
script order. Later widgets paint above earlier ones.

```lua
return {
    screen = function(ctx, ui)
        local state = ctx.get("state", "counter")
        return ui.panel { width = 360, children = {
            ui.text { text = "Count: " .. state.value },
            ui.list { children = { ui.text { text = "First entry" } } },
            ui.button { text = "Add one", on = "increment", payload = { amount = 1 } },
        } }
    end,
    on = {
        increment = function(event, ctx)
            local state = ctx.get("state", "counter")
            ctx.patch("state", "counter", { value = state.value + event.payload.amount })
        end,
    },
}
```

Declare `increment` in the module's events and `counter` in its components. A screen's
context has only `module`, `get` and `query`. The host tracks both component reads (including
absent components) and query membership. Rust compares these reads with the world and
calls Luau again only when they change. Resizing, moving anchors and drawing frames run
in Rust. There is no tick, random stream, mutable context or memory between calls. A
fresh budgeted VM enforces the normal script limits; trees allow 512 widgets and 32 levels.
Hot loading replaces the functions and caches. Failures show in the view's problem panel.

## Layout and readability

`crates/script/src/screens.rs` parses the tree. `crates/view/src/screens.rs` measures,
places, checks, paints and hit-tests it. `galaxy-gm types` describes the screen API.

- `text` and `button` have `text`; buttons also require a declared local `on` event and
  optional JSON `payload`. `panel`, `list` and `column` flow children down; `row` flows
  them across. `children` is a consecutive list of widget tables, starting at 1; a lone
  widget, named entries or gaps fail with a `children` error instead of dropping content.
  Malformed fields report their names. `id` labels diagnostic paths and bot targets.
- `width`, `height`, `pad`, `gap`, `size`, `x` and `y` are logical pixels. Available width
  is inherited; row children share the width left after fixed widths and gaps. Height
  comes from content unless explicit. Defaults: 20px text, 12px gaps, and the style's frame padding
  on panels/buttons (16px in the neutral default). `x` or `y` makes a child explicitly placed instead of flowing.
- Text is shaped and wrapped headlessly by `galaxy_render::text::TextMeasure` with the
  same bundled Fredoka font and metrics as glyphon drawing. The module owns colours
  through `[screen]` in `style.toml`; invalid contrast is refused before drawing.
  Text outside a containing panel gets its own flat paper surface, including root text,
  columns and labels anchored away from their parent. Authors need no extra wrapper.
- Readability findings name the module, script and widget path. They cover text below
  16px, missing glyphs, empty labels, text outside its box, small button targets (44px),
  widgets outside the viewport or parent, sibling overlaps, overlaps of painted widgets
  from separate screens, and missing anchors. Overlap findings name both widget paths.
  Unknown properties and undeclared button events fail parsing at their widget path.
- `align` places a widget against the window's edges instead of its parent's flow:
  `top-left`, `top`, `top-right`, `left`, `center`, `right`, `bottom-left`, `bottom` or
  `bottom-right`. `x` and `y` are then margins from those edges (offsets from the middle
  on a centred axis), and the widget keeps its own width and height. A status panel at
  `top-right` stays in that corner at any window size and display scale, so play, bot
  runs, audit and film measure viewport problems against the size actually drawn. A
  widget is anchored or aligned, not both, and grid cells flow.
- `galaxy-gm check` runs the module's screens after the first tick at 1280x720, without a
  GPU, and then presses (below). Branches it can't reach are checked while playing.

### Pressing in check

`crates/view/src/presses.rs`: from the first tick, and with the player moved into each
place, check presses every visible button and `ui.target` (not talk bubbles, which have
their own check), two presses deep. A press is an ordinary input through a `Session`:
the module's handlers run, a trip they start lands as play would land it, and one quiet
tick follows. Each new screen state, with talk silent, gets the same readability checks
as play, life name signs included, so a sign under a room panel or a list a script opens
is a `press` finding such as ``place `demo.hall`, after pressing `open`: demo.host/sign:
overlaps demo:scripts/main.luau/screen/list[2]``. A handler that fails on a press is a
finding too. States are told apart by what is on screen and by what event handlers
changed (fields a handler set to something other than the start world, and start
components removed), so a press that only moves a page counter or a selection still
gets its second press; a screen already checked is not reported twice. Changes from the
cadence and engine services don't count, so they can't split one state into many. At
most 32 new states per start are explored, and check's `pressed` output lists the states
per start and whether that cap stopped it. Keys, drags, scrolls, three presses deep, talk answers and clock changes
are not pressed: those states need a test or a recorded run. The pressed worlds are
copies; the module's own state is untouched. `tests/play/sign-panel` opens a list over a
name sign from a button and must fail check; its first screen alone passes.

Each painted widget contributes one ordered panel/text layer between the base overlay
and engine chrome. GPU buffers and glyphon renderers grow to the required layer count.
This preserves panel/text/panel/text order at arbitrary nesting depths; grouping all
panels before all text would incorrectly show covered labels. Hit testing walks the
same order backwards, so an opaque panel also blocks a covered button.

## Named anchors

`engine.anchor { name, target, x, y, height }` publishes `publisher#name`. `target` is the
entity followed; empty follows the publisher itself. Offsets are in world units in the
target's rotated frame. Multiple named anchors use separate publishing entities pointing
at the same target. Modules can publish these in start data, or write them with the
`engine.anchor` capability.

A widget with `anchor = "marker#label"` is detached from flow. The view interpolates the
target's pose, projects it through the current camera, places the widget above that point
and clamps it to an 8px viewport margin. `x` and `y` offset that placement. This is generic:
the publisher need not have a model. Rooms additionally publish `place.<full-id>`, `spot.<name>`, `being.<full-id>` and
`being.<full-id>.<node-name>` through this same map; see [Rooms and travel](rooms-and-travel.md).
A missing target or projection reports a missing
anchor rather than silently discarding the widget.

## Clicks, recording and trying it

Left click hit-tests the displayed tree in logical pixels. The chosen button becomes an
ordinary input with source `ui`, its event name and payload, applied on the next tick.
The world changes only through recorded events. Open/closed screens are derived from recorded world components, so
audit and film reconstruct them from the embedded module files and replayed world. Anchor
placement uses the note's existing camera, size and interpolation metadata. No screenshot
or live VM state is used to reconstruct a screen. Notes also keep the last drawn scroll
offsets and active drag; audit restores them before layout.

The bot takes `[[click]]` entries with `tick` and either `x`/`y` or `widget` in an inputs
file. `widget` is a full diagnostic path or a unique `id`, including on a root button.
It also accepts `point:x,y`, equivalent to the finite logical-pixel `x`/`y` fields.
It lays out the current world before that tick, resolves the widget's current centre
(including moving anchors), and uses the same hit test. Missing or ambiguous names,
non-buttons and covered centres fail the run. Recordings still contain the normal `ui`
event and payload; replay does not repeat the lookup. Use `galaxy play` for pointer
instructions, then replay or film its recording. Other
headless input runners refuse pointer instructions because they do not have a viewport.
`galaxy-gm guide screens` gives a complete standalone authoring and playtest recipe;
`galaxy-gm fields inputs` lists both click forms.

From the repository root, with a dev build installed:

```sh
galaxy --build <name> play --modules engine/tests/play/screens
galaxy --build <name> play --modules engine/tests/play/screens --inputs engine/tests/play/screens-inputs.toml --ticks 70
galaxy-gm --build <name> check engine/tests/play/screens-unreadable
galaxy --build <name> sim replay <session>/recording.json
galaxy-gm --build <name> audit <session>/recording.json
galaxy-gm --build <name> film <session>/recording.json --from 25 --to 26 --out screen-film
```

The first button increments a counter; Hide closes the screen and Space opens it again.
The label follows a moving marker without invoking Luau. The unreadable module deliberately
fails check. The bot adds one, takes a note, hides and reopens the screen, then takes another
note. Headless tests exercise those contracts; GPU draw-order tests are explicitly ignored.

Pure functions were chosen over persistent UI VMs or imperative open/close commands so
replays, hot loads and checking share the same result. A full immediate-mode Luau UI would
run scripts per frame; storing every widget in the world would contaminate simulation
history with layout. Neither is needed for these widgets.

## Grids, scrolling and deltas

`ui.grid { columns = 3, children = {...} }` lays out equal-width cells in rows, each row
as tall as its tallest cell. Cells flow without anchors or x/y; explicit cell widths
still obey containment and overlap checks. `ui.scroll { id, height, children, on?, payload? }`
flows a vertical list in a fixed viewport. Rust clamps its offset, clips all descendant
panels and glyphs using the same GPU scissor, and clips hit testing. A minimum 12px inner
padding keeps rows clear of the frame. A separate lane (`handle_width`, default 32px) plus an 8px gap holds the
scroll track; none of its grab area covers content. The thumb is at least `handle_min` (default 44px) tall;
a shorter track is a readability finding. Dragging preserves the grab point, captures the
pointer outside the track and releases on mouse-up. Clicking the track centres the
thumb there. Both routes emit the same offset event as wheel scrolling.
The wheel over a list scrolls it; elsewhere it zooms the camera. Nested lists use the
innermost one under the pointer. The full content is measured and checked headlessly,
including entries currently outside the scrolling viewport. Viewport clipping is allowed;
clipped text inside its own widget, unreadable cells and overlapping entries still fail.

`ui.delta { value = -12 }` displays a signed number in the style's loss or gain colour on
the containing surface or paper. The sign carries the meaning as well as colour. Dimensions and the 16px minimum
are the same as text. `ui.icon { icon = "token" }` resolves a local module name from:

```toml
# icons.toml
[icons]
token = "T"
```

Icons are module-owned glyph strings, embedded with the module in recordings. The GM
checks all declared glyphs against the actual bundled font, including unused declarations,
and reports any referenced icon missing from the catalogue. This uses existing text
shaping and deterministic text assets rather than introducing a second image asset path.

## Dragging and view state

```lua
ui.grid { columns = 2, children = {
    ui.drag_source { id = "token", text = "Small token", value = "small" },
    ui.drop_target { id = "slot", text = "Slot", on = "place",
        accepts = { small = "", large = "Needs a wider slot" },
        notes = { small = "Fills the slot: 1 left" } },
} }
```

A source has a string `value`; every target maps values to verdict strings in `accepts`.
An empty string permits the drop, a nonempty string explains rejection, and an absent
value means "Not accepted". Picking up immediately colours every target and shows its
verdict in words on a softly tinted surface. An optional `notes` maps accepted values to
what that drop will change, drawn on a line under `Drop here`. A consequence that never
refuses (a cost, a slowdown) then shows at pickup beside the refusals, without Luau
running during the drag. A note must belong to a value the target accepts, and is
nonempty text; anything else is a screen error. Layout reserves the largest measured
verdict or note, so targets never move when picking up. All verdict and note labels
receive glyph and readability checks. Picking up lifts the actual source surface and label, leaving an
empty socket. Its size and pointer grab point stay attached to the pointer. Release on
an accepting target sends its `on` event with its `payload` plus `value`; release elsewhere
or on a rejecting target returns home. Both use a 240ms smoothstep settling animation,
computed from simulation tick and interpolation, independent of frame rate.

A target may contain children above its verdict label. Keep a fixed-height child area
for its occupant so empty and occupied slots have the same geometry. The event handler
stores the placement in world state; the screen then emits the source under its new
parent, with the **same id and value in the same screen script**. This identity lets Rust
suppress the new copy while the lifted token animates into its new box, and lets the
player pick it up there again. IDs for movable sources must be unique within that
script. The rich-screen demo shows this pattern, including occupied-slot rejection.
A source removed or changed by state updates cannot complete its old drag. Targets use the
current tree, and modules should validate world operations in their event handlers.

Scrolling normally changes only presentation. An optional scroll `on` sends a recorded
input with `payload` plus the new `offset` in logical pixels, only when the clamped offset
changes. Thus a module that changes the world when scrolling replays that change exactly.
Drops always use recorded inputs, just like clicks. Neither gesture invokes Luau per frame.

`Screens::Presentation` keeps scroll offsets by full widget path, plus the source,
value, stable identity, original bounds, grab point and pointer position of an active
drag, a captured scroll thumb, and a settling token with its start time. Played frames
record changed presentation snapshots separately from simulation inputs. World replay ignores those
snapshots; film applies them on their recorded ticks. Notes keep their own exact last
drawn snapshot in `Moment`, so a pointer move arriving before F8 cannot alter the note's
redraw. Audit restores that snapshot. Filming around a selected note prioritises that
snapshot at the note's tick. Notes without screen metadata default to an unscrolled
view with no drag. Recordings with the earlier experimental screen metadata are refused with a message to use their
original build; current screen snapshots carry presentation version 1.
Hot loading resets widgets and their state. This keeps presentation out of world hashes
and saves; storing only a drop would lose unfinished drags, while invoking Luau every
pointer move would break the event-only screen contract.

The bot uses `[[pointer]]` entries with `tick`, `action` (`pickup`, `move`, `drop`, `scroll`),
`widget` (a unique ID or full path) and optional `delta` (scroll pixels, positive down).
Separate pickup and drop ticks allow mid-drag notes. Targets must be visible at their
centre and uncovered. `entries::thumb`, `entries::track-start` and
`entries::track-end` name scrollbar parts for pickup/move/drop using the same captured
pointer path as live input. Gestures run before clicks at the same tick; both run before world
inputs. A play run with scripted pointer gestures ignores live pointer and focus
changes for those gestures; F8 editing remains interactive. Headless sim/shot/film
input runners refuse gestures; play them once and then
replay or film the resulting recording. Help, `fields inputs`, `types` and `guide screens`
cover this surface.

From the repo root:

```sh
galaxy --build <name> play --modules engine/tests/play/rich-screens
galaxy --build <name> play --modules engine/tests/play/rich-screens --inputs engine/tests/play/rich-screens-inputs.toml --ticks 80
galaxy-gm --build <name> check engine/tests/play/rich-screens/rich
galaxy --build <name> sim replay <session>/recording.json
galaxy-gm --build <name> audit <session>/recording.json
galaxy-gm --build <name> film <session>/recording.json --note 2 --before 0 --after 1
```

The generic module offers sixteen entries, two token sizes and two slots. The bot
scrolls, notes the list, picks up the large token, notes its rejection at the narrow
slot, and drops it into the wide slot. Headless tests compare rebuilt overlays at each
note and world hashes after replay. GPU clipping tests are explicitly ignored.

`tests/play/rich-screens-motion.toml` drags the thumb, returns a token from a refusing
slot, places it in an accepting slot, then picks it up again and returns it to that
slot after another refusal. Run it for 140 ticks, audit its eight notes (including
settling frames), and film ticks 1 through 134 to inspect the actual movement with
`galaxy-gm motion sheet` and `strip`. Headless tests assert one visible token throughout,
state-driven placement, return motion, scroll events, replay hashes and rebuilt notes.

`tests/play/drop-notes` has one token and two slots that both accept it; only the far
slot has a note. `tests/play/drop-notes-inputs.toml` (90 ticks) picks the token up,
notes mid-drag, drops it on the far slot, lifts it out of there and drops it on the near
slot. Headless tests assert the note shows on the far slot only while dragging, neither
slot moves on any tick, every note rebuilds identically and the replay hash matches.

## Talk bubbles

The talk service supplies an internal bubble widget from recorded world components:
a column of heading, subtitle, line, and a Continue/Done marker or a row of answer
buttons, sized by the `[screen]` `bubble_*` keys.
It uses this same measurement, layout, anchor, clipping, ordered paint and hit-testing
path, with the screen style's bubble surface. No separate talk look is encoded. Module-owned
width and offsets affect its geometry. A whole bubble is a click target (hit
testing does not descend into its texts); its input includes the current line so stale or
duplicate advances are recorded no-ops. A bubble with answers is not itself a target; its
buttons are. Speaker picking runs only when no
painted widget covers the pointer. See [Talk](talk.md) for focus checks combining screen
surfaces, room geometry and each authored line.

Life signs are internal anchored text widgets with the screen style's sign surface. Resting
signs show name and activity; moving signs show the name, which joins the bubble while
speaking. Directories use ordinary screen queries over life, being and location fields.
No separate sign paint or game look is introduced. See [Places and beings](places-and-beings.md).

## Module-owned screen style

`[screen]` in `style.toml` supplies the whole scene's screen look, including all
scripts, talk bubbles and life signs. As with `[sky]` and `[course]`, modules are
sorted by ID; the last one supplying this table wins, and `galaxy-gm check` reports
multiple suppliers. Tables do not merge field by field. Omitted keys use the neutral
engine default, and modules without a table work unchanged. This lets a style-only
module dress shared screen content without adding paint to its Luau trees.

`galaxy-gm fields style` is the complete field/range/default reference; `guide style`
explains authoring. For example:

```toml
[screen]
panel = "#eeeeee"
button = "#dddddd"
text = "#202020"
radius = 8.0
frame_padding = 16.0
handle_width = 32.0
handle_min = 44.0
```

The colour fields are `paper`, `panel`, `button`, `text`, `gain`, `loss`, `border`,
`bubble`, `sign`, `held`, `socket`, `accept`, `refuse`, `resting`, `track`, `handle`,
`handle_active` and `highlight` (the outline of a hovered target's model). Surface and text colours must be opaque: the arbitrary world behind
them cannot undermine contrast. Linear-sRGB luminance checks every text/gain/loss
against every possible text-bearing surface at 4.5:1, including nested deltas and
both drag verdicts. Both handle states must contrast with the track at 3:1, and a
dashed resting frame's `border` its `resting` fill. Failures
name `[screen]` and both fields. An invalid table never reaches painting; playing
shows its diagnostic and uses the last valid supplier or the neutral default.

Talk bubble text sizes are `bubble_title` (16-40), `bubble_subtitle`, `bubble_text` and
`bubble_marker` (16-32), with `bubble_title_weight` (400-700). Sizes are logical pixels:
`radius` and `handle_radius` (0-24), `border_px` and
`hint_border_px` (0-4), `highlight_px` (2-12), `frame_padding` (12-32), `target_min` (44-64), `handle_width`
(24-48), `handle_min` (44-80), `socket_dash` and `resting_dash` (0-16; zero is solid).
Non-finite or
out-of-range values fail check. `target_min` is a checked floor for actual targets,
not a resizer. A scrolling viewport must fit the minimum handle. Explicit widget
`pad` overrides default frame padding; scroll content retains its minimum 12px inset.
Corners and borders are capped by the widget padding to keep paint out of its text.
Standalone text and signs have a flat opaque backing covering every glyph. Screen
surfaces have no renderer-owned tinted shadow.

The engine still measures and places the tree, clips and hit-tests it, captures the
pointer, reserves verdict space, and performs the same 240ms drop/return animation.
Changing sizing fields changes spacing through those same rules; changing paint
alone leaves every box and gesture unchanged. No style field enters world state.
`--watch` reloads it through the normal asset path. A style-only reload records the
new files with `presentation_only`, without a hot-loaded simulation fact; older
recordings retain their original hot-load semantics. Audit and film use those embedded
files, including changes during a session, rather than today's module folder. Each
note also keeps its effective screen style, so notes on either side of a reload
within the same simulation tick retain the exact paint they showed.

From the repository root, these two modules have identical files except their style:

```sh
galaxy --build <name> play --modules engine/tests/play/screen-light
galaxy --build <name> play --modules engine/tests/play/screen-dark
# Repeat for each module:
galaxy --build <name> play --modules engine/tests/play/screen-light --inputs engine/tests/play/screen-style-inputs.toml --ticks 140
galaxy-gm --build <name> check engine/tests/play/screen-light/styled
galaxy --build <name> sim replay <session>/recording.json
galaxy-gm --build <name> audit <session>/recording.json
galaxy-gm --build <name> film <session>/recording.json --from 1 --to 134 --out <film>
galaxy-gm --build <name> motion track <film> --thing styled.marker
galaxy-gm --build <name> motion sheet <film> --count 12
galaxy-gm --build <name> motion strip <film> --screen "300,240;900,240"
galaxy-gm --build <name> motion flow <film> --screen --region 300,150,600,400
galaxy-gm --build <name> motion flicker <film> --screen
```

Drag the scroll handle, drop a large token on the refusing narrow slot, then on the
accepting wide slot, and pick it up there again. The moving marker's bubble uses the
same style. The bot leaves eight notes covering the drag, hints and both settling
paths. Matching geometry settings make both runs' boxes, inputs, presentation snapshots
and world hashes identical while their paint differs. Existing screen fixtures use
the neutral default. Headless tests reconstruct all notes and exercise style reload,
invalid contrast/sizing and module order. GPU tests remain opt-in.

A table in the existing style asset avoids a second loader, watch path or recording
store. Per-widget colours would duplicate the look in every script and make whole-theme
contrast checks impossible without enumerating runtime branches. Skin scripts or
module-owned motion would move frame work into Luau and change the shared behaviour.

## Edges instead of a viewport size

`align` was chosen over passing the window size to `ctx`: a size in the context would be a
tracked read, so every resize would call Luau again and authors would redo edge
arithmetic in each script. Alignment keeps resizing in Rust, beside the anchor
placement, and the readability checks see the same rectangles at every size.
`crates/view/tests/targets.rs` lays the generic fixture out at 1280x720 and 1024x576 and
checks the three edge widgets against both.

## Hovers and recorded point inspection

Every widget accepts `hover = "A label"`. `ui.hover { id = "help", anchor =
"being.demo.person", text = "Click to talk" }` declares a label for an anchored thing
without painting a permanent widget or adding a world component. Keep it conditional
on the visible place. Existing named-anchor forms work; `being.<id>.<part>` restricts
a model hover to that mesh part; an empty model node uses a 44px anchor target.
Container labels also apply to descendants without their own hover. Entity IDs resolve longest first. The last matching
declaration wins. Life signs and talk bubbles sharing the anchor use its declaration
too; an explicit widget hover takes precedence.

Rust picks the topmost clipped screen surface first, then the nearest front-facing
posed mesh surface. Model-less anchors have a 44px target at their projected point;
foreground geometry blocks them. The tooltip appears immediately and clears on
pointer exit, focus loss, drag/scroll capture or a travel cut. It follows interpolated
anchors without calling Luau per frame. Its paint uses `[screen]` text/paper, padding,
radius and border, with the same bundled font and contrast checks.

The engine owns the 320px maximum width, wrapping and placement. It tries positions
around the target and nearby readable widgets, keeps an 8px viewport margin, and
avoids text, controls, signs and speech bubbles. Empty panel paper is usable space.
If no candidate fits, the report names covered widgets; empty labels, missing glyphs
and viewport overflow are also findings. Tooltips do not intercept input. Hover picking
also includes unpainted containers with labels; input hit testing only includes painted
surfaces, so adding a container hover cannot cover a button or speaker.

`check` collects a `hovers` array from every staged scene: initial screens, rooms,
talk lines and life poses. Entries name the place, active talk line, widget, entity,
being, part, point, scroll offsets and the hover's text, bounds and readability.
Null means no authored label. Rectangle partitions find exposed widget areas even
when centres are covered. Each descendant is scrolled through its ancestors without
events. Mesh triangle probes fall back to logical pixels for partly covered parts.
Repeated target/text/talk/readability observations merge. This covers authored states,
not arbitrary future script branches.

`galaxy-gm look <recording> --note 3` resolves the saved F8 pointer; `--point 320,240`
substitutes a logical-pixel point. `--tick 45 --point 320,240` uses the tick endpoint
(`alpha = 1`) at 1280x720, or `--size 1600x900`. Notes use their exact scale, size,
fractional pose and style. Divide image coordinates by the note scale. JSON returns
widget, entity, being, part, anchor, place and hover; absent values are null. No GPU
is needed. `guide hovers` and `look --help` cover this workflow.

The optional pointer lives in the existing version-1 presentation snapshot; old
snapshots default to none. F8 freezes the last drawn pointer, even if another input
arrives before the note is taken. Audit and film share the same layout and picker.
There is no timer or cached target to save, and no change to world hashes or simulation
inputs. A tooltip entity or separate inspector picker would duplicate that contract.

Bot `[[pointer]] action = "move"` works without a drag. `widget` names an ordinary
widget, an anchored hover or target ID, or `point:x,y`. Repeated moves track a moving target;
moving to empty space clears it. The life and talk demos declare hovers. The recorded
motion recipe is in [Motion tools](motion-tools.md).

## Resting buttons

```lua
ui.button { id = "add", text = "Add one", on = "add",
    resting = if full then "The tray is full: empty it first." else nil }
```

A button with `resting = "<reason>"` is an action that exists but can't happen now. It
keeps its place and size, so nothing moves when it wakes, and is drawn with the `[screen]`
`resting` fill (unset, the `panel` colour) and a frame dashed every `resting_dash`
pixels in `border`, at least 1px wide. Its text keeps the full-contrast `text` colour:
resting is never greyed out. Check refuses a style whose resting look can't be told from a
ready button: a dashed frame must contrast with the fill at 3:1, and a solid one
(`resting_dash = 0`) needs a fill other than `button`. Text on `resting` gets the usual
4.5:1 check. The frame is capped by the padding like any border, so check also refuses a
resting button whose `pad` is under 1px. Only buttons rest, and the reason must not be
empty.

Hovering shows the reason, after the button's own `hover` label if it has one; check's
`hovers` array and `look` report it like any label. A click on it hits it (it still
covers what is behind it) and records nothing, and the pointer stays an arrow. Check
never presses it. A bot `[[click]]` that lands on one, by name or point, is not
clicked: `galaxy play` prints `<widget>: resting: <reason>`, lists
`{ tick, widget, resting }` in its report's `resting_clicks` and plays on, and the
GM's `bot` report copies that list. Headless callers of `bot_click` get the same text as
an error. Replays never see the lookup, so they are unchanged.

From the repository root, with a dev build installed:

```sh
galaxy-gm --build <name> check engine/tests/play/resting/demo
galaxy --build <name> play --modules engine/tests/play/resting --inputs engine/tests/play/resting-inputs.toml --ticks 75
galaxy --build <name> sim replay <session>/recording.json
galaxy-gm --build <name> audit <session>/recording.json
```

The tray holds two. Empty rests while it is empty and Add while it is full. The bot
hovers the resting Empty, clicks it (reported, not clicked), fills the tray, clicks the
resting Add, then empties it, leaving three notes. `crates/view/tests/resting.rs` checks
the paint, hover, clicks and replay headlessly.

A flag that hides the button was rejected: the player then can't see the action exists.
Greyed text fails the contrast rules and reads as broken. A separate widget kind would
duplicate the button's layout and checks for one state.

## Clickable models

`ui.target { id, anchor, on, payload, text }` is a hover declaration that a click can
press. Its anchor uses the same forms as `ui.hover` (`being.<id>`, `being.<id>.<part>`,
`publisher#name`, ...), and the same picking: the nearest posed mesh surface under the
pointer, or a 44px box for a model-less anchor that no nearer geometry hides. A left
click on it records `on` with `payload` and source `ui`, exactly like a button, applied
on the next tick. `text` is its optional hover label. Painted widgets are hit-tested
first, so a panel or label in front blocks the click; an active drag or a travel cut
does too. A target comes before talk-speaker picking when both share a model.

While the pointer is over a target, the view lifts that entity's model by a twentieth of
its largest extent, draws its outline in the `[screen]` `highlight` colour at
`highlight_px`, and the window shows a hand pointer. The pointer is a hand over buttons,
bubbles and drag sources too. Picking uses the resting, unlifted mesh, so lifting never
moves the target out from under the pointer. All of this is derived each frame from the
presentation pointer: notes, audit and film redraw a mid-hover frame exactly, and no
world state, Luau call or timer is involved.

Bot `[[click]] widget = "<target id>"` and `[[pointer]] action = "move"` find a visible
point on the target's model through the same picking, and fail when it is covered or
off screen. `look` reports a target with a label like a hover.

From the repository root, with a dev build installed:

```sh
galaxy-gm --build <name> check engine/tests/play/targets/demo
galaxy --build <name> play --modules engine/tests/play/targets --inputs engine/tests/play/targets-inputs.toml --ticks 210
galaxy --build <name> sim replay <session>/recording.json
galaxy-gm --build <name> audit <session>/recording.json
galaxy-gm --build <name> film <session>/recording.json --from 36 --to 66 --size 1280x720 --out target-film
```

The bot glides into the hall, hovers and clicks the first figure (its camera glides to
frame it), lands that glide with a click, goes back with Esc, frames the second figure,
goes back with a right-click, shrinks the window to 1024x576 and leaves with Esc. A
painted label covers the second figure's face; clicks there do nothing. Status, key
strip and log stay at their edges throughout. `crates/view/tests/targets.rs` runs the
same journey headlessly and redraws all six notes from a replay.

A separate pickable-object component was rejected: what can be clicked depends on the
screen's state (a building is clickable from the dock view, not from inside a room),
which screens already express. A per-frame Luau hover callback would break the event-only
screen contract. Lift and outline are fixed engine behaviour; their colour and width are
module style.
