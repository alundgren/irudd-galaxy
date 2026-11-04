# Core: a quiet working shift

Every change here follows [the direction doc](../../docs/direction.md); compare your screenshots with its reference images.

Fly a small tug, cut ironstone from three solid rocks, dock at either of two
stations and sell it, then trade between them: Cinder Kiln turns ore into plate,
and Lantern Dock wants plate to build a galley. Once the galley is built, its
cook sells rations that the Kiln's crews buy. The hold takes six units of any
mix. At the Kiln's fitting cradle, Orrin fits parts bought with what you earn: a
lens that mines faster, pods for a bigger hold and thrusters for a higher top
speed, within the hull's power and mass. Two traders fly their own routes between
the stations while you work, and in flight the radio plays each station's channel
over the tug's quiet hum. Out in the reach the rocks turn slowly, the stations' rings
turn and their lamps breathe, and the stars drift in layers as you fly. The field is
small and finite (36 ore); there is no timed objective.

Use installed engine/GM 0.7.0 or newer. This version was authored and verified
with `0.7.0-c571bdc1442a` (the look: style, models and screens). No engine build is needed.

```sh
galaxy play --modules game/modules
```

| Control | Action |
| --- | --- |
| W / S | Forward / reverse thrust; let go and the ship coasts to a stop in a second |
| A / D | Turn left / right, 180° a second |
| W, A, S or D docked | Fly straight off the dock: no separate undock step |
| Shift | Boost |
| Space | Approach and mine the nearest rock with ore |
| E | Dock at the nearest berth |
| 0 | Cancel the current flight/mining order |
| M | Docked: open the exchange |
| Enter | In a room that buys: sell everything it buys |
| B, Esc or right-click | Back from a room to the whole station, or shut the log |
| U | Docked: launch without flying on |
| L | Open or close the ship log |
| R | In flight: the radio's next channel; past the last it turns off, and R again turns it on |
| Drag, or click | In the fitting cradle: a part onto its socket to fit it, or onto the tray to take it off; or Choose a part, then Fit here |
| Mouse wheel | Zoom in flight; scroll the log |
| F8 | Take a playtest note |
| Shift+Esc | Quit; the engine saves the session (closing the window does too) |

Everything is also a click: the orders in flight (`Space · Mine` and `Dock at` either
station, in the strip at the bottom), the station's buildings
and their signs, the station key, Sell, Buy, Back, Launch, the log and the radio's Prev, Off/On and
Next. Click a
keeper to hear them. Manual thrust or turning cancels an order. Rocks,
station hulls and the traders are solid; orders steer toward their targets, so use manual
flight to clear an obstruction when approaching from the far side of a station.

## Flight and traffic

