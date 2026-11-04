# Core UI kit: evidence

Screenshots of the core's screen kit (`game/modules/core`: `style.toml`, `icons.toml`,
`icons/`, `scripts/kit.luau`) and the screens restyled with it: the status chip, the
ship log and the radio. Drawn with `galaxy shot` on build `0.8.0-6197849021a7-dev`.

| Picture | What it shows |
| --- | --- |
| `kit-sheet.png`, `kit-sheet-vs-reference.png` | Every look and icon, beside `slice-dock-bubble.png` |
| `hud-flight-1280x720.png`, `hud-flight-vs-reference.png`, `hud-flight-before-after.png` | Flight with a full hold: the radio strip top left, the status chip top right, the log's book bottom left |
| `log-open-1280x720.png`, `log-open-vs-reference.png`, `log-open-before-after.png` | The log open at Cinder Kiln after the trade route's first two runs: the galley pinned, one line per visit |
| `docked-bubble-1280x720.png`, `docked-bubble-vs-reference.png` | Docked at Lantern Dock, Bram's bubble with its amber name tag |
| `radio-before-after.png` | The radio, enlarged twice: the old parchment box and the new dark strip |
| `small-window-960x540.png` | The same three screens at 960x540 |

The flight and log shots run the module's `tests/trade.inputs` (to tick 250, and to
tick 1300 then Back and L); the bubble shot docks at Lantern Dock with seed 3 and
starts Bram's introduction.

## The kit sheet

`sheet/` is a small module that draws the sheet through `kit.luau` alone. To draw it,
copy it to a scratch folder together with the core's `style.toml`, `icons.toml`,
`icons/` and `scripts/kit.luau` (into its `scripts/`), then:

```sh
galaxy shot --modules <scratch folder> --ticks 10 --out kit-sheet.png
```
