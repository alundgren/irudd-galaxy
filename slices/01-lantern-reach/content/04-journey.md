# 04 The journey: 17 beats

Back to [SPEC.md](SPEC.md). Beats register through the beat API (preconditions, steps, end state). `core` beats live in the `core` pack. `gm` beats are registered by the named `gm` script (see `06-game-master.md`) and never fire without the `gm` pack.

**How goals show:** the HUD shows one goal line at a time, the active beat's current step. When several beats are active, the newest stays in the HUD and the others wait in the dockmaster's "anything for me?" list. A goal never shows a timer, a counter bar or a percentage. Quantities appear in words ("20 ice"). Every beat can be ignored, and the sandbox carries on.

**Acts:** Act 1 is b01 to b05. Act 2 starts when b06 starts. Act 3 starts when b11 starts. Act 4 starts when b15 or b16 starts, whichever comes first.

## Beat table

| ID | Act | Owner | Giver | Goal line (what the player reads) | Preconditions | End state | Rewards and unlocks | History facts written |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| `beat.b01_undock` | 1 | core | Ilse Marrow | Ilse needs ice for the Kettle Dock greenhouse. Undock and look for the pale rocks. | New save: docked at Lantern Wharf in a Wren, 40 cr, empty hold | `ship.player` undocked from the Wharf | Starts b02 | `beat_started`, `undocked`, `beat_completed` |
| `beat.b02_mine_by_hand` | 1 | core | Ilse Marrow | Hold Space beside an ice rock until your hold is full. | b01 complete | At least 20 ice in the hold | Starts b03 | `mined`, `beat_completed` |
| `beat.b03_first_delivery` | 1 | core | Ilse Marrow (Odo Fenn receives) | Fly into Kettle Dock's ring, press Space, and give Odo the ice. | b02 complete | 20 ice handed to `ledger.kettle_greenhouse` | 120 cr; Kettle Dock stage 1 (a fogged glass dome, for good); Coop standing +5; mining lens on Orrin's list | `docked`, `delivered`, `stage_built{kettle_dock, 1}`, `standing_changed`, `beat_completed` |
| `beat.b04_first_sale` | 1 | core | Orrin Vale | Orrin can fit a mining lens for 600 credits. Sell what you mine to pay for it. | b03 complete | `upgrade.mining_lens` fitted | Mining rate x1.5. The first sale shows the price dipping unit by unit. | `sold`, `upgrade_fitted`, `beat_completed` |
| `beat.b05_pause` | 1 | core | Ilse Marrow | Press P or Esc whenever you like. The whole galaxy waits for you. | b04 complete | The PAUSED card has opened since b04 started (if it already has, b05 completes on the next undock) | Act 1 done; nav chip on Orrin's list once `player.dockings` >= 6 | `paused`, `beat_completed` |
| `beat.b06_nav_chip` | 2 | core | Orrin Vale | Orrin has a nav chip: click where you want the Wren to go. | b05 complete and `player.dockings` >= 6 | `upgrade.nav_chip` fitted | `cap.click_fly`. Orrin then offers the rock cradle and the docking clamp as their own conditions are met; they aren't part of this end state. | `upgrade_fitted`, `cap_unlocked`, `beat_completed` |
| `beat.b07_cinder_drift` | 2 | core | Ilse Marrow (Rook Adeyemi receives) | Ilse has 20 plate for Rook at Cinder Hook. The Cinder lane starts east of the Wharf. | b06 complete | 20 consigned plate handed to `ledger.hook_consignment` at Cinder Hook. Ilse loads it when the hold has 20 free; until then: "Make room for 20 plate and Ilse will load it." | 250 cr; `upgrade.lane_chart` (`cap.map_travel`); Coop +4; Cinder Hook's first snapshot | `jumped`, `docked`, `delivered`, `scouted`, `upgrade_fitted`, `cap_unlocked`, `beat_completed` |
| `beat.b08_chasing_a_gap` | 2 | core | Rook Adeyemi | Rook says Brassmouth pays well for glassite. Go and see how well, twice. | b07 complete | At least 30 glassite sold at Brassmouth Exchange across at least 2 separate dockings | Compact standing +3 on the first sale (Saffron learns the player's name); one of the order slate's conditions | `sold`, `market_flooded` (likely), `scouted`, `standing_changed`, `beat_completed` |
| `beat.b09_hook_dispute` | 2 | gm: `gm.cinder_dispute` | Rook Adeyemi | The Cooperative and the Compact both want the Hook's ore depot. Supply it in whose name you like, or not at all. | b07 complete and `player.dockings_at.cinder_hook` >= 3 | The depot has an owner (see "The Cinder Hook dispute") | 300 cr if the side you backed wins; the depot changes hands on the map and in the station art; market changes | `dispute_started`, `side_chosen`, `delivered`, `territory_changed`, `standing_changed`, `chronicle` |
| `beat.b10_rumor` | 2 | gm: `gm.glassite_rumor` | Pip Calloway | Pip heard of a fat glassite vein at the Ember Shoals, out on Cinder Drift's far edge. | b08 complete, `player.dockings_at.brassmouth_exchange` >= 3, and b09 started | The rumor is resolved: followed, or spread to all 5 stations | A rich vein, or a planted buoy and a 300 cr thank-you (see "The rumor") | `rumor_heard`, `rumor_spread`, `rumor_followed`, `rumor_resolved`, `chronicle` |
| `beat.b11_orders` | 3 | core | Orrin Vale | Step 1: Orrin can fit an order slate. Shift-click to line up orders. Step 2: Save a loop between two stations you trust, and let it run twice. | b08 complete, `cap.map_travel`, and `player.dockings` >= 26 | Step 1: order slate fitted (2000 cr). After `player.queues_done` >= 4, Orrin fits the loop log free. Step 2: `player.loop_laps` >= 2 | `cap.queues`, then `cap.loops` | `upgrade_fitted` (x2), `cap_unlocked` (x2), `queue_done`, `loop_saved`, `loop_lap`, `beat_completed` |
| `beat.b12_ice_shortage` | 3 | gm: `gm.ice_shortage` | Odo Fenn | Rimewater is thin, and Kettle Dock is short of ice. Odo has two ideas. | b11 complete, `field.rimewater` at 70% or less of full, `player.units_mined.ice.lantern_shallows` >= 120, and no other big change active in the Shallows | The shortage is resolved: way A, way B, or ignored (see "The ice shortage") | High ice prices while it lasts; a lasting change at Kettle Dock depending on the way | `shortage_started`, `shortage_helped`, `shortage_resolved`, `delivered`, `standing_changed`, `chronicle` |
| `beat.b13_first_tender` | 3 | core | Tamsin Oddway | Step 1: Tamsin Oddway at Kettle Dock wants to show you her tender. Step 2: Tamsin will build you a tender for 2400 credits. Step 3: Give Thimble one of your loops and a floor price. | b11 complete and `player.loop_laps` >= 4 | Thimble completes a loop lap that includes a patient sell | `cap.automation`; Coop +3 | `mechanic_taught{tender, tamsin_oddway}`, `patient_wait` and `patient_sold` (Little Pail), `tender_acquired`, `tender_assigned`, `patient_sold` (Thimble), `loop_lap`, `beat_completed` |
| `beat.b14_pilot_changes` | 3 | core | Orrin Vale | Orrin says you fly differently these days. Ask him what he means. | b13 complete and at least one trait earned | The player asked Orrin at the Wharf | Orrin names the deed and the effect in one line (from the trait's line in `05-people.md`) | `npc_line`, `beat_completed` (the `trait_earned` fact was written when the trait was earned) |
| `beat.b15_fragment` | 4 | gm: `gm.ancients_ch1` | Nell Quillon | Step 1: Nell Quillon at Glass Drift would love to see what you found in that rock. Step 2: Nell asks you to carry the fragment through a lane, and listen. Step 3: Tell Nell what you heard. | `fragment_found` exists (see `01-galaxy.md`) | Step 3 done, and the player chose to leave the fragment with the Observatory or keep it | Canon node `canon.fragments_sing` opens; 400 cr survey fee; Lamplighter +15 (+5 more if left); if left, the fragment glows in the Observatory's lens tower for good | `beat_step`, `canon_opened`, `standing_changed`, `chronicle`, `beat_completed` |
| `beat.b16_hullwrights_question` | 4 | core | Orrin Vale | Orrin asks whether the Wren is still the ship for you. | b13 and b14 complete, `player.dockings` >= 60, docked at the Wharf | Prestige done: Tandem or Heron | The new hull; fee 8000 cr; hull ranks reset; everything else stays (see `03-ships-and-controls.md`) | `prestige`, `beat_completed`, `chronicle` |
| `beat.b17_coming_home` | 4 | core, plus `gm.chronicle` and `gm.sixth_station` when installed | Ilse Marrow | Ilse wants to hear how the new hull flies. Ask her what changed while you were out. | b16 complete; since `prestige`, at least 3 counted dockings at other stations, at least one outside the Shallows | The player asked Ilse "What changed?" at the Wharf | Ilse tells up to 3 changes, most specific first; a chronicle entry naming the pilot; a quiet end card ("The Lantern Reach. The galaxy keeps going.") that closes with any key, and play goes on | `asked_what_changed`, `npc_line` (up to 3), `chronicle`, `beat_completed` |

Without the `gm` pack, Ilse's b17 answer comes from `core` facts only (stage 1, territory unchanged, the tender, the trait, the prestige), using her fallback lines.

## The Cinder Hook dispute (b09)

**Situation.** Cinder Hook's ore depot is where the camp weighs and sells its ore. The Wharf Cooperative wants fair ore prices for its smelter and miners. The Tollway Compact wants to control the Cinder ore trade. Rook just wants plate.

**Taking a side.** Rook's dock talk offers three choices, and the player can change at any time: "Supply in the Cooperative's name", "Supply in the Compact's name", "In nobody's name". Each change writes `side_chosen`.

**Tallies.** Two ledgers, `ledger.hook_depot_coop` and `ledger.hook_depot_compact`, both start at 0.

- Each unit of plate the player or their tender sells at Cinder Hook while a side is chosen adds 1 to that side's tally. Rations add 0.5. The player is still paid market price.
- Each counted docking at Cinder Hook by `craft.ninefold` adds 4 to the Compact tally; each by `craft.little_mercy` adds 3 to the Cooperative tally.

**Change of hands.** When one tally reaches 100 and leads the other by 30 or more, the depot changes hands at the next calm moment (the player docked or paused).

**If ignored.** NPC traffic alone tips it to the Compact after roughly 55 to 60 minutes of play (Compact gains about 1.8 a minute, Cooperative about 1.2). A player who backs a side with about 3 to 4 holds of plate decides it themselves well before then.

| Outcome | Place (what changes visibly) | Market and world | Standing | Paired opportunity |
| --- | --- | --- | --- | --- |
| Cooperative wins | Depot flies the green Cooperative lantern banner (`kit.banner.coop`); Cinder Hook joins the Cooperative border on the map | Hook iron ore base 6 → 8 (miners paid fairly); Wharf iron ore base 14 → 12 and Wharf plate base 30 → 27 (Cinder ore reaches the Wharf smelter); Little Mercy may also visit the Observatory | Backed the Coop: Coop +12, Compact −6. Backed the Compact: Compact +4 (for trying), Coop −2 | For the Compact side: Saffron courts sellers at Brassmouth. Brassmouth glassite `sat_recover` 0.05 → 0.07, for good. |
| Compact wins | Depot flies the brass Compact banner (`kit.banner.compact`); the Compact border reaches Cinder Hook on the map | Hook iron ore base 6 → 5; Brassmouth iron ore base 16 → 14; Ninefold's hold 60 → 80 | Backed the Compact: Compact +12, Coop −6. Backed the Coop: Coop +4, Compact −2 | For the Cooperative side: Ilse opens the Hookside claim. Cinder Hook ice base 14 → 17, and `field.hookside` regrowth doubles. |

If the side the player backed wins, Rook hands over 300 cr "from the new owners". Backing nobody costs nothing and earns nothing.

## The rumor (b10)

**Creation.** When b10's preconditions hold and the player docks at Brassmouth, Pip Calloway tells it: `rumor.ember_shoals_vein`, source `npc.pip_calloway` (heard from "a Compact surveyor off the Ninefold"). `field.ember_shoals` appears on the player's map as "Ember Shoals (rumored)".

**Truth value, fixed at creation from history:**

- **False, a Compact plant**, if the depot already went to the Cooperative, or the player's Cooperative tally is larger than their Compact tally. The Compact wants Cooperative-leaning miners out of the Cinder Belt so the Ninefold gets the ore. While the rumor is active, the Ninefold buys up to 20 Hook ore per docking instead of 10.
- **True** otherwise. A real Compact survey found the vein; Pip just talked.

**Spread.** Each NPC craft docking carries the rumor to that station (`rumor_spread`). Dockmasters there may mention it, citing the spread fact.

**Following it.** Flying within 500 u of (5950, 1500) writes `rumor_followed` and reveals the Shoals' rocks:

- **True:** 6 rich glassite rocks of 40 units each, hardness 0.45, no regrowth (240 units, worth about 7000 cr spread across buyers). Pip and Juno react.
- **False:** 3 small iron rocks (20 units each) and a Compact survey buoy (`prop.compact_buoy`). Pressing Space within 40 u of the buoy reads its log: `rumor_resolved{truth = false, planted_by = faction.tollway_compact}`. At the player's next docking with Rook or Ilse, the dock talk choice "Someone planted the Shoals rumor" gives Coop +5, Compact −3, and 300 cr from Rook "for the trouble". The cost of a false rumor is one trip, and the trip pays back in money, standing and a story.

**If not followed.** Once the rumor has reached all 5 stations it resolves on its own (`rumor_resolved{followed = false}`). If it was true, Juno Brask's skiff `craft.magpie` works the vein (3 of the 6 rocks are mined out over the next 30 Cinder Drift region pulses, and the rest stay for the player). If false, Hesper says "The Shoals were empty, they say." No penalty either way.

## The ice shortage (b12)

**Start.** `shortage_started{shortage.rimewater_ice}`, citing the count of the player's `mined` facts at Rimewater and the field's remaining fraction.

**The shock, touching three stations:** while active, at Kettle Dock, Lantern Wharf and Cinder Hook, the ice base price is x1.4 and ice consumption per pulse is x2. Kettle Dock's greenhouse lamps dim (`kit.light.warm_green` → `kit.light.dim_green`), and from stage 2 its rations output halves.

**The paired opportunity:** ice pays well. The most each station pays rises to 21.8 cr (Kettle), 18.2 (Wharf) and 25.5 (Hook), while Brassmouth still sells ice at about 9.2 and Hookside ice is free to mine.

**Two ways to help.** Odo offers both in dock talk:

| Way | What the player does | Completes when | Lasting result |
| --- | --- | --- | --- |
| A: "Bring ice from outside the Shallows" | Sell ice whose stack origin is outside the Shallows (Hookside, Brassmouth) at Kettle Dock, Lantern Wharf or Cinder Hook. Paid at the high market price. | 40 such units in `ledger.ice_relief` | Coop +8. "The ice road": Little Mercy adds Brassmouth to its route menu, so Brassmouth ice keeps reaching the Shallows. |
| B: "Let Rimewater rest" | Agree with Odo; Rimewater's regrowth goes x4 until it is back to 80% full. Bring 20 plate for a condenser (paid at Kettle's plate price). | 20 plate in `ledger.kettle_condenser`, and the player hasn't mined Rimewater since agreeing | Coop +10. Kettle Dock gets a condenser (`kit.kettle.condenser`): its ice drink falls by 2 per pulse, for good. |

If the player mines Rimewater after agreeing to way B, the rest lapses (Odo mentions it once, with no penalty) and way A is still open.

**If ignored.** After 40 station pulses at Kettle Dock with neither way complete, the Compact's Even Keel brings ice: Kettle's ice stock +120, and the shortage ends (`shortage_resolved{by = faction.tollway_compact}`). Lasting result: Even Keel favours Kettle Dock on its route menu. Saffron mentions the Compact's kindness. Nobody punishes the player.

**End.** Ice bases and consumption return to normal, the lamps relight, and `shortage_resolved` is written. Rimewater rest (way B) continues until 80% full.

## The tender lesson (b13)

1. **Step 1, the demonstration.** On the player's first Kettle Dock docking after b13 opens, Tamsin's own tender `craft.little_pail` (an NPC tender, hold 24) docks with 12 ice. It patient-sells them with its floor at 90% of Kettle's ice price at that moment. The player watches it sell a few units, the price dip, and the tender stop and wait. Tamsin's line `.01` cites its `patient_wait`. The player is free to leave. Any activity around Kettle pulses the market, the price climbs back, and Little Pail sells the rest (`patient_sold`). That takes about 4 Kettle pulses whatever the starting price, roughly 3 minutes of ordinary play. Step 1 completes at the player's next Kettle docking after that, when Tamsin says how long it waited and what it earned (`.02`).
2. **Step 2.** Tamsin builds `craft.thimble` for 2400 cr. It appears berthed at Kettle Dock.
3. **Step 3.** The player assigns a saved loop and sets a floor for each `sell` in it. The beat ends when Thimble finishes a lap that included a patient sell (`patient_sold` plus `loop_lap` for Thimble).
