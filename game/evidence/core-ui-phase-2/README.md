# Core UI phase 2: evidence

The core's screens moved onto the kit (`scripts/kit.luau`, `[screen.looks.*]` in
`style.toml`): the hint strip, station and trader name tags, the docked plate, the
station key and Launch, the trade, office and site rooms, and the fitting cradle.
Played on build `0.8.0-2281e9bc927a-dev` with `galaxy play --inputs` at 1280x720 and
960x540 (logical; the pictures are the notes' frames). The before pictures are the
module at `0660db69`.

Each screen has a before and after at both sizes, and the 1280x720 after beside its
direction reference (`game/docs/direction/`).

| Picture | Run, note | Reference | What it shows |
| --- | --- | --- | --- |
| `flight-hint-*` | `tests/play.inputs`, note 1 | `slice-overworld.png` | The hint strip: a cream box, then a dark pill with the key ("Hold Space") standing out; station name tags dark |
| `flight-labels-*` | `tests/play.inputs`, note 2 | `slice-mining.png` | Full hold in flight: trader and station names on small dark tags, not cream boxes |
| `docked-key-launch-*` | `tests/play.inputs`, note 3 | `ui-lab-station-up-close.png` | Docked: the plate compact and dark, the key's icons dark with an icon per room kind, Launch gold. At 960x540 the key no longer overlaps Launch and the plate no longer overlaps the Exchange sign |
| `exchange-sold-*` | `tests/play.inputs`, note 9 | `places-lab-market.png` | The exchange after selling: one ruled sheet with ware icons, heavy prices, Back beside the title, the receipt as a dark ticket with its CONFIRMED tag and a blue `+` |
| `office-*` | `tests/play.inputs`, note 10 | `places-lab-market.png` | The office: each station's posted prices on its own sheet, the station's name standing out |
| `exchange-resting-*` | `galaxy shot`, Kiln's exchange opened with a full hold (tick 700) | `slice-dock-bubble.png` | Cinder Kiln's exchange with a full hold: Sell gold, Buy 1 resting dashed with its reason beside it. At 960x540 it used to run off the bottom and cover the ship |
| `mixed-cargo-*`, `mixed-receipt-*` | `tests/mixed-trade.inputs`, ticks 1650 and 1670 | | Lantern's exchange with 2 ore, 3 ice and 1 plate aboard, then after selling just the plate. Sell everything and the receipt sit above the wares, which scroll; before, at 960x540 they ran below the window |
| `cradle-fitted-*` | `tests/fit-play.inputs`, note 5 | `places-lab-cradle-drag.png` | The cradle after fitting the lens: the receipt on one line, power and mass as one short line with icons (whole sentences on hover). At 960x540 it used to run off the bottom. The cradle's camera sits left of the berth and further back, so the fitted ship shows right of the panel at both sizes |
| `cradle-talk-*` | `tests/fit-play.inputs`, note 7 | `places-lab-office-talk.png` | Orrin's bubble beside the narrower cradle panel, with the ship beside it at both sizes |

Problems the runs reported (`problems` in the play report):

- Before, at 960x540: Launch overlapped the key, the Exchange sign overlapped the plate,
  the exchange and cradle panels ran outside the viewport, and the keepers' bubbles
  overlapped the room panel.
- After: none at either size. The one resting click in `fit-play` is that run's own
  click on the resting Buy 1.

The small window had its own bots then, `mixed-trade` and `cradle-small` (960x540
logical at 125% display scaling); both were dropped when the tests moved to set states.
`tests/trade-panel.luau` checks that Sell everything and the receipt come before the
wares and that the wares scroll within what 960x540 leaves them. Measured with
`galaxy shot --crop ship` in the cradle, the ship's centre moved from x=419 to x=563 at
960x540 (the panel ends at 454) and from 559 to 751 at 1280x720.

Still open: hover tooltips stay the engine's paper, because the installed build has no
look for them.
