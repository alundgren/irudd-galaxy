# Core: a quiet working shift

Fly a small tug, cut ironstone from three solid rocks, dock at either of two
stations and sell it, then trade between them: Cinder Kiln turns ore into plate,
and Lantern Dock wants plate to build a galley. Once the galley is built, its
cook sells rations that the Kiln's crews buy. The hold takes six units of any
mix. The field is small and finite (36 ore); there is no timed objective.

Use installed engine/GM 0.5.0 or newer. This version was authored and verified
with `0.5.0-da110809f31d-dev`. No engine build is needed.

```sh
galaxy play --modules game/modules
```

| Control | Action |
| --- | --- |
| W / S | Forward / reverse thrust; release to stop |
| A / D | Turn left / right |
| Shift | Boost |
| Space | Approach and mine the nearest rock with ore |
| E | Dock at the nearest berth |
| 0 | Cancel the current flight/mining order |
| M | Docked: open the ore exchange |
| Enter | In a room that buys: sell everything it buys |
| B | Back from a room to the whole station |
| U | Docked: launch |
| L | Open or close the ship log |
| Mouse wheel | Zoom in flight; scroll the log |
| F8 | Take a playtest note |
| Esc | Quit; the engine saves the session |

Everything is also a click: the order buttons in flight, the signs on the
station's buildings, the station key, Sell, Buy, Back, Launch and the log. Click a
keeper to hear them. Manual thrust or turning cancels an order. Rocks and
station hulls are solid; orders steer toward their targets, so use manual
flight to clear an obstruction when approaching from the far side of a station.

## The station up close

Docking cuts to the station drawn large: its deck, tower, berth (with your ship
moored), crane, rim lamps and a skiff ferrying across the deck. The station is
the menu. Each building carries a sign you can press, and hovering the building
or its sign names its keeper and what is offered there. The station key along
the bottom reaches the same rooms, and Launch sits at the bottom right.

A room opens as a panel on the side away from its building, 520 of 1280 pixels
wide, so the building and most of the station stay in view. Inside a room only
that room shows: the status panel, title, key, log and launch step aside and
come back with Back.

- **Exchange**: your hold and credits, then one line per good with its price
  and stock as plain text. A good the station buys gets `Sell N for X cr` when
  you carry it (with `[Enter]` when it is the only one; `Sell everything here`
  appears when there are several). A good it sells gets `Buy 1` and `Buy N`,
  the most that fits and that you can pay for. A full hold replaces the buy
  buttons with one line, `Hold full: buy buttons rest until you sell.`; short
  credits and an empty stock each say so. Each trade shows its receipt in the
  room and in the log. The crate pile beside the exchange follows its ore stock.
- **Office**: every station's posted prices, what a station is building and how
  far it has got, and how much ore the field still holds.
- **Galley site** (Lantern Dock): what will be built, how much plate is in, and
  what it will bring. Once built it is the **Galley**, kept by Ines Varga, who
  sells rations.

Each room has a keeper standing by it, wearing a name sign with what they do
(`Tova Reyes · buys ore`). Inside a room only that room's keeper wears one, and
the open log hides them all except the keeper who is speaking, so a bubble always
shows whose words it holds. Station names and room names never share a word.

## Trade, prices and the galley

Lantern Dock buys ore and plate; Cinder Kiln buys ore and rations and sells
plate; the galley, once built, sells rations (`market` lines in `start.toml`).
Every line has a base price, a target stock and a rate. The price of each unit
is the base raised while stock is short of target and lowered while it is over,
by half the shortfall, within 70% to 130% of base (`scripts/rules.luau`). A
trade is priced unit by unit, so selling six ore pays a little less for the last
than the first and splitting a trade never pays more.

Markets move on visits, never on time. A counted visit eases each of that
station's lines back towards its target by its rate. A docking counts only when
the ship has really been away since it last docked there: flown 20 units from
the berth, mined, or docked at the other station. A quick launch-and-redock is
still a docking, with its log line, but it is not a visit and settles nothing.

