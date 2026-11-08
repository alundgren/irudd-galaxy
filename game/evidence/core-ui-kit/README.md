# Core UI kit: evidence

Screenshots of the core's screen kit (`game/modules/core`: `style.toml`, `icons.toml`,
`icons/`, `scripts/kit.luau`) and the screens restyled with it: the status chip, the
ship log and the radio. Drawn with `galaxy shot` on build `0.8.0-76eece08d63b-dev`
(the kit sheet and the radio on `0.8.0-6197849021a7-dev`).

| Picture | What it shows |
| --- | --- |
| `kit-sheet.png`, `kit-sheet-vs-reference.png` | Every look and icon, beside `slice-dock-bubble.png` |
| `hud-flight-1280x720.png`, `hud-flight-vs-reference.png`, `hud-flight-before-after.png` | Flight with a full hold: the radio strip top left, the status chip top right, the log's book bottom left |
| `log-open-1280x720.png`, `log-open-vs-reference.png`, `log-open-before-after.png` | The log open at Cinder Kiln after the trade route's first two runs: the galley pinned, one row per visit |
| `docked-bubble-1280x720.png`, `docked-bubble-vs-reference.png` | Docked at Lantern Dock, Bram's bubble with its amber name tag |
| `radio-before-after.png` | The radio, enlarged twice: the old parchment box and the new dark strip |
| `small-window-960x540.png` | The same three screens at 960x540 |
| `chip-mixed-cargo-before-after.png` | The chip with 4 ore, 1 plate and 1 rations aboard: `Hold 6/6` broken into fragments, then credits and the hold on one row and the three wares on the next |
| `log-return-visit-before-after.png` | Docked at Cinder Kiln, launched, flew about and docked there again: one merged row, then two visits |
| `log-fit-summary-before-after.png` | The fitting visit of `tests/fit.inputs`: eight lines of receipts and explanations, then `sold 6 ore +2 more` |
| `log-pins-trimmed-before-after.png` | Ines's offer left unanswered, then 64 newer entries (32 launch-and-redock trips): the pin gone, then still pinned |
| `log-open-960x540-before-after.png` | The log open in flight at 960x540: the ship under the panel, then clear of it |

The flight and log shots run the module's `tests/trade.inputs` (to tick 250, and to
tick 1299, then Back at 1300 and L at 1301, drawn at 1310); the bubble shot docks at
Lantern Dock with seed 3 and starts Bram's introduction (`tests/conversation.toml` up
to tick 500, drawn at 520). The before pictures are the module at `bea339d3`.

The other routes, as the review gave them:

- Mixed cargo: `tests/trade.inputs` buying one ration at 2490 and one plate in place
  of the sell at 2830, launch at 2840, mine at 2850, drawn at 3100.
- Return visit: dock at Cinder Kiln at tick 1, launch at 300, hold Ahead from 301 to
  421, dock at Cinder Kiln at 480, L at 900, drawn at 905.
- Fitting visit: `tests/fit.inputs` to tick 560, Back at 565, L at 566, drawn at 650
  once the view has settled.
- Pins: Ines's offer left unanswered, then 32 launch-and-redock trips (64 newer entries;
  seed 7), then L at 5500, drawn at 5560.
- Log in flight: L at tick 200, drawn at 220.

## Clearing the ship at 960x540

The ship sits mid-window. Flying, `galaxy shot --crop ship` puts it at x 454 to 506
(tick 220 above). Moored on a cradle it is larger: its tail reaches about x 416 at
Cinder Kiln (`log-open` and the fitting visit at 960x540). The open log now ends at
x 404 (24 margin + 380), so it covers neither. `tests/log.luau` keeps that bound.

## The kit sheet

`sheet/` is a small module that draws the sheet through `kit.luau` alone. To draw it,
copy it to a scratch folder together with the core's `style.toml`, `icons.toml`,
`icons/` and `scripts/kit.luau` (into its `scripts/`), then:

```sh
galaxy shot --modules <scratch folder> --ticks 10 --out kit-sheet.png
```
