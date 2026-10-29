# 05 People: NPCs, factions and pilot traits

Back to [SPEC.md](SPEC.md). Data files: `npcs.toml`, `lines.toml`, `factions.toml`, `traits.toml`.

## How people speak

- **Every line cites a fact.** A line is eligible only when its cited fact kind exists with the stated condition and the line hasn't been used for that fact yet (`npc_line` facts track use). If no line qualifies, the NPC says nothing.
- **Dock talk:** on each counted docking, at most one line from the station's NPCs: the most specific eligible line (the one with the most matching conditions) whose fact is newer than the player's previous docking there. Ties go to the newest fact.
- **"What changed?":** any dockmaster can be asked. They give up to 3 eligible lines citing facts since the player's last docking at that station, at most one per place, most specific first.
- **Fast-model phrasing:** the short-line model may rephrase an eligible line in the NPC's voice, citing the same fact, in at most 20 words. If the call fails or is slow, the authored line below is used word for word. The model may never pick a fact that has no authored line here.
- Placeholders: `{qty}`, `{ware}`, `{station}`, `{n}`, `{pulses}`, `{avg}`, `{floor}`, `{hull}`, `{pilot}` (the pilot's name), `{tender}` (the tender's name).

## Named NPCs

### Ilse Marrow

| Field | Value |
| --- | --- |
| ID | `npc.ilse_marrow` |
| Role and station | Dockmaster, Lantern Wharf |
| Faction | Wharf Cooperative |
| Voice | Dry and warm. She counts in deliveries, not days, and says a little less than she means. Ends on something practical. |
| Remembers | `delivered`, `stage_built`, `shortage_*`, `side_chosen`, `territory_changed`, `market_flooded`, `prestige`, `chronicle` |

| Line | Cites | Text |
| --- | --- | --- |
| `line.ilse_marrow.01` | `delivered{ledger = kettle_greenhouse}` | Odo's been counting your ice. He counts slower than I do, but he gets there. |
| `line.ilse_marrow.02` | `stage_built{kettle_dock, stage >= 2}` | Kettle lit another ring. You can see it from my window if you squint. |
| `line.ilse_marrow.03` | `territory_changed{cinder_hook, to = wharf_coop}` | Cooperative banner over the Hook depot. Rook pretends not to care. He cares. |
| `line.ilse_marrow.04` | `territory_changed{cinder_hook, to = tollway_compact}` | The Compact has the Hook depot now. We'll buy our ore like anyone else. |
| `line.ilse_marrow.05` | `shortage_started` | Rimewater's thin. A lot of your ice is in our tanks, and not much is left out there. |
| `line.ilse_marrow.06` | `shortage_resolved{by = tollway_compact}` | The Compact brought ice in. Kind of them. They'll want thanking. |
| `line.ilse_marrow.07` | `market_flooded{station = brassmouth_exchange}` | Saffron says you sold Brassmouth so much glassite they're using it as doorstops. |
| `line.ilse_marrow.08` | `prestige{to = tandem}` | A Tandem. Wide in the hips. She'll carry what you can't. |
| `line.ilse_marrow.09` | `prestige{to = heron}` | A Heron. You'll see the prices before I do, then. |
| `line.ilse_marrow.10` | `station_founded` | There's a new light on the map. {station}. Folk say it's there because of you. |

### Orrin Vale

| Field | Value |
| --- | --- |
| ID | `npc.orrin_vale` |
| Role and station | Hullwright, Lantern Wharf (fits every upgrade except the lane chart and the price glass, and asks the prestige question) |
| Faction | Wharf Cooperative |
| Voice | Talks about hulls like old dogs, fondly and a bit suspicious of anything new. Judges pilots by their habits, never their money. |
| Remembers | `upgrade_fitted`, `hull_rank`, `trait_earned`, `rock_spent`, `tender_acquired`, `prestige` |

