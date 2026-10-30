# 07 Pacing, number checks and the journey bot

Back to [SPEC.md](SPEC.md).

## Pacing principles

- **Every threshold is an activity count or a price, and every one is pack data.** They live in `pacing.toml` (profile `slice`) with the rest of the `core` pack tables. Beats, upgrades, traits, ranks and scripts read the values from there.
- **The engine assumes no pace.** No engine or script-host code may hard-code a threshold, a rate or an expected session length. The real game is meant to be about 10x slower. A future `full` profile multiplies the counts (for example nav chip at 60 dockings, prestige at 600 dockings) and scales prices and holds to match. Suggested test: run the journey bot on a `x10` profile and check it still finishes, just in more simulated steps.
- **Minutes below are estimates for checking the design.** They assume a human who reads the goal lines and explores a little, and the stats in this spec. They are never data, never shown, and never used by the simulation.

Rates behind the estimates, from the cycle models used for this spec:

| Rate | Act 1 | Acts 2 to 4 |
| --- | --- | --- |
| Counted dockings per minute | about 0.5 | about 0.55 |
| Jumps per minute | 0 | about 0.45 |
| Income per minute (credits) | about 120 | about 140 to 170 |
| Rocks spent per minute | about 0.25 | about 0.15 |

## Pacing table

