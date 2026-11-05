# Core: a quiet working shift

Every change here follows [the direction doc](../../docs/direction.md); compare your screenshots with its reference images.

Fly a small tug, cut ironstone, ice and rare glassite from the rocks of the reach, dock
at either of two stations and sell it, then trade between them: Cinder Kiln turns ore into plate,
and Lantern Dock wants plate to build a galley. Once the galley is built, its
cook sells rations that the Kiln's crews buy. The hold takes six units of any
mix. At the Kiln's fitting cradle, Orrin fits parts bought with what you earn: a
lens that mines faster, pods for a bigger hold and thrusters for a higher top
speed, within the hull's power and mass. Five traders fly their own routes across
the reach while you work, and in flight the radio plays each station's channel
over the tug's quiet hum. Out in the reach the rocks turn slowly, the stations' rings
turn and their lamps breathe, steam drifts off the stacks, and the stars drift in layers
as you fly. The field holds 84 ore, 100 ice and 9 glassite, and a rock cut down to
pebbles grows back to half on your next counted visit to a station; there is no timed
objective.

You fly, aim, cut and dock by hand: fly up to a rock, point the nose at it and hold
Space, and the cutter's beam bites, chips fly back up it, and each unit lands in the hold
with a puff and a count. Fly slowly into a station's glowing berth ring and its clamps
take you in. Two controls are earned at the cradle and change what a press does: the
docking jets fly you into a berth from anywhere, and the reach rig flies out to a rock and
cuts it from sixteen units off, aiming itself. The hand controls never go away.

Use installed engine/GM 0.8.0 or newer: the screen kit needs its named looks, rich
text spans, drawn icons and UI motion, and the world needs the effects (beams,
`engine.emitter`, `[particles.*]`, halos and camera feel), which no 0.7.0 release has.
This version was verified with `0.8.0-aa24600ae2cd-dev`; an older engine is refused with
how to install a newer one. No engine build is needed.

```sh
galaxy play --modules game/modules
```

| Control | Action |
| --- | --- |
| W / S | Forward / reverse thrust; let go and the ship coasts to a stop in a second |
| A / D | Turn left / right, 180° a second |
| W, A, S or D docked | Fly straight off the dock: no separate undock step |
| Shift | Boost |
| Space (held) | Cut the rock the nose points at, within 7 units; let go to stop. In a berth's ring: dock |
| E | With the docking jets: fly into the nearest berth. Without: says how docking is done |
| 0 | Cancel what the jets or the rig are flying |
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

Everything is also a click, on the thing itself rather than a column of buttons. In
flight, click a rock in the cutter's reach to put the beam on it without holding Space
(click it again, or fly, to stop); a rock out of reach says so. Hover a station for its
posted prices and how to dock there; with the jets fitted, a click on it flies you in, and
with the rig fitted a click on a rock flies out to it and cuts. Docked, the station's
buildings and their signs, the station key, Sell, Buy, Back, Launch, the log and the
radio's Prev, Off/On and Next are clicks too. Click a keeper to hear them. Thrust or
turning lifts the beam and drops whatever the jets or the rig were flying. Rocks, station
hulls and the traders are solid; the jets and the rig steer toward their targets, so fly
by hand to clear an obstruction when approaching from the far side of a station.

## Mining and docking by hand

The cutter sits in the nose and reaches 7 units from it (`rules.REACH`). Space held cuts
the rock the nose points at: aim counts while the nose points within 12 degrees of the
rock's edge (`rules.off_aim`), and turning further lifts the beam until the nose comes
back. The beam (`engine.beam` on the ship, its look picked by the gap: `cutter_near`,
`cutter` or `cutter_far` in `style.toml`) runs from the nose to the rock's drawn surface,
with sparks and dust where it bites and chips flying back up it. The first unit comes
loose half a second in and one every second after (two with the lens); the time counts
however it is broken up, so turning off and back on loses nothing. Each unit throws a
chunk of stone from the cut to the nose, an amber puff at the hold and a `+N` beside the
hold's count. The sixth unit (ninth with the pods) is the hold's own beat: a bigger puff,
a small camera kick, and the beam lets go. A rock cut to pebbles crumbles with its own
burst and kick, and the log says how many rocks still hold ore, before the full hold
when the same unit fills it.