| Line | Cites | Text |
| --- | --- | --- |
| `line.orrin_vale.01` | `upgrade_fitted{nav_chip}` | The chip's settling in. Ships take a few dockings to trust a new brain. |
| `line.orrin_vale.02` | `trait_earned{rimesight}` | {n} rocks worked to dust. You find the good face first now. I've watched you. |
| `line.orrin_vale.03` | `trait_earned{ringwise}` | You don't dock any more, you arrive. The ring knows you. |
| `line.orrin_vale.04` | `trait_earned{market_ear}` | You stop selling before the price does. That's an ear, that is. |
| `line.orrin_vale.05` | `trait_earned{lanerunner}` | {n} times through the lanes. The lamps lean toward you a bit. |
| `line.orrin_vale.06` | `trait_earned{good_neighbour}` | Half the Shallows has your name in their ledgers. Nicely. |
| `line.orrin_vale.07` | `hull_rank{wren, rank = 3}` | Three stripes on the Wren. She's properly yours now. |
| `line.orrin_vale.08` | `tender_acquired` | Tamsin's tender? Sound little hull. Don't let it get ideas. |
| `line.orrin_vale.09` | `prestige` | The Wren's on berth six. She'll be fine. I talk to her. |

### Odo Fenn

| Field | Value |
| --- | --- |
| ID | `npc.odo_fenn` |
| Role and station | Head grower and dockmaster, Kettle Dock |
| Faction | Wharf Cooperative |
| Voice | Soft-spoken. Measures everything in what grows and calls the greenhouse "we". Thanks people by naming exactly what they brought. |
| Remembers | `delivered`, `stage_built`, `shortage_*`, `patient_sold` at Kettle Dock |

| Line | Cites | Text |
| --- | --- | --- |
| `line.odo_fenn.01` | `delivered{ledger = kettle_greenhouse}` | {qty} ice. The beans will hear about this. |
| `line.odo_fenn.02` | `stage_built{kettle_dock, 1}` | First pane's up. It fogs in the mornings. Well. When we call it morning. |
| `line.odo_fenn.03` | `stage_built{kettle_dock, 2}` | Second ring's lit. Mostly your ice. And your plate, mostly. |
| `line.odo_fenn.04` | `stage_built{kettle_dock, 3}` | The orchard lamps are on. We'll need glassite for them now, if you're passing. |
| `line.odo_fenn.05` | `shortage_helped{way = rest}` | Rimewater's resting. So am I, a little. |
| `line.odo_fenn.06` | `shortage_helped{way = import}` | Ice from outside the Shallows. Tastes different. The tomatoes don't mind. |
| `line.odo_fenn.07` | `patient_sold{station = kettle_dock, craft = player tender}` | {tender} sat with us a while and sold when it should. Polite little thing. |

### Tamsin Oddway

| Field | Value |
| --- | --- |
| ID | `npc.tamsin_oddway` |
| Role and station | Tender keeper, Kettle Dock (teaches the tender and builds it); flies `craft.little_pail` |
| Faction | Wharf Cooperative |
| Voice | Brisk and cheerful. A tinkerer who explains by doing, and talks to tenders the way a shepherd talks to dogs. Patience is her whole philosophy: "The dock tells you when." |
| Remembers | `loop_lap`, `market_flooded`, `tender_*`, `patient_*`, `loop_stalled` |

| Line | Cites | Text |
| --- | --- | --- |
| `line.tamsin_oddway.01` | `patient_wait{craft = little_pail}` | See Little Pail sitting there? She's not stuck. She's listening. |
| `line.tamsin_oddway.02` | `patient_sold{craft = little_pail}` | {pulses} ships came and went, and she sold at {avg}. No clock in her at all. |
| `line.tamsin_oddway.03` | `loop_lap{ship = player, n >= 4}` | You've run that loop {n} times. Ready for something to run it for you? |
| `line.tamsin_oddway.04` | `market_flooded{ship = player}` | You emptied a whole hold into {station}. A tender would've waited. |
| `line.tamsin_oddway.05` | `tender_assigned` | A floor of {floor} for {ware}? Sensible. {tender} agrees. |
| `line.tamsin_oddway.06` | `patient_sold{craft = player tender}` | {tender} waited {pulses} dockings at {station} and got {avg} a unit. Told you. |
| `line.tamsin_oddway.07` | `loop_stalled{craft = player tender}` | {tender}'s sitting at {station} with nothing to do. Its loop's run dry. |

### Rook Adeyemi

| Field | Value |
| --- | --- |
| ID | `npc.rook_adeyemi` |
| Role and station | Depot master and dockmaster, Cinder Hook |
| Faction | None (works for whoever owns the depot) |
| Voice | Blunt, with short sentences and dusty humour. Respects anyone who shows up with plate. |
| Remembers | `delivered`, `side_chosen`, `territory_changed`, `rumor_resolved`, `rock_spent` in the Cinder Belt |