The ship flies by the locked flight preset (#47), at the core's view of 64 units:
top speed 20 units a second (0.3 view heights), thrust 30 and braking 23.3 units a
second squared, turns at 180° a second, and keeps a little inertia (0.4): the nose
swings through a turn while the old heading carries on for a moment, and letting go
of thrust coasts to a stop in exactly a second. From rest W flies a straight line
wherever the nose points. Moored, the ship faces the station; it leaves facing away,
so W, A, S or D flies it off on the tick the key is pressed, and U or Launch leaves
the berth without flying on (`scripts/launch.luau`). The view glides back out from the
station to the ship's view in a second, the reverse of docking in, with no fade and no
title card, while the ship is already flying (each station's `engine.room` has
`glide = 1.0`, which glides both ways).

A save from before the locked flight keeps its old flight: top speed 12, thrust 18,
braking 16 and a 0.7-second stop. Its ship's `fit` loads with `hull_speed` 12, the
bare hull it was built with (a new game sets 20), so the cradle shows the speed that
ship really flies and a part changes only what it says: its thrusters still give 15.
Such a save has no traders either. Start a new game for the locked flight and traffic.

Everything that looks solid is solid, and nothing takes damage: ships, traders,
rocks and stations bump and push. A ship that flies into a rock or a station
stops on its drawn surface and glances off round it; a trader the ship flies into is
pushed aside and steers back onto its route. Rocks and stations never move, so
every mining spot stays where it is; a rock only turns slowly in place. A Mine order
parks the ship on the rock's drawn surface (`range = 0`), and the engine's finished
`hold` keeps it there while the rock is cut smaller and turns.

Two traders work the reach (`scripts/traffic.luau`): Little Mercy, a hauler, runs
along the south between moorings off Lantern Dock's and Cinder Kiln's far sides, and
Quiet Lamp, a lamp launch, flies the long way round the north of the field between
the stations' outer sides. Each flies its leg's waypoints without stopping, docks
onto its mooring facing the station, waits there eight seconds and flies back.
Their routes and moorings keep clear of the rocks and the player's berths, so they
never block a mining spot or a berth. They don't trade: markets move only on the
player's visits. Saves keep where they are and what they are doing.

A trader under way wears its name label (`scripts/labels.luau`) at any distance and
zoom, except within 17 units of a station's centre, where it would sit on the hull. The
label is a `yield` widget, so the engine leaves it undrawn while its trader is off
screen, while it would leave the window, and while it would overlap the status,
log button, bottom strip, radio or the other trader's label; it comes back once clear. It
sits on the side of the trader away from your ship, so it never covers it. Pointing at
a trader always names it and its run.

Measured on films of the ship's probes (`galaxy film --probes-only`, then
`galaxy-gm film show`): turns at 180.1° a second; top speed 20, 95% of it 0.67 seconds
after W; letting go at full speed coasts 9.7 units and stops 1.0 seconds later; in a
90° turn under thrust the course lags the nose by up to 27° and tracks it again a
third of a second after the turn ends. The mining approach parks 0.004 units from the
rock's surface and stays within 0.05 of it for the whole hold (12.9 seconds, six cuts,
the rock turning), ending 0.004 away.

## The reach moves on its own

With nothing pressed the reach is never quite still, and everything that moves on its
own is slow and quiet beside what you can act on:

- **Rocks** turn where they float (`engine.spin` in `start.toml`: 3, -4 and 2.5 degrees
  a second) and never drift, so every mining spot stays put. Their solid outline turns
  with them, and a Mine order's hold keeps the ship on the turning surface.
- **Stations' rings** turn about their hubs (`spin` on the ring part in
  `recipes/station.toml` and `recipes/kiln_station.toml`: Lantern Dock 6 degrees a second
  one way, Cinder Kiln 5 the other), and the ring's lamps breathe between full and 45%
  glow every four or five seconds while the mast's tip lamp blinks down to 20% every 2.5
  or 3 seconds (`pulse`, `pulse_low`). Both are drawn only: they change no world state,
  so saves and replays are untouched. The station up close, which is the menu, stays
  still.
- **Stars** lie in three layers (`star_layers` in `style.toml`): far ones creep behind
  near ones as the ship flies.

Measured on films with nothing pressed (`galaxy film`, then the motion tools): rock1
turns 17.9 degrees in six seconds, at 3.0 a second, smoothly (judder 0.00); Lantern
Dock's ring turns steadily on `motion sheet --crop lantern_exterior`; at the bright end
of its pulse the ring shows 804 pixels brighter than 220 against 284 at the dim end.
Flying west for a second, stars matched between frames scroll at 0.20-0.25, 0.55-0.60
and 1.05-1.15 pixels per world unit, the three layers.

## Radio and sound

Each station broadcasts a channel (`channel` lines in `start.toml`): Lantern Dock
radio and Cinder Kiln radio, each reaching 80 units, with full signal within the inner
30% of that. In flight the radio sits at the bottom right: the channel's name, its
signal as bars and a word that always agree (`|··  Faint`, `||·  Fair`, `|||  Strong`),
the station's line on a ticker, and Prev, Off (On when off) and Next · R. Prev and Next
step through the channels in range and then Off; their look never changes. Docked, the
radio hides and its music fades out under the station's own sound. Reopen a save
taken in flight and the tuned channel's track fades in again on its own.

Once a second the radio measures every channel (`scripts/radio.luau`, rules in
`scripts/signal.luau`). It keeps the channel it has until another is at least 0.25
stronger for three seconds running, or until it has been out of range for five
seconds, so it never flickers: over the rocks both stations come in about evenly and
the radio keeps whichever it had, and near a station its own channel takes over once.
A brief gap keeps the channel (at least `Faint`) and never flashes `No signal`. The word
moves only once the signal is 0.05 past a threshold. Coming out of a station or starting
a game, it tunes straight to the strongest station. A channel chosen with Prev, Next or
R holds until it is out of range. The ticker shows each of the station's three lines
for twelve seconds. A change of channel crossfades the music over three seconds;
turning the radio off fades it over two.