The galley site stands on Lantern Dock's deck as a scaffold from the start. Bram
mentions it, the office and the site's own room show its progress, and the
exchange's plate line says the plate builds it. Plate sold at Lantern Dock goes
into the galley at the posted price, without filling the stock, until 10 are in.
The sale that completes it turns the scaffold into the galley, brings Ines to
stand by it with her own introduction, and opens rations, a new good and a
return cargo for the Kiln run. The Kiln's exchange posts its rations price from
the start, and Sefa mentions her crews would pay for them.

Measured on the headless route `tests/trade.inputs` (seed 7): one hold of ore
sells for 97 cr at the Kiln and 73 at Lantern Dock; the three plate runs earn
25, 22 and 13 cr as the Kiln's plate stock runs down faster than visits restock
it; a hold of rations earns 24 cr. The galley is built on the third plate
delivery, about a minute of play after the first sale (80 seconds in the click
route), with 157 cr in hand; the route ends at 181 cr.

## The keepers talk

Click a keeper and they speak in first-person bubbles, one short sentence per
click, with their name on its own line at the top of the bubble. The first
conversation introduces them: who they are, what they do and where the other
station is. After that a click gets one line that follows the shift: the exchange
keeper talks about what you carry that they buy, the sale you just made, or
their own idle line; the dockmaster about the galley's plate while it is being
built, then the ore left in the field (`scripts/voices.luau`). Talk ends when you launch and never
becomes the flight message.

Each conversation you hear is one ship log entry that names the speaker and the
station, with the lines quoted in order, even when something else is logged
while you are still listening. A line already in the log isn't logged again. The log says it lists newest first.

## Screens

Each kind of thing keeps one look, all with pure black text: amber for anything
you can press (orders, signs, the key, Sell, Back, Launch, the log button), a
pale blue panel for status and rooms, white bubbles and name signs for the people
who speak, and cream paper for what just happened and for headings. Status sits top right, the log button
bottom left, orders top left in flight, and the latest message with the flight
keys at the bottom. Prices and stock are plain text, never buttons.

Hints teach and then retire (`scripts/teach.luau`). Each flight key leaves the
hint line after three uses, and messages stop explaining Space and E once those
were used three times. A room stops saying "Click Tova to talk" after three
conversations. Messages report what happened, not how fast it happens.

Screens are laid out for the 1280x720 design size in logical pixels, inside a
24-pixel margin (`scripts/layout.luau`). Smaller windows clip the right and
bottom edges, because a screen can't yet place widgets against the window's
edges.

## Design rules

- **Every change is seen.** Ore arrives in the hold, the rock develops a
  visible cut, and a trade changes credits, the posted stock and price, and for
  ore the crate pile. Plate delivered to the galley site shows as progress, and
  the finished galley replaces the scaffold. The ship log keeps a line for each
  docking, trade, launch, full hold, the galley built and each conversation.
  Empty sales never create money, and a trade that can't happen does nothing.
- **Shocks come with opportunities.** Exhausting a rock leaves other rocks to
  work and both buyers available. A full hold is an invitation to trade. There
  are no damage penalties or scripted shocks in this first loop.
- **No fake urgency.** Prices move only with trades and counted visits, never
  with time. Nothing expires while the player looks around. Mining produces one
  unit per second once within seven world units; the six-unit hold bounds each
  trip.

The slice supplies the calm mining/trading feel, warm inked machinery, readable
consequences and the agreed station design. Its Rust, progression, factions and
economy code are not copied; the price formula, recount rule and growth from
delivered goods follow the slice's behaviour. All models come from committed `recipes/` through
`galaxy-gm model make game/modules/core`; the generated glTF files are
committed. The engine owns flight, collision, steering, travel, walks, talk and
hovers. Luau handles player intent, once-per-second extraction and skiff trips,
trade rules and pricing, the galley's build, the log and the module-styled
screens. Each station is one place
whose set is the station up close; its buildings, keepers and skiff stand in it.

## Verify

Check and test the shipped module collection in place. Standalone examples live
under `game/examples/`, outside that collection:

```sh
galaxy-gm check game/modules/core
galaxy-gm test game/modules/core
```

