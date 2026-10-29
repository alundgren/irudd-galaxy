# 06 The game master's eight changes, and Ancients chapter 1

Back to [SPEC.md](SPEC.md). The `gm-examples` pack (`packs/gm-examples`, manifest `id = "gm"`) holds one Luau script per change type from the slice doc. A new save installs it, so all eight show up in play. Each script uses only the declared hooks (event hooks, counters, world-state stores, history, canon graph, market levers, mechanic registry, order verbs, runtime asset kit, beat API, legibility channels). Each write carries a cause: the script ID plus the fact IDs it cites.

## How the pack sits on `core`

`core` carries all 17 beats, including the dispute, the rumor, the shortage and the fragment (`04-journey.md`). `gm-examples` demonstrates the eight change types in two ways:

- **Varying the authored beats and places.** It rewrites a dormant beat's tunables from history (which stations a shortage touches, who planted a rumor), draws what `core` changes onto the map (faction borders), and opens what comes after an authored chapter (the next canon node). It never registers a beat, never pays a beat's reward and never blocks one.
- **Changes only the game master makes.** Kettle Dock's stages 2 and 3, rock tagging, the sixth station and the chronicle.

| # | Change type (slice doc) | What `core` authors | What `gm-examples` adds |
| --- | --- | --- | --- |
| 1 | Grow a station from deliveries | Kettle stage 1 (b03) | `gm.kettle_growth`: stages 2 and 3, counting ordinary sales at Kettle |
| 2 | Run a shortage that reacts to history | b12: Rimewater ice shortage, two ways, an ignored outcome | `gm.shortage_tuning`: the shock lands where the player's own Rimewater ice went, and way A's quota follows how much they sold |
| 3 | Move faction territory | b09: the depot changes owner, banner and markets | `gm.territory_map`: faction borders drawn on the map, moved at the handover |
| 4 | Plant a rumor with a truth value | b10: the Ember Shoals rumor, true or a stale tale | `gm.compact_plant`: the Compact plants it if the player leans Cooperative, with a buoy to expose |
| 5 | Add a new verb with a teacher | b13: the tender and the patient sell | `gm.rock_tagging`: Juno teaches rock tagging |
| 6 | Open an Ancients chapter | b15: chapter 1 (the fragment sings) | `gm.ancients_bearing`: opens the next sealed node and marks its bearing |
| 7 | Create the sixth station | nothing | `gm.sixth_station` |
| 8 | Write the chronicle | nothing (dockmasters answer "What changed?" from their own lines) | `gm.chronicle` |

Rules every script below follows:

- **Never required.** Removing the pack never stops a beat from starting or finishing. The validator rejects a `gm` script that registers a beat, writes a beat's step, pays credits or standing for a beat, or that `core` refers to.
- **Varies through tunables, not copies.** To change an authored beat, a script writes that beat's tunables (`08-contract.md`) while the beat is dormant, citing the facts it read. The beat reads them when it starts. Once a beat has started, its tunables are frozen for that run.
- **Calm application:** writes that change the world apply when the player is docked or paused, never during a mining run or a lane transit.
- **One big change per region:** the shortage, the dispute's change of hands and the sixth station never overlap in one region. A script whose change would overlap waits until the region is clear.
- **Legibility:** every change names a **place** (something visible changes there) and a **person** (an NPC with an eligible line citing the fact).
- **Shock and opportunity:** a script that worsens something must open a paired opportunity the player can reach.
- **Rollback:** each script declares an in-world rollback line and how it unwinds without taking anything from the player. A rolled-back tunable goes back to its authored value for the beat's next start. Rollback writes its own `chronicle` entry.

## 1. Grow a station from deliveries: `gm.kettle_growth`

