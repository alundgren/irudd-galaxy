# Core: a quiet working shift

Fly a small tug, cut ironstone from three solid rocks, dock at either of two
stations and sell it, then trade between them: Cinder Kiln turns ore into plate,
and Lantern Dock wants plate to build a galley. Once the galley is built, its
cook sells rations that the Kiln's crews buy. The hold takes six units of any
mix. The field is small and finite (36 ore); there is no timed objective.

Use installed engine/GM 0.7.0 or newer. This version was authored and verified
with `0.7.0-244988df669c-dev`. No engine build is needed.

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
| M | Docked: open the exchange |
| Enter | In a room that buys: sell everything it buys |
| B, Esc or right-click | Back from a room to the whole station, or shut the log |
| U | Docked: launch |
| L | Open or close the ship log |
| Mouse wheel | Zoom in flight; scroll the log |
| F8 | Take a playtest note |
| Shift+Esc | Quit; the engine saves the session (closing the window does too) |

Everything is also a click: the order buttons in flight, the station's buildings
and their signs, the station key, Sell, Buy, Back, Launch and the log. Click a
keeper to hear them. Manual thrust or turning cancels an order. Rocks and
station hulls are solid; orders steer toward their targets, so use manual
flight to clear an obstruction when approaching from the far side of a station.

## The station up close

Docking glides in from the ship's view until the station fills the screen: its
deck, tower, berth (with your ship moored), crane, rim lamps and a skiff ferrying
across the deck. The station is the menu. Under the pointer a building lifts, takes
a blue outline and names its keeper and what is offered there, and the pointer
becomes a hand; clicking the building opens its room. Each building also carries a
sign you can press, the station key along the bottom reaches the same rooms, and
Launch sits at the bottom right.

Opening a room glides the view to frame its building, and the room opens as a
panel on the side away from it, 520 pixels wide, so the building and its keeper
stay in view beside it. Each glide takes a second; a click during one lands it at
once. Inside a room only that room shows: the status panel, title, key, log and
launch step aside and come back with Back, which glides out to the whole station
again. Signs rest while the view glides, so they never crowd the moving view.
Opening a room, going back and opening the log each end whatever is being said, so
a bubble never rides a glide over the status panel or sits on a room panel or the
log. Nothing is lost: an unfinished conversation begins again at a click, and an
unanswered offer is still on offer.

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
the open log hides them all; while someone speaks only they wear theirs, so a
bubble always shows whose words it holds. Station names and room names never share a word.

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
delivery, about a minute of play after the first sale (88 seconds in the click
route), with 157 cr in hand; the route ends at 181 cr.

## The keepers talk

Click a keeper and they speak in first-person bubbles, one short sentence per
click. The bubble's heading is their name, with what they do under it in smaller,
lighter type (`Exchange keeper`, `Dockmaster`, `Cook`); the station is left out
because you are standing in it. A small `Continue` or `Done` sits at the bubble's
end. While someone speaks only they wear their name sign. The first
conversation introduces them: who they are, what they do and where the other
station is. After that a click gets one line that follows the shift: the exchange
keeper talks about what you carry that they buy, the sale you just made, or
their own idle line; the dockmaster about the galley's plate while it is being
built, then the ore left in the field (`scripts/voices.luau`). Talk ends when you launch and never
becomes the flight message.

Once her galley stands and she has introduced herself, Ines thanks you for the
plate and offers a ration on the house, with `Take it` and `No, thanks` in the
bubble. Taking it puts one ration in the hold with a receipt in the log; either
answer ends the offer for good, and her everyday line comes back. While the hold
is full the offer waits, unanswered, and she says her everyday line instead.

Each conversation you hear is one ship log entry that names the speaker and the
station, with the lines quoted in order and any answer you gave (`You: Take it.`),
even when something else is logged while you are still listening. A line already in the log isn't logged again. The log says it lists newest first.

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

Each piece of a screen sits against a window edge with a 24-pixel margin
(`scripts/layout.luau`), so the HUD holds at any window size and display scale:
the design size is 1280x720 logical pixels, and everything also fits 1024x576
(1280x720 on a display scaled to 125%). The room cameras frame each keeper so
their bubble clears the room's panel at both sizes.

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
settling a market, quick taps not counting, hints, keepers' lines) and fourteen
scenarios: both complete station trips, collision under held thrust, refused
out-of-place actions, repeat trips through depletion with an empty-sale check,
the station itself (rooms from signs and M, refused sales outside the exchange,
a building of another station, the log held shut inside a room, keeper talk, the
sale receipt and log lines, the skiff and launch), talk (an introduction once,
one line per click, the repeat line following the hold and the sale, one log
entry per conversation with no repeats, talk ended by launch, and hints
retiring), one conversation (a sale in the middle of an introduction leaves it
one entry, and opening the log ends an introduction, which begins again once
it shuts), trade
(buying refused with a full hold, ore and plate priced unit by unit, plate into
the galley, a quick redock tap that is not a visit, the site trading nothing
before it is built, visits restocking, the galley built with its keeper and her
sign, rations bought and sold, visit and docking counts), back (a building
clicked opens its room and moves the room camera to frame it, Esc, right-click
and B go back and frame the whole station again, Esc shuts the log, signs resting
while the view glides and while someone else speaks, launching from a room), and
Ines's gift taken (one ration and its receipt, her follow-up line, one log entry
with the answer, the offer gone), declined (nothing changes hands, a late second
answer is stale, the offer gone), waiting while the hold is full, and going back
mid-talk (Back on her unanswered offer and on its first line ends the bubble and
keeps the offer, a late answer to it is stale, the third time it is taken and
logged once; opening the log ends a bubble and a click through it starts none). Play records
`engine.place_arrived` and `engine.camera_settled` when a glide lands; scenarios
give them by hand where a check needs the view settled. `loop.inputs`,
`kiln.inputs`, `repeat.inputs` and `trade.inputs` are equivalent headless
routes. Three bots use real screen clicks: `play.inputs` docks (the view glides in),
talks to both Lantern Dock keepers, points at and clicks the exchange building,
lands a glide with a click, goes back with Esc and with a right-click, shuts the
log with Esc and takes twelve notes (`play.bot.toml`, 1000 ticks), and
`trade-play.inputs` plays the whole trade loop through the galley and Ines's gift
with twelve notes (`trade.bot.toml`, 4150 ticks), and `back-talk.inputs` plays
that loop up to Ines's offer, then goes back with Esc on the unanswered offer and
with a right-click on its first line, takes it the third time, opens the log while
Tova speaks and opens her exchange while she speaks (`back-talk.bot.toml`, 3920
ticks, thirteen notes). All three declare 1280x720 physical pixels. The GM test
report includes their frame times, dimensions, problems and deterministic replay
results. On a display scaled to 125% that window is 1024x576 logical pixels, and
all three run there without problems.

```sh
galaxy play --modules game/modules --inputs game/modules/core/tests/play.inputs --ticks 1000 --size 1280x720
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
market room, and the two stations differ in their towers and paint. Docking
glides in, buildings lift and outline under the pointer and open their rooms when
clicked, the view glides to each building, and Esc and right-click go back. The
key has no news ribbons because nothing at a station changes on its own yet.
There is no music or station sound. Ines's ration is the only offer so far, and
the only thing anyone gives. Further art and pacing changes wait for a human
playtest.