The music is cued by recorded events (`radio_lantern`, `radio_kiln` and `radio_off` in
`audio.toml`), so every save and every player hears the same tracks; nothing is
generated while playing. Each station's track was made once with `galaxy-gm music`
and brought to a quiet loop with the audio tools: trimmed before its fade-out, set to
-23 LUFS and looped with a two-second blend, stored as 16-bit WAV in `music/` (3.2 and
3.7 MB; the audio tools write only WAV, and a compressed file would be about a tenth). The
reach's ambience is the tug's low engine hum with a slow beat over a faint cabin hush;
Lantern Dock's deck has soft low ventilation, a low hum and the lamps' warm buzz;
Cinder Kiln's a banked furnace roar, a low drone and a slow bellows breath. They are
authored 16-second loops in `sounds/`, chosen by the player's place.

Measured in the real mix (`galaxy-gm audio mix`, specs in `audio/`): in flight the
music plays at -26.2 LUFS and the hum and hush at -37.8, eleven LU under it; docked,
each deck plays alone (Lantern Dock -33.3 LUFS, Cinder Kiln -33.1). No mix comes near
clipping (peaks -13.5 dBFS in flight, -20.3 docked). Every loop's seam as played steps
by at most 1.9 dB.

## The station up close

Docking glides in from the ship's view until the station fills the screen: its
deck, tower, berth (with your ship moored), crane, rim lamps and a skiff ferrying
across the deck. The station is the menu. Under the pointer a building lifts, takes
an orange outline and names its keeper and what is offered there, and the pointer
becomes a hand; clicking the building opens its room. Each building also carries a
sign you can press, the station key along the bottom reaches the same rooms, and
Launch sits at the bottom right.

Opening a room is a cut: about a second's dip to a title card naming the place
(`Exchange · Lantern Dock`), and the room is there; a click lands it at once.
`Station [B]` cuts back out the same way. Each room is a place of its own, a
staged set built from the module's recipes (`recipes/*_room.toml`): a floor,
walls and furniture, open at the front, seen from one fixed camera that frames
its people on the left, with the room's panel on the right. The rooms are the
exchange (counter, jar shelves, the crate stack), the office (desk, notice board,
porthole), the galley (stove with steam, serving counter, a table) or its building
site, and the fitting cradle (the ship on its pad under a hoist, a bench and a
parts rack). Lamps flicker, steam rises, a fan turns. Inside a room only that
room shows: the status panel, title, key, log and launch step aside and come back
when you leave. Opening a room, going back and opening the log each end whatever
is being said. Nothing is lost: an unfinished conversation begins again at a
click, and an unanswered offer is still on offer.

The station's people live there. Each keeper stands in their own room and has a
day of acts (`start.toml`, `engine.life`): Tova behind her counter, then stacking
crates, then counting jars; Bram at his desk, posting prices, watching the berth;
and so on. The day moves on with the work done at a station, a trade or a fit,
never with time, so after a sale you watch Tova get up from the counter and walk
to the crate stack. A walk you leave in the middle finishes while you are away.

- **Exchange**: your hold and credits, then one line per good with its price
  and stock as plain text. A good the station buys gets `Sell N for X cr` when
  you carry it (with `[Enter]` when it is the only one; `Sell everything here`
  appears when there are several). A good it sells gets `Buy 1` and `Buy N`,
  the most that fits and that you can pay for. When buying can't happen now,
  `Buy 1` stays in place drawn resting (a dashed frame on the panel, black text)
  with its reason beside it: `Hold full: sell first.`, short credits or an empty
  stock. Hovering it gives the reason too, and a click on it does nothing. Each
  trade shows its receipt in the room and in the log. The crate pile on the deck
  and the crate stack in the exchange follow its ore stock.
- **Office**: every station's posted prices, what a station is building and how
  far it has got, and how much ore the field still holds.
- **Galley site** (Lantern Dock): what will be built, how much plate is in, and
  what it will bring. Once built it is the **Galley**, kept by Ines Varga, who
  sells rations.
