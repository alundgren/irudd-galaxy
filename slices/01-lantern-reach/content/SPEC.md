# Lantern Reach content spec

This spec defines the concrete content for slice 01: places, numbers, people, beats and the example game master scripts. Later waves turn it into TOML data (the `core` pack's tables) and Luau scripts (the `core` and `gm-examples` packs). [`08-contract.md`](08-contract.md) maps every piece onto the foundation contract (`crates/contract`) and lists the contract extensions it needs. Wherever this spec gives a number, that number is the starting value of a tunable pack datum, not an engine constant.

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
| [`06-game-master.md`](06-game-master.md) | The 8 example `gm-examples` pack scripts, layered on the authored beats, and the Ancients chapter 1 canon graph |
| [`07-pacing.md`](07-pacing.md) | Pacing table (activity thresholds with minute estimates), number checks, the journey bot's route |
| [`08-contract.md`](08-contract.md) | Mapping from this content to the contract's types, commands and events; the contract extensions and who builds them; rounding and naming decisions |

## Conventions

### Space and units

- World units are `u`. The slice plays on the `z = 0` plane of a right-handed world: `+x` is east, `+y` is north on the map. Every position in this spec is `(x, y)` with `z = 0`, so a later 3D view uses the same coordinates.
- Speeds are in `u` per simulated second, turn rates in degrees per simulated second. The simulation runs at a fixed step the engine chooses. Nothing in content reads wall-clock time.
- Ship speed means top speed under the flight keys or orders. By hand the ship flies with tank controls and some inertia (the locked preset from issue #47): A/D turn it at the hull's turn rate, W/S thrust along its heading, Shift boosts x2, and letting go coasts until the auto-brake stops it, in 1.0 s from any speed. Orders steer straight to their point. All of it is data (`hulls.toml`, `ship_rules.toml`).
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
| Station part (runtime asset kit) | `part.<owner>.<name>` | `part.kettle.pane_dome_a` |
| Tunable (a number or choice a beat reads, which a gm script may vary) | `<situation>.<name>` | `shortage.rimewater_ice.way_a_quota` |
| Map zone | `zone.<faction>.<place>` | `zone.wharf_coop.shallows` |
| Prop (a thing in space that isn't a rock or station) | `prop.<name>` | `prop.old_claim_marker` |
| Rumor | `rumor.<name>` | `rumor.ember_shoals_vein` |
| Canon node | `canon.<name>` | `canon.lane_lanterns` |
| Script | `core.<name>`, `gm.<name>` | `gm.shortage_tuning` |
| NPC line | `line.<npc>.<nn>` | `line.ilse_marrow.04` |
| Craft (non-player ships) | `craft.<name>` | `craft.little_mercy` |

The player's ship is `ship.player`. The player's first tender is `craft.thimble`.

### Packs

- **`core` pack** (`packs/core`, manifest `id = "core"`): the authored backbone. It holds every table in this spec and **all 17 beats**, including the Cinder Hook dispute (b09), the rumor (b10), the ice shortage (b12) and the fragment with Ancients chapter 1 (b15). Each of those is an authored, state-driven beat: it fires from history and world state through the same hooks the game master uses. A save with only `core` installed plays the whole journey to the closing beat (b17).
- **`gm-examples` pack** (`packs/gm-examples`, manifest `id = "gm"`, which keeps every script and fact kind in the `gm.` namespace): the eight example game master scripts in `06-game-master.md`. A new save installs it, so all eight change types show up in play. It **layers on and varies** the authored beats and places, and adds the changes only the game master makes (Kettle Dock stages 2 and 3, rock tagging, the sixth station, the chronicle). It never registers a beat, never pays a beat's reward, and is never needed to finish the journey.
- **Direction of reference:** `gm-examples` depends on `core` and reads its facts, ledgers and tunables. Nothing in `core` (data, scripts or lines) names anything defined in `gm-examples`. The pack validator checks this.
- **How a gm script varies a core beat:** every number and choice a core beat reads lives in a **tunable** (see `08-contract.md`), loaded from `pacing.toml`. A gm script may rewrite a tunable with a cause while the beat is dormant; the core beat reads whatever is there when it starts. Rolling the gm script back restores the authored value for the next start. Nothing a beat already paid is taken back.
- **Guardrail test:** the journey bot runs once with both packs and once with `gm-examples` removed. Both runs must complete all 17 beats (`galaxy play --beat N` jumps are allowed in the shortened test variant).

## Activity: pulses and counters

Nothing in the slice advances with time. The world moves on **activity**, measured in two ways.

### Market and field pulses

A **pulse** is a unit of activity at a place. Markets (stock and saturation) and rock fields (regrowth) advance by pulses.

| Event | Station pulse at that station | Station pulse at each other station in the region | Region pulse |
| --- | --- | --- | --- |
| A counted docking by any ship (player, tender or NPC craft) | 1.0 | 0.5 | 1.0 |
| A lane arrival into the region by any ship | 0.5 at every station in the region | (included) | 1.0 |

- **Counted docking:** a docking counts only if, since its previous docking at that same station, the ship has flown at least 300 u, mined at least one unit, or docked somewhere else. This stops undock-redock loops from manufacturing pulses.
- Pulse weights, the 300 u rule and every rate fed by pulses are pack data (`rules.toml`).
- Pulses are fractional (a 0.5 pulse is half of every per-pulse rate). They are world state, stored in milli-pulses (see `08-contract.md`), not player counters: every ship's counted docking makes them.
- While the game is paused, no ship moves, so no pulses happen. NPC craft fly in simulated time and generate pulses when they dock; that is world activity, and no rule reads elapsed time.
- Expected pulse rates at the starting NPC traffic plus a player docking about every 80 s (see `02-economy.md`), used only for estimates: Lantern Shallows about 4 region pulses per minute of play, Cinder Drift about 3, Brassmouth Narrows about 2. Brassmouth Exchange gets about 1.4 station pulses a minute from NPC traffic alone.

### Activity counters

Read-only counters per player, station and region. Thresholds in this spec are written against them. All are monotonic integers unless noted.

**`player.*` counters count the player's own ship (`ship.player`, whatever its hull) only.** A tender's activity never moves a `player.*` counter: it has its own `craft.<id>.*` counters. This is a contract change (the foundation currently counts every player-owned ship in the player scope; see `08-contract.md`).

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

Fact kind names follow the contract. Kinds an engine system writes are contract events, and their names are the contract's `EventKind` names (`dock`, `mine`, `sell`, `rock_depleted` ...). Kinds a pack script writes are recorded facts, written here without their pack prefix; in data and scripts they are `core.<kind>` or `gm.<kind>`. `08-contract.md` lists every kind with its exact contract name, which ones already exist, and which are extensions.

The fact kinds the content uses:

| Kind | Fields | Written by |
| --- | --- | --- |
| `dock`, `undock` | ship, station, counted (whether the docking counts; see above) | engine |
| `mine` | ship, rock, field, ware, qty (one fact per mining run: start to stop on one rock) | engine |
| `rock_depleted` | ship, rock, field | engine |
| `rock_reformed` | rock, field | engine, when regrowth brings a spent rock back to 25% of its yield |
| `sell`, `buy` | ship, station, ware, qty, total, avg_price, sat_after, origin (the field or station the stack came from) | engine (one fact per ware, origin and selling burst: a patient sell that waits in between writes one per burst) |
| `market_flooded` | ship, station, ware, sat_after | engine, when a sale leaves saturation at 0.5 or more |
| `scout` | ship, station, snapshot (prices of all wares) | engine, on each counted docking |
| `jump` | ship, lane, from_region, to_region | engine |
| `paused` | session | engine (the app), **every** time the PAUSED card opens. The sim doesn't tick while paused, so the fact commits on the first tick after the card closes. |
| `deliver` | ship, station, ledger, ware, qty | core scripts (dock-talk handovers) |
| `beat_started`, `beat_step`, `beat_completed` | beat, step | beat API |
| `upgrade_fitted` | upgrade, hull | core |
| `cap_unlocked` | cap | core |
| `order_done` | ship, verb, target, result | engine |
| `queue_done` | ship, orders | engine |
| `loop_saved`, `loop_lap`, `loop_stalled` | ship, loop, reason | engine |
| `tender_acquired`, `tender_assigned` | craft, loop, ware, floor | core |
| `patient_wait` | craft, station, ware, floor, price | engine, when a tender starts waiting |
| `patient_sold` | craft, station, ware, qty, avg_price, floor, pulses_waited | engine, when a tender's floored sell finishes its stack (`pulses_waited` may be 0) |
| `patient_blocked`, `patient_unblocked` | craft, station, ware, floor, ceiling | engine, when a market change puts a standing floor above the most that station can now pay, and when it can pay it again (see `03-ships-and-controls.md`) |
| `floor_changed` | craft, station, ware, from, to | engine, when the player revises a tender's floor |
| `trait_earned` | trait, deed_count | core |
| `hull_rank` | hull, rank | core |
| `prestige` | from_hull, to_hull, fee | core |
| `standing_changed` | faction, deed, delta | core (the delta is never shown) |
| `regard_changed` | faction, from, to | core |
| `stage_built` | station, stage | core (stage 1), gm (stages 2 and 3) |
| `shortage_started`, `shortage_helped`, `shortage_resolved` | shortage, stations, quota, tuned (whether a tunable it read was last written by a script rather than pack data), way, by | core |
| `dispute_started`, `side_chosen`, `territory_changed` | dispute, side, from, to | core |
| `rumor_heard`, `rumor_spread`, `rumor_followed`, `rumor_resolved` | rumor, npc, station, source, truth, followed | core |
| `rumor_exposed` | rumor, planted_by | gm (`gm.compact_plant`), when the player reads the Compact's buoy |
| `fragment_found` | ship, rock | core |
| `shown` | item, npc, station (the first time each NPC sees a found item such as the fragment: a dockmaster notices it on docking, and Nell is shown it at b15 step 1) | core |
| `canon_opened` | node | core (b15's two nodes), gm (`canon.second_fragment`) |
| `mechanic_taught` | mechanic, teacher | core (tender), gm (rock tagging) |
| `tagged`, `untagged` | ship, rock | engine, once `gm.rock_tagging` has taught tagging |
| `station_founded` | station, kind, faction, cites | gm |
| `border_moved` | faction, zone, station | gm (`gm.territory_map`) |
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
9. **The journey never needs the game master.** Every beat's preconditions, steps, end state and reward are in `core`. A `gm-examples` script may change *how* a beat plays (which stations, which quota, who planted a rumor, what the map shows), never *whether* it can finish.
10. **Every required conversation can close.** A beat step that waits on an NPC speaking always has a line that qualifies, because its fallback cites a fact the step's own preconditions guarantee (see `05-people.md`, "Beat conversations").

## Decisions taken in this spec

The open questions this spec had to answer, with the choice made. Each is also listed in the PR.

1. **Ownership of journey beats (owner decision, confirmed on PR #11).** `core` carries all 17 beats, including the dispute (b09), the rumor (b10), the shortage (b12) and the fragment (b15), as authored state-driven beats. `gm-examples` demonstrates the eight change types by varying those beats and places through tunables, map layers and extra lines, and by the changes only it makes. The tender (b13) is `core`, so change type 5 ("a new verb with a teacher") is shown by **rock tagging**, which the slice doc lists as the alternative.
2. **Lanes are currents, not warps.** You enter a lane mouth (Space at Tier 0) and a current carries every hull at 500 u/s along a real path in world space. Travel stays continuous and pausable.
3. **The patient sell is tender-only by ring rules.** All ships share the verbs. The difference is that docking rings give tenders a *standing berth*, so a tender can wait docked; a crewed ship trades and clears the ring. Tamsin explains this in-world.
4. **Prestige keeps all fittings.** The hullwright moves every module (control upgrades, lens, hold extender) to the new hull. Only hull ranks reset. The Wren stays berthed and visible at the Wharf and can't be flown again in the slice.
5. **Standing is a hidden number, shown only through five regard tiers.** Regard changes greetings, price spread, tips and offers. It never locks a market.
6. **Rumor truth comes from history.** In `core`, the Ember Shoals rumor's truth is fixed when Pip tells it, by which NPC craft brought the story to Brassmouth (the newest NPC `dock` there): Ninefold or Quiet Lamp make it true, Even Keel makes it a stale tale. With `gm-examples`, `gm.compact_plant` turns it into a Compact plant if the player leans Cooperative in the dispute at that moment.
7. **Rocks have authored positions.** Rock positions are listed in `01-galaxy.md` (seeded once, then frozen as data), so bot runs and saves are stable.
8. **The fragment is found by activity.** It appears in the next glassite rock the player's own ship spends after b13, once the player has mined at least 80 glassite in all.
9. **Snapshot age is shown in dockings**, for example "seen 6 dockings ago", never in time.
10. **Hullwright fee: 8000 credits.** At the estimated income this lands near 2 hours, after the activity gates, so the fee feels considered without being a wall.
11. **Every pause opening is a fact.** `paused` is written each time the card opens, so a beat can ask "has the player paused since step X?" by comparing fact order.
12. **Queues hold up to 8 orders, and a loop is a saved queue.** One limit for both, so any loop can be built by queueing it first.
13. **Standing floors are never lowered for the player.** When a market change makes a tender's floor unreachable, the sale is marked blocked, someone says so, and the player chooses what to do (see `03-ships-and-controls.md`).
14. **Regrowth is the only way a field refills.** A spent rock comes back only when regrowth brings it to 25% of its yield, so depletion is the units mined minus the regrowth, whatever the mining style.
15. **Beat conversations are not ambient talk.** A beat step's line may cite a fact that ambient dock talk already used, and every one has a fallback (see `05-people.md`).
16. **Money is whole centicredits in the sim.** Prices are computed per unit and rounded to 0.01 cr; totals are sums of rounded unit prices. Screens show credits with at most one decimal. See `08-contract.md` for this and the other fixed-point rules.
