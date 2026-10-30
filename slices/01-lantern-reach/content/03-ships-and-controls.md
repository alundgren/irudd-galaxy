# 03 Ships, controls and the tender

Back to [SPEC.md](SPEC.md). Data files: `hulls.toml`, `upgrades.toml`, `controls.toml`.

## Hulls

| Stat | Wren (`hull.wren`) | Tandem (`hull.tandem`) | Heron (`hull.heron`) | Tender (`hull.tender`) |
| --- | --- | --- | --- | --- |
| Who flies it | The player, from the start | The player, after prestige | The player, after prestige | Nobody: automated |
| Speed (u/s) | 60 | 50 | 78 | 45 |
| Turn (deg/s) | 270 | 160 | 330 | 180 |
| Hold (units) | 20 | 40 | 16 | 24 |
| Mining rate (units/s at hardness 1.0) | 1.0 | 1.0 | 0.8 | 0.5 |
| Price reach | Docked only | Docked only | Long glass (see below) | Docked only |
| Abilities | None; a good little ship | Coupling; Second berth | Long glass; Seam sense; Survey notes | Standing berth (patient sell) |
| Look | Small, round-nosed, one warm cockpit lamp | Broad twin-hulled hauler joined by a cargo spine | Long-necked, narrow, with a lens at the prow | A little box with a lamp and a single grabber arm |

Mining rate in practice (units per second) = hull rate × hardness × 1.5 with the mining lens. For example a lensed Wren mines ice at 1.5/s, iron at 0.9/s and glassite at 0.45/s.

### Abilities

- **Coupling (Tandem):** adds the `transfer` verb. It moves cargo between the Tandem and one of the player's tenders when both are docked at the same station or within 120 u in space. Typical use: the Tandem mines a full 40-unit hold, hands it to a tender at the station, and flies off while the tender patient-sells it.
- **Second berth (Tandem):** the player may own two tenders. Tamsin builds the second one (`craft.button`) for the same 2400 cr.
- **Long glass (Heron):** live prices for every station in the current region. Entering any lane mouth refreshes the snapshots of every station in the destination region.
- **Seam sense (Heron):** shows the remaining units of every rock within 600 u. Within 600 u of a rumor's site, it shows whether the vein is real before the player commits to mining.
- **Survey notes (Heron):** each field the Heron flies through (within its radius) since its last docking at the Glass Drift Observatory counts as one survey note. Nell buys each note for 120 cr (Lamplighter standing +2 per note, at most 3 notes per docking). Survey notes are a deed, not a ware.
- **Standing berth (tender):** see "The tender" below.

### Hull ranks

Each flown hull earns up to three **ranks** from deeds done in that hull. The hull shows a rank as a painted lantern stripe on its side, never as a number. Ranks belong to the hull, and they are what prestige resets.

| Hull | Rank 1 | Rank 2 | Rank 3 |
| --- | --- | --- | --- |
| Wren | "Settled": 25 dockings in this hull; turn +10% | "Worn in": 300 units sold from this hull; hold +2 | "Known by the lanes": 20 jumps in this hull; speed +5% |
| Tandem | "Steady": 25 dockings; turn +10% | "Broad-backed": 400 units sold; hold +4 | "Good company": 10 transfers to a tender; each tender's hold +4 |
| Heron | "Steady": 25 dockings; turn +10% | "Far-sighted": 15 dockings at stations whose snapshot was more than 10 dockings old; Long glass also refreshes snapshots of stations within 2500 u at each jump | "Quick": 20 jumps; speed +5% |

Each rank writes a `hull_rank` fact. Orrin notices it on the next Wharf docking.

## Hullwright prestige (Hearthbound)

At Lantern Wharf, Orrin Vale's question (beat b16) offers the Wren's Hearthbound branch: **Tandem** (a hauler that pairs with automation) or **Heron** (a scout with longer price reach).