- **Fitting cradle** (Cinder Kiln): Orrin Vale fits parts to the ship (below).

Inside a room its keeper wears a name sign with what they are doing
(`Tova Reyes · stacking crates`), and the open log hides it; while someone speaks
only they wear theirs, so a bubble always shows whose words it holds. Station
names and room names never share a word.

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
The sale that completes it turns the scaffold into the galley, brings Ines into
stand by it with her own introduction, and opens rations, a new good and a
return cargo for the Kiln run. The Kiln's exchange posts its rations price from
the start, and Sefa mentions her crews would pay for them.

Measured on the headless route `tests/trade.inputs` (seed 7): one hold of ore
sells for 97 cr at the Kiln and 73 at Lantern Dock; the three plate runs earn
25, 22 and 13 cr as the Kiln's plate stock runs down faster than visits restock
it; a hold of rations earns 24 cr. The galley is built on the third plate
delivery, about a minute of play after the first sale (88 seconds in the click
route), with 157 cr in hand; the route ends at 181 cr.

## Fitting the ship

The fitting cradle stands on Cinder Kiln's deck from the start, with three parts on
its rack, priced, so the first visit already shows what to aim for:

| Part | Socket | Price | Does | Power | Mass |
| --- | --- | --- | --- | --- | --- |
| Cutting lens | Drill mount | 90 cr | mines 2 ore a second instead of 1 | 2 | 1 |
| Hold pods | Hold | 70 cr | the hold carries 9 instead of 6 | 0 | 3 |
| Tuned thrusters | Engine | 110 cr | top speed 25 instead of 20 | 2 | 1 |

The hull has 3 power: whatever is fitted always runs, so a part that would draw
more is refused, and the lens and the thrusters can't run together. It carries 3
mass freely; each unit over costs 10% of top speed and turn rate (never below
half), so the pods with anything else make the ship slower with wider turns. The
control rack holds the controls the ship flies by (the Mine and Dock orders): they
are always fitted, cost no power or mass and never come off. The rules are
`scripts/fit.luau`; the room is `scripts/cradle.luau`.

The room shows the budgets in plain words (`Power 2 of 3 in use, 1 free`, `Mass 4:
the hull carries 3 freely. 1 over: 10% slower, wider turns`) and the ship's numbers
(hold, ore a second, top speed, turn rate); their hover names the controls. Below are the three sockets and the
parts tray, each part under its own socket. A card in the tray gives its name, its
price (or `In your locker`), what it does and what it draws and weighs; its hover
says the effect in words. Drag a card onto its socket to fit it: a part not yet
yours is bought first. Drag a fitted part onto the tray to take it off into the
locker; fitting it again costs nothing. Clicking works the same: `Choose` under a
part, and its empty socket (or the tray, for a fitted part) offers `Fit here` (or
`Take it off here`), or says `Cannot:` with the reason, and the budgets give way to what it would do
(`If you fit the cutting lens: 90 cr · mines 1 to 2 a second, better · power free 3
to 1`) before anything changes. The cradle's panel fits a 1024x576 window. Picking a part up tints every target at
once, sky blue with `Drop here` or vermilion with `Cannot drop:` and the
reason in full: the wrong socket (`it goes in the drill mount`), power (`it needs
2 power; 1 is free`), credits (`it costs 90 cr; you have 40`), or cargo that
wouldn't fit once the pods are off (`you carry 9; the hold would take 6`). A
refused drop returns the part and changes nothing. Every socket keeps one size and
the tray keeps its row, so nothing moves while a part is held.

A fit gives one receipt under the room's title (`Fitted the cutting lens.` with
the credits spent, and a `CONFIRMED` tag beside the title) and one log line with what the part does; Orrin's next line is
flavour about it (`That lens cuts twice as quick.`), never the receipt again. His
bubble opens beside him, away from the room's panel. Fitted pods show on the hull.
The ship's fit is world state (`fit` on the ship, `part` on each part), so it
saves and resumes.