| Milestone | Condition (all pack data) | Est. minutes (human, cumulative) |
| --- | --- | --- |
| **Act 1** | | |
| b01 to b03, first delivery | 20 ice handed over | 3 to 5 |
| Mining lens (b04) | 600 cr, after b03 | 7 to 10 |
| b05, pause | 1 PAUSED card opened after b04 started | 8 to 12 |
| Act 1 ends | b05 complete | **10 to 15** |
| **Act 2** | | |
| Nav chip (b06) | `player.dockings` >= 6, 225 cr | 14 to 18 |
| b07, Cinder Hook and the lane chart | b06 complete | 17 to 21 |
| Rock cradle | `player.rocks_spent` >= 4, 700 cr | 18 to 28 |
| b09 starts | `player.dockings_at.cinder_hook` >= 3 | 24 to 30 |
| b08, chasing a gap | 30 glassite sold at Brassmouth over 2 dockings | 25 to 32 |
| Docking clamp | `player.dockings` >= 14, 700 cr | 25 to 32 |
| Hold extender | `player.units_sold` >= 150, 1500 cr | 25 to 40 |
| b10, the rumor | `player.dockings_at.brassmouth_exchange` >= 3 | 32 to 40 |
| Kettle Dock stage 2 (`gm-examples` only) | 60 ice and 20 plate in the ledger | 30 to 55 |
| b09 resolved by the player | a tally of 100 with a lead of 30 | 40 to 55 (if ignored, about 56 minutes after it starts: roughly 85) |
| Wren rank 1 | 25 dockings in the Wren | 40 to 50 |
| **Act 3** | | |
| Order slate (b11 step 1) | b08 complete, map travel, `player.dockings` >= 26, 2000 cr | 45 to 55 |
| Loop log | `player.queues_done` >= 4 | 50 to 60 |
| b11 complete | `player.loop_laps` >= 2 | 55 to 65 |
| Wren ranks 2 and 3 | 300 units sold; 20 jumps | 50 to 65 |
| b12, the ice shortage | Rimewater at 70% or less, 120 or more Shallows ice mined, b11 complete | 60 to 90 (it depends on how much ice the player mined: that's the point) |
| First pilot trait | Lanerunner 24 jumps, Rimesight 14 rocks, Ringwise 40 dockings, Market ear 10 split sales, Good neighbour 12 ledger deliveries | 60 to 80 |
| b13 offered | `player.loop_laps` >= 4 | 62 to 72 |
| Little Pail's demonstration | Starts at a Kettle docking with ice saturation 0.6 or less; ends when Little Pail's `patient_sold` exists and the player docks at Kettle again | plus about 2 to 4 (it waited 3 to 5 Kettle pulses in every case checked) |
| Thimble bought | 2400 cr | 65 to 78 |
| b13 complete, **first automation** | Thimble finishes a lap with a patient sell | **70 to 90** |
| b14 | b13 complete and a trait earned | 72 to 92 |
| **Act 4** | | |
| Fragment found | after b13, 80 glassite mined in all, then the next glassite rock spent | 75 to 95 |
| Sixth station (`gm-examples` only) | b13, `player.dockings` >= 50, 2 of b09, b10 and b12 resolved | 85 to 100 |
| b15 complete | 3 steps: show Nell, one lane jump, back to Nell | 85 to 105 |
| b16, **prestige** | b13, b14, `player.dockings` >= 60, 8000 cr | **105 to 125** |
| b17, coming home | 3 dockings elsewhere since prestige, 1 outside the Shallows | 110 to 130 |

The journey bot takes roughly half these minutes because it never hesitates. Its run is counted in simulated steps, not minutes.

## Number checks

All figures come from the price model in `02-economy.md`, starting from the opening stocks in the market tables unless noted. They were computed unit by unit, including how each unit bought or sold moves stock.

### Price gaps make trading worthwhile

Net result of one full Wren hold (20 units), sold into a fresh market:

| Load | Cost | Revenue | Net | Time per round trip (lensed Wren, estimate) |
| --- | --- | --- | --- | --- |
| Mined Tinroot ore to the Wharf (Act 1) | 0 | 241 | 241 | about 1.5 min |
| Mined Rimewater ice to Kettle Dock (Act 1) | 0 | 201 | 201 | about 1.4 min |
| Mined glassite to Brassmouth | 0 | 692 | 692 | about 3 min, including the lane |
| Cinder Hook glassite to Brassmouth | 422 | 692 | 270 | about 2 min |
| Brassmouth plate to Cinder Hook | 617 | 941 | 324 | about 2 min |
| Wharf plate to Cinder Hook | 717 | 941 | 224 | about 2 min |
| Cinder Hook ore to Brassmouth | 140 | 282 | 142 | about 2 min |
| Wharf rations to Cinder Hook | 332 | 531 | 199 | about 2 min |

The Act 2 circuit (mined glassite to Brassmouth, Brassmouth plate to Cinder Hook, back to the Glass Drift) earns about 1000 cr on a fresh first lap and about 650 cr a lap once its buyers are partly saturated, every 3.5 to 4 minutes. That is roughly 2.5x Act 1's best loop. Upgrade costs in loads: the lens is about 3 Act 1 holds; the nav chip about one fresh hold or four to five saturated ice holds; the order slate about 3 circuits; the tender about 4 circuits; the prestige fee about 12 circuits. The whole spend before prestige is 8125 cr of fittings and the first tender plus the 8000 cr fee, 16,125 in all (the optional price glass is extra). The nav chip reduction saves 675 cr, about one partly saturated Act 2 circuit. The 60-docking prestige gate and later costs stay unchanged, keeping the target near 2 hours; the old cycle model is an estimate, not a measured end-to-end play time.

### Act 1 into the nav chip

Act 1 still ends at b05, targeting 10 to 15 minutes; b06 targets 14 to 18.
The six-docking gate is normally met while earning the lens. The slice nav chip
cost is **225 cr** in `data/upgrades.toml`: at roughly 50 cr per saturated ice hold,
it takes at most five extra holds, instead of about eighteen at 900 cr. The app's
key-driven journey test reads that price from the loaded data and bounds the extra
ice runs at five. Alternating ore and ice as the route below prescribes can be
quicker. No early-market recovery or consumption values change: b08's saturation
lesson and Little Pail's later patient-selling demonstration keep their economics.

### Saturation punishes blind full loads

| Case | Revenue | Average per unit |
| --- | --- | --- |
| 20 glassite into a fresh Brassmouth | 692 | 34.6 (vs 50 at the first unit) |
| The same 20 glassite again, one trip later (saturation still 0.45) | 318 | 15.9 |
| Split instead: 10 at Brassmouth, 10 at the Observatory, fresh | 735 (425 + 310) | 36.8 (and each buyer is only half-saturated for next time) |
| A Tandem's 50 glassite dumped at Brassmouth | 1090 | 21.8 |
| The same 50 sold patiently by a tender, floor 40 | 2093 | 41.9 (it waits about 26 Brassmouth pulses) |

A repeated full load earns less than half of the first. Splitting across buyers and coming back later both pay, which is the lesson b08 teaches and Market ear rewards.

### The tender earns its keep

| Case | Pulses waited | Revenue | Average per unit |
| --- | --- | --- | --- |
| 24 glassite at Brassmouth (saturation 0.2 from an earlier sale), dumped | 0 | 560 | 23.3 |
| The same, patient sell with floor 40 | 14 (about 10 minutes of NPC traffic alone) | 993 | 41.4 |
| 24 plate at Cinder Hook (saturation 0.1, stock 80), dumped | 0 | 928 | 38.7 |
| The same, patient sell with floor 50 | 9 | 1243 | 51.8 |
| Little Pail's 12-ice demo at Kettle Dock, floor at 90% of the price when it docks, started only at ice saturation 0.6 or less | 3 to 5 in every case checked (stock 0 to 320, saturation 0 to 0.6, pulse weights 1 and 0.5, with and without the shortage and stage 2). It always sells 2 to 5 units before its first wait. | | |
| The same demo if it were allowed at saturation above 0.6 | 0 in 426 of the cases checked (for example stock 120 and saturation 0.7: all 12 sell at once), which is why it waits for 0.6 | | |

The tender earns about 40 to 80 cr a minute on top of the player's own play. That changes what the player does next: they stop dumping and start feeding the tender. It doesn't replace their income, so automation feels like relief, not a solved game.

### The shortage can actually happen

Rimewater holds 240 units. It regrows 0.15 units per Shallows region pulse, about 2.5 to 4 pulses a minute, so about 0.4 to 0.6 units a minute. Spent rocks reform only from that regrowth (`01-galaxy.md`), so they add nothing on top of it. By minute 65 that is 162 to 260 region pulses, or 24 to 39 units back. Rimewater is at 70% (168 units) once the player has mined 96 to 111 units there. A player who has mined 120 there (the b02 load plus later Kettle sales and, with `gm-examples`, stage 2 deliveries) is at 60 to 66%, whichever rocks they mined and in whatever order (a run-by-run model of 6 to 8 holds, finishing each rock or always taking the richest, gave the same result). That crosses the 70% trigger, and the 120-unit gate holds too. The cheaper nav chip does not force that much ice mining in Act 1; the bot deliberately returns to Rimewater in step 8 until both shortage gates hold. A player who never touches the ice never causes the shortage, and the beat simply never fires. Way B's rest (regrowth x4) refills from 65% to 80% in about 15 to 24 minutes of play.

Under the previous rule (a spent rock reformed at 25% after 16 pulses, on top of regrowth), a player who finished each rock before moving on got 7.5 units back per spent rock and stayed near 80% after 120 units mined. The shortage would never have fired for them. That is why reforming now comes only from regrowth.

### Fields never dead-end

- Tinroot and Rimewater regrow, and a spent rock reforms once regrowth has brought it to 25% (about 34 Shallows pulses for a Tinroot rock and 50 for a Rimewater rock, if it gets all the regrowth: roughly 8 to 20 minutes). Tinroot regrows 0.75 to 1.2 units a minute and never empties fully in practice. The Wharf always buys ore and Kettle always buys ice, so a player with no credits can always mine their way back.
- The Glass Drift (300 units, regrowing about 0.45 a minute) falls to roughly a third by Act 4 if the player leans on it. That pushes them toward the Shoals (if the rumor was true), buying Cinder Hook glassite, or other wares. Spent rocks reform from regrowth (about 42 Cinder Drift pulses for one rock), so it never goes fully dark.
- The Cinder Belt (960 units) is deliberately slow to regrow (0.15 per pulse; a spent 60-unit rock needs 100 pulses to reform). It's the "rich, depleting" belt Rook complains about.

### The dispute resolves whether or not the player acts

NPC traffic alone adds about 1.8 a minute to the Compact tally and about 1.2 to the Cooperative tally. The Compact reaches 100 with a lead of 30 about 56 minutes after the dispute starts. A player who backs a side adds 20 to 30 per hold of plate, so 3 to 4 holds settle it.

### Exploit guards

- Docking repeatedly to make pulses doesn't work: a docking counts only after 300 u of flight, mining, or a docking elsewhere.
- Plate shuttles between Brassmouth and Cinder Hook saturate Cinder Hook's plate (0.02 per unit, 0.05 recovered per pulse), so they pay well once and then about two thirds as well (941, then 643).
- A patient sell can only be as fast as the market around it, so two tenders on one buyer share the same recovery.

## Journey bot route

The dev harness's bot (`galaxy sim run --bot journey`) must complete all 17 beats in two runs: **run A** with `core` and `gm-examples` installed, and **run B** with `core` only. The route is a policy, not a script of positions. Every choice is deterministic, and the fast-model layer is replaced by its seeded fake. Where the runs differ, the step says so. Every order below is a contract `Order` (`08-contract.md`); "queue" means shift-click orders, "loop" means a saved queue.

1. **b01 to b03.** Undock. Mine Rimewater ice by hand until the hold holds 20. Dock at Kettle Dock and hand the ice to Odo.
2. **b04, b05.** Mine Tinroot ore and sell it at the Wharf until the bot has 600 cr; buy the mining lens. Then open the PAUSED card and close it. That writes a `paused` fact after b04 started, so b05 completes.
3. **b06.** Alternate Tinroot ore (sold at the Wharf) and Rimewater ice (sold at Kettle) until `player.dockings` >= 6 and the bot has 225 cr; buy the nav chip.
4. **b07.** With 20 hold free, take Ilse's plate. Fly to `mouth.shallows_to_cinder`, enter the lane, and dock at Cinder Hook; hand the plate to Rook. The lane chart arrives.
5. **Act 2 circuit (b08, b09).** Repeat: mine the Glass Drift; sell at most 12 glassite per visit at Brassmouth and the rest at the Observatory; at Brassmouth buy plate and sell it at Cinder Hook. When b09 starts, choose "Supply in the Cooperative's name" and keep it. On Wharf trips, buy the rock cradle, docking clamp and hold extender as soon as each condition holds. Run A: also carry Wharf plate and Rimewater ice to Kettle on Shallows trips (stage 2). b08 completes on the second Brassmouth sale; b09 completes when the Cooperative tally wins.
6. **b10.** When Pip tells the rumor, fly within 500 u of the Ember Shoals; that resolves it. Run A: the bot backed the Cooperative, so `gm.compact_plant` made it a plant; the bot reads the buoy and, at its next Cinder Hook docking, tells Rook. Run B: the truth comes from the craft that brought the story; the bot mines whatever is there into the next hold either way.
7. **b11.** At 26 dockings (b08 done, map travel held), buy the order slate. Complete 4 queues of three orders each (`mine` Tinroot, `dock` the Wharf, `sell` iron ore all); Orrin fits the loop log. Queue the seven orders below and save them as the loop "glass run" (7 of the 8 allowed), then run it until `player.loop_laps` >= 4:
   `mine field.glass_drift` → `dock` Glass Drift Observatory → `sell` glassite all → `dock` Cinder Hook → `buy` glassite fill, `max_price` 25 → `dock` Brassmouth Exchange → `sell` glassite all.
   Every lap moves units (the mined glassite alone does), so it never stalls. The next lap's `mine` routes back from Brassmouth through the lane.
8. **b12.** Mine Rimewater ice and sell it at Kettle until the shortage starts. Help by way A: buy ice at Brassmouth and sell it at any of the shortage's stations until `ledger.ice_relief` reaches the quota in `shortage_started` (40 in run B; run A reads the tuned value).
9. **b13.** Dock at Kettle Dock.
   - **Step 1.** If Tamsin says `.08` (Kettle's ice saturation is above 0.6, which is likely right after step 8), shuttle Tinroot ore to the Wharf and dock at Kettle after each run: every Wharf docking is half a Kettle pulse, and Kettle's own traffic adds more. Once Little Pail has written `patient_wait` and `patient_sold`, dock at Kettle again; Tamsin's `.02` completes step 1.
   - **Step 2.** With at least 3400 cr (the tender, plus up to about 950 cr for a first load of 24 plate: Brassmouth never asks more than 38.9 at the Stranger spread), buy Thimble.
   - **Step 3.** Queue and save the loop "plate run", assign it to Thimble, and set the floor 50:
     `dock` Brassmouth Exchange → `buy` plate fill → `dock` Cinder Hook → `sell` plate all, `floor` 50.
     *Lap 1 starts at Kettle Dock:* the `dock` order flies to `mouth.shallows_to_brassmouth`, rides the lane and docks at Brassmouth, where the `buy` happens, so Thimble never buys away from a seller. Every unit costs at most 38.9 and the floor is 50, so a lap never sells at a loss. *Lap 2 and every later lap start at Cinder Hook:* the first `dock` rides `lane.cinder_brassmouth` back to Brassmouth. Cinder Hook's plate ceiling is 71.5 (1.3 x base 55), and no lever in the slice changes that base, so the floor of 50 is never blocked; with plate stock drunk at 2 a pulse and saturation recovering 0.05 a pulse, the price always climbs back past 50.
   - **Fallback.** If Thimble writes `loop_stalled` (for example, Brassmouth had no plate left) or `patient_blocked` before b13 completes, the bot assigns the loop "Wharf plate run" instead: `dock` Lantern Wharf → `buy` plate fill → `dock` Kettle Dock → `sell` plate all, `floor` 40 (Kettle's plate ceiling is 49.4).
   - b13 completes at Thimble's first lap that ends with a `patient_sold`.
10. **b14.** If no trait is earned yet, keep running the Act 2 circuit until one is (Ringwise at 40 dockings is the usual first). Dock at the Wharf and ask Orrin.
11. **b15.** Keep mining the Glass Drift until the fragment drops (80 glassite mined in all, then the next glassite rock the bot spends). Dock at the Observatory first, so Nell is the first to see it (step 1). Jump through any lane and back (step 2). Dock at the Observatory, tell Nell, and leave the fragment with her (step 3).
12. **b16.** Keep running circuits until `player.dockings` >= 60 and the bot has 8000 cr. Dock at the Wharf and choose the Tandem. A third run chooses the Heron.
13. **b17.** Dock at Kettle Dock, Cinder Hook and Brassmouth Exchange, return to the Wharf and ask Ilse what changed. Her answer always ends with `.17`.

**What each run checks at the end:**

- **Both runs:** all 17 `beat_completed` facts, in an order the preconditions allow; no `loop_stalled` left unanswered; credits never negative.
- **Run A:** a fact for each change type: `stage_built{kettle_dock, 2}`, `shortage_started{tuned = true}`, `border_moved`, `rumor_exposed`, `mechanic_taught{rock_tagging}` (the bot accepts Juno's offer at the Hook in step 7), `canon_opened{second_fragment}`, `station_founded` (Meltwater Halt, from way A) and at least one `chronicle` entry.
- **Run B:** no fact kind and no cause from the `gm` namespace anywhere in history.

**A shortage-during-demo test** runs separately: start b13 step 1 with the shortage active (Little Pail must not start, and Tamsin says `.12`); then, in a second variant, start the demonstration and resolve the shortage while Little Pail is waiting with cargo aboard. Little Pail's floor must be revised at the next Kettle docking, `patient_sold` must follow, and b13 and prestige must still complete.

**A short pause test** runs separately: open and close the PAUSED card before b04 starts, buy the lens, then open and close it again. b05 must complete (two `paused` facts; only the second is newer than b04's start).
