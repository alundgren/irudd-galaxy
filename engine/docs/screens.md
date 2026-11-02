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
  comes from content unless explicit. Defaults: 20px text, 12px gaps, 16px padding on
  panels/buttons. `x` or `y` makes a child explicitly placed instead of flowing.
- Text is shaped and wrapped headlessly by `galaxy_render::text::TextMeasure` with the
  same bundled Fredoka font and metrics as glyphon drawing. Colours are engine-owned:
  dark ink on paper, or a warm button surface. Screens cannot choose low-contrast colours.
  Text outside a containing panel gets its own flat paper surface, including root text,
  columns and labels anchored away from their parent. Authors need no extra wrapper.
- Readability findings name the module, script and widget path. They cover text below
  16px, missing glyphs, empty labels, text outside its box, small button targets (44px),
  widgets outside the viewport or parent, sibling overlaps, overlaps of painted widgets
  from separate screens, and missing anchors. Overlap findings name both widget paths.
  Unknown properties and undeclared button events fail parsing at their widget path.
- `galaxy-gm check` runs the module's screens after the first tick at 1280x720, without a
  GPU. This checks the initial state; branches opened later are also checked while playing.

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
The world changes only through that recorded event. There is no hover or pointer drawing
in this increment. Open/closed screens are derived from recorded world components, so
audit and film reconstruct them from the embedded module files and replayed world. Anchor
placement uses the note's existing camera, size and interpolation metadata. No screenshot
or live VM state is used to reconstruct a screen.

The bot takes `[[click]]` entries with `tick` and either `x`/`y` or `widget` in an inputs
file. `widget` is a full diagnostic path or a unique `id`, including on a root button.
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
history with layout. Neither is needed for this increment.
