# ui-lab report

**Question.** Does "a screen is a Luau function returning a widget tree" cover the
game's screens, and feel right? What should change before the engine adopts it?

**Short answer.** It covered all four screens with sixteen engine-owned widgets, and
Luau never ran per frame, the cradle's drag included. The model holds, with seven
changes for the engine (at the end). The feel is still to be judged: that is Andreas's
call after playing the lab (`README.md`).

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

Yes. From the scripted run (`--shots`), 767 frames through every room:

| | |
|---|---|
| Screen runs | 32: one at load and one per event (31 events) |
| Screen runs while a part was held | exactly 1 per drag (the pick-up), 0 while moving, for all 4 drags |
| Slowest screen run | 0.4 ms |
| Slowest layout | 1.0 ms |
| Frame build, scene and UI (CPU, no GPU) | median 0.02 ms, p95 0.3 ms |

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
   panels, the work in the scene and other signs. Hit areas must not move on hover, or
   hovering flickers. Screens can't do this; it is an engine service.
4. **Add grid and scrolling list widgets** before the core builds screens with lists.
5. **Module icons and a glyph check.** Either a symbol font or icons as shapes, and the
   missing-glyph check kept.
6. **A `delta` widget** for before-and-after numbers.
7. **Check screens in `galaxy-gm check`.** Run each screen headless on recorded states
   at 1280x720 and fail on rule breaks, the way the lab's `--shots` does, with a Luau
   type file for the `ui` widgets so typos fail before running. Layout needs the font's
   shaper, which works without a GPU (the lab's tests use a stand-in).