| Part | Behaviour |
| --- | --- |
| Trigger | From Kettle stage 1 (b03, `core`) on, every unit of ice or plate any ship of the player's sells at Kettle Dock adds to `ledger.kettle_greenhouse` (cause: the `sell` fact). When the ledger reaches stage 2 (60 ice and 20 plate, cumulative, b03's 20 ice included) or stage 3 (120 ice and 50 plate), the stage is built at the next calm moment. |
| Writes | `station.kettle_dock` stage → 2 or 3. Asset kit parts added (see `01-galaxy.md`). Market lever: stage 2 adds a rations producer (base 15, target 120, +2 per pulse, uses 1 ice per pulse) and sets the ice drink to −4. Stage 3 adds a glassite consumer (base 30, target 40, −1, sat 0.04, recover 0.04), sets rations to +3, and widens the ring to 100 u. Then `stage_built{kettle_dock, stage}`. Cause: the facts that completed the stage. After stage 1 and stage 2, it adds Odo's side goal for the next ring (a dock-talk request with its own goal line, never one of the 17 beats): "Odo's greenhouse needs {ice} more ice and {plate} more plate for its next ring." |
| Legibility | Place: the new greenhouse ring on the station mesh and its map icon; map note "Kettle Dock: a new greenhouse ring". Person: Odo, `line.odo_fenn.03` or `.04`, citing `stage_built`; Ilse `.02` at the Wharf. |
| Opportunity | No shock. The new stock (rations to buy at Kettle; glassite to sell there from stage 3) is the opportunity. |
| Rollback | Odo: "We've taken the new ring down to re-seal it. Your ice is still counted, every bit." The stage, parts and new market entries revert. The ledger is untouched, so the stage comes back when a fixed script does. |

## 2. Run a shortage that reacts to history: `gm.shortage_tuning`

Varies `core`'s b12 (`04-journey.md`, "The ice shortage"). Without this script the shortage touches Kettle Dock, Lantern Wharf and Cinder Hook and asks 40 units for way A.

| Part | Behaviour |
| --- | --- |
| Trigger | Each `sell{ware = ice, origin = field.rimewater}` by `ship.player`, while `shortage.rimewater_ice` hasn't started. |
| Writes | Tunables, citing the `sell` facts it summed: `shortage.rimewater_ice.stations` = Kettle Dock plus the two other ice buyers where the player has sold the most Rimewater ice (ties, and any slot with no sales, fall back to the authored order: Lantern Wharf, Cinder Hook, Glass Drift Observatory). `shortage.rimewater_ice.way_a_quota` = half of the Rimewater ice the player sold at those stations, rounded to the nearest 10 and kept between 30 and 60. It never touches the price or drink factors, the trigger, or any reward. When the shortage starts, `core` writes `shortage_started{stations, quota, tuned = true}`. |
| Legibility | Place: the shock lands on the stations the player's ice went to; their map icons show the shortage note "Ice short at {stations}". Person: Odo opens with `.09`, citing `shortage_started{tuned = true}` and naming the station that took the most of the player's ice. |
| Opportunity | Same as the authored shortage: ice pays up to 1.4 x base at each of the shortage's stations, Brassmouth sells it cheap and Hookside is free to mine. A player whose ice went to the Observatory finds a third high buyer there. |
| Rollback | Odo: "We counted the ice wrong. It's the usual three stations that are short." The tunables return to their authored values. If the shortage has already started, it plays out as it began (tunables are frozen once a beat starts); the chronicle notes the rollback. |

## 3. Move faction territory: `gm.territory_map`

Draws `core`'s b09 handover (`04-journey.md`, "The Cinder Hook dispute") onto the map. Without this script, the map shows only each station's owner colour.

| Part | Behaviour |
| --- | --- |
| Trigger | A new save (draws the starting borders), and each `territory_changed{cinder_hook}`. |
| Writes | Map zones (`08-contract.md`): `zone.wharf_coop.shallows` around Lantern Wharf and Kettle Dock, `zone.tollway_compact.narrows` around Brassmouth Exchange, `zone.lamplighters.glass_drift` around the Observatory, and a hatched `zone.contested.cinder_hook` (radius 700 u around the Hook, taking in the Cinder Belt and Hookside). On `territory_changed`, at the next calm moment: the hatched zone is removed and the winner gets a new zone `zone.<faction>.cinder_hook` over the same area, drawn in their colour; the change is shown as the colour spreading out from the depot's banner. Writes `border_moved{faction, zone, station}` citing `territory_changed`. |
| Legibility | Place: the border on the map, and the map note "The {faction} border now takes in Cinder Hook". Person: Hesper `.07`, citing `border_moved`; Rook `.04` and Ilse `.03`/`.04` already cite the `core` fact. |
| Opportunity | No shock: the border is a drawing. The authored outcome's paired opportunity (`04-journey.md`) stands. |
| Rollback | Hesper: "The map-makers are redrawing the Hook. The lamps don't mind." The zones go back to what they were before the handover. The depot's owner, banner and markets are `core`'s and don't change. |