Measured on the headless route `tests/fit.inputs` (seed 7): the beam cuts once a
second, so a hold of 6 takes six cuts bare and three with the lens, and 9 with the
pods take five. From launching at the Kiln to docking at Lantern Dock takes 2.8
seconds bare or with the lens, 2.9 with the lens and pods (top speed 18, turns 162° a
second) and 2.6 with the thrusters. The
first Kiln sale (97 cr) pays for the lens; the second for the pods.

## The keepers talk

Click a keeper and they speak in first-person bubbles, one short sentence per
click. The bubble's heading is their name, with what they do under it in smaller,
lighter type (`Exchange keeper`, `Dockmaster`, `Cook`, `Fitter`); the station is left out
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

The screens follow the direction doc's screen language (`style.toml`), all with pure
black text on light surfaces: parchment panels with a warm ink border and rounded
corners for rooms, the status chip, the station plate, the log, the radio and the hint
strip; mustard for anything you can press or pick up (orders, signs, the key, Sell,
Back, Launch, the log button, the part cards); the panel with a dashed frame for a
button that rests; white bubbles for speech; lantern-gold name signs on the people who
speak; and cream paper for loose labels. Status sits top right with credits first, the
small log button (`Log · L`) bottom left, the station plate top left when docked
(the station's name, `Unclaimed`, as no faction holds a core station yet, and `Docked
here`), the radio bottom right in flight, and one strip at the bottom in flight: the
latest message, `Space · Mine` beside the one flight key still to learn (`W · fly
ahead`), and `Dock at` either station, its posted prices on hover. In flight each
station wears its name under it while it is in view, a `yield` label like the
traders'. A trading room is a table with the columns Ware, Price each, Stock and Hold,
each good's verbs under its row. Each log entry carries its kind as a word (`TALK`,
`SHIP`), and each receipt a `CONFIRMED` tag. Amounts are black like all text; a gain
or a loss shows by its sign and its word (`Paid`, `Spent`). The engine's screen style
has one panel surface and one text colour, so the dark status chip and hint strip of
the direction wait on the engine, and so do a book icon for the log button, an amber
name mark on bubbles and a blue or vermilion mark beside an amount. Prices and stock are plain text, never
buttons, and so are the fitting room's labels (`Empty`, `On the ship`, the sockets'
names).

## The look

The core is drawn by the direction doc's art direction (`style.toml`, `recipes/`):
dusky plum space from `#4e3672` to `#2a1f48` with sparse warm stars and soft rose,
amber, teal and violet haze, warm plum ink (`#2a1a2e`) round every model, cel bands
with violet shadows and a warm rim light, under the slice's low key light (about 31°
up), so rocks and buildings turn a shadow side where the violet and the hatching show. Ink weight follows the kind of thing:
stations 4.5 logical pixels, ships 3.5, rocks 3, props (keepers, crates, skiffs, berth
rings) 2.5, with hatching heaviest on rocks. The ironstone is rust-brown with warm
amber cut faces. The tug is coral with mustard wings, a mustard dorsal stripe, a glass
canopy and a teal tail pennant; Little Mercy is teal with a mustard stripe and Quiet
Lamp cream with a midnight stripe, each with its pennant. The stations are cream with
their owner's trim: Lantern Dock round, with lantern-green trim and warm gold, on a
timber deck; Cinder Kiln square, a block with four corner kilns round a chimney, with
brass trim and deep red caps, on a fired-clay deck. Each carries its mark on its dock
arm, face up: Lantern Dock a lit gold lantern on a green disc, Cinder Kiln a brass
flame on a deep red square; the tug wears Lantern Dock's lantern on its left wing.
Little Mercy's stripe runs down its spine over the cargo capsules. Its exchange and office are the
same buildings in its own paint (`recipes/kiln_exchange.toml`,
`recipes/kiln_office.toml`). Every collision outline is unchanged: the stripes,
pennants and marks sit inside the footprints.

Hints teach and then retire (`scripts/teach.luau`). The strip shows one key and what
it does at a time (`E · dock at the nearest station` once the hold is full, else W,
A or D, Shift, E, and 0 while an order runs), each until it has been used three times.
Messages report what happened and leave the keys to the strip. A room stops saying "Click Tova to talk" after three
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
  the finished galley replaces the scaffold. A fit changes the budgets and the
  ship's numbers, the lens fills the hold twice as fast, and fitted pods show on
  the hull. The ship log keeps a line for each docking, trade, fit, launch, full
  hold, the galley built and each conversation. Empty sales never create money,
  and a trade or fit that can't happen does nothing.