| Line | Cites | Text |
| --- | --- | --- |
| `line.rook_adeyemi.01` | `delivered{ledger = hook_consignment}` | Ilse's plate. Tell her it's already holding up a wall. |
| `line.rook_adeyemi.02` | `side_chosen{side = wharf_coop}` | Cooperative plate, is it. Noted. Everyone's noting. |
| `line.rook_adeyemi.03` | `side_chosen{side = tollway_compact}` | Compact name on your plate. Saffron will like that. I like the plate. |
| `line.rook_adeyemi.04` | `territory_changed{cinder_hook}` | New banner. Same ore. Same me. |
| `line.rook_adeyemi.05` | `rumor_resolved{truth = false}` | Empty Shoals? Someone wanted you out there. Worth asking who. |
| `line.rook_adeyemi.06` | `rock_spent{field = cinder_belt, n >= 10}` | You're eating my belt one rock at a time. Leave some for the Hook. |

### Juno Brask

| Field | Value |
| --- | --- |
| ID | `npc.juno_brask` |
| Role and station | Prospector, Cinder Hook; flies the skiff `craft.magpie`; teaches rock tagging (`gm.rock_tagging`) |
| Faction | None |
| Voice | Chatty and superstitious about rocks; names the good ones. Half her advice is folklore and half is exactly right. |
| Remembers | `tagged`, `rock_spent` (glassite), `fragment_found`, `rumor_heard` |

| Line | Cites | Text |
| --- | --- | --- |
| `line.juno_brask.01` | `tagged{ship = magpie}` | See my lamp on that rock? Means I'm coming back for it. |
| `line.juno_brask.02` | `tagged{ship = player, n >= 3}` | Three lamps out. You're a prospector now. Sorry. |
| `line.juno_brask.03` | `rock_spent{ware = glassite, ship = player}` | Glassite rocks glint when they go. Did you see it? |
| `line.juno_brask.04` | `fragment_found` | A rock with a voice in it. Didn't I say? I definitely said. |
| `line.juno_brask.05` | `rumor_heard` | Ember Shoals, is it. I've heard that one before. Sometimes it's even true. |

### Saffron Vey

| Field | Value |
| --- | --- |
| ID | `npc.saffron_vey` |
| Role and station | Compact factor and dockmaster, Brassmouth Exchange |
| Faction | Tollway Compact |
| Voice | Polished and pleasant. Quotes the ledger, and has a smile that keeps accounts. Never rude, never quite warm. |
| Remembers | `sold` at Brassmouth, `market_flooded`, `side_chosen`, `territory_changed`, `shortage_resolved`, `prestige` |

| Line | Cites | Text |
| --- | --- | --- |
| `line.saffron_vey.01` | `sold{ware = glassite, station = brassmouth_exchange, first}` | Glassite. Lovely. The Exchange will remember your name. |
| `line.saffron_vey.02` | `market_flooded{ware = glassite, station = brassmouth_exchange}` | {qty} glassite in one go. Generous. Our buyers needed a lie-down. |
| `line.saffron_vey.03` | `side_chosen{side = tollway_compact}` | The Compact appreciates plate with its name on it. |
| `line.saffron_vey.04` | `side_chosen{side = wharf_coop}` | Cooperative plate at the Hook. Well. Ledgers are long. |
| `line.saffron_vey.05` | `shortage_resolved{by = tollway_compact}` | Our barges kept Kettle Dock's taps running. Do mention it to people. |
| `line.saffron_vey.06` | `prestige{to = heron}` | A Heron. You'll read my prices from across the Narrows. Do be kind. |

### Pip Calloway

| Field | Value |
| --- | --- |
| ID | `npc.pip_calloway` |
| Role and station | Dockhand, Brassmouth Exchange (tells the rumor) |
| Faction | Tollway Compact |
| Voice | Eager and gossipy, and means well. Repeats what captains say as if it were his own idea, then admits it wasn't. |
| Remembers | `rumor_*`, `jumped`, `sold` at Brassmouth |

| Line | Cites | Text |
| --- | --- | --- |
| `line.pip_calloway.01` | `rumor_heard{rumor = ember_shoals_vein}` | A surveyor off the Ninefold says the Ember Shoals are thick with glassite. Thick! |
| `line.pip_calloway.02` | `rumor_resolved{truth = true}` | It was true? I knew it. I said so, didn't I? |
| `line.pip_calloway.03` | `rumor_resolved{truth = false}` | Empty? Oh. I had it off a Compact surveyor. I'm sorry. Truly. |
| `line.pip_calloway.04` | `jumped{ship = player, n >= 20}` | You're through that lane more often than the barges are. |
| `line.pip_calloway.05` | `sold{ship = player, station = brassmouth_exchange, n >= 5}` | Saffron says you're good for the books. From her that's practically a hug. |

