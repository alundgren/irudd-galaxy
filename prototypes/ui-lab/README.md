# ui-lab

A throwaway prototype that tries one idea before the engine commits to it: **a screen is
a Luau function returning a widget tree**. The engine (here, this lab's Rust) owns every
widget, its look, its layout and its behaviour; the screen only says what is on it. Only
the findings carry over: see [REPORT.md](REPORT.md).

It builds four things on the slice's renderer and text code, docked at Lantern Wharf:

- **The station up close**: each module has a sign; hover lifts it and shows the keeper
  and one line; click glides to its room.
- **The station key** along the bottom, with New, Changed and Closed ribbons and a
  one-line headline on hover.
- **Talk bubbles**: one line at a time over the speaker, click for the next.
- **The market room** beside its module, and **Orrin's cradle**, where parts are dragged
  onto the Wren's sockets.

## Run

From this folder, on any OS (Windows checked; macOS and Linux unchecked):

```sh
cargo xtask cargo run                      # the window, 1280x720
cargo xtask cargo run -- --shots shots     # headless scripted run: a PNG per step, PASS or FAIL
```

The scripted run also hovers everything the pointer can reach in every state it saves:
each hover must hold while the pointer rests, break no rule and run no Luau, and a hover
that breaks a rule gets its own picture.

To see a state that isn't scripted (for an agent, or to reproduce a bug), `--look` plays
steps headless and, at each `shot`, saves a PNG and prints what is on screen as JSON: what
is hovered, what popped up where, every target with its label, what it does and a point
to reach it, and any rule breaks.

```sh
cargo xtask cargo run -- --look looks "hover spot_canteen; shot canteen-hover"
cargo xtask cargo run -- --look looks "click key_market; hover buy1_rations; shot; sweep"
```

Steps: `hover W`, `click W`, `press W`, `release`, `leave`, `drag W V` (hold W over V),
`drop`, `back`, `wait N`, `zoom STEPS [W]`, `shot [NAME]`, `sweep` (hover everything here).
`W` is a widget id, a path from the JSON, or `x,y`.

`cargo xtask` here is the engine's xtask (`.cargo/config.toml`), so the lab builds in the
shared build folders and waits for the build permit like everything else.

## Try

| | |
|---|---|
| Talk | Click the bubble for the next line. The goal appears when Ilse is done. |
| Rooms | Click a module or a key icon. Esc, right-click or Station goes back. |
| Market | Sell and buy. Fill the hold and the buy buttons rest, with the reason on hover. |
| Cradle | Drag a part from the rack or the locker onto a socket. The seam lamp is refused (power); the lens shows what it changes before you let go. Fit the hold pods, fill the hold at the market, then try taking the pods off. |
| Zoom | The mouse wheel at the station, toward the pointer. Zoomed out, signs move beside their modules to stay clear of each other. |
| Log | The button at the bottom left: the lines heard so far. |
| F3 | What the lab measures: screen runs, their time, layout time, frame time, and how often the screen ran while a part was held. |
| F5 | Start again. |

Rule breaks show in a bar along the top, outlined on screen.

## Change a screen while it runs

The screens are in [`lab/`](lab): `main.luau` (state, events, which screen),
`station.luau`, `market.luau`, `cradle.luau`, `room.luau`, `common.luau` (bubble, status,
log, receipt), `fit.luau` (the fit rules) and `data.luau`. `prelude.luau` is the `ui`
widget set the engine offers. Save a file and the lab reloads it; the state is kept.
Try breaking a rule (give a text a `color`, make a column too narrow, use `→`) and watch
the bar.
