# Lantern Reach content spec

This spec defines the concrete content for slice 01: places, numbers, people, beats and the example game master scripts. Later waves turn it into TOML data (the `core` pack's tables) and Luau scripts (the `core` and `gm` packs). Wherever this spec gives a number, that number is the starting value of a tunable pack datum, not an engine constant.

Sources it builds on: issue #10 (brief), the kickoff comment on PR #11 (decisions that override the brief), `design/vertical-slice-lantern-reach.html` (scope, 17 beats, 20 systems, 8 change types) and `design/high-level-plan.html` (pillars and locked decisions).

## Files

| File | Covers |
| --- | --- |
| `SPEC.md` (this file) | Conventions, IDs, activity pulses, history facts, counters, content invariants, decisions |
| [`01-galaxy.md`](01-galaxy.md) | Regions, lanes, stations, rock fields and rocks, regrowth, sixth-station placement rules |
| [`02-economy.md`](02-economy.md) | Wares, the price model, per-station market tables, saturation, NPC traders |
| [`03-ships-and-controls.md`](03-ships-and-controls.md) | Hulls, hull ranks, prestige, control tiers, upgrades, order verbs, the tender and its patient sell |
| [`04-journey.md`](04-journey.md) | The 17 beats, plus the full rules of the shortage, the dispute and the rumor |
| [`05-people.md`](05-people.md) | Ten named NPCs with line pools, three factions, pilot traits |
| [`06-game-master.md`](06-game-master.md) | The 8 example `gm` pack scripts, and the Ancients chapter 1 canon graph |
| [`07-pacing.md`](07-pacing.md) | Pacing table (activity thresholds with minute estimates), number checks, the journey bot's route |

## Conventions

### Space and units

- World units are `u`. The slice plays on the `z = 0` plane of a right-handed world: `+x` is east, `+y` is north on the map. Every position in this spec is `(x, y)` with `z = 0`, so a later 3D view uses the same coordinates.
- Speeds are in `u` per simulated second, turn rates in degrees per simulated second. The simulation runs at a fixed step the engine chooses. Nothing in content reads wall-clock time.
- Ship speed means top speed under WASD or orders. Arcade flight: the ship reaches top speed almost at once and stops when the key is released (no inertia). The ship's heading swings toward the input direction at the hull's turn rate, and it moves along its heading, so turn rate shapes how a hull feels.
- Minute estimates in this spec are only for checking the design. They are never data the engine reads, and none of them appears in the game.

### Stable IDs

Every authored thing has a stable, lowercase, dotted ID that saves, scripts and history facts use. The families:

| Family | Pattern | Example |
| --- | --- | --- |
| Region | `region.<name>` | `region.lantern_shallows` |
| Station | `station.<name>` | `station.kettle_dock` |
| Lane and lane mouth | `lane.<a>_<b>`, `mouth.<from>_to_<to>` | `lane.shallows_cinder`, `mouth.shallows_to_cinder` |
| Rock field and rock | `field.<name>`, `rock.<field>.<nn>` | `field.rimewater`, `rock.rimewater.03` |
| Ware | `ware.<name>` | `ware.glassite` |
| Faction | `faction.<name>` | `faction.wharf_coop` |
| NPC | `npc.<first>_<last>` | `npc.ilse_marrow` |
| Hull | `hull.<name>` | `hull.wren` |
| Upgrade | `upgrade.<name>` | `upgrade.mining_lens` |
| Capability (control flag) | `cap.<name>` | `cap.click_fly` |
| Beat | `beat.bNN_<name>` | `beat.b07_cinder_drift` |
| Trait | `trait.<name>` | `trait.rimesight` |
| Ledger (delivery tally) | `ledger.<name>` | `ledger.kettle_greenhouse` |
| Rumor | `rumor.<name>` | `rumor.ember_shoals_vein` |
| Canon node | `canon.<name>` | `canon.lane_lanterns` |
| Script | `core.<name>`, `gm.<name>` | `gm.ice_shortage` |
| NPC line | `line.<npc>.<nn>` | `line.ilse_marrow.04` |
| Craft (non-player ships) | `craft.<name>` | `craft.little_mercy` |

The player's ship is `ship.player`. The player's first tender is `craft.thimble`.

### Packs

- **`core` pack**: the authored backbone. It holds every table in this spec and the beats marked `core`. It must reach the prestige choice (b16) and the closing beat (b17) with no `gm` pack installed.
- **`gm` pack**: the eight example game master scripts in `06-game-master.md`. A new save installs it. It owns the beats marked `gm` (b09, b10, b12, b15) and adds Kettle Dock stages 2 and 3, rock tagging, the sixth station and the chronicle. Without it those beats never fire and the journey still finishes.

## Activity: pulses and counters

Nothing in the slice advances with time. The world moves on **activity**, measured in two ways.

### Market and field pulses

A **pulse** is a unit of activity at a place. Markets (stock and saturation) and rock fields (regrowth) advance by pulses.

| Event | Station pulse at that station | Station pulse at each other station in the region | Region pulse |
| --- | --- | --- | --- |
| A counted docking by any ship (player, tender or NPC craft) | 1.0 | 0.5 | 1.0 |
| A lane arrival into the region by any ship | 0.5 at every station in the region | (included) | 1.0 |

- **Counted docking:** a docking counts only if, since its previous docking at that same station, the ship has flown at least 300 u, mined at least one unit, or docked somewhere else. This stops undock-redock loops from manufacturing pulses.
- Pulse weights, the 300 u rule and every rate fed by pulses are pack data (`pulses.toml`).
- While the game is paused, no ship moves, so no pulses happen. NPC craft fly in simulated time and generate pulses when they dock; that is world activity, and no rule reads elapsed time.
- Expected pulse rates at the starting NPC traffic plus a player docking about every 80 s (see `02-economy.md`), used only for estimates: Lantern Shallows about 4 region pulses per minute of play, Cinder Drift about 3, Brassmouth Narrows about 2. Brassmouth Exchange gets about 1.4 station pulses a minute from NPC traffic alone.

### Activity counters

Read-only counters per player, station and region. Thresholds in this spec are written against them. All are monotonic integers unless noted.

| Counter | Counts |
| --- | --- |
| `player.dockings` | Counted dockings by `ship.player` (any hull) |
| `player.dockings_at.<station>` | Per station |
| `player.jumps` | Lane transits completed by `ship.player` |
| `player.units_mined.<ware>.<region>` | Units mined by `ship.player` |
| `player.rocks_spent` | Rocks `ship.player` took to zero |
| `player.units_sold` and `player.units_sold.<ware>` | Units sold by `ship.player` |
| `player.split_sales` | Dockings where the player sold part of a ware stack and kept the rest, then later sold the rest at a different station |
| `player.ledger_deliveries` | Dockings where the player delivered to any ledger (goal handovers, Kettle growth, dispute and shortage ledgers) |
| `player.queues_done` | Queues of 3 or more orders completed by `ship.player` |
| `player.loop_laps` | Laps of a saved loop completed by `ship.player` |
| `player.stations_visited` | Distinct stations docked at |
| `player.sessions` | Times the game was opened on this save |
| `hull.<id>.dockings`, `hull.<id>.units_sold`, `hull.<id>.jumps` | Per-hull counters for hull ranks; reset when that hull is left at prestige |
| `craft.<id>.laps`, `craft.<id>.patient_sells` | Per tender |
| `station.<id>.pulses`, `region.<id>.pulses` | Running pulse totals (fractional) |
| `since.<fact_id>.<counter>` | Any counter measured since a given history fact (for example dockings since prestige) |

## History facts

The history log is append-only. Every fact has `id`, `seq` (monotonic), `kind`, `actor` (a stable ID or `world`), the fields below, and `cause` (the script or engine system that wrote it, plus any facts it cites). Facts that happen somewhere also store `place` (a station, field, mouth or region ID) and `pos` when a position matters (sixth-station placement uses it).

The fact kinds the content uses:

| Kind | Fields | Written by |
| --- | --- | --- |
| `docked`, `undocked` | ship, station | engine |
| `mined` | ship, rock, field, ware, qty (one fact per mining run: start to stop on one rock) | engine |
| `rock_spent` | ship, rock, field | engine |
| `rock_reformed` | rock, field | engine |
| `sold`, `bought` | ship, station, ware, qty, total, avg_price, sat_after | engine (one fact per ware per docking) |
| `market_flooded` | ship, station, ware, sat_after | engine, when a sale leaves saturation at 0.5 or more |
| `scouted` | ship, station, snapshot (prices of all wares) | engine, on each counted docking |
| `jumped` | ship, lane, from_region, to_region | engine |
| `paused` | (none) | engine, the first time in a session the PAUSED card opens |
| `delivered` | ship, station, ledger, ware, qty | core and gm scripts |
| `beat_started`, `beat_step`, `beat_completed` | beat, step | beat API |
| `upgrade_fitted` | upgrade, hull | core |
| `cap_unlocked` | cap | core |
| `order_done` | ship, verb, target, result | engine |
| `queue_done` | ship, orders | engine |
| `loop_saved`, `loop_lap`, `loop_stalled` | ship, loop, reason | engine |
| `tender_acquired`, `tender_assigned` | craft, loop, ware, floor | core |
| `patient_wait` | craft, station, ware, floor, price | engine, when a tender starts waiting |
| `patient_sold` | craft, station, ware, qty, avg_price, floor, pulses_waited | engine, when a patient sell finishes |
| `trait_earned` | trait, deed_count | core |
| `hull_rank` | hull, rank | core |
| `prestige` | from_hull, to_hull, fee | core |
| `standing_changed` | faction, deed, delta | core (the delta is never shown) |
| `regard_changed` | faction, from, to | core |
| `stage_built` | station, stage | core (stage 1), gm (stages 2 and 3) |
| `shortage_started`, `shortage_helped`, `shortage_resolved` | shortage, way, by | gm |
| `dispute_started`, `side_chosen`, `territory_changed` | dispute, side, from, to | gm |
| `rumor_heard`, `rumor_spread`, `rumor_followed`, `rumor_resolved` | rumor, npc, station, truth | gm |
| `fragment_found` | ship, rock | core |
| `shown` | item, npc, station (the first time a dockmaster is shown a found item, such as the fragment) | core |
| `canon_opened` | node | gm |
| `mechanic_taught` | mechanic, teacher | core (tender), gm (rock tagging) |
| `tagged`, `untagged` | ship, rock | engine, once the gm pack registers tagging |
| `station_founded` | station, kind, faction, cites | gm |
| `chronicle` | entry, cites, place, person | gm |
| `asked_what_changed` | npc, station | engine |
| `npc_line` | npc, line, cites | legibility channel (so used lines retire) |

## Content invariants

Every table and script in this spec follows these, and the script validator checks the ones it can.

1. **No timers.** No countdowns, progress bars, "days left", expiry or cooldowns measured in time. Waiting is always for a world condition (a price, a stock level, a hold) that activity changes.
2. **No level scaling.** Prices, yields, rewards and fees are fixed by tables and by world state, never by the player's wealth, hull or progress.
3. **No destroyed player property.** Nothing removes credits, cargo, hulls, tenders or fittings except the player's own trades, the hullwright fee and the player's own deliveries. Rollbacks give back what they took.
4. **Every shock comes with an opportunity.** A script that makes something worse for the player somewhere makes something better somewhere they can reach.
5. **Every change shows in a place and a person.** It changes something visible at a station, field or map border, and a named NPC can cite the fact that caused it.
6. **Story never gates the sandbox.** Every beat is optional. Mining, trading, flying and buying upgrades never wait on a beat, except where an upgrade's own unlock condition names one (the mining lens, nav chip, lane chart, order slate and tender), and each of those beats fires from ordinary play.
7. **Text is important or absent.** Every NPC line cites a history fact. If no fact qualifies, nobody speaks.
8. **One big change per region at a time.** A shortage, a territory change or a new station never overlaps another big change in the same region.

## Decisions taken in this spec

The open questions this spec had to answer, with the choice made. Each is also listed in the PR.

1. **Ownership of journey beats.** The beats tied to change types (dispute b09, rumor b10, shortage b12, fragment b15) live in the `gm` pack. The tender (b13) is `core`, so change type 5 ("a new verb with a teacher") is shown in the `gm` pack by **rock tagging**, which the slice doc lists as the alternative.
2. **Lanes are currents, not warps.** You enter a lane mouth (Space at Tier 0) and a current carries every hull at 500 u/s along a real path in world space. Travel stays continuous and pausable.
3. **The patient sell is tender-only by ring rules.** All ships share the verbs. The difference is that docking rings give tenders a *standing berth*, so a tender can wait docked; a crewed ship trades and clears the ring. Tamsin explains this in-world.
4. **Prestige keeps all fittings.** The hullwright moves every module (control upgrades, lens, hold extender) to the new hull. Only hull ranks reset. The Wren stays berthed and visible at the Wharf and can't be flown again in the slice.
5. **Standing is a hidden number, shown only through five regard tiers.** Regard changes greetings, price spread, tips and offers. It never locks a market.
6. **Rumor truth comes from history.** The Ember Shoals rumor is a Compact plant if the player has leaned Cooperative in the dispute when the rumor is created; otherwise it is true.
7. **Rocks have authored positions.** Rock positions are listed in `01-galaxy.md` (seeded once, then frozen as data), so bot runs and saves are stable.
8. **The fragment is found by activity.** It appears in the next glassite rock the player's own ship spends after b13, once the player has mined at least 80 glassite in all.
9. **Snapshot age is shown in dockings**, for example "seen 6 dockings ago", never in time.
10. **Hullwright fee: 8000 credits.** At the estimated income this lands near 2 hours, after the activity gates, so the fee feels considered without being a wall.
