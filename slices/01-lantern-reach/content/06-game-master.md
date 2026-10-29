# 06 The game master's eight changes, and Ancients chapter 1

Back to [SPEC.md](SPEC.md). The `gm` pack holds one Luau script per change type from the slice doc. A new save installs it. Each script uses only the declared hooks (event hooks, counters, world-state stores, history, canon graph, market levers, mechanic registry, order verbs, runtime asset kit, beat API, legibility channels). Each write carries a cause: the script ID plus the fact IDs it cites.

Rules every script below follows:

- **Calm application:** writes apply when the player is docked or paused, never during a mining run or a lane transit.
- **One big change per region:** the shortage, the dispute's change of hands and the sixth station never overlap in one region. A script whose change would overlap waits until the region is clear.
- **Legibility:** every change names a **place** (something visible changes there) and a **person** (an NPC with an eligible line citing the fact).
- **Shock and opportunity:** a script that worsens something must open a paired opportunity the player can reach.
- **Rollback:** each script declares an in-world rollback line and how it unwinds without taking anything from the player. Rollback writes its own `chronicle` entry.

## 1. Grow a station from deliveries: `gm.kettle_growth`

| Part | Behaviour |
| --- | --- |
| Trigger | A `sold` or `delivered` fact at Kettle Dock brings `ledger.kettle_greenhouse` to stage 2 (60 ice and 20 plate, cumulative) or stage 3 (120 ice and 50 plate). |
| Writes | `station.kettle_dock.stage` → 2 or 3. Asset kit parts added (see `01-galaxy.md`). Market lever: stage 2 adds a rations producer (base 15, target 120, +2 per pulse, uses 1 ice per pulse) and sets the ice drink to −4. Stage 3 adds a glassite consumer (base 30, target 40, −1, sat 0.04, recover 0.04), sets rations to +3, and widens the ring to 100 u. Then `stage_built{kettle_dock, stage}`. Cause: the delivery facts that completed the stage. After stage 1 and stage 2, it registers Odo's side goal for the next ring (a goal line, not one of the 17 beats): "Odo's greenhouse needs {ice} more ice and {plate} more plate for its next ring." |
| Legibility | Place: the new greenhouse ring on the station mesh and its map icon; map note "Kettle Dock: a new greenhouse ring". Person: Odo, `line.odo_fenn.03` or `.04`, citing `stage_built`; Ilse `.02` at the Wharf. |
| Opportunity | No shock. The new stock (rations to buy at Kettle; glassite to sell there from stage 3) is the opportunity. |
| Rollback | Odo: "We've taken the new ring down to re-seal it. Your ice is still counted, every bit." The stage, parts and new market entries revert. The ledger is untouched, so the stage comes back when a fixed script does. |

## 2. Run a shortage that reacts to history: `gm.ice_shortage`

Full rules in `04-journey.md` (b12).

