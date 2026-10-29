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
| b05, pause | 1 PAUSED card | 8 to 12 |
| Act 1 ends | b05 complete | **10 to 15** |
| **Act 2** | | |
| Nav chip (b06) | `player.dockings` >= 6, 900 cr | 14 to 18 |
| b07, Cinder Hook and the lane chart | b06 complete | 17 to 21 |
| Rock cradle | `player.rocks_spent` >= 4, 700 cr | 18 to 28 |
| b09 starts | `player.dockings_at.cinder_hook` >= 3 | 24 to 30 |
| b08, chasing a gap | 30 glassite sold at Brassmouth over 2 dockings | 25 to 32 |
| Docking clamp | `player.dockings` >= 14, 700 cr | 25 to 32 |
| Hold extender | `player.units_sold` >= 150, 1500 cr | 25 to 40 |
| b10, the rumor | `player.dockings_at.brassmouth_exchange` >= 3 | 32 to 40 |
| Kettle Dock stage 2 | 60 ice and 20 plate in the ledger | 30 to 55 |
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
| Little Pail's demonstration | about 4 Kettle Dock pulses | plus about 3 |
| Thimble bought | 2400 cr | 65 to 78 |
| b13 complete, **first automation** | Thimble finishes a lap with a patient sell | **70 to 90** |
| b14 | b13 complete and a trait earned | 72 to 92 |
| **Act 4** | | |
| Fragment found | after b13, 80 glassite mined in all, then the next glassite rock spent | 75 to 95 |
| Sixth station | b13, `player.dockings` >= 50, 2 of b09, b10 and b12 resolved | 85 to 100 |
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

The Act 2 circuit (mined glassite to Brassmouth, Brassmouth plate to Cinder Hook, back to the Glass Drift) earns about 1000 cr on a fresh first lap and about 650 cr a lap once its buyers are partly saturated, every 3.5 to 4 minutes. That is roughly 2.5x Act 1's best loop. Upgrade costs in loads: the lens is about 3 Act 1 holds; the nav chip about 4; the order slate about 3 circuits; the tender about 4 circuits; the prestige fee about 12 circuits. The whole spend before prestige is 8800 cr of fittings plus the 8000 cr fee, 16,800 in all. The cycle model reaches about 16,500 earned by minute 118, which keeps prestige near 2 hours.

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
| Little Pail's 12-ice demo at Kettle Dock, floor at 90% of the opening price | 4, at any starting price checked | | |

The tender earns about 40 to 80 cr a minute on top of the player's own play. That changes what the player does next: they stop dumping and start feeding the tender. It doesn't replace their income, so automation feels like relief, not a solved game.

### The shortage can actually happen

Rimewater holds 240 units. It regrows 0.15 units per Shallows region pulse, about 2.5 to 4 pulses a minute, so about 0.4 to 0.6 units a minute. A player who has mined 120 ice there by minute 60 to 70 (the b02 load, a few Act 1 ice runs, and the ice for Kettle's stage 2) has about 30 to 40 back, leaving Rimewater at about 60 to 65% full. That crosses the 70% trigger. A player who never touches the ice never causes the shortage, and the beat simply never fires. Way B's rest (regrowth x4) refills from 65% to 80% in about 15 to 25 minutes of play.

### Fields never dead-end

- Tinroot and Rimewater regrow, and spent rocks reform at 25% after 16 Shallows pulses (about 5 minutes). The Wharf always buys ore and Kettle always buys ice, so a player with no credits can always mine their way back.
- The Glass Drift (300 units, regrowing about 0.45 a minute) falls to roughly a third by Act 4 if the player leans on it. That pushes them toward the Shoals (if the rumor was true), buying Cinder Hook glassite, or other wares. Spent rocks reform after 24 Cinder Drift pulses, so it never goes fully dark.
- The Cinder Belt (960 units) is deliberately slow to regrow (0.15 per pulse; reform after 40 pulses). It's the "rich, depleting" belt Rook complains about.

### The dispute resolves whether or not the player acts

NPC traffic alone adds about 1.8 a minute to the Compact tally and about 1.2 to the Cooperative tally. The Compact reaches 100 with a lead of 30 about 56 minutes after the dispute starts. A player who backs a side adds 20 to 30 per hold of plate, so 3 to 4 holds settle it.

### Exploit guards

- Docking repeatedly to make pulses doesn't work: a docking counts only after 300 u of flight, mining, or a docking elsewhere.
- Plate shuttles between Brassmouth and Cinder Hook saturate Cinder Hook's plate (0.02 per unit, 0.05 recovered per pulse), so they pay well once and then about two thirds as well (941, then 643).
- A patient sell can only be as fast as the market around it, so two tenders on one buyer share the same recovery.

## Journey bot route

The dev harness's bot (`galaxy sim run --bot journey`) plays with the `core` and `gm` packs installed and must complete all 17 beats. A second run with `core` only must complete b01 to b08, b11, b13, b14, b16 and b17. The route below is a policy, not a script of positions. Every choice is deterministic, and the fast-model layer is replaced by its seeded fake.

1. Undock; mine Rimewater ice to full; dock at Kettle Dock and hand it over (b01 to b03).
2. Loop Tinroot ore to the Wharf until 600 cr; buy the lens; open and close the pause card (b04, b05).
3. Alternate Tinroot ore (Wharf) and Rimewater ice (Kettle) until 6 dockings and 900 cr; buy the nav chip (b06).
4. Take Ilse's plate, fly to `mouth.shallows_to_cinder`, jump, and dock at Cinder Hook (b07).
5. Run the circuit: Glass Drift glassite to Brassmouth (sell at most 12 units per visit; sell the rest at the Observatory), Brassmouth plate to Cinder Hook (sold in the Cooperative's name once b09 starts). On Wharf trips, buy the cradle, clamp and hold extender as their conditions are met, and carry plate and ice to Kettle (stage 2).
6. When Pip tells the rumor, fly to the Ember Shoals. Backing the Cooperative makes the rumor false, so the bot reads the buoy and tells Rook.
7. At 26 dockings, buy the order slate. Complete 4 three-order queues, save the loop "glass run" (mine the Glass Drift, dock at the Observatory, sell glassite, dock at Cinder Hook, buy glassite, dock at Brassmouth, sell glassite), and run it for 4 laps (b11).
8. Mine Rimewater ice into Kettle until the shortage starts. Help by way A: buy Brassmouth ice, sell it at Kettle (b12).
9. Dock at Kettle and watch Little Pail (shuttling Wharf to Kettle generates the pulses). Buy Thimble; assign the loop "plate run" (buy plate at Brassmouth, dock at Cinder Hook, sell plate with floor 50) (b13).
10. Ask Orrin (b14). Keep mining glassite until the fragment drops; show Nell; jump through any lane; return to Nell and leave the fragment with her (b15).
11. Keep running circuits until 60 dockings and 8000 cr; choose the Tandem (a third run chooses the Heron) (b16).
12. Dock at Kettle, Cinder Hook and Brassmouth; return to the Wharf and ask Ilse what changed (b17). The sixth station (Meltwater Halt, from way A) should be in her answer.