- **Fee:** 8000 cr, paid to Orrin and not refunded. The offer appears once b16's preconditions hold. The player may decline and ask again on any later Wharf docking.
- **What resets:** hull ranks only. The new hull starts at rank 0 with its own rank counters.
- **What stays:** the world (stations, stages, territory, depletion, rumors, NPC memory), credits minus the fee, faction standing, history, **pilot traits** (all of them), tenders and their loops, every capability, and every fitted module. Orrin moves the mining lens, the hold extender, the price glass, the nav chip, cradle, clamp, lane chart, order slate and loop log across ("They're yours, not the Wren's.").
- **The Wren** stays berthed at the Wharf's sixth berth, visible from the dock, with its stripes. It can't be flown again in the slice. Nothing the player built is lost.
- The confirm screen lists exactly what stays and what resets, in one line each. It shows no countdown and no comparison chart.
- Writes `prestige{from_hull, to_hull, fee}`. Saves keep both hulls' rank records.

## Control tiers

Controls are progression. Each capability is a flag (`cap.*`) set by an upgrade or a beat. Tiers group them for presentation. They aren't a strict ladder: a player can have map travel before click-to-mine.

| Tier | Capabilities | What the player can do | Comes from |
| --- | --- | --- | --- |
| 0: hands on | (base) | **WASD** flies screen-relative, stopping on release. **Space** does the one thing in reach (see below). **P** or **Esc** opens the PAUSED card and stops the whole galaxy. **M** opens the map (view only). **Tab** shows the hold. | New save |
| 1: click to fly | `cap.click_fly` | Left-click a point within 450 u of the ship: it flies there and stops. Any WASD input cancels. | `upgrade.nav_chip` |
| 2: click to work | `cap.click_mine`, `cap.click_dock` | Click a rock: the ship flies into mining reach and mines until the hold is full or the rock is spent. Click a station: it flies into the ring and docks. | `upgrade.rock_cradle`, `upgrade.docking_clamp` |
| 3: map travel | `cap.map_travel` | Click a station, field or lane mouth on the map: the ship routes there, entering lanes on its own. No distance limit. | `upgrade.lane_chart` (beat b07 reward) |
| 4: queues | `cap.queues` | **Shift-click** appends orders, up to 8. The queue shows as a row of verb icons; any order can be removed, and a `sell` icon can take a floor. | `upgrade.order_slate` |
| 5: loops | `cap.loops` | Save the current queue as a named loop (so a loop has up to 8 orders, the same limit as a queue; up to 3 saved loops). A loop repeats until the player stops it or it stalls. There is no separate loop editor: to change a loop, load it into the queue, edit, and save it again. | `upgrade.loop_log` |
| First automation | `cap.automation` | Tenders run saved loops while the player does anything else. | Beat b13: building Thimble (step 2) |

**Space priority at Tier 0:** inside a docking ring, dock; else inside a lane mouth, enter the lane; else within 40 u of a loose fragment, pick it up; else within mining reach of a rock, mine while held. Docked, Space undocks.

## Upgrades

All upgrades are fitted by the named seller at their station. Costs are fixed. Unlock conditions are activity counts or beats, never time. An upgrade appears on the seller's list, with one goal line, as soon as its condition holds.