### Nell Quillon

| Field | Value |
| --- | --- |
| ID | `npc.nell_quillon` |
| Role and station | Archivist, Glass Drift Observatory (Ancients chapter 1) |
| Faction | Lamplighter Society |
| Voice | Precise and easily delighted. Asks questions she half knows the answer to, and treats every fact like a present. |
| Remembers | `docked` at the Observatory, `fragment_found`, `shown`, `canon_opened`, `rock_spent` in the Glass Drift, `prestige` |

| Line | Cites | Text |
| --- | --- | --- |
| `line.nell_quillon.01` | `docked{station = glass_drift_observatory, first}` | A visitor! Did you bring ice or questions? Both are welcome. |
| `line.nell_quillon.02` | `fragment_found` | You found it in a rock you mined yourself? Not bought? Not given? Good. |
| `line.nell_quillon.03` | `shown{item = fragment, npc not a Lamplighter}` | I hear you've found something. I'd have liked to hear it from you. |
| `line.nell_quillon.04` | `canon_opened{node = fragments_sing}` | It sang in the lane. We thought they all would. Now we know one did. |
| `line.nell_quillon.05` | `rock_spent{field = glass_drift, n >= 6}` | Every rock in the Drift is a crumb of something bigger. Mind how you crumble them. |
| `line.nell_quillon.06` | `prestige{to = heron}` | A Heron! Survey notes, please. As many as you can bear. |

### Hesper Lune

| Field | Value |
| --- | --- |
| ID | `npc.hesper_lune` |
| Role and station | Keeper and dockmaster, Glass Drift Observatory (sells the price glass) |
| Faction | Lamplighter Society |
| Voice | Calm and old. Speaks in lantern images, and is patient with everyone, even the Compact. |
| Remembers | `jumped`, `rumor_spread`, `regard_changed`, `territory_changed`, `station_founded` |

| Line | Cites | Text |
| --- | --- | --- |
| `line.hesper_lune.01` | `jumped{ship = player, n >= 10}` | The lane lamps know your hull now. They brighten a little when you pass. |
| `line.hesper_lune.02` | `rumor_spread{station = glass_drift_observatory}` | Rumours come here to be weighed. Most of them are light. |
| `line.hesper_lune.03` | `regard_changed{faction = lamplighters, to = trusted}` | The Society asked me to say it: you are welcome at any lamp. |
| `line.hesper_lune.04` | `territory_changed{cinder_hook}` | The Hook changed banners. The lanes don't mind who owns the ore. |
| `line.hesper_lune.05` | `station_founded` | Someone has lit a new lamp out there. {station}. We'll visit. |
| `line.hesper_lune.06` | `rumor_resolved{truth = false, followed = false}` | The Shoals were empty, they say. Most rumours are. |

## Factions

Standing is a hidden integer per faction, from −100 to 100, starting at 0. The player never sees the number. It shows only as **regard**: how people treat you.

| Regard | Standing | How it shows |
| --- | --- | --- |
| Cold | −10 or less | The dockmaster uses your hull's name, not yours. Lines are short. Asking prices use spread 1.20. No tips, no rumors. They still trade and still give core goals. |
| Stranger | −9 to 9 | Polite: "Pilot." Spread 1.15. |
| Known | 10 to 29 | They use your name and mention one of your deeds when you dock. Spread 1.15. Once per docking the dockmaster shares a price they heard ("Kettle was paying 15 for ice when Little Mercy left"), citing an NPC craft's `sold` fact. |
| Trusted | 30 to 59 | Warm: they ask after your tender or hull. Spread 1.10. Rumors that reach their stations are told to you first. A lamp in the faction's colour lights at your berth as you approach. |
| Kin | 60 or more | "One of ours." Spread 1.06. The faction's chronicle entries name you. Their stations hang your pilot's name plate on the ring. |

Each change of tier writes `regard_changed`, and an NPC of that faction has a line for it on the next docking.

### Wharf Cooperative (`faction.wharf_coop`)