## 4. Plant a rumor with a truth value: `gm.compact_plant`

Varies `core`'s b10 (`04-journey.md`, "The rumor"). Without this script, the rumor's truth comes from the craft that brought it to Brassmouth.

| Part | Behaviour |
| --- | --- |
| Trigger | While `rumor.ember_shoals_vein` doesn't exist: each `side_chosen`, each `sell` of plate or rations at Cinder Hook in a side's name, and `territory_changed{cinder_hook}`. The player **leans Cooperative** if the depot went to the Cooperative, or their own contributions to the Cooperative tally exceed those to the Compact tally. |
| Writes | While the player leans Cooperative: the tunable `rumor.ember_shoals.override = {truth = false, planted_by = faction.tollway_compact, source = "a Compact surveyor off the Ninefold"}`, citing the tally facts. When the lean flips back, it clears the override. `core` reads the override when Pip tells the rumor and records `planted_by` on the `Rumor`. If the rumor was planted: at the Shoals, a Compact survey buoy `prop.compact_buoy` sits beside the old claim marker. Pressing Space within 40 u of it reads its log and writes `rumor_exposed{planted_by = faction.tollway_compact}`. At the player's next docking with Rook or Ilse, a dock-talk choice "Someone planted the Shoals rumor" gives Coop +5, Compact −3, and 300 cr from Rook (`.08`). While the planted rumor is active, the Ninefold buys up to 20 Hook ore per docking instead of 10 (a trader-menu lever). |
| Legibility | Place: the buoy at the Shoals. Person: Rook `.08` and Pip `.06`, citing `rumor_exposed`. |
| Opportunity | The shock is one wasted trip and a busier Ninefold at the Hook. The opportunity is the buoy's proof: 300 cr and Cooperative standing, plus the 3 iron rocks `core` already lays out. |
| Rollback | Pip: "Forget what I said about the surveyor. Saffron says I had it muddled." The override clears. A buoy not yet read fades from the map. Credits and standing already given stay. |

## 5. Add a new verb with a teacher: `gm.rock_tagging`

The tender is `core`'s first automated craft (b13), so this script shows the change type with **rock tagging**, the slice doc's other example.

| Part | Behaviour |
| --- | --- |
| Trigger | `cap.loops` unlocked, `player.rocks_spent` >= 10, and the player has docked at Cinder Hook since unlocking loops. |
| Writes | First, a demonstration: Juno's skiff `craft.magpie` flies to a Cinder Belt rock within sight of the player's next Cinder Belt visit, sets a lamp-buoy on it (`tagged{ship = magpie}`) and mines it. Next, at the next Hook docking, Juno offers to teach. On acceptance: the mechanic registry flag `mech.rock_tagging` for this save; the reserved input slot (key **T** near a rock; right-click a rock with click controls); the reserved UI slot (a lamp icon on tagged rocks and on the map); the `tag` verb in the order verb API; and `mine{field, prefer = tagged}`, which picks the richest tagged rock so loops survive a spent rock. Up to 6 tags at once. Writes `mechanic_taught{rock_tagging, juno_brask}`. |
| Legibility | Place: Juno's lamp burning on a Cinder Belt rock, then the player's own lamps on the map. Person: Juno `.01` cites Magpie's `tagged`; `.02` cites the player's third tag. |
| Opportunity | No shock. Tagged loops stall less, which matters for tenders. |
| Rollback | Juno: "Pull your lamps in for a bit, the buoys are acting up. Your loops will pick rocks the old way." Tags are cleared, `prefer = tagged` falls back to the richest rock, and the flag turns off. The mechanic is re-taught without a new demonstration when the script returns. |

## 6. Open an Ancients chapter: `gm.ancients_bearing`

`core`'s b15 runs Ancients chapter 1 over the canon graph below: Nell sees the fragment (`canon.fragment_is_lens_shard` opens), the fragment sings in a lane (`canon.fragments_sing` opens, and the lane lamps pulse in rings of light around the ship during that transit), and the player leaves the fragment with the Observatory (`part.gdo.fragment_case` glows in the lens tower) or keeps it. This script opens what comes after.