| ID | Name | Effect | Cost | Unlock condition | Seller |
| --- | --- | --- | --- | --- | --- |
| `upgrade.mining_lens` | Mining lens | Mining rate x1.5 | 600 | b03 complete | Orrin (Wharf) |
| `upgrade.nav_chip` | Nav chip | `cap.click_fly` | 225 | b05 complete and `player.dockings` >= 6 | Orrin |
| `upgrade.rock_cradle` | Rock cradle | `cap.click_mine` | 700 | nav chip fitted and `player.rocks_spent` >= 4 | Orrin |
| `upgrade.docking_clamp` | Docking clamp | `cap.click_dock` | 700 | nav chip fitted and `player.dockings` >= 14 | Orrin |
| `upgrade.lane_chart` | Lane chart | `cap.map_travel` | gift | b07 complete | Rook hands it over at Cinder Hook |
| `upgrade.hold_extender` | Hold extender | Hold +10 (any hull) | 1500 | `player.units_sold` >= 150 | Orrin |
| `upgrade.price_glass` | Price glass | Live prices of stations within 1200 u | 1800 | docked at all 5 authored stations | Hesper (Glass Drift Observatory) |
| `upgrade.order_slate` | Order slate | `cap.queues` | 2000 | b08 complete, `cap.map_travel`, and `player.dockings` >= 26 | Orrin |
| `upgrade.loop_log` | Loop log | `cap.loops` | gift | `player.queues_done` >= 4 | Orrin |
| `craft.thimble` (tender) | First tender | `cap.automation` (on purchase, b13 step 2, so step 3 can assign it a loop) | 2400 | b13 step 1 done | Tamsin (Kettle Dock) |
| `craft.button` (second tender) | Second tender | A second tender | 2400 | Tandem hull and b13 complete | Tamsin |

Every fitting writes `upgrade_fitted`. Every capability writes `cap_unlocked`. Fittings are never removed.

## Order verbs

One verb set is shared by the player ship, tenders and NPC craft (the order verb API). Queues and loops are lists of these orders. A tender trades with the player's credits: its `buy` spends them and its `sell` earns them. `08-contract.md` maps each verb onto `contract::verbs::Order`.

| Verb | Parameters | Behaviour |
| --- | --- | --- |
| `fly` | point, station, rock, field or lane mouth | Fly in a straight line within a region. Across regions it routes through lanes (player ships need `cap.map_travel`). |
| `mine` | rock, or field (optionally `prefer = tagged`) | Fly into reach and mine until the hold is full or the rock is spent. Given a field, it moves on to the richest unspent rock until the hold is full or the field is spent. |
| `dock` | station | Fly to the station, through lanes if it is in another region, into the ring, and dock. Every order that names a station other than the one the ship is docked at starts with this. |
| `buy` | ware, quantity or `fill`, optional `max_price` | Buy unit by unit **at the station the ship is docked at**; stop at the quantity, a full hold, no stock, no credits or above `max_price`. Not docked, or docked where the ware isn't sold: `skipped`. |
| `sell` | ware, quantity or `all`, optional `floor` | Sell unit by unit **at the station the ship is docked at** while the price is at or above `floor`. A **crewed** ship then finishes the order and keeps the rest. A **tender** waits in its standing berth (the patient sell). Not docked, or docked where the ware isn't bought: `skipped`. |
| `wait` | condition: `hold_full`, `hold_empty`, `price_at_least{ware, value}`, `stock_at_least{ware, n}`, `tender_at{station}` | Hold position until the condition is true. Docked waits need a standing berth. A docked crewed ship clears the ring, so its wait finishes at once with result `cleared`. Undocked, any ship may wait. No condition is ever a time. |
| `loop` | loop name | Repeat the loop's orders. A lap ends after its last order; the next lap starts with its first order wherever the ship is. |
| `transfer` | ware, quantity, tender | Tandem only (Coupling). |
| `tag` | rock | Only after `gm.rock_tagging` teaches it (see `06-game-master.md`). |

**Failures are calm.** An order that can't act (no stock, no money, nothing to sell, a spent rock) finishes with result `skipped`, and the queue moves on. If a whole loop lap changes nothing (no units moved and no credits changed), the loop **stalls**: the ship stops (or the tender stays docked), a `loop_stalled` fact is written, and the next NPC the player meets who has a line for it says so. Nothing breaks and nothing is lost.

## The tender (first automated craft)

The **tender** is a small automated craft that runs one of the player's saved loops with the same verbs, while the player flies elsewhere. It arrives after the player's own loops (beat b13). It has one job the player's ship can't do: the **patient sell**.

