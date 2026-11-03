# Core: a quiet working shift

Fly a small tug, cut ironstone from three solid rocks, dock at either of two
stations, sell at its ore exchange, and launch again. Lantern Dock pays
12 credits per ore; Cinder Kiln pays 16. The hold takes six units. This is a
small, finite field (36 ore), without a progression tree or a timed objective.

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
| Enter | In the ore exchange: sell the whole hold |
| B | Back from a room to the whole station |
| U | Docked: launch |
| L | Open or close the ship log |
| Mouse wheel | Zoom in flight; scroll the log |
| F8 | Take a playtest note |
| Esc | Quit; the engine saves the session |

Everything is also a click: the order buttons in flight, the signs on the
station's buildings, the station key, Sell, Back, Launch and the log. Click a
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

- **Ore exchange**: the posted price, your hold and credits, the station's
  stock and its crate pile beside the building. Sell is the only verb; with an
  empty hold it is replaced by a plain line. A sale shows its receipt in the
  room and in the log.
- **Office**: the posted prices of both stations and how much ore the field
  still holds.

Each room has a keeper standing by it, wearing a name sign with what they do
(`Tova Reyes · buys ore`). Inside a room only that room's keeper wears one, and
the open log hides them all except the keeper who is speaking, so a bubble always
shows whose words it holds. Station names and room names never share a word.

## The keepers talk

Click a keeper and they speak in first-person bubbles, one short sentence per
click, with their name on its own line at the top of the bubble. The first
conversation introduces them: who they are, what they do and where the other
station is. After that a click gets one line that follows the shift: the exchange
keeper talks about your hold or the sale you just made, the dockmaster about the
ore left in the field (`scripts/voices.luau`). Talk ends when you launch and never
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
  visible cut, and a sale changes credits, the posted station stock and its crate
  pile. The ship log keeps a line for each docking, sale, launch, full hold and
  conversation.
  Empty sales never create money.
- **Shocks come with opportunities.** Exhausting a rock leaves other rocks to
  work and both buyers available. A full hold is an invitation to trade. There
  are no damage penalties or scripted shocks in this first loop.
- **No fake urgency.** Prices are posted and stable. Nothing expires while the
  player looks around. Mining produces one unit per second once within seven
  world units; the six-unit hold bounds the first shift.

The slice supplies the calm mining/trading feel, warm inked machinery, readable
consequences and the agreed station design. Its Rust, progression, factions and
economy are not copied. All models come from committed `recipes/` through
`galaxy-gm model make game/modules/core`; the generated glTF files are
committed. The engine owns flight, collision, steering, travel, walks, talk and
hovers. Luau handles player intent, once-per-second extraction and skiff trips,
trade rules, the log and the module-styled screens. Each station is one place
whose set is the station up close; its buildings, keepers and skiff stand in it.

## Verify

Check and test the shipped module collection in place. Standalone examples live
under `game/examples/`, outside that collection:

```sh
galaxy-gm check game/modules/core
galaxy-gm test game/modules/core
```

There are five Luau rule tests and eight scenarios: both complete station trips,
collision under held thrust, refused out-of-place actions, repeat trips through
depletion with an empty-sale check, and the station itself (rooms from signs and
M, refused sales outside the exchange, a building of another station, the log
held shut inside a room, keeper talk, the sale receipt and log lines, the skiff
and launch), and talk (an introduction once, one line per click, the repeat line
following the hold and the sale, one log entry per conversation with no repeats,
talk ended by launch, and hints retiring), and one conversation (a sale in the
middle of an introduction leaves it one entry, and the speaker keeps their name
while the log is open). `loop.inputs`, `kiln.inputs` and `repeat.inputs` are equivalent
headless routes; `play.inputs` uses real screen clicks, talks to both Lantern Dock
keepers and takes nine notes. `play.bot.toml` declares that 900-tick route at 1280x720 physical pixels. The GM
test report includes its frame times, dimensions and deterministic replay
result. On a display scaled above 100%, that window is smaller than 1280x720
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

The tick-650 save is inside Lantern Dock's ore exchange with six ore aboard.
The resumed and uninterrupted runs must end on the same hash. Bot input ticks
and play's `--ticks` are absolute world ticks, including when resuming. The
scenario schema cannot express save/load steps, so these checks run through the
installed CLI separately.

## One self-critique against the slice

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