- **Shocks come with opportunities.** Exhausting a rock leaves other rocks to
  work and both buyers available. A full hold is an invitation to trade. There
  are no damage penalties or scripted shocks in this first loop.
- **No fake urgency.** Prices move only with trades and counted visits, never
  with time. Nothing expires while the player looks around. Mining produces one
  unit per second (two with the lens) once within seven world units; the hold
  (six, or nine with the pods) bounds each trip.

The slice supplies the calm mining/trading feel, warm inked machinery, readable
consequences and the agreed station design. Its Rust, progression, factions and
economy code are not copied; the price formula, recount rule and growth from
delivered goods follow the slice's behaviour. All models come from committed `recipes/` through
`galaxy-gm model make game/modules/core`; the generated glTF files are
committed. The engine owns flight, collision, steering, travel, walks, talk,
hovers and the sound mixer. Luau handles player intent, the traders' orders and their labels,
once-per-second extraction and skiff trips,
trade rules and pricing, the galley's build, the ship's fit, the radio's tuning, the log
and the module-styled screens. Each station is one place
whose set is the station up close; its buildings and skiff stand in it, and each
of its rooms is a place inside it with its own set, where its keeper lives.

## Verify

Check and test the shipped module collection in place. Standalone examples live
under `game/examples/`, outside that collection:

```sh
galaxy-gm check game/modules/core
galaxy-gm test game/modules/core
```

