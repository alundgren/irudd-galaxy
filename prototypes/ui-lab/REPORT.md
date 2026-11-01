# ui-lab report

**Question.** Does "a screen is a Luau function returning a widget tree" cover the
game's screens, and feel right? What should change before the engine adopts it?

**Short answer.** It covered all four screens with sixteen engine-owned widgets, and
Luau never ran per frame, the cradle's drag and the station zoom included. The model
holds, with nine changes for the engine (at the end). The owner played it: how the
screens look is to be iterated while building the core, but two things feel wrong at a
level the screen model doesn't reach (below, "Where the player is").

## Where the player is

The owner's verdict after playing: the screens work, but the station doesn't feel like
a place you are in.

- **Talk.** A bubble is a label hanging off a building, not a person speaking. It stays
  on that building wherever you click, though clicking a module stands for walking over
  to it.
- **Rooms.** Opening the cradle cuts to a different scene; opening the office nudges the
  station view. Neither reads as going somewhere, and they don't match each other.

Both come from one gap: the lab has no notion of where the player is. Screens can only
attach to what the world publishes, and the world here has buildings and sockets, not
places and people. The engine needs a place model: where the player is, places that
nest (station, module, interior), people as beings in places, and a view that moves
between places rather than cutting. Modules supply which places and people a station
has. Leave room for real walking later. This wants research and its own lab before the
engine builds screens on top of it.

## What was built

All four are Luau functions in `lab/` that return a tree. The Rust side reads the tree,
lays it out with the real font, draws it in the slice's renderer and handles input.

| Screen | Built from | File |
|---|---|---|
| The station up close: a sign on every module, hover lift, click to glide to the room | `hotspot` on scene anchors | `station.luau` |
| The station key with New / Changed / Closed ribbons and a headline on hover | `tile` with `ribbon` and `hint` | `station.luau` |
| Talk: one line at a time over the speaker, click for more, goal afterwards | `bubble` | `common.luau`, `main.luau` |
| The market room beside its module; buy buttons rest when the hold is full | `panel`, `row`, `button` with `rest` | `market.luau` |
| Orrin's cradle: drag parts onto sockets, refusals and previews before you let go | `drag`, `drop` on sockets and inline | `cradle.luau`, `fit.luau` |

The widget set: `screen`, `column`, `row`, `panel`, `text`, `button`, `tile`, `state`,
`meter`, `icon`, `bubble`, `hotspot`, `drag`, `drop`, `spacer`, `divider`
(`lab/prelude.luau`).

Style roles: speech, goal, status, action, sign and radio as planned, plus three the
screens needed: **room** (a room's own panel), **receipt** (one receipt per
transaction) and **plain** (grouping with no surface). Radio has a look but no screen
used it.

## Did Luau stay off the per-frame path?

Yes. From the scripted run (`--shots`), 4965 frames through every room, 218 of the
hovers swept:

| | |
|---|---|
| Screen runs | 34: one at load and one per event (33 events) |
| Screen runs while a part was held | exactly 1 per drag (the pick-up), 0 while moving, for all 5 drags (one cancelled with Esc) |
| Screen runs while zooming the station in and out | 0 |
| Screen runs while hovering (218 hovers, in every saved state) | 0 |
| Slowest screen run | 0.6 ms |
| Slowest layout | 0.9 ms |
| Frame build, scene and UI (CPU, no GPU) | median 0.03 ms, p95 0.35 ms |

The cradle needed one idea to make that work: **verdicts at pick-up**. Picking a part
up is an event; the screen runs once and gives every drop target its verdict for this
part: `ok = true` with a `preview` tree, `ok = false` with its `reason`, or no `ok` when
the target doesn't take this part at all. While the part is in hand the engine does the
rest: the ghost, the ghost part on the ship, sky-blue or vermilion rings, the reason in
full, the preview for the target under the pointer. Every rule (power, credits, cargo
against hold) refuses the same way and at the same moment, before you let go.

Hover never runs Luau either: hover looks, hints, the station key's headlines and the
hovered sign are engine work.

## What the readability rules caught

A screen can't choose a colour: no widget has a colour property, text is pure black or
pure white for the surface it sits on, state colours come only through `state` and
`ribbon`, which need their word, and a resting button is dashed and needs its reason.
What layout and drawing check, with the widget's path: words cut off, rows wider than
their room, buttons that wrap, a room panel over 45% of the width, layers that overlap
each other or run off screen, signs under a panel, a bubble with no clear spot, a
refusing target with no reason, characters the font can't draw, and unknown widgets,
properties, icons or anchors.

The rules found real mistakes while the lab was built, each fixed in the screen, not
by loosening a rule:

- Market columns 4 px too narrow for "HOLD" and the buy buttons.
- `→` in the fit preview: the bundled font has no arrows.
- A hovered sign growing up into the goal banner.
- Orrin's bubble covering the drill mount he was talking about.
- The canteen's header running past its panel (its text column couldn't wrap).
- The open ship log overlapping the station key.