A station's berth is the glowing ring off its dock. Fly into it and stop (8 units a second
or slower) and the clamps take you in with a puff of vapour; Space in the ring does it at
once. A ring you have just launched from takes you only once you have left it. The
ship's exhaust follows the thrust, and the view leans a little ahead as you speed up
(`engine.camera_feel`).

When every rock is cut down, the message says what you carry and who buys it, or, with an
empty hold, to dock at either station and come back: fresh stone drifts in while you are
docked (`rules.worked_out`, `rules.regrow`).

Two controls are earned in the cradle's control rack (`start.toml`, socket `rack`):

| Control | Price | What a press does once it is fitted |
| --- | --- | --- |
| Docking jets | 80 cr | E, or a click on a station, flies the ship into its berth from anywhere |
| Reach rig | 140 cr | Space, or a click on a rock, flies out to a rock and cuts it from 16 units off, aiming itself; its beam is long and cyan (`beams.reach`) |

The first Kiln sale (97 cr) pays for the jets; three holds pay for the rig. A bought
control is never taken off.

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
every mining spot stays where it is; a rock only turns slowly in place. A scripted Mine
(a test's or a bot's) parks the ship on the rock's drawn surface (`range = 0`), and the
engine's finished `hold` keeps it there while the rock is cut smaller and turns; the reach
rig holds nine units off instead.

Five traders work the reach (`scripts/traffic.luau`), each its own hull and livery:

- Little Mercy, a hauler, runs along the south between moorings off Lantern Dock's and
  Cinder Kiln's far sides.
- Quiet Lamp, a lamp launch, flies the long way round the north of the field between the
  stations' outer sides.
- Tern, a courier, runs the buoyed lane straight across the reach (x = 5, north to
  south), crossing the player's runs between the berths and the ore. It rests at the
  north buoys first and sets off about half a minute in.
- Cold Kettle, an ice barge, hauls slowly from the ice camp beyond the west drift to a
  mooring off Lantern Dock's south-west.
- Pell's Luck, a prospector's tug, runs from the glassite out north-east to a mooring off
  Cinder Kiln's far east side.

 Each flies its leg's waypoints without stopping, docks
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
a trader always names it and its run. Labels and pointing wait while the view still
glides out after a launch: from a room it starts low among the moored traders.

Measured on films of the ship's probes (`galaxy film --probes-only`, then
`galaxy-gm film show`): turns at 180.1° a second; top speed 20, 95% of it 0.67 seconds
after W; letting go at full speed coasts 9.7 units and stops 1.0 seconds later; in a
90° turn under thrust the course lags the nose by up to 27° and tracks it again a
third of a second after the turn ends. The mining approach parks 0.004 units from the
rock's surface and stays within 0.05 of it for the whole hold (12.9 seconds, six cuts,
the rock turning), ending 0.004 away.

## The field and the sky

The reach is laid out in drifts (`start.toml`), each its own material with its own
silhouette and colour, in several sizes (`engine.model` scale; a bigger rock holds more):

- **The ore drift** round the middle: rust-brown ironstone in three shapes (`rock`, the
  long `rock_lump` and the blocky `rock_chunk`), seven rocks of 8 to 18 ore. rock1 stays
  the nearest rock to the start and to both berths, so the first Space finds ore.
- **The ice drift** out west past Lantern Dock: eight pale mint-cream floes (`ice`, the
  tall `ice_shard`) with blue-violet shadow faces (`looks.ice`), 8 to 20 ice each.
- **Glassite**, far out north-east: two dark violet rocks bristling with glowing lilac
  crystals, 5 and 4 units. As a rock is mined its crystals break off and wear to stumps:
  `ore.stages` names its four models (`glassite`, `glassite_chipped`, `glassite_worn`,
  `glassite_stubs`), each taking an equal share of the rock's units, while the rock part
  they all share keeps shrinking through `engine.model` morph. Once it is empty it becomes
  `ore.empty`, `glassite_pebbles`: a few dark pebbles with warm glowing caps.
- Pebbles and ice splinters (`debris`, `debris_ice`) and faint drifting dust
  (`[particles.dust]` from three `engine.emitter` points) round the drifts: drawn only,
  never solid, never mined.