| Property | Value |
| --- | --- |
| ID and default name | `craft.thimble` ("Thimble"; the player may rename it) |
| Built and taught by | Tamsin Oddway, Kettle Dock |
| Cost | 2400 cr |
| Stats | Speed 45, turn 180, hold 24, mining rate 0.5 (no lens) |
| Starts | Berthed at Kettle Dock |
| Runs | One saved loop, assigned at any dockmaster. Assigning copies the loop, so later edits to the player's own loop don't change the tender until the player assigns it again. |
| Routes | Uses lanes freely |
| Can't | Accept goals, talk, carry consigned cargo or the fragment, find the fragment, or hand over to ledgers through dock talk (with `gm-examples`, its ordinary sales at Kettle Dock still count to the greenhouse ledger) |
| Upkeep | None. No fuel, no wages, no wear. It can't be destroyed or lost. |
| Recall | "Bring Thimble home" (at any dockmaster, or from the hold screen) sends it to Kettle Dock to wait, without selling. Its cargo stays aboard until the player assigns a loop again. |
| Wallet | Trades with the player's credits. A `buy` with no credits is `skipped`. |

### The patient sell

A tender's `sell{ware, all, floor}` order works like this, at a station with a standing berth (every authored station gives tenders one):

1. Sell unit by unit while the station's price is at or above `floor`.
2. When the price drops below `floor`, stay docked and write `patient_wait{craft, station, ware, floor, price}`.
3. On each **station pulse** at that station (any ship's counted docking there or nearby, or a lane arrival into the region), re-check the price. As saturation clears and stock is drunk down, the price climbs. When it reaches `floor`, go back to step 1.
4. When the stack is gone, write `patient_sold{qty, avg_price, floor, pulses_waited}` and continue the loop. `pulses_waited` is 0 if the price never dipped; that still counts as a patient sell for b13.

It never reads a clock. If the market is quiet, the tender waits until something happens there. The player sets the floor when assigning the loop. The assign screen shows the last price the player saw there, the most the station has ever paid them, and the most it can pay right now (its **ceiling**, `1.3 x` the current base), with no suggested floor.

**Validation at assignment.** The validator rejects a floor above the current ceiling, and Tamsin explains: "Kettle has never paid that. Thimble would sit there forever." If the floor is above the station's ceiling *without* active levers (for example, during the shortage, when the ice base is x1.4), the floor is accepted and the screen adds one line: "Only while the shortage lasts."

**Revalidation when a market changes.** Whenever a market lever changes a ware's base at a station (the shortage starting or ending, the dispute's base changes, a gm lever, a rollback), every standing floor for that ware at that station is checked against the new ceiling:

- **Floor above the new ceiling:** the order is **blocked**. The tender stays docked in its standing berth, keeps its cargo, and writes `patient_blocked{craft, station, ware, floor, ceiling}`. A blocked sell is not a stall: the loop doesn't count a dead lap, and nothing is sold below the player's floor. The tender's map and hold icons show a small closed lamp, and the next dockmaster the player meets says so (Tamsin's `.09` at Kettle Dock, the station's own dockmaster elsewhere, citing `patient_blocked`).
- **The player chooses,** at any dockmaster or from the hold screen: revise the floor (any value up to the new ceiling; writes `floor_changed`, and the sale resumes at once), assign another loop (the cargo stays aboard and the new loop's first matching `sell` sells it), or "Bring Thimble home" (it flies to Kettle Dock with its cargo). The floor is never changed for the player.
- **If the ceiling rises again** above the floor before the player acts (a new shortage, a rollback), the order unblocks by itself and writes `patient_unblocked`.

The same check runs when a loop is assigned and on every save load, so a blocked order never hides.

Why it matters: a full hold sold at once saturates the buyer. A 24-unit glassite load dumped at Brassmouth after a recent sale averages about 23 cr a unit; the same load sold patiently with a floor of 40 averages about 41 (see `07-pacing.md`). After prestige, the Tandem's Coupling makes this the natural pair: the Tandem hauls, and the tender sells.

### In-world rule

Tamsin explains it in b13: "Rings only let crewed ships trade and go. Keeps the ring clear. A tender's got no crew to get bored, so we give it a standing berth. It can sit there and listen to the dock for as long as it likes."