| Part | Behaviour |
| --- | --- |
| Trigger | `beat_completed{b15}`, at the player's next docking at the Glass Drift Observatory. |
| Writes | Opens `canon.second_fragment` (sealed until b15 completes) and writes `canon_opened{second_fragment}`. Adds a map layer: a faint amber bearing line starting at the fragment (the Observatory if it was left there, the player's ship if kept) and pointing toward Brassmouth Narrows, with no end point, because the node gives a direction only. If the fragment was kept, its hold icon hums louder while the ship faces the Narrows. |
| Legibility | Place: the bearing on the map. Person: Nell `.08`, citing `canon_opened{second_fragment}`. |
| Opportunity | No shock. It opens the hook a second chapter would build on, inside the canon locks: nothing says where a second fragment is, or that one exists. |
| Rollback | Nell: "I've put my notes on the song's direction aside for now. The fragment is just as you left it." The node goes back to sealed and the bearing is removed. The fragment stays wherever the player chose to put it. |

## 7. Create the sixth station: `gm.sixth_station`

| Part | Behaviour |
| --- | --- |
| Trigger | Evaluated at each docking: b13 complete, `player.dockings` >= 50, and at least two of b09, b10 and b12 resolved (all three are `core` beats). |
| Choosing | Each candidate kind (below) whose requirement holds is scored by how many of **its own anchor facts** (listed per kind) lie within 900 u of its site. The highest score wins; ties go to the newest anchor fact. The placement must pass every rule in `01-galaxy.md`. |
| Writes | A new station `station.<kind>` assembled from runtime asset kit parts; its market tables; a new dockmaster NPC with a brief and at least 3 lines citing the founding facts; a map note; one NPC craft of the matching faction gets it in its route menu; and `station_founded{kind, faction, cites}`. |
| Legibility | Place: the station on the map and in space, with the map note "New: {station}". Person: the nearest authored dockmaster on the player's next docking there (Ilse `.10`, Hesper `.05`) and the new dockmaster. |
| Opportunity | The station is the opportunity: a new market near something the player did. |
| Rollback | The nearest dockmaster: "{station} has shut its ring while they fix the frame. They'll be back." The ring closes and the station stays as scaffold (`part.scaffold`). Its market is disabled, and any tender loop order targeting it is skipped. The player owns nothing there, so nothing is lost. |

Example kinds the script can choose from:

| Kind | Site | Faction | Requirement | Anchor facts (scored) | Market (base prices) | Dockmaster brief (starting point) |
| --- | --- | --- | --- | --- | --- | --- |
| Meltwater Halt | (-100, 720) | Wharf Cooperative | Shortage helped (A or B), or 4 or more Rimewater rocks spent by the player | The player's `mine` and `rock_depleted` at Rimewater; `deliver` to `ledger.ice_relief` or `ledger.kettle_condenser` | Buys plate 40, rations 20; sells ice 9 (+2 per pulse) | Wynn Halloran, a retired ice cutter. Speaks slowly, counts rocks by name. Remembers the shortage and who helped. |
| Shoals Prospect | (6000, 1900) | None | True rumor followed and 20 or more glassite mined at the Shoals by the player | The player's `mine` at the Shoals; `rumor_followed` | Sells glassite 18; buys rations 28, ice 15, plate 50 | Dace Morrow, a lucky prospector. Talks fast and owes the player a drink, and says so. |
| The Listening Post | (4600, 1300) | Lamplighter Society | `canon_opened{fragments_sing}` | `canon_opened`, and the `jump` fact of the transit in which the fragment sang | Buys glassite 40, rations 24 | Brother Amsel, Nell's apprentice. Whispers, and listens to the lane all day. |
| Tollhouse Annex | (2560, -3990) | Tollway Compact | Depot went to the Compact, and the player sold 60 or more glassite at Brassmouth | The player's `sell{ware = glassite}` at Brassmouth | Buys glassite 44 (its own saturation: a second buyer in the Narrows), iron ore 15; sells plate 27 | Corvin Ash, Saffron's junior. Eager to impress, keeps a pocket ledger. |

The sites are the examples checked in `01-galaxy.md`. If a kind's anchor facts lie elsewhere (for example, the fragment sang in a different lane), the script looks for the nearest site within 900 u of them that passes every placement rule, and drops that kind if there is none.

## 8. Write the chronicle: `gm.chronicle`

| Part | Behaviour |
| --- | --- |
| Trigger | Any fact of these kinds: `stage_built`, `shortage_started`, `shortage_resolved`, `territory_changed`, `border_moved`, `rumor_resolved`, `rumor_exposed`, `canon_opened`, `station_founded`, `trait_earned`, `prestige`, `tender_acquired`, `mechanic_taught`, `regard_changed` (to Kin), and every rollback. |
| Writes | A `chronicle{entry, cites, place, person}` entry: one sentence of at most 25 words from a template filled with fact fields. The chronicle screen lists entries in order under "The Lantern Reach", with a "You" filter for entries where the player is an actor. Nothing in it asks for action. |
| Legibility | Place: the entry names it. Person: every dockmaster's "What changed?" answer may draw on these entries, using the NPC's own line for the fact when one exists, and the entry sentence in their voice otherwise. Ilse's b17 answer may use them as extra candidates; it doesn't need them. |
| Opportunity | No shock. |
| Rollback | A rolled-back change gets its own entry, for example "Kettle Dock's second ring came down for re-sealing." Entries are never deleted. |

Entry templates (examples):

| Fact | Template |
| --- | --- |
| `stage_built` | Kettle Dock lit its {stage_word} greenhouse ring, built from {pilot}'s ice and plate. |
| `territory_changed` | The Cinder Hook depot came under the {faction}, after {who} kept it in plate. |
| `shortage_resolved` | Rimewater ran thin; {who} brought the Shallows through it. |
| `rumor_resolved` | The Ember Shoals rumor turned out {truth_word}. {pilot} {followed_phrase}. |
| `canon_opened` | A fragment found by {pilot} sang in the {lane} lane, and the Lamplighters wrote it down. |
| `station_founded` | {station} opened near {anchor}, because of {cause_phrase}. |
| `prestige` | {pilot} traded the Wren for a {hull} at Lantern Wharf. The Wren rests at berth six. |

## Ancients chapter 1: canon graph

The canon graph is `core` data (`canon.toml`), read through the canon hook. Scripts and lines cite canon nodes, and the canon check rejects any script or line that contradicts a node or asserts a forbidden one.

| Status | Meaning |
| --- | --- |
| locked | True canon from the start. Scripts and NPCs may say it, never change it. |
| sealed | True, but not yet revealed. Only the named opener may open it, at the named step. |
| open | Revealed. Now behaves like locked. |
| question | Deliberately unanswered. Scripts may raise it or have an NPC guess (phrased as a guess), but not answer it in chapter 1. |
| forbidden | Must never be asserted by any script or line in chapter 1. |

| Node | Status at start | Statement | Opened by |
| --- | --- | --- | --- |
| `canon.lantern_makers` | locked | Long before the Reach was settled, a people the Lamplighters call the Lantern-makers built the lane currents. Nobody knows what they looked like. | |
| `canon.lane_lanterns` | locked | The lamps along the lanes are Lantern-maker work. The Lamplighter Society was founded to keep them lit, and doesn't know how they work. | |
| `canon.glass_drift_lens` | locked | The Glass Drift is the wreck of one great lens. Glassite is its dust. | |
| `canon.no_fragment_trade` | locked | The Society never buys or sells fragments. It keeps them, or returns them to whoever found them. | |
| `canon.fragment_is_lens_shard` | sealed | The fragment is an unbroken shard of the great lens, not dust. It is warm and can't be scratched. | `core`, b15 step 1 |
| `canon.fragments_sing` | sealed | Carried through a lane current, a fragment sings in tune with the lane lamps. | `core`, b15 step 2 |
| `canon.second_fragment` | sealed | The fragment's song is loudest when facing Brassmouth Narrows. (A direction only, never a location.) | `gm.ancients_bearing`, or a later chapter; only after b15 completes |
| `canon.lens_purpose` | question | What the great lens was for. | Not in chapter 1 |
| `canon.who_broke_the_lens` | question | Who or what broke it. | Not in chapter 1 |
| `canon.no_living_ancients` | forbidden | Any claim that a Lantern-maker is alive or present in the Reach, or that an Ancients object grants power over ships, minds or prices. | Never in chapter 1 |

Edges (each "supports" the next): `lantern_makers` → `lane_lanterns`; `lantern_makers` → `glass_drift_lens` → `fragment_is_lens_shard` → `fragments_sing` → `second_fragment`; `fragments_sing` relates to `lane_lanterns`; `glass_drift_lens` raises `lens_purpose` and `who_broke_the_lens`; `no_fragment_trade` explains why `ware.fragment` is not for sale.