- Lane buoys (`buoy`) blink along the courier's lane and at its moorings.
- Far off, past where the ship can fly: a ringed planet sunk well below the field (its
  model sits 60 units down, so it draws behind everything) to the north-west, and a
  lighthouse on the reach's eastern edge. Both wear `looks.far`: faint ink, two bands and
  a shadow close to the sky's violet, so they sit back in the haze.

Each rock yields its own ware (`ware` on the `ore` component; ore by default), and Space
mines the nearest rock with anything left. Ice and glassite are wares like ore: Lantern
Dock buys ice, Cinder Kiln buys glassite (below). The office and the dockmaster count
ore only.

The sky (`[sky]` in `style.toml`) has 300 stars, mostly tiny, in four parallax layers,
and 16 soft clouds, mostly violet with rose, amber and teal patches. Behind the field it
has depth drawn in ink, never as a photographic starfield (`[[sky.layers]]`): long plum
hatching that barely creeps, rose and violet dust a little nearer, shorter violet
hatching crossing it in the middle distance, tiny peach specks and, nearest, a sparse
lantern-gold dust that slides past faster than the stars, so speed reads. Far away
(`[[sky.landmarks]]`) a great dusky planet's limb rises out of the bottom-left corner,
banded and hatched in its violet shadow; rose and teal nebula washes streak across the
left; and another station hangs deep in the haze to the north, out of reach. Everything
far fades into a violet haze (`haze`, `atmosphere`) by how far back it sits, so it stays
low in contrast and darker than any hull, and the backdrop never looks busy.

The stations read as harbours, each in its owner's shape language:

- **Lantern Dock** (`recipes/station.toml`) is round and hand-built: a turning spoked
  ring with eight lamps, a three-tier core under a lantern cap, a greenhouse dome
  glowing leaf-green, a green water tank, four lantern posts round the ring, a mast with
  pennants, a banner on the core, and a dock arm with a lamp string, a crate pile and a
  barrel. A little steam rises off its kettle cap.
- **Cinder Kiln** (`recipes/kiln_station.toml`) is square and ordered: a still brass quay
  frame, four deep-red kilns whose mouths glow round a tall chimney that breathes steam,
  a fan turning on the stack, corner lamp posts with red banners, a tank farm, and a
  dock arm with a toll gate and crates in neat rows.

Both are drawn at 1.35 times their model's size. Their collision outlines stay clear of
the berths and the traders' moorings: Lantern Dock reaches 7.3 units to the front and
sides and 12.4 back along its dock arm, Cinder Kiln 5.6 and 12.2.

Lamps, windows and engines glow (`halo` on the station, ship, buoy, glassite and far
looks). Every ship leaves a short trail while it flies (`engine.emitter` with
`[particles.exhaust]` on the player's ship, `[particles.wake]` on the traders), and
steam (`[particles.steam]`) is cream cel puffs that swell and drift off.

## The reach moves on its own

With nothing pressed the reach is never quite still, and everything that moves on its
own is slow and quiet beside what you can act on:

- **Rocks** turn where they float (`engine.spin` in `start.toml`: 1.5 to 6 degrees a
  second, either way) and never drift, so every mining spot stays put. Their solid outline
  turns with them, and the engine's `hold` keeps a parked ship on the turning surface.
- **Stations' rings** turn about their hubs (`spin` on the ring part in
  `recipes/station.toml` and `recipes/kiln_station.toml`: Lantern Dock 6 degrees a second
  one way, Cinder Kiln 5 the other), and the ring's lamps breathe between full and 45%
  glow every four or five seconds while the mast's tip lamp blinks down to 20% every 2.5
  or 3 seconds (`pulse`, `pulse_low`). Both are drawn only: they change no world state,
  so saves and replays are untouched. The station up close, which is the menu, stays
  still.
- **Stations** move in their parts: Lantern Dock's lantern posts, lamp string and
  greenhouse each pulse on their own beat; Cinder Kiln's fan turns, its kiln mouths glow
  and its corner lamps and toll-gate lamp pulse; steam drifts off both.