| Field | Value |
| --- | --- |
| Wants | Ice and plate for the Shallows, fair ore prices, and Kettle Dock growing |
| Territory | Lantern Shallows (Lantern Wharf, Kettle Dock); claims Cinder Hook |
| Colour and mark | Lantern green; a lantern over a kettle |
| Remembers | Deliveries, and whom you sided with at Cinder Hook |

| Deed | Standing change |
| --- | --- |
| b03 first delivery to Kettle Dock | +5 |
| b07 consignment delivered | +4 |
| Every 10 units of ice or plate sold at Kettle Dock, or delivered to any Cooperative ledger | +1 (at most +2 per docking) |
| Shortage helped: way A / way B | +8 / +10 |
| Told Rook or Ilse who planted the rumor | +5 |
| Bought a tender from Tamsin | +3 |
| Dispute: backed the Cooperative and won / backed it and lost | +12 / +4 |
| Dispute: backed the Compact and won / backed it and lost | −6 / −2 |

### Tollway Compact (`faction.tollway_compact`)

| Field | Value |
| --- | --- |
| Wants | Glassite, and control of the Cinder ore trade |
| Territory | Brassmouth Narrows (Brassmouth Exchange); the lanes' trade (its barges); claims Cinder Hook |
| Colour and mark | Brass and deep red; a toll key |
| Remembers | Big sales, and whether you flooded their market |

| Deed | Standing change |
| --- | --- |
| First glassite sale at Brassmouth (b08) | +3 |
| Every 15 units sold at Brassmouth | +1 (at most +2 per docking) |
| A sale at Brassmouth that floods it (`market_flooded`) | −2 (at most once per docking) |
| Every 20 units of ice bought at Brassmouth during the shortage | +1 |
| Dispute: backed the Compact and won / backed it and lost | +12 / +4 |
| Dispute: backed the Cooperative and won / backed it and lost | −6 / −2 |
| Exposed the planted rumor to the Cooperative | −3 |

### Lamplighter Society (`faction.lamplighters`)

| Field | Value |
| --- | --- |
| Wants | Fragments, survey data, and safe passage for researchers; the lane lamps kept lit |
| Territory | Glass Drift Observatory; it tends the lane lamps of all three lanes but claims no ground |
| Colour and mark | Amber on midnight blue; an open lamp |
| Remembers | What you found, and whom you told first |

| Deed | Standing change |
| --- | --- |
| First docking at the Observatory | +3 |
| Every 15 units sold at the Observatory | +1 (at most +1 per docking) |
| Showed the fragment to Nell or Hesper before anyone else (`shown` names them first) | +5 |
| b15 complete / fragment left with the Observatory | +15 / +5 more |
| Heron survey note sold to Nell | +2 each |

**Whom you told first:** after `fragment_found`, the first dockmaster the player docks with notices the glint in the hold and asks about it, which writes `shown{item = fragment, npc}`. If that is not a Lamplighter, the Society learns secondhand: no penalty, just Nell's line `.03`.

## Pilot traits

Traits are earned by doing one kind of deed many times. Each has a small qualitative effect, never a large power gain. Traits are never shown as a bar or a count. When earned, the effect applies at once and `trait_earned` is written. b14 is the moment someone names it. Traits are kept through prestige.

| ID | Name | Deed | Times | Effect | Who notices |
| --- | --- | --- | --- | --- | --- |
| `trait.rimesight` | Rimesight | Rocks spent by your own ship (`player.rocks_spent`) | 14 | The richest face of every rock glints faintly. Mining while facing that face yields 15% more units per unit of mining time. | Orrin (`.02`) |
| `trait.ringwise` | Ringwise | Counted dockings (`player.dockings`) | 40 | Docking rings catch you from 25% further out. On docking, the dockmaster greets you with a line about your last visit there, if one qualifies. | Orrin (`.03`) |
| `trait.market_ear` | Market ear | Split sales (`player.split_sales`) | 10 | The sell screen shows where the next unit's price will land, and the last-seen price for that ware at the nearest other buyer. | Orrin (`.04`) |
| `trait.lanerunner` | Lanerunner | Jumps (`player.jumps`) | 24 | Lane mouths catch you from 105 u instead of 70 u. Arriving through a lane, you hear the newest eligible dock-talk line of that region over the radio. | Orrin (`.05`), Hesper (`.01`) |
| `trait.good_neighbour` | Good neighbour | Ledger deliveries (`player.ledger_deliveries`) | 12 | Standing gains from deliveries are 50% larger. A rumor reaches you at the first station it spreads to, wherever you are docked. | Orrin (`.06`) |