| Part | Behaviour |
| --- | --- |
| Trigger | On `mined` or `rock_spent` in Rimewater: b11 complete, Rimewater at 70% or less, `player.units_mined.ice.lantern_shallows` >= 120, Shallows clear of other big changes. |
| Writes | `shortage_started` (cites the player's Rimewater `mined` facts). Market levers at Kettle Dock, Lantern Wharf and Cinder Hook: ice base x1.4 and ice consumption x2. Kettle's lights swap to `kit.light.dim_green`; Kettle's rations output x0.5. Registers b12 with its two ways and ledgers. Way B sets Rimewater regrowth x4. On resolution the levers revert, then the lasting write for the way taken: A adds Brassmouth to Little Mercy's route menu; B adds `kit.kettle.condenser` and lowers Kettle's ice drink by 2 per pulse; ignored raises Kettle's weight in Even Keel's route menu. |
| Legibility | Place: Kettle's lamps visibly dim; map note "Ice short at Kettle Dock, Lantern Wharf and Cinder Hook". Person: Odo gives the goal; Ilse `.05` cites `shortage_started`; Odo `.05` or `.06`, or Ilse `.06`, at the end. |
| Opportunity | Ice pays up to 21.8 (Kettle), 18.2 (Wharf) and 25.5 (Hook); Brassmouth sells it at about 9.2 and Hookside is free to mine. Way B leaves Kettle a permanent condenser. |
| Rollback | Ilse: "False alarm on the ice, as it turns out. Odo's lamps are back up. Whatever you sold, you keep." Levers and lights revert. Credits, standing gained and the condenser (if built) stay. |

## 3. Move faction territory: `gm.cinder_dispute`

Full rules in `04-journey.md` (b09).

| Part | Behaviour |
| --- | --- |
| Trigger | b07 complete and `player.dockings_at.cinder_hook` >= 3. |
| Writes | `dispute_started`; the two depot ledgers; tally updates from the player's sales (in the chosen name) and from the Ninefold's and Little Mercy's dockings. On change of hands: `station.cinder_hook.depot_owner`, territory border, banner part (`kit.banner.coop` or `kit.banner.compact`), the market base changes for that outcome, the losing side's opportunity lever, standing deeds, and `territory_changed` citing the tally facts. |
| Legibility | Place: the banner on Cinder Hook's depot, and the faction border moving on the map. Person: Rook `.04`; Ilse `.03` or `.04`; Saffron `.03` or `.04`; Hesper `.04`. |
| Opportunity | The losing side courts the player: Brassmouth glassite recovers faster (Compact lost), or Hook pays more for ice and Hookside regrows faster (Cooperative lost). |
| Rollback | Rook: "The banner's coming down while they sort the papers. Your plate's still in my walls, and your credits are still yours." The depot returns to contested with tallies kept. Market bases revert; standing already gained stays. |

## 4. Plant a rumor with a truth value: `gm.glassite_rumor`

Full rules in `04-journey.md` (b10).

| Part | Behaviour |
| --- | --- |
| Trigger | b10 preconditions hold and the player docks at Brassmouth. |
| Writes | `rumor.ember_shoals_vein {source = pip_calloway, truth, planted_by?}`. Truth is computed from the dispute tallies at that moment. `field.ember_shoals` is seeded (6 rich glassite rocks, or 3 iron rocks and the buoy), with the rumor's fact as cause. While false and active, the Ninefold's Hook ore cap is 20. `rumor_spread` on each NPC craft docking; `rumor_followed` within 500 u; `rumor_resolved`. |
| Legibility | Place: "Ember Shoals (rumored)" on the map; the rocks or the buoy at the site. Person: Pip `.01` to `.03`; Juno `.05`; Rook `.05`; Hesper `.02` or `.06`. |
| Opportunity | If false, the shock is one wasted trip. The opportunity is the buoy's proof, which is worth 300 cr and Cooperative standing when told to Rook or Ilse, and the 3 iron rocks. If true, there is no shock. |
| Rollback | Pip: "Forget the Shoals story. Saffron says I had it muddled." Unmined rumor rocks fade from the map. Anything the player already mined stays theirs. |

## 5. Add a new verb with a teacher: `gm.rock_tagging`

The tender is the `core` pack's first automated craft (b13), so this script shows the change type with **rock tagging**, the slice doc's other example.

| Part | Behaviour |
| --- | --- |
| Trigger | `cap.loops` unlocked, `player.rocks_spent` >= 10, and the player has docked at Cinder Hook since unlocking loops. |
| Writes | First, a demonstration: Juno's skiff `craft.magpie` flies to a Cinder Belt rock within sight of the player's next Cinder Belt visit, sets a lamp-buoy on it (`tagged{ship = magpie}`) and mines it. Next, at the next Hook docking, Juno offers to teach. On acceptance: the mechanic registry flag `mech.rock_tagging` for this save; the reserved input slot (key **T** near a rock; right-click a rock with click controls); the reserved UI slot (a lamp icon on tagged rocks and on the map); the `tag` verb in the order verb API; and `mine{field, prefer = tagged}`, which picks the richest tagged rock so loops survive a spent rock. Up to 6 tags at once. Writes `mechanic_taught{rock_tagging, juno_brask}`. |
| Legibility | Place: Juno's lamp burning on a Cinder Belt rock, then the player's own lamps on the map. Person: Juno `.01` cites Magpie's `tagged`; `.02` cites the player's third tag. |
| Opportunity | No shock. Tagged loops stall less, which matters for tenders. |
| Rollback | Juno: "Pull your lamps in for a bit, the buoys are acting up. Your loops will pick rocks the old way." Tags are cleared, `prefer = tagged` falls back to the richest rock, and the flag turns off. The mechanic is re-taught without a new demonstration when the script returns. |

## 6. Open an Ancients chapter: `gm.ancients_ch1`

Runs beat b15 (see `04-journey.md`) over the canon graph below.

| Part | Behaviour |
| --- | --- |
| Trigger | `fragment_found`. |
| Writes | Registers b15. At step 1 (Nell sees the fragment), opens `canon.fragment_is_lens_shard`. At step 2 (the fragment is carried through any lane), opens `canon.fragments_sing` and writes `canon_opened`; during that transit the lane lamps pulse in rings of light around the ship. At the choice, if the fragment is left, adds `kit.gdo.fragment_case` (a glow in the lens tower) and transfers the fragment to the Observatory with the player's consent; if kept, the fragment's hold icon hums near lane mouths. Pays the 400 cr fee and the standing. Writes the `chronicle` entry. |
| Legibility | Place: the singing lane lamps; the fragment case at the Observatory. Person: Nell `.02` and `.04`; Juno `.04`. |
| Opportunity | No shock. The chapter opens the `canon.second_fragment` hook for later chapters. |
| Rollback | Nell: "I've put my notes on the singing aside for now. The fragment is just as you left it." Opened nodes go back to sealed. The fragment stays wherever the player chose to put it. |

## 7. Create the sixth station: `gm.sixth_station`

| Part | Behaviour |
| --- | --- |
| Trigger | Evaluated at each docking: b13 complete, `player.dockings` >= 50, and at least two of b09, b10 and b12 resolved. |
| Choosing | Each candidate kind (below) whose requirement holds is scored by how many of **its own anchor facts** (listed per kind) lie within 900 u of its site. The highest score wins; ties go to the newest anchor fact. The placement must pass every rule in `01-galaxy.md`. |
| Writes | A new station `station.<kind>` assembled from runtime asset kit parts; its market tables; a new dockmaster NPC with a brief and at least 3 lines citing the founding facts; a map note; one NPC craft of the matching faction gets it in its route menu; and `station_founded{kind, faction, cites}`. |
| Legibility | Place: the station on the map and in space, with the map note "New: {station}". Person: the nearest authored dockmaster on the player's next docking there (Ilse `.10`, Hesper `.05`) and the new dockmaster. |
| Opportunity | The station is the opportunity: a new market near something the player did. |
| Rollback | The nearest dockmaster: "{station} has shut its ring while they fix the frame. They'll be back." The ring closes and the station stays as scaffold (`kit.scaffold`). Its market is disabled, and any tender loop order targeting it is skipped. The player owns nothing there, so nothing is lost. |

Example kinds the script can choose from:

| Kind | Site | Faction | Requirement | Anchor facts (scored) | Market (base prices) | Dockmaster brief (starting point) |
| --- | --- | --- | --- | --- | --- | --- |
| Meltwater Halt | (-100, 720) | Wharf Cooperative | Shortage helped (A or B), or 4 or more Rimewater rocks spent by the player | The player's `mined` and `rock_spent` at Rimewater; `delivered` to `ledger.ice_relief` or `ledger.kettle_condenser` | Buys plate 40, rations 20; sells ice 9 (+2 per pulse) | Wynn Halloran, a retired ice cutter. Speaks slowly, counts rocks by name. Remembers the shortage and who helped. |
| Shoals Prospect | (6000, 1900) | None | True rumor followed and 20 or more glassite mined at the Shoals by the player | The player's `mined` at the Shoals; `rumor_followed` | Sells glassite 18; buys rations 28, ice 15, plate 50 | Dace Morrow, a lucky prospector. Talks fast and owes the player a drink, and says so. |
| The Listening Post | (4600, 1300) | Lamplighter Society | `canon_opened{fragments_sing}` | `canon_opened`, and the `jumped` fact of the transit in which the fragment sang | Buys glassite 40, rations 24 | Brother Amsel, Nell's apprentice. Whispers, and listens to the lane all day. |
| Tollhouse Annex | (2560, -3990) | Tollway Compact | Depot went to the Compact, and the player sold 60 or more glassite at Brassmouth | The player's `sold{ware = glassite}` at Brassmouth | Buys glassite 44 (its own saturation: a second buyer in the Narrows), iron ore 15; sells plate 27 | Corvin Ash, Saffron's junior. Eager to impress, keeps a pocket ledger. |

The sites are the examples checked in `01-galaxy.md`. If a kind's anchor facts lie elsewhere (for example, the fragment sang in a different lane), the script looks for the nearest site within 900 u of them that passes every placement rule, and drops that kind if there is none.

## 8. Write the chronicle: `gm.chronicle`

| Part | Behaviour |
| --- | --- |
| Trigger | Any fact of these kinds: `stage_built`, `shortage_started`, `shortage_resolved`, `territory_changed`, `rumor_resolved`, `canon_opened`, `station_founded`, `trait_earned`, `prestige`, `tender_acquired`, `mechanic_taught`, `regard_changed` (to Kin), and every rollback. |
| Writes | A `chronicle{entry, cites, place, person}` entry: one sentence of at most 25 words from a template filled with fact fields. The chronicle screen lists entries in order under "The Lantern Reach", with a "You" filter for entries where the player is an actor. Nothing in it asks for action. |
| Legibility | Place: the entry names it. Person: every dockmaster's "What changed?" answer draws on these entries since the player's last docking at that station, using the NPC's own line for the fact when one exists, and the entry sentence in their voice otherwise. Ilse's b17 answer covers everything since prestige. |
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

Scripts cite canon nodes, and the canon check rejects any script that contradicts a node or asserts a forbidden one.

| Status | Meaning |
| --- | --- |
| locked | True canon from the start. Scripts and NPCs may say it, never change it. |
| sealed | True, but not yet revealed. Only the named script may open it, at the named step. |
| open | Revealed. Now behaves like locked. |
| question | Deliberately unanswered. Scripts may raise it or have an NPC guess (phrased as a guess), but not answer it in chapter 1. |
| forbidden | Must never be asserted by any script or line in chapter 1. |

| Node | Status at start | Statement | Opened by |
| --- | --- | --- | --- |
| `canon.lantern_makers` | locked | Long before the Reach was settled, a people the Lamplighters call the Lantern-makers built the lane currents. Nobody knows what they looked like. | |
| `canon.lane_lanterns` | locked | The lamps along the lanes are Lantern-maker work. The Lamplighter Society was founded to keep them lit, and doesn't know how they work. | |
| `canon.glass_drift_lens` | locked | The Glass Drift is the wreck of one great lens. Glassite is its dust. | |
| `canon.no_fragment_trade` | locked | The Society never buys or sells fragments. It keeps them, or returns them to whoever found them. | |
| `canon.fragment_is_lens_shard` | sealed | The fragment is an unbroken shard of the great lens, not dust. It is warm and can't be scratched. | `gm.ancients_ch1`, b15 step 1 |
| `canon.fragments_sing` | sealed | Carried through a lane current, a fragment sings in tune with the lane lamps. | `gm.ancients_ch1`, b15 step 2 |
| `canon.second_fragment` | sealed | The fragment's song is loudest when facing Brassmouth Narrows. (A direction only, never a location.) | A later chapter, and only after b15 completes |
| `canon.lens_purpose` | question | What the great lens was for. | Not in chapter 1 |
| `canon.who_broke_the_lens` | question | Who or what broke it. | Not in chapter 1 |
| `canon.no_living_ancients` | forbidden | Any claim that a Lantern-maker is alive or present in the Reach, or that an Ancients object grants power over ships, minds or prices. | Never in chapter 1 |

Edges (each "supports" the next): `lantern_makers` → `lane_lanterns`; `lantern_makers` → `glass_drift_lens` → `fragment_is_lens_shard` → `fragments_sing` → `second_fragment`; `fragments_sing` relates to `lane_lanterns`; `glass_drift_lens` raises `lens_purpose` and `who_broke_the_lens`; `no_fragment_trade` explains why `ware.fragment` is not for sale.