- **Stars** lie in four layers (`star_layers` in `style.toml`): far ones creep behind
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
30% of that. In flight the radio is a slim dark strip at the top left, out of the
work area round the ship: the radio icon and the channel's station, its signal as
gold bars and a word that always agree (one lit bar `Faint`, two `Fair`, three
`Strong`), Prev, Off (On when off) and Next · R beside them, and the station's line
on a ticker under them, which flashes softly when it changes. Prev and Next
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
music plays at -26.4 LUFS and the hum and hush at -37.8, eleven LU under it; docked,
each deck plays alone (Lantern Dock -33.3 LUFS, Cinder Kiln -33.1). No mix comes near
clipping (peaks -10.3 dBFS in flight, -8.0 docked as the dock clunks). Every loop's seam
as played steps by at most 1.9 dB.

Sound follows the world while the ship flies, on three loops in their own channels of
the effects bus (`[[follow]]` in `audio.toml`). The drive is a warm low hum that rises
in pitch and fullness with the ship's speed: almost nothing at rest (a tenth of its
gain, 0.8 times its pitch), cruising and then boosting higher (1.45 times at 36 units a
second). Each station's own deck sound swells as the ship passes its rim and is gone
some forty units out. A new game, a resumed save or leaving a dock starts them
(`scripts/sound.luau` records `drive_on` after a resume or a hot load, unless docked);
docking stops them under the deck's own sound. A bump against a rock, a station or a
trader knocks softly on the hull: the manifest's `contact_speed` (2 units a second) has
the engine record `engine.contact`, and `scripts/sound.luau` records `bump` when the
ship is in it, so traders bumping each other far off stay silent. Repeated cues (thrust,
the chips, clicks, blips, coins, clunks, the radio's static) each vary a little in pitch
and gain, so none sounds stamped.

Screens sound from `[ui]` in `audio.toml`, played by the engine on the frame the player
acts: hover is the faintest wooden tick (at most one every 0.08 s, so a fast sweep
thins), a press is the click, softer and lower on parchment and the radio's dark
buttons, a room's panel and the log open and close with the rising and falling note
pairs, a talk bubble opens with a blip (clicking a bubble or a person only ticks, so
the next line's blip leads), picking a part up lifts, letting it go sets it down, and a
resting button or a refusing socket answers with the soft refusal. The plate, status
chip, radio and hint strip come and go silently as the ship docks and leaves, where the
dock's clunk speaks. A panel sounds as it appears or goes, whether a key or a click
opened it; `scripts/sound.luau` cues clicks for keys only, since a click on a screen
comes from the source `ui` and already sounds.

Every action answers with a short, soft sound on the effects bus. Flying: thrust
starting and stopping (W or S) and a boost (Shift), warm whooshes with no sub-bass.
Mining: a click as the order is taken, a quiet beam hum and a chip for each second's
ore, and a two-note chime when the hold is full; ordering a full hold to mine is a soft
two-note refusal. Docking clunks on arrival and unclamps on launch. Screens: a click
for the dock order and the radio's buttons, a rising pair of notes as a room or the log
opens and a falling pair as it closes, coins and a ding for a sale, a crate and a ding
for a purchase, a stamp and a ding for a paid part's receipt, and a chime when a sale
finishes a building. Fitting: a part seats with a mechanical chunk, on or off. Talk:
a bubble blip as each line appears. Radio: a little static as it tunes a channel in.

They are synthesised by `sounds/make.py` (sines, decaying modes and filtered noise with
a fixed seed), levelled with `audio process` and measured with `audio inspect`. The
cues come from facts the game already records (`docked`, `mined`, `sold`, `bought`,
`fitted`, `grown`, the radio's channel cues and an answer chosen; a talk starting blips
as its bubble opens, from `[ui]`) and from events that `scripts/sound.luau` records once
per real change, because a key sends a fact on press and on release: thrust on and off
(W less S, so S pressed against a held W stops it and let go starts it again), boost,
undocking, a key's click, a refusal, the chips, the full hold, a talk line appearing
and a bump. A clicked line blips only when the talk really moved on, so a double click or a stale
repeat blips once; the script keeps the last line it cued in `blipped` for that. It is
first in the manifest, so it reads the world as the other scripts found it, and it
writes only those facts and that note. The
hum repeats exactly every second with 0.12 s fades, so the copy each `mined` fact
starts blends into the last and the hum holds steady while the beam cuts. The three
chips take turns so they never tick like a clock.

In the real mix the effects sit under the radio: in flight (the drive's hum and the
stations' loops included) and mining their bus is -30.6 and -28.7 LUFS against the
music's -26.4, a median 5 dB under it in the seconds
both play. Two thirds of their energy is between 300 Hz and 3 kHz, where the music has
a fifth of its own, so the music does not cover them, and almost none is above 3 kHz
or below 100 Hz. Docked, with the radio faded, they sit over the deck bed. No journey
reaches the output clamp; the busiest, a trade run with rooms, sales and purchases a
few ticks apart, peaks at -6.0 dBFS. The drive's own checks (`audio/drive-check.toml`,
`drive-rest-check.toml`, `station-pass-check.toml`): cruising, the effects bus is -34 to
-36 dBFS a second against -48 at rest; resting at Lantern Dock's rim -37, and -43 once
seventy units out. Nobody has listened to them yet.

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
`Back · B` cuts back out the same way. Each room is a place of its own, a
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
day of two acts (`start.toml`, `engine.life`): Tova behind her counter, then
stacking crates; Bram at his desk, then posting prices; Ines cooking, then serving;
and so on. One day runs for both stations and moves on with the work done there, a trade or a fit,
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

Lantern Dock buys ore, ice and plate; Cinder Kiln buys ore, glassite and rations
and sells plate; the galley, once built, sells rations (`market` lines in `start.toml`).
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
control rack holds the controls the ship flies by: the hand cutter and hand berthing
are always there, and the docking jets and the reach rig are bought from a row of
buttons under the sockets (`Docking jets · 80 cr`, resting with the reason while they
can't be paid for). Controls cost no power or mass and never come off. The rules are
`scripts/fit.luau`; the room is `scripts/cradle.luau`.

The room shows the budgets in plain words (`Power 2 of 3 in use, 1 free`, `Mass 4:
the hull carries 3 freely. 1 over: 10% slower, wider turns`) and the ship's numbers
(hold, ore a second, top speed, turn rate). Below are the three sockets and the
parts tray, each part under its own socket. A card in the tray gives its name, its
price (or `In your locker`), what it does and what it draws and weighs; its hover
says the effect in words. Drag a card onto its socket to fit it: a part not yet
yours is bought first. Drag a fitted part onto the tray to take it off into the
locker; fitting it again costs nothing. Clicking works the same: `Choose` where a
part would go from (its empty socket, or the tray's `On the ship` for a fitted
part), and its socket offers `Fit here` (or the tray `Take it off here`) while every
other socket says `Cannot:` with the reason, and a dark box beside the panel, over
the room's upper floor, says what it would do before anything changes: `If you fit
the cutting lens`, then one line per change (`90 cr`, `Mines 1 to 2 a second + better`,
`Power free 3 to 1`; a blue + for better, a vermilion - for worse), then `Put back`.
The panel keeps its keeper's line and budgets, so the click way adds no height and
the cradle still fits a 960x540 window; the longest preview (five changes) ends above
the ship on its cradle there. Picking a part up tints every target at
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
even when something else is logged while you are still listening. A line already in the log isn't logged again.

The ship log (`scripts/log.luau` keeps the entries, `scripts/logbook.luau` draws it)
is a small book button at the bottom left (`Log`, with a blue `N new` tag when
something came in since you last looked); L or the book opens it at the top left.
Pinned at the top is what is still waiting: an offer you heard and haven't answered
(`OFFER`, under who made it and where, with its answers) and what a station you have
docked at is building from your deliveries (`WANTED`: `3 of 10 plate in`). Pins
outlast the 60 entries the log keeps: a heard offer is marked on its gift (`heard`),
and a building's station counts its visits. Below, newest first, is one row per
visit: `VISIT` (or `HERE` for the one going on now) with the station and Lines, then
one short line of who you heard and what happened, each thing in its first few words
and what doesn't fit counted (`Ines Varga · sold 6 ore +2 more`, at most 38
characters). Every docking is a visit of its own, even back at the same station with
nothing logged in between: the launch's entry ends the visit (`leaves` on `entry`).
What happened in flight between two dockings is a `FLIGHT` line. Lines opens a visit
to show its entries in full and in order, and Hide closes it. Each entry remembers
where the ship was (`at` on `entry`), and the docking's own entry already belongs to
the station it docks at. The open log is 380 pixels wide, so at 960x540 it ends short
of the ship mid-window, moored or flying.

## Screens

The screens follow the direction doc's screen language through one kit: the looks in
`style.toml` (`[screen]` and `[screen.looks.*]`), the drawn icons in `icons.toml` and
`icons/`, and `scripts/kit.luau`, which every script uses to build a surface of a
kind. Each kind of thing has its own look, so a glance tells them apart:

| Kind | Look | Kit call |
| --- | --- | --- |
| Status (the HUD chip) | dark ink, lantern-gold border, white text | `kit.chip(ui, w)` |
| A key hint | dark ink pill, thin gold frame | `kit.pill(ui, w)` |
| A place (a room panel, the open log) | parchment, ink frame, soft shadow | `kit.place(ui, w)` |
| A place's title | large and heavy | `kit.heading(ui, w)` |
| The trade table | a lighter sheet in the place, ruled column heads | `kit.sheet(ui, w)`, `kit.heads(ui, { { "Ware", 160 }, ... })` |
| A receipt | dark ticket with a gold `CONFIRMED` tag (or a sky-blue `NOTICE`) | `kit.ticket(ui, w, tag)` |
| Speech | white bubble with an ink frame, the name on an amber tag | the engine's bubbles get it from `bubble_look`; `kit.bubble(ui, w)` for a panel |
| A kind word (`JOB`, `OFFER`, `VISIT`) | ink capitals on a flat cream tag | `kit.tag(ui, word, w)` |
| Buttons | primary mustard; secondary parchment; dark on the radio; resting dashed with its reason | `kit.button(ui, w, "primary" / "secondary" / "dark")`, `resting = "<reason>"` |
| A warning | pale vermilion, vermilion frame, warning icon | `kit.warning(ui, w)` |
| A gain or a loss | a black amount with a blue `+` or vermilion `-` mark (lighter tints on dark) | `spans = kit.amount(64, "cr")`, `kit.amount(n, unit, true)` on dark |
| A name and a role | the name heavy with an amber rule, the role quieter | `spans = kit.name("Tova Reyes", "Exchange keeper")` |
| A world label (a station or trader name in flight) | a small dark tag | `kit.label(ui, w)` |
| The docked station's plate | dark ink, thin gold frame, the name heavy white | `kit.plate(ui, w)`, `look = kit.LOOK.plate_name` |
| A key in a hint | the key heavy white with a gold rule, then what it does | `spans = kit.hint("W · fly ahead")` |
| A figure in running text (a price, a count) | heavy black | `{ text = "17 cr", look = kit.LOOK.figure }` |

Which look goes where: the persistent HUD (status, plate, station key, radio, hint
strip, world labels) is dark ink; places (room panels, the log) are parchment; speech
is white; the one press that moves you on (Launch, Sell) is gold, quieter verbs (Buy,
Back, paging) parchment, and the station key's icons dark. Docked, the key sits
centred along the bottom between the log's book and Launch, clear of both at 960x540.
Trading, office and site rooms, and the cradle, are `layout.ROOM` wide (45% of
960x540), so the ship on its cradle stays in view at both window sizes.

Each builder takes the screen's `ui` and a widget table, sets the kind's look and
returns the widget, so every other widget property still works; `kit.LOOK` names the
looks and `kit.SIZE` the text sizes (title 28, heading 20, body 18, small 16). Icons:
`book`, `credits` (a painted coin), `hold`, `ore`, `plate`, `rations`, `power`, `mass`,
`dock`, `radio`, `back`, `prev`, `next`, `switch`, the parts `lens`, `pods`,
`thrusters` and `controls`, and `warning`, `up`, `down`, `check`, `ship`, `station`,
`pin`. All but the coin take the ink of what they sit in. `kit.ware(ware)` and
`kit.part(socket)` give a ware's or a socket's part's icon; `kit.icon(ui, name, w)`
draws one alone. Colours never appear in Luau: a new kind of surface is a new look in
`style.toml`.

Text is pure black on light and pure white on dark; the state colours of the direction
doc only mark a shape beside the words. Motion is short and calm (`style.toml`):
panels fade in over 0.16 s with an 8-pixel rise and out over 0.12 s, a button lifts
1.5 pixels under the pointer and sinks as much when pressed, and a changed number or
line (`cue = "flash"`) flashes pale gold, or a lighter plum on dark, for half a second.
`galaxy play --reduced-motion` settles all of it at once.

Where things sit: the status chip top right (credits with the coin beside the hold
with the crate, then a row of each ware aboard with its icon; `scripts/chip.luau`
gives every piece its own width, so all five wares fit and the chip stays two rows
tall). While the beam cuts, the hold's count reads short (`4/6`, the crate says what it
is) and the units this cut has brought in sit beside it (`+2`). The log's book sits
bottom left, the radio top left in flight, the station plate top left when docked (the
station's name, `Unclaimed`, as no faction holds a core station yet, and `Docked here`),
and one strip at the bottom in flight: the latest message and the one key that fits
where the ship is. Rocks and stations are click targets: under the pointer a rock lifts
and outlines and says what a click does and how much of its ware is left, and a station
shows its posted prices. In flight each station wears its name under it while it is in
view, a `yield` label like the traders'. A trading room is a table with the columns Ware, Price each,
Stock and Hold, each good's verbs under its row, and each receipt a `CONFIRMED` tag.
Prices and stock are plain text, never buttons, and so are the fitting room's labels
(`Empty`, `On the ship`, the sockets' names). The rooms, the trade table, the hint
strip, the station key and the talk around them still wear the base looks; they move
onto the kit's looks in a later change.

Measured on films of the headless trade route (`tests/trade.inputs`, 145 flight
frames at each size, probes only): at 1280x720 and at 960x540 the ship is never
under the radio, the status chip or the log's book (closest 293 and 146 pixels, from
the radio), and neither is the rock being mined.

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
it does where the ship is (`Space · dock at Lantern Dock` in a berth's ring, `Hold Space
· cut the rock ahead` with the nose on a rock in reach, `A or D · point your nose at the
rock` beside one, the way to dock once the hold is full, else W, A or D, Shift, and 0
while the jets or the rig fly), each until it has been used three times.
Messages report what happened and leave the keys to the strip. A room stops saying "Click Tova to talk" after three
conversations. Messages report what happened, not how fast it happens.

Each piece of a screen sits against a window edge with a 24-pixel margin
(`scripts/layout.luau`), so the HUD holds at any window size and display scale:
the design size is 1280x720 logical pixels, and everything also fits 960x540 (and
1024x576, 1280x720 on a display scaled to 125%). The room cameras frame each keeper so
their bubble clears the room's panel at both sizes.

## Design rules

- **Every change is seen.** Ore arrives in the hold, the rock develops a
  visible cut, and a trade changes credits, the posted stock and price, and for
  ore the crate pile. Plate delivered to the galley site shows as progress, and
  the finished galley replaces the scaffold. A fit changes the budgets and the
  ship's numbers, the lens fills the hold twice as fast, and fitted pods show on
  the hull. The ship log keeps a line for each docking, trade, fit, launch, full
  hold, rock cut down, the galley built and each conversation. Empty sales never create money,
  and a trade or fit that can't happen does nothing.
- **Hands first, help earned.** Flying, aiming, cutting and berthing are always done
  by hand and never removed; the jets and the rig are bought and change what a press
  does, and both are visible (the long cyan beam, the ship flying itself in).
- **Shocks come with opportunities.** Exhausting a rock leaves other rocks to
  work and both buyers available, and a worked-out rock grows back on a visit. A full hold is an invitation to trade. There
  are no damage penalties or scripted shocks in this first loop.
- **No fake urgency.** Prices move only with trades and counted visits, never
  with time. Nothing expires while the player looks around. Mining produces one
  unit per second of beam time (two with the lens) within seven units of the nose; the hold
  (six, or nine with the pods) bounds each trip.

The slice supplies the calm mining/trading feel, warm inked machinery, readable
consequences and the agreed station design. Its Rust, progression, factions and
economy code are not copied; the price formula, recount rule and growth from
delivered goods follow the slice's behaviour. All models come from committed `recipes/` through
`galaxy-gm model make game/modules/core`; the generated glTF files are
committed. The engine owns flight, collision, steering, travel, walks, talk,
hovers and the sound mixer. Luau handles player intent, the traders' orders and their labels,
aim, beam time and extraction, and skiff trips,
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

There are thirty-nine Luau rule tests (hold, depletion, prices against stock and their
bounds, unit-by-unit selling and buying, what a purchase is bounded by, visits
settling a market, quick taps not counting, hints, keepers' lines, and in
`tests/fit.luau` the bare hull, parts adding up with the mass cost and its floor,
every rule's refusal and its words, the budgets in plain words, mining two a
second, and an old save's bare hull keeping its old flight, and in `tests/labels.luau` where traders' labels are offered: near the ship and
far off alike, never over a station, while moored or while docked, both of a close pair
(the engine's `yield` keeps them apart), and on the side away from your ship, and in `tests/signal.luau` the signal by distance, bars
matching their word, the shown level holding on a threshold, two even stations never
flickering for a minute, a stronger station taking over only when steady, a brief gap
keeping the channel, a chosen channel holding, and Prev and Next stepping through, and
in `tests/mining.luau` the cutter's reach from the nose, aim within the slack, a unit
for every second of beam time however it is broken up, the berth ring, regrowth to half
and the worked-out message for an empty and a full hold) and twenty-seven scenarios (two of them, `sound.toml` and
`sound-talk.toml`, under the sound below):
mining and docking by hand (`hand-mining.toml`: Space out of reach says so, the beam on
the rock on the tick Space is pressed, a unit at half a second and one a second after,
the sixth filling the hold and letting the beam go once, then turned and flown into
Lantern Dock's ring and clamped in; `hand-aim.toml`: turning lifts the beam, a quarter
turn off says to point the nose, turning back resumes with no beam time lost, a click
latches the beam and flying lifts it, a far rock is too far, and E without the jets
flies nowhere and says how docking is done), the earned controls (`assists.toml`: the
first sale buys the jets but not the rig, a bought control can't come off, E flies her
in from the reach, and after three holds the rig cuts from the Kiln's berth with no key
held, aiming itself; `rig-stop.toml`: a tap of W lifts the rig's beam and no more ore
comes in), the radio (`radio.toml`: Lantern Dock's channel in the first
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
out-of-place actions, repeat trips through depletion with an empty-sale check (the unit that works rock1
out and fills the hold logs both),
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
then taken off once sold). `log-pins.toml` leaves Ines's offer unanswered, then makes
32 quick launch-and-redock trips: the offer stays marked heard after its entry has
been dropped, and each launch ends a visit. `tests/log.luau` checks visits (a return
to the same station with nothing logged between is a new one), the short visit line,
pins outlasting dropped entries and the open log's width at 960x540, and
`tests/chip.luau` that every row of the chip fits with all three wares aboard. Play
records `engine.place_arrived` when a cut or glide lands; scenarios give it by hand
where a check needs the view settled. `loop.inputs`,
`kiln.inputs`, `repeat.inputs` and `trade.inputs` are equivalent headless
routes, and so is `fit.inputs`. Eight bots run in a window: `hand-mining.inputs` mines and
docks with the keys a player presses and clicks a far rock (`hand-mining.bot.toml`, 570
ticks, eight notes), and seven more use real screen clicks, their flights between
stations given as scripted Mine and Dock: `play.inputs` docks (the view glides in),
points at and clicks the exchange building (a cut into the room), talks to Tova at
her counter, lands the office's cut with a click and talks to Bram at his desk, goes
back with Esc and with a right-click, shuts the log with Esc and takes twelve notes
(`play.bot.toml`, 1110 ticks), and
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
nothing pressed. All eight declare 1280x720 physical pixels. The GM test
report includes their frame times, dimensions, problems and deterministic replay
results. On a display scaled to 125% that window is 1024x576 logical pixels, and
the seven click bots run there without problems, and the hand-mining route, shot there
while cutting, full and in the ring, fits too.

The sound has its own numeric checks: each bed's loudness, peak and loop seam, the
mining hum's level, and the mix in flight, mining, docked with talk, trading, fitting
and docked at each station (`audio/*-check.toml`). `tests/sound.toml` checks that each
cue is recorded once per real change, and `tests/sound-talk.toml` that duplicate and
stale clicks on a talk line blip once:

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
click way previews that, in its box beside the panel. The cradle's sign
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
follow thrust; each waits on the engine. Ines's ration is the only offer so far, and
the only thing anyone gives. Further art and pacing changes wait for a human
playtest.