There are thirty-three Luau rule tests (hold, depletion, prices against stock and their
bounds, unit-by-unit selling and buying, what a purchase is bounded by, visits
settling a market, quick taps not counting, hints, keepers' lines, and in
`tests/fit.luau` the bare hull, parts adding up with the mass cost and its floor,
every rule's refusal and its words, the budgets in plain words, mining two a
second, and an old save's bare hull keeping its old flight, and in `tests/labels.luau` where traders' labels are offered: near the ship and
far off alike, never over a station, while moored or while docked, both of a close pair
(the engine's `yield` keeps them apart), and on the side away from your ship, and in `tests/signal.luau` the signal by distance, bars
matching their word, the shown level holding on a threshold, two even stations never
flickering for a minute, a stronger station taking over only when steady, a brief gap
keeping the channel, a chosen channel holding, and Prev and Next stepping through) and
twenty-one scenarios: the radio (`radio.toml`: Lantern Dock's channel in the first
second, kept while a hold is mined at the first rock, the music stopped on docking at
Cinder Kiln, the Kiln's channel straight away on flying off there, and Lantern Dock's
taking over once beside its berth; `radio-tune.toml`: R to Cinder Kiln held by hand
out past Lantern Dock, Off and On, Prev, R past the last channel to off and on again,
and R doing nothing docked; `radio-hot-load.toml`: a hot load's fact in flight
cueing the playing channel once more; `radio-resume.toml`: a resume in flight cueing
it once more, and one with the radio off or docked cueing nothing), both complete station trips, collision under held thrust (stopped on the
rock's surface, then glancing off round it, the rock unmoved but turning 3 degrees a
second), flying straight off the
dock (`undock.toml`: Shift alone stays moored, W leaves on the tick it is pressed facing
away from the station in a straight line and coasts to a stop in a second, D leaves
Cinder Kiln turning), traffic (`traffic.toml`: Little Mercy waits, sets off, is bumped
aside by the ship and still moors off Cinder Kiln; Quiet Lamp moors there, flies back
and moors off Lantern Dock; their arrivals never dock the player), refused
out-of-place actions, repeat trips through depletion with an empty-sale check,
the station itself (rooms from signs and M, refused sales outside the exchange,
a building of another station, the log held shut inside a room, keeper talk, the
sale receipt and log lines, the skiff and launch), talk (an introduction once,
one line per click, the repeat line following the hold and the sale, one log
entry per conversation with no repeats, talk ended by launch, and hints
retiring), one conversation (a sale in the middle of an introduction leaves it one entry, and the speaker keeps their sign while their bubble shows), trade
(buying refused with a full hold, ore and plate priced unit by unit, plate into
the galley, a quick redock tap that is not a visit, the site trading nothing
before it is built, visits restocking, the galley built with its keeper and her
sign, rations bought and sold, visit and docking counts), back (a building clicked cuts into its room, Esc, right-click and B cut back out to the station, Esc shuts the log, signs worn only in their keeper's own room and resting while the view moves, launching from a room), and
Ines's gift taken (one ration and its receipt, her follow-up line, one log entry
with the answer, the offer gone), declined (nothing changes hands, a late second
answer is stale, the offer gone), waiting while the hold is full, and going back
mid-talk (Back on her unanswered offer and on its first line ends the bubble and
keeps the offer, a late answer to it is stale, the third time it is taken and
logged once; inside a room the log stays shut, so it ends no bubble), and
the fitting loop (`fit.toml`: the rack stocked and priced on a new game; a part that
can't be paid for, in the wrong socket or short of power doing nothing; the lens
bought and fitted with one receipt and Orrin's line, taken off (the flight staying
the locked preset) and fitted again for nothing; the next hold mined two a second; the
pods making the hold 9, the ship 18 and 162° and its model the podded hull; the pods refused coming off with nine aboard,
then taken off once sold). Play records `engine.place_arrived` when a cut or glide lands; scenarios give it by hand where a check needs the view settled. `loop.inputs`,
`kiln.inputs`, `repeat.inputs` and `trade.inputs` are equivalent headless
routes, and so is `fit.inputs`. Seven bots use real screen clicks: `play.inputs` docks (the view glides in), points at and clicks the exchange building (a cut into the room), talks to Tova at her counter, lands the office's cut with a click and talks to Bram at his desk, goes back with Esc and with a right-click, shuts the log with Esc and takes twelve notes (`play.bot.toml`, 1110 ticks), and
`trade-play.inputs` plays the whole trade loop through the galley and Ines's gift
with twelve notes (`trade.bot.toml`, 4150 ticks), and `back-talk.inputs` plays
that loop up to Ines's offer, then goes back with Esc on the unanswered offer and
with a right-click on its first line, takes it the third time, goes back out with B while Tova speaks in her exchange, then comes back in and hears her again (`back-talk.bot.toml`, 3990 ticks, thirteen notes), and `fit-play.inputs` plays the fitting loop with real drags
(`fit.bot.toml`, 2340 ticks, thirteen notes): the cradle on the first visit, the lens
held with no credits and every socket's refusal, a click on the resting Buy 1 with a
full hold (reported in `resting_clicks`, nothing bought), the lens fitted, the
thrusters refused for power while held, Orrin's talk, mining with the lens, the pods
fitted over the free mass, and the pods refused coming off with nine aboard, and
`flight.inputs` flies with the traffic (`flight.bot.toml`, 720 ticks, nine notes): across
Little Mercy's route with her label showing, bumping her aside (her label below her,
clear of your ship), pointing at her, docking
at Lantern Dock, flying straight off with W, coasting to a stop and mining from the
rock's surface, and `radio-play.inputs` plays the radio (`radio.bot.toml`, 1560 ticks, ten
notes): Lantern Dock from the first second and through mining, the ticker's next line,
Next, Off and On, Prev and R clicked and pressed, docking at Cinder Kiln with the radio
hidden, and flying off onto the Kiln's channel, and `zoom-play.inputs` flies with the
traffic zoomed fully in and then fully out (`zoom.bot.toml`, 480 ticks, six notes):
Little Mercy off screen with no label drawn, her label yielding while it would sit on the
message, named beside the ship clear of the HUD, then named far off zoomed out, Quiet
Lamp by Cinder Kiln and Mercy moored with none, and the rocks and rings turning with
nothing pressed. All seven declare 1280x720 physical pixels. The GM test
report includes their frame times, dimensions, problems and deterministic replay
results. On a display scaled to 125% that window is 1024x576 logical pixels, and
all seven run there without problems.

The sound has its own numeric checks: each bed's loudness, peak and loop seam, and the
mix in flight and docked at each station (`audio/*-check.toml`):