The owner's first try found one the rules missed: hovering the canteen during Ilse's
welcome grew its sign into her bubble, and the two mixed, the bubble's "more" marker over
the sign's text. A draw layer draws all its panels and then all its text, so any two
widgets overlapping on one layer interleave. The lab now draws grown signs after the
bubbles and keeps them clear of them, puts a hovered sign on the front layer, and fails
any two floating widgets (signs, pins, bubbles, socket labels) that overlap on one
layer. The run hovers three signs during the welcome to keep it fixed.

## Room on the station

At the slice's spacing neighbouring modules sat roughly 110 px apart at 1280x720: a bubble or a
hovered sign always covered a neighbour. The lab moves them out on long struts, on a
wide ellipse that matches the screen, roughly 230 px apart, and draws them 1.4 times
bigger. Placement also got stricter: bubbles and signs keep clear of every other
module, and a hovered sign never covers its own module (it opens beside it when the
room above is taken). For the core: lay a station out with roughly a bubble's width
between modules, and frame the view to the band between the title and the key.

## Zoom at the station

The wheel zooms the station view from 2.5 times closer to 2.5 times farther, toward the
pointer; zooming back out recentres. The screens didn't change and never ran: signs,
bubbles and hit areas follow the anchors each frame. What it needed was all sign
placement, engine side:

- A module under a panel or off the screen gets no sign.
- A sign keeps clear of every other module's point, the panels and the signs already up:
  above, else below, else beside its module.
- With no clear spot it folds into a pin, which opens into the full sign on hover. At
  this range and 1280x720 no sign needed one, so pins are untried.

Zoomed out, the rule caught signs piled on each other before this. Zoomed out at a
single station there is little more to see: farther zoom pays off where there is more
around, in flight. The slice already has that (its wheel zoom, and `Zoom` ship
modifiers that upgrades can carry).

## What the widget set couldn't do well

1. **Tables.** Rows can't line up columns. The market needed a fixed `width` on every
   cell and a spacer in the header row, and got them wrong first. Lists of wares, parts
   and log lines want a grid or table widget.
2. **Long lists.** There is no scrolling list. The rack, the locker and the log fit
   because they are short here.
3. **Icons.** The lab draws its icons from shapes in Rust. A module needs to bring its
   own icons (vector or sprite), with the same unknown-name check.
4. **Changes with direction.** A preview line has two meanings to show: which way the
   number moved, and whether that is good. The lab phrased every line so that more is
   better ("Power free 1 to 2"). A `delta` widget that takes from, to and which way is
   better would remove the trap.
5. **Not tried.** Keyboard use (numbered answers, focus), text input, the radio panel,
   the fitting animation, other window sizes than 1280x720, macOS and Linux.

## What carries over

Keep:

- A screen is a Luau function from state to a widget tree, run only after an event,
  a state change or a reload. The state lives on the module side; events change it.
- The engine owns widgets, looks, roles, text colour, layout, hover and drag.
- Verdicts at pick-up for drag and drop.
- Rule breaks are errors with the widget's path, shown and failing the check.
- Hot reload keeps the state (tested).
- A time budget on every Luau call stops a runaway script and the game carries on (tested).

Change before the engine adopts it:

1. **Scene anchors come from the world.** Screens attach to named points in the scene:
   `module.market`, `socket.nose`, a speaker. In the lab the world hard-codes them; the
   engine needs entities to publish UI anchors that screens refer to by name.
2. **The screen root shouldn't own scene state.** In the lab the root says which scene
   to show, which module the camera frames and what the ship wears. The camera framing
   can stay a request from the screen; the ship's fit belongs to the world.
3. **Anchored widgets need engine placement.** Bubbles, signs and socket labels each
   needed candidates (above, aside, below), sliding back on screen, and keeping clear of
   panels, the work in the scene, other signs, other modules and bubbles; crowded signs
   fold into pins, hidden modules lose theirs. Hit areas must not move on hover, or
   hovering flickers. Screens can't do this, and zoom makes it run every frame; it is an
   engine service.
4. **Add grid and scrolling list widgets** before the core builds screens with lists.
5. **Module icons and a glyph check.** Either a symbol font or icons as shapes, and the
   missing-glyph check kept.
6. **A `delta` widget** for before-and-after numbers.
7. **Check screens in `galaxy-gm check`.** Run each screen headless on recorded states
   at 1280x720 and fail on rule breaks, the way the lab's `--shots` does, with a Luau
   type file for the `ui` widgets so typos fail before running. Layout needs the font's
   shaper, which works without a GPU (the lab's tests use a stand-in).
8. **Draw order per widget.** The renderer draws a layer's panels, then its text. The
   engine's UI should give each floating widget its own place in the order (or a
   layer each), so one in front of another covers it whole; until then, the overlap
   rule stays.
9. **Agents must see hovers.** The canteen bug passed every check because nobody scripted
   that hover. The GM needs what the lab now has: `check` hovers everything reachable
   in each recorded state (reachable, steady, no rule break, no Luau), and a `look`
   command plays steps (hover, click, drag, zoom) and returns a picture with a text
   description of the screen: what is hovered, what popped up where, every target with
   its label and what it does. Text and picture together, so an agent that misreads an
   image still gets the facts.
