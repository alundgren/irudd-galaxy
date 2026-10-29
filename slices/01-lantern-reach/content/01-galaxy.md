# 01 Galaxy layout

Back to [SPEC.md](SPEC.md). Positions are `(x, y)` in world units on the `z = 0` plane, with `+y` north.

## Regions

A region is a disc. Stations, fields and lane mouths sit inside it; lanes run between regions through open space.

| ID | Name | Centre | Radius | Crossing at Wren speed (60 u/s) | Role in the arc |
| --- | --- | --- | --- | --- | --- |
| `region.lantern_shallows` | Lantern Shallows | (0, 0) | 900 | 1800 u, 30 s | Home: calm ice and iron, first goals, the hullwright |
| `region.cinder_drift` | Cinder Drift | (5200, 1800) | 1000 | 2000 u, 33 s | Rich, depleting belts; the first real price gaps; the dispute; the Ancients |
| `region.brassmouth_narrows` | Brassmouth Narrows | (2600, -4400) | 700 | 1400 u, 23 s | Trade hub: high glassite demand that saturates |

The map shows all three regions, their lanes and the five authored stations from the start. Prices at a station are unknown until the player docks there (see `02-economy.md`, "Snapshots").

## Lanes

Lanes are **lantern currents**: lines of old lamps (the Lamplighters keep them lit) with a current that carries any ship. At Tier 0 the player flies into a lane mouth and presses Space; with map travel (`cap.map_travel`) routed orders enter lanes on their own. Inside a lane the ship moves along the straight line between the two mouths at the **lane speed of 500 u/s** for every hull, can't steer, and arrives at the far mouth stopped. Pause works mid-lane. Each completed transit is a `jumped` fact and one `player.jumps`.

Lane mouth radius: **70 u** (the ship must be within 70 u of the mouth point for Space to enter). `trait.lanerunner` raises it to 105 u.

| Lane | Mouth A (region side) | Mouth B (region side) | Length | Transit at 500 u/s |
| --- | --- | --- | --- | --- |
| `lane.shallows_cinder` | `mouth.shallows_to_cinder` (737, 255) | `mouth.cinder_to_shallows` (4368, 1512) | 3842 u | 7.7 s |
| `lane.shallows_brassmouth` | `mouth.shallows_to_brassmouth` (397, -672) | `mouth.brassmouth_to_shallows` (2305, -3901) | 3751 u | 7.5 s |
| `lane.cinder_brassmouth` | `mouth.cinder_to_brassmouth` (4860, 988) | `mouth.brassmouth_to_cinder` (2824, -3865) | 5263 u | 10.5 s |

Mouths sit 120 u inside their region's edge, on the line between the two region centres.

## Stations

Every authored station has a **docking ring** centred on the station. Pressing Space inside the ring docks (Tier 0); click-to-dock flies into the ring and docks. Docking opens the dock screen: the dockmaster (talk, goals, "what changed?"), the market, and the hullwright where there is one.

| ID | Name | Region | Position | Ring radius | Faction | Dockmaster | Hullwright | Market | Look |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| `station.lantern_wharf` | Lantern Wharf | Shallows | (-150, 100) | 100 u | Wharf Cooperative | Ilse Marrow | Orrin Vale | yes | A long timber-coloured pier station strung with warm lamps; the hullwright's cradle arm on its east side; six berths |
| `station.kettle_dock` | Kettle Dock | Shallows | (450, -350) | 80 u (100 u from stage 3) | Wharf Cooperative | Odo Fenn | none | yes | A squat round co-op hub shaped like a kettle; greenhouse rings bolt on in stages (see below) |
| `station.cinder_hook` | Cinder Hook | Cinder Drift | (5000, 1800) | 90 u | Contested at start (see dispute) | Rook Adeyemi | none | yes | A rough mining camp hung off a hooked spar, with an ore depot box that flies the owner's banner |
| `station.glass_drift_observatory` | Glass Drift Observatory | Cinder Drift | (5800, 2500) | 80 u | Lamplighter Society | Hesper Lune | none | yes | A slim tower of lenses and brass rings aimed at the Glass Drift; a lamp burns on top |
| `station.brassmouth_exchange` | Brassmouth Exchange | Brassmouth Narrows | (2600, -4600) | 120 u | Tollway Compact | Saffron Vey | none | yes | A big brass trumpet-mouthed hub with a ring of trade booths and barges always nosing in |