```sh
galaxy-gm audio check game/modules/core/audio/flight-check.toml
galaxy-gm audio mix game/modules/core/audio/flight.toml
```

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
galaxy sim run --modules game/modules/core --seed 7 --ticks 840 --inputs evidence/loop-resumed.inputs
galaxy play --modules game/modules --load evidence/room.db --ticks 700
```

The first tick after a resume starts with the engine's `engine.resumed` input, so a
resumed save ends on the hash of an uninterrupted run given that input on the tick
after the save. `evidence/loop-resumed.inputs` is `loop.inputs` plus:

```toml
[[input]]
tick = 651
name = "engine.resumed"
source = "resume"
```

Without it the uninterrupted run's hash differs, as it should. Every comparison below
adds that input on the tick after its save. The tick-650 save is inside Lantern Dock's
exchange with six ore aboard. The same holds for a save taken just after the galley is
built: run `trade.inputs` to tick 2475 with `--save`, load it for 365 ticks, and
compare with an uninterrupted 2840-tick run; the galley, Ines and her sign come back
with it. A save taken in the fitting cradle with the lens and pods fitted
(`fit.inputs` to tick 935) resumes for 525 ticks to the same hash as an uninterrupted
1460-tick run, and reopens in the window with both parts on and the hold at 9. A save from before
the cradle existed still loads and plays, without the cradle; start a new game to
see it. A save from before the traders likewise loads and plays without them, and its
ship keeps the old, slower flight numbers; start a
new game for both. A save taken while a trader is mid-route, mid-bump or moored resumes
to the same hash as an uninterrupted run, and so does one taken with the radio tuned by
hand (`radio-tune.toml`'s inputs to tick 650, resumed for 550). A save from before the
radio loads and plays without it; start a new game for the radio. A save taken mid-hold, the rock
turning under the ship (a Mine order at tick 1, saved at 150, resumed for 250), resumes to
the same hash as an uninterrupted run. A save from before the rocks turned loads and
plays with its rocks still; its rings, lamps and stars move anyway, as they are drawn only. Resumed or hot
loaded in flight, the radio cues the channel it was playing once more, so the same
track fades in without input; docked, off or with no signal it cues nothing. Bot
input ticks and play's `--ticks` are absolute world ticks, including when resuming.
The scenario schema cannot express save/load steps, so these checks run through the
installed CLI separately; `radio-resume.toml` gives the resume input directly.

## One self-critique against the slice

The trade loop is the thinnest one: three goods, one thing to build, one
milestone. Plate is the only good that builds anything, so after the galley
opens nothing else grows, and the Kiln's plate margin shrinks run by run until
visits restock it. The galley's sign sits low on its front so it clears the
dockmaster's name sign.

The fitting loop is the thinnest one too: one fitter, three parts on one rack, one
part per socket and no hulls, blueprints or parts ordered from friends. The fit is
instant, with no crane or fitting moment. While a part is held, a socket that will take it says `Drop here` with the part's
cost and effect, but not the whole change (the mass cost of the pods); only the
click way previews that, in place of the budgets. The cradle's sign
sits low beside it, on the rim side, so it clears the dockmaster's and Orrin's
name signs at both window sizes.

The traffic is the thinnest living world: two traders on fixed routes that never
trade, talk or change anything, so nothing out in space reacts to what you do.
The rocks turn, the rings turn and the lamps breathe, but on fixed clocks: nothing
drifts, flickers or reacts to the ship passing. A trader's label sits a fixed number of
pixels from its anchor, so zoomed right in it overlaps the top of its own hull.

The station view is now the agreed design rather than a deck and a separate
market room, and the two stations differ in their shapes and paint. Docking
glides in, buildings lift and outline under the pointer and open their rooms when
clicked, each room is a staged set its keeper lives in, and Esc and right-click go
back. The deck itself has no one on it now: everyone is at work indoors. The rooms
are lit by the same light as space, with their lamps and walls glowing rather than
casting light, and their days move only with trades and fits. The key has no news
ribbons because nothing at a station changes on its own yet.
The radio is the thinnest one too: two stations, one track each (an 18- and a
21-second loop), three lines each, no lane beacons, no static at the edges and no
news. Its ticker swaps to the next line without a fade, the engine hum doesn't yet
follow thrust, and the radio shares the status panel's look; each waits on the
engine. Ines's ration is the only offer so far, and
the only thing anyone gives. Further art and pacing changes wait for a human
playtest.