There are ten Luau rule tests (hold, depletion, prices against stock and their
bounds, unit-by-unit selling and buying, what a purchase is bounded by, visits
settling a market, quick taps not counting, hints, keepers' lines) and nine
scenarios: both complete station trips, collision under held thrust, refused
out-of-place actions, repeat trips through depletion with an empty-sale check,
the station itself (rooms from signs and M, refused sales outside the exchange,
a building of another station, the log held shut inside a room, keeper talk, the
sale receipt and log lines, the skiff and launch), talk (an introduction once,
one line per click, the repeat line following the hold and the sale, one log
entry per conversation with no repeats, talk ended by launch, and hints
retiring), one conversation (a sale in the middle of an introduction leaves it
one entry, and the speaker keeps their name while the log is open), and trade
(buying refused with a full hold, ore and plate priced unit by unit, plate into
the galley, a quick redock tap that is not a visit, the site trading nothing
before it is built, visits restocking, the galley built with its keeper and her
sign, rations bought and sold, visit and docking counts). `loop.inputs`,
`kiln.inputs`, `repeat.inputs` and `trade.inputs` are equivalent headless
routes. Two bots use real screen clicks: `play.inputs` talks to both Lantern
Dock keepers and takes nine notes (`play.bot.toml`, 900 ticks), and
`trade-play.inputs` plays the whole trade loop through the galley with eleven
notes (`trade.bot.toml`, 3805 ticks). Both declare 1280x720 physical pixels. The
GM test report includes their frame times, dimensions and deterministic replay
results. On a display scaled above 100%, that window is smaller than 1280x720
logical pixels and reports the clipped edges as problems.

```sh
galaxy play --modules game/modules --inputs game/modules/core/tests/play.inputs --ticks 900 --size 1280x720
galaxy sim replay SESSION/recording.json
galaxy-gm audit SESSION/recording.json
galaxy-gm film SESSION/recording.json --from 20 --to 45 --size 1280x720 --out flight-film
galaxy-gm motion track flight-film --thing ship --to rock1
```

`SESSION` is the folder printed by play. Keep recordings, saves and films outside
this module. Curated screenshots live in `game/evidence/`.

### Save/load verification

Play saves at session end and `--save FILE` also writes a chosen file. Reopen
that save in the window with `galaxy play --modules game/modules --load SAVE`.
The open room is world state, so a save taken inside the ore exchange reopens
there. Run from the repo root with an external working folder in place of
`evidence`:

```sh
galaxy sim run --modules game/modules/core --seed 7 --ticks 650 --inputs game/modules/core/tests/loop.inputs --save evidence/room.db
galaxy sim load evidence/room.db --modules game/modules/core --ticks 190 --inputs game/modules/core/tests/loop.inputs
galaxy sim run --modules game/modules/core --seed 7 --ticks 840 --inputs game/modules/core/tests/loop.inputs
galaxy play --modules game/modules --load evidence/room.db --ticks 700
```

The tick-650 save is inside Lantern Dock's exchange with six ore aboard. The
same holds for a save taken just after the galley is built: run `trade.inputs`
to tick 2475 with `--save`, load it for 365 ticks, and compare with an
uninterrupted 2840-tick run; the galley, Ines and her sign come back with it.
The resumed and uninterrupted runs must end on the same hash. Bot input ticks
and play's `--ticks` are absolute world ticks, including when resuming. The
scenario schema cannot express save/load steps, so these checks run through the
installed CLI separately.

## One self-critique against the slice

The trade loop is the thinnest one: three goods, one thing to build, one
milestone. Plate is the only good that builds anything, so after the galley
opens nothing else grows, and the Kiln's plate margin shrinks run by run until
visits restock it. Buy buttons are left out with a full hold instead of drawn
resting, because a button can't be drawn resting yet. The galley's sign sits
low on its front so it clears the dockmaster's name sign.

The station view is now the agreed design rather than a deck and a separate
market room, and the two stations differ in their towers and paint. What is
still missing is engine-side: the arrival is a fade to a title card rather than
a zoom, buildings don't lift or outline under the pointer and only their signs
open rooms, rooms open at once with no glide, and right-click and Esc don't go
back. The key has no news ribbons because nothing at a station changes on its
own yet. There is no music or station sound. In a bubble the
speaker's name and Continue have the same size and weight as the line, and the
bubble has no way to offer a choice, so no keeper makes an offer yet. Further
art and pacing changes wait for a human playtest.