Nearest neighbours (for checking): Wharf to Kettle 750 u (12.5 s in a Wren); Cinder Hook to the Observatory 1063 u (17.7 s). Every station is at least 190 u from the nearest rock field edge and at least 326 u from the nearest lane mouth.

### Kettle Dock build stages

The stage decides the station's mesh parts (runtime asset kit IDs) and its market. Stage 1 comes from beat b03 (`core`). Stages 2 and 3 come from `gm.kettle_growth`; without the `gm` pack Kettle stays at stage 1. Totals count everything delivered to `ledger.kettle_greenhouse`, and from stage 1 on, every unit of ice or plate the player or a tender sells at Kettle Dock counts too.

| Stage | Needs (cumulative ledger) | Asset kit parts added | Market change |
| --- | --- | --- | --- |
| 0 | start | `kit.kettle.hub`, `kit.kettle.spout` | buys ice and plate |
| 1 | 20 ice (b03 handover) | `kit.kettle.pane_dome_a` (one fogged glass dome) | none |
| 2 | 60 ice and 20 plate | `kit.kettle.greenhouse_ring_a`, `kit.light.warm_green` | starts producing rations |
| 3 | 120 ice and 50 plate | `kit.kettle.greenhouse_ring_b`, `kit.kettle.orchard_lamps` | starts buying glassite (lamp-glass); ring radius 100 u; rations output +50% |

## Rock fields

Three rock kinds, each with a **hardness** that multiplies the ship's mining rate (units per second while Space is held, or while a mine order runs).

| Kind | Ware | Hardness | Look and behaviour |
| --- | --- | --- | --- |
| Ice rock | `ware.ice` | 1.0 | Pale blue-white, soft; flakes drift off while mined and the rock rounds down smoothly |
| Iron rock | `ware.iron_ore` | 0.6 | Rust-brown and lumpy; cracks open in chunks, showing darker veins as it shrinks |
| Glassite rock | `ware.glassite` | 0.3 | Faceted and translucent; slow to cut; each facet goes dull as it is mined, and the rock gives a faint glint when spent |
| Rich glassite (Ember Shoals only, if the rumor is true) | `ware.glassite` | 0.45 | Like glassite, with bright seams |

**Rock size and shrinking.** A rock's drawn radius is `6 + 3 * sqrt(remaining)` u (a full 30-unit ice rock is 22 u across; a full 60-unit belt rock is 29 u). Mining is possible while the ship is within 40 u of the rock's edge. A rock at zero is **spent**: it is drawn as a faint dust ring, can't be mined, and stays spent until it reforms.

**Regrowth is driven by activity.** Each field has a regrowth pool, fed by **region pulses** (`SPEC.md`):

- Each region pulse adds `regrow` units to the field, given to the unspent rock with the lowest remaining fraction, capped at its full yield.
- A spent rock reforms at 25% of its full yield after `reform` region pulses since it was spent. That writes a `rock_reformed` fact.
- A rock being mined right now does not regrow.
- Depletion persists through saves and through prestige.

| ID | Region | Kind | Centre | Radius | Rocks | Yield per rock | Total | `regrow` per region pulse | `reform` (region pulses) |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| `field.rimewater` | Shallows | Ice | (320, 380) | 200 | 8 | 30 | 240 | 0.15 | 16 |
| `field.tinroot` | Shallows | Iron | (-480, -320) | 220 | 12 | 40 | 480 | 0.3 | 16 |
| `field.cinder_belt` | Cinder Drift | Iron (rich, depleting) | (5300, 1250) | 280 | 16 | 60 | 960 | 0.15 | 40 |
| `field.glass_drift` | Cinder Drift | Glassite | (5550, 2150) | 240 | 12 | 25 | 300 | 0.15 | 24 |
| `field.hookside` | Cinder Drift | Ice | (4650, 2150) | 140 | 5 | 30 | 150 | 0.2 | 16 |
| `field.ember_shoals` | Cinder Drift | Set by the rumor (hidden until `rumor_heard`) | (5950, 1500) | 160 | 6 or 3 | 40 or 20 | 240 or 60 | 0 | never |

During the ice shortage, if the player has agreed to let Rimewater rest (b12, way B), Rimewater's `regrow` is multiplied by 4 until Rimewater is back to 80% full.

