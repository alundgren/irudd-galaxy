# Core v0: a quiet working shift

Fly a small tug, cut ironstone from three solid rocks, dock at either of two
stations, enter its ore exchange, sell, and launch again. Lantern Dock pays
12 credits per ore; Cinder Kiln pays 16. The hold takes six units. This is a
small, finite field (36 ore), without a progression tree or a timed objective.

Use installed engine/GM 0.5.0 or newer. This version was authored and verified
with `0.5.0-fe9f0c5f13ed`. No engine build is needed.

```sh
galaxy play --modules game/modules/core
```

| Control | Action |
| --- | --- |
| W / S | Forward / reverse thrust; release to stop |
| A / D | Turn left / right |
| Shift | Boost |
| Space | Approach and mine the nearest rock with ore |
| E | Dock at the nearest berth |
| 0 | Cancel the current flight/mining order |
| M | Enter the market from a landing deck |
| Enter | Sell the whole hold in the market |
| B | Return from the market to the landing deck |
| U | Launch from the landing deck |
| Mouse wheel | Zoom |
| F8 | Take a playtest note |
| Esc | Quit; the engine saves the session |

The buttons also mine, select either destination, enter/leave a market and sell.
Manual thrust or turning cancels an order. Rocks and station hulls are solid;
orders steer toward their targets, so use manual flight to clear an obstruction
when approaching from the far side of a station.

## Design rules

- **Every change is seen.** Ore arrives in the hold ledger, the rock develops a
  visible cut, and a sale changes credits, the posted station stock and its crate
  pile. A sentence says what happened. Empty sales never create money.
- **Shocks come with opportunities.** Exhausting a rock leaves other rocks to
  work and both buyers available. A full hold is an invitation to trade. There
  are no damage penalties or scripted shocks in this first loop.
- **No fake urgency.** Prices are posted and stable. Nothing expires while the
  player looks around. Mining produces one unit per second once within seven
  world units; the six-unit hold bounds the first shift.

The slice supplies the calm mining/trading feel, warm inked machinery, and
readable consequences. Its Rust, progression, factions and economy are not
copied. All models come from committed `recipes/` through
`galaxy-gm model make game/modules/core`; the generated glTF files are committed.
The engine owns flight, collision, steering and travel. Luau handles player
intent, once-per-second extraction, trade rules and the module-styled screens.
Each station has a landing-deck room and a separate market room, linked by doors.

## Verify

The installed GM loads sibling modules automatically. Quiet Garden is a separate
game with its own player and scene style, so checking the repository's sibling
set reports conflicts. Copy **only** this `core` folder to an otherwise empty
parent outside the checkout, then run (replace `isolated/core` with that copy):

```sh
galaxy-gm check isolated/core
galaxy-gm test isolated/core
```

There are three Luau rule tests and five scenarios: both complete station trips,
collision under held thrust, refused out-of-place actions, and repeat trips
through depletion with an empty-sale check. The repeating trip then mines a
different rock. `loop.inputs`, `kiln.inputs` and `repeat.inputs` are equivalent
headless routes; `play.inputs` uses real screen clicks and takes six notes.

```sh
galaxy play --modules game/modules/core --inputs game/modules/core/tests/play.inputs --ticks 840 --size 1280x720
galaxy sim replay SESSION/recording.json
galaxy-gm audit SESSION/recording.json
galaxy-gm film SESSION/recording.json --from 20 --to 45 --size 1280x720 --out flight-film
galaxy-gm motion track flight-film --thing ship --to rock1
galaxy-gm film SESSION/recording.json --from 400 --to 500 --size 1280x720 --out dock-film
galaxy-gm motion sheet dock-film --at 13.5,14.0,14.4,15.0
```

`SESSION` is the folder printed by play. Keep recordings, saves and films outside
this module. Curated screenshots live in `game/evidence/core-v0/`. The PR records
the exact head's check/test results and the bot's frame-time report.

### Save/load verification

Play saves at session end and `--save FILE` also writes a chosen file. The current
engine can resume it with `galaxy sim load`, but **cannot reopen a save in the
window**. A generic play-load feature is requested in the PR; this module does
not emulate persistence. The complete human save-and-resume journey is pending
that installed feature.

These commands verify continuity across a mid-mining save. Run from the repo
root with an external working folder in place of `evidence`:

```sh
galaxy sim run --modules game/modules/core --seed 7 --ticks 150 --inputs game/modules/core/tests/loop.inputs --save evidence/mid.db
galaxy sim load evidence/mid.db --modules game/modules/core --ticks 690 --inputs game/modules/core/tests/loop.inputs --record evidence/resumed.json
galaxy sim run --modules game/modules/core --seed 7 --ticks 840 --inputs game/modules/core/tests/loop.inputs
galaxy sim replay evidence/resumed.json
```

The resumed and uninterrupted final hashes must agree. Loading the bot's end
save with zero ticks must also match the bot's final hash, with 72 credits,
six station stock, six ore left in the first rock and the player back in space.
The current scenario schema cannot express save/load steps, so these checks run
through the installed CLI separately.

## One self-critique against the slice

The short trip keeps the slice's calm work-and-return rhythm. The initial
station was hidden behind the ledger; placing it lower made the field readable.
Berths now face the ore field, and mining reach covers approaches to all three
rock outlines. Collision stops the ship at the rock instead of passing through
it. The market's visible stockpile makes the transaction concrete.

The rooms are intentionally spare: a deck, a lamp, a counter and a stockpile.
There is no station life, sound, dialogue or promise of the slice's longer
journey. The engine's one-second title cut makes arrival clear. Further art and
pacing changes wait for a human playtest.