### Rock positions

Rocks are numbered `01` upward in this order. Positions were seeded once (seed 20261029, at least 55 u apart, inside 92% of the field radius) and are now authored data.

| Field | Rock positions |
| --- | --- |
| `field.rimewater` | (230, 360) (450, 360) (380, 520) (410, 430) (310, 500) (390, 310) (440, 260) (180, 280) |
| `field.tinroot` | (-610, -300) (-460, -460) (-330, -440) (-340, -280) (-590, -440) (-550, -200) (-400, -180) (-310, -210) (-490, -180) (-390, -440) (-280, -320) (-470, -380) |
| `field.cinder_belt` | (5190, 1360) (5260, 1180) (5470, 1140) (5330, 1310) (5380, 1480) (5210, 1420) (5170, 1080) (5390, 1360) (5360, 1160) (5050, 1250) (5520, 1360) (5340, 1020) (5430, 1090) (5210, 1270) (5480, 1260) (5290, 1090) |
| `field.glass_drift` | (5450, 2120) (5690, 2260) (5430, 2180) (5360, 2120) (5530, 1950) (5640, 2310) (5590, 2000) (5670, 2190) (5760, 2230) (5720, 2140) (5430, 2010) (5620, 2110) |
| `field.hookside` | (4740, 2230) (4720, 2050) (4670, 2110) (4590, 2190) (4620, 2050) |
| `field.ember_shoals` | (6000, 1510) (5900, 1600) (6040, 1440) (5880, 1380) (6060, 1580) (5840, 1450). If the rumor is false, only rocks 01 to 03 exist (iron, 20 units each), and a Compact survey buoy `prop.compact_buoy` sits at (5950, 1500). |

### The Ancients fragment

`ware.fragment` is never in a field table. Once beat b13 is complete and `player.units_mined.glassite` (all regions) is at least 80, the next glassite rock that `ship.player` spends drops one fragment into the hold (written as a `fragment_found` fact). A tender never finds it. If the hold is full, the fragment waits as a glint on the dust ring until the player flies within 40 u with a free slot. Only one fragment exists in the slice.

## The sixth station: placement rules

The sixth station is never authored. `gm.sixth_station` (see `06-game-master.md`) grows it from history. The validator rejects a placement unless every rule holds:

1. **Inside a region:** at least 150 u inside a region's edge.
2. **Spacing:** at least 600 u from every station, 200 u from the edge of every rock field, and 250 u from every lane mouth.
3. **Caused by the player:** within 900 u of the `pos` of a cited history fact whose actor is `ship.player` or one of their tenders. The fact must be one of `rock_spent`, `mined`, `sold`, `rumor_followed`, `delivered` (to a shortage or dispute ledger), `canon_opened` or `jumped` (lane mouth position).
4. **One at a time:** only one sixth station per save in the slice, and none while a shortage, dispute or rumor is active in that region.
5. **Complete station:** a dockmaster NPC (new, with a brief: name, voice in 2 lines, what they remember, and at least 3 lines citing the founding facts), a ring radius from 80 to 110 u, a market with 2 to 4 of the five tradeable wares, and at least one ware the player has sold at least 20 of. Its parts come from the runtime asset kit only.
6. **Faction:** one of the three factions or unaligned, and it must match the cited cause (for example, a Lamplighter station must cite `canon_opened` or a Glass Drift Observatory fact).
7. **Calm arrival:** applied while the player is docked or paused. It is announced by a map note at the site and by the dockmaster of the nearest authored station on the player's next docking there.

Candidate sites for the four example station kinds in `06-game-master.md`. All four pass rules 1 and 2.

| Kind | Example site | Region | Anchor fact |
| --- | --- | --- | --- |
| Meltwater Halt (co-op ice hostel) | (-100, 720) | Shallows | Shortage helped, or Rimewater rocks spent |
| Shoals Prospect (prospector camp) | (6000, 1900) | Cinder Drift | True rumor followed and Shoals glassite mined |
| The Listening Post (Lamplighter lamp-house) | (4600, 1300) | Cinder Drift | `canon_opened` after the fragment sang in a lane from `mouth.cinder_to_shallows` |
| Tollhouse Annex (Compact counting-house) | (2560, -3990) | Brassmouth Narrows | Dispute won by the Compact, with player glassite sales at Brassmouth |
