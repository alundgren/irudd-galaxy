# 08 Mapping onto the contract

Back to [SPEC.md](SPEC.md). This file maps every piece of content in this spec onto the foundation contract (`crates/contract` on `slice/01/foundation`, PR #13, at `c77e770`), and lists the contract extensions the content needs, with the workstream that builds each one. Engineers implement against this one list instead of inventing adapters. Extensions can land in later waves; each one names the first piece of content that needs it.

Status words: **exists** (in the contract today, used as is), **extend** (an existing type gains fields or variants; old saves still load unless noted), **new** (a new type, store, command or event).

Every contract change goes through W1, as `AGENTS.md` says: change `crates/contract`, run `cargo xtask contract`, review the schema diff.

## 1. Decisions that apply everywhere

### Names

- **Spec IDs replace the foundation's provisional data IDs.** The renames in `packs/core/data/`:

  | Foundation (provisional) | This spec |
  | --- | --- |
  | `faction.wharf_cooperative` | `faction.wharf_coop` |
  | `faction.lamplighter_society` | `faction.lamplighters` |
  | `ware.refined_plate` | `ware.plate` |
  | `ware.ancients_fragment` | `ware.fragment` |
  | `npc.ilse`, `npc.hullwright` | `npc.ilse_marrow`, `npc.orrin_vale` (plus the other eight NPCs) |
  | `field.shallows.ice`, `field.shallows.iron` | `field.rimewater`, `field.tinroot` |
  | `field.cinder.iron`, `field.cinder.glassite` | `field.cinder_belt`, `field.glass_drift` |
  | `field.narrows.ice` | removed (no field in the Narrows); `field.hookside` added. `field.ember_shoals` is created live by b10. |
  | `rock.<region>.<kind>.NN` | `rock.<field>.NN`, so each field's `rock_prefix` is `rock.<field name>` (for example `rock.rimewater`) |
  | `part.kettle.core` | `part.kettle.hub` and `part.kettle.spout` |

- **ID families and their newtypes.** Region, lane, station, ware, rock, field, ship (`ship.player`, `craft.*`), hull, NPC, faction, rumor, part (`part.*`), trait, beat and asset IDs use the existing newtypes. `cap.*` and `mech.*` are `FlagId` values. Rock kinds are `RockTypeId`s: `rock_type.ice`, `rock_type.iron`, `rock_type.glassite`, `rock_type.rich_glassite`. **New newtypes:** `UpgradeId` (`upgrade.*`), `LedgerId` (`ledger.*`), `CanonId` (`canon.*`), `LineId` (`line.*`), `ZoneId` (`zone.*`), `PropId` (`prop.*`), `TunableId` (dotted names like `shortage.rimewater_ice.way_a_quota`), `SituationId` (`shortage.*`, `dispute.*`) and `LoopName` (a player's loop name, free text up to 24 characters).
- **Lane mouths** are lane ends, not entities: `mouth.<from>_to_<to>` names `Lane.a_gate` or `Lane.b_gate`. Orders and clicks address a mouth as `{lane, end}`.
- **Fact kinds.** A kind written by an engine system is a contract **event**, and its name is its `EventKind` name. This spec uses those names (`dock`, `undock`, `mine`, `rock_depleted`, `sell`, `buy`, `deliver`, `jump`, `scout`, `order_done`, `trait_earned`, `prestige`), not the names an earlier draft used (`docked`, `mined`, `sold`, `rock_spent` ...). A kind written by a pack script is a **recorded fact** (`Command::RecordFact`) and carries its pack's prefix in data and scripts: `core.shortage_started`, `gm.border_moved`. The spec drops the prefix in prose. Section 4 lists every kind.
- **Script names in causes.** `CauseKind::Script` gains `script: String` (for example `gm.shortage_tuning`), so causes, rollbacks and the chronicle can name the script, not only the pack revision. **Extend**, W1.
- **The `gm-examples` pack's manifest ID is `gm`.** `PackId` allows no hyphen, and `gm` keeps the `gm.` namespace that `AGENTS.md` already uses for live-created things (`gm.sixth_station`). The folder is `packs/gm-examples`; its manifest has `depends = ["core"]`.

### Numbers

The sim stores fixed-point integers, so results are exact, hash the same everywhere, and survive save and load:

| Quantity | Stored as | Example |
| --- | --- | --- |
| Credits and every price (`Player.credits`, `Ware.base_price`, `MarketLine.*price`, `PriceSnapshot.prices`, `Buy`/`Sell` totals, `Condition::PriceAtLeast/AtMost.price`, order floors and ceilings, fees) | whole **centicredits** (1 cr = 100) | the hullwright fee is 800000 |
| Market stock and per-pulse rates; rock `remaining` and regrowth; ledger totals; dispute tallies | **milli-units** (1 unit = 1000) | `regrow` 0.15 is 150; a ration adds 500 to a tally |
| Saturation, `sat_per_unit`, `sat_recover` | **1/10000** | 0.025 is 250; the cap 0.7 is 7000 |
| Pulses (station and region totals, pulse weights) | **milli-pulses** | a neighbour's docking is 500 |
| Cargo in a hold; `Mine`, `Buy`, `Sell`, `Deliver` quantities | whole units | |

- **Rounding.** A unit's price is computed from the formula in `02-economy.md` in `f64` (only `+ − × ÷` and clamps, which are the same on every platform), then rounded half away from zero to a centicredit. That unit is paid exactly that. A sale's total is the sum of its rounded unit prices. The number checks in `07-pacing.md` were rerun with this rounding: no result moved by more than 0.05 cr.
- **Scaled rates** (a lever's x1.4 or x0.5) are rounded half away from zero to a milli-unit when the lever is applied, and stored.
- **Mining.** A ship keeps a mining accumulator in milli-units. Each time it passes 1000, one unit leaves the rock and enters the hold, and the `Mine` event for that run counts it. A rock with less than 1000 milli-units is **spent**; regrowth brings it back (`01-galaxy.md`).
- **Data files** write credits and rates as ordinary decimals (`base_price = 14`, `regrow = 0.15`); the loader converts them and rejects anything finer than the stored precision.
- **Screens** show credits with at most one decimal, and units whole.

## 2. Data files

`contract::data::DATA_FILES` lists 9 files today. The content needs these (all `core`, under `packs/core/data/`, in this load order). The manifest's `data` list matches.

| File | Contract type | Status | What it holds (spec section) |
| --- | --- | --- | --- |
| `wares.toml` | `WaresFile` / `Ware` | exists | Six wares (`02-economy.md`). `ware.fragment` has `tradeable = false`. |
| `hulls.toml` | `HullsFile` / `Hull` | extend | Wren, Tandem, Heron, tender, and the NPC hulls `hull.coop_skiff` (Little Mercy), `hull.compact_barge` (Ninefold, Even Keel), `hull.lamp_launch` (Quiet Lamp), `hull.prospector_skiff` (Magpie). `Hull` gains `turn_rate`, `price_reach` (`docked`, `long_glass`), `abilities: Vec<AbilityId>`, and a `[[rank]]` table per hull (deed counter, threshold, effect). `max_speed` and `mining_rate` take this spec's numbers. |
| `factions.toml` | `FactionsFile` / `Faction` | extend | Three factions. `Faction` gains `colour`, `mark` and `claims: Vec<StationId>`. The regard tiers (thresholds and spreads) and the deed table (deed ID, delta, per-docking cap) are new `[[regard]]` and `[[deed]]` tables in the same file. |
| `regions.toml` | `RegionsFile` / `Region` | exists | Three regions (`01-galaxy.md`). |
| `lanes.toml` | `LanesFile` / `Lane` | extend | Three lanes. `Lane` gains `a_mouth` and `b_mouth` IDs (names for the gates). |
| `stations.toml` | `StationsFile` / `StationDef` / `MarketLineDef` | extend | Five stations, their rings (`dock_radius`), starting parts, and **their market lines**. There is no separate `markets.toml`. `MarketLineDef` gains `role` (`buys`, `sells`), `target`, `rate` (signed; replaces `production` and `consumption`), `sat_per_unit`, `sat_recover`, `needs` (an input ware and how much per pulse, for the smelters and the greenhouse) and `opens_at_stage`. Prices come from `base_price` and the price model; `price`, `min_price` and `max_price` are derived, not authored. |
| `rocks.toml` | `RocksFile` / `RockType` / `RockField` | extend | Four rock types and five fields. `RockType.hardness` keeps its name but its meaning flips to this spec's: a **multiplier** on mining rate, lower is slower (glassite 0.3). `RockType.regrowth` moves to the field. `RockField` gains `regrow` (milli-units per region pulse), `rocks: Vec<Vec2>` (the authored positions; when present, `count` must equal its length and the seeded layout is not used) and `yield` (replaces `amount_min` and `amount_max` for authored fields). |
| `npcs.toml` | `NpcsFile` / `Npc` | extend | Ten NPCs. `Npc` gains `voice` and `remembers` (the fast-model brief), and `flies: Option<ShipId>`. |
| `start.toml` | `StartFile` | exists | 40 cr, the Wren at Lantern Wharf. |
| `rules.toml` | new `RulesFile` | new | Pulse weights, the counted-docking rule (300 u), lane speed (500), mouth radius (70), mining reach (40), queue length (8), saved loops (3), the patient-sell ceiling factor (1.3), the regrowth rules. |
| `pacing.toml` | new `PacingFile` (`[profile.slice]`) | new | Every threshold, cost and tunable in this spec: beat preconditions, upgrade unlocks, trait counts, rank counts, the fee, and the situation tunables (`shortage.rimewater_ice.*`, `dispute.hook.*`, `rumor.ember_shoals.*`, `tender_demo.*`). Loaded into the `tunables` store at new game. |
| `upgrades.toml` | new `UpgradesFile` | new | Upgrades and the tenders: cost, seller, effect, unlock condition (a `Condition` over counters and flags). |
| `controls.toml` | new `ControlsFile` | new | Capabilities and how tiers group them, for presentation. |
| `traders.toml` | new `TradersFile` | new | The four NPC craft: hull, faction, route menu, ware menu, per-docking limits. Little Pail and Magpie are here too, as scripted craft. |
| `traits.toml` | new `TraitsFile` | new | Five traits: deed counter, count, effect ID. |
| `ledgers.toml` | new `LedgersFile` | new | `ledger.kettle_greenhouse`, `ledger.hook_consignment`, `ledger.hook_depot_coop`, `ledger.hook_depot_compact`, `ledger.ice_relief`, `ledger.kettle_condenser`: station, wares counted. |
| `canon.toml` | new `CanonFile` | new | The canon graph (`06-game-master.md`): nodes, status, opener, edges. |
| `lines.toml` | new `LinesFile` | new | Every `core` line: ID, NPC (or `any_dockmaster`), cited fact pattern, text, and `beat` lines' beat and step (`05-people.md`). `gm-examples` ships its own `lines.toml` with the lines marked **(gm)**. |

W1 extends `DATA_FILES` and the loader; W3 owns the market and rock data shapes, W2 hulls and traders, W4 pacing, upgrades, traits, ledgers and lines, W5 canon.

## 3. World state

| Content | Contract | Status |
| --- | --- | --- |
| Regions, lanes, stations (`build.stage`, `build.parts`, `owner`, `dock_radius`) | `Region`, `Lane`, `Station`, `BuildState` | exists |
| Kettle stages and parts, the banner, the condenser, the fragment case | `Command::SetStationStage`, `SetStationPart` | exists |
| Depot owner | `Station.owner`, `Command::SetStationOwner` | exists |
| Markets | `Market`, `MarketLine` | **extend**: `MarketLine` gains `role`, `target`, `stock_milli` (replaces `stock`), `sat` (1/10000), `sat_per_unit`, `sat_recover`, `rate_milli`, `needs`, `base_factor` and `rate_factor` (the levers, so a lever is undone exactly), and `ceiling()` derived. W3. |
| Pulse totals | none | **new**: `Station.pulses_milli`, `Region.pulses_milli` (`u64`), advanced by W3's pulse system from `dock{counted}` and `jump` events of **every** ship. Not counters: counters stay player-only. |
| Rocks and regrowth | `Rock`, `RockField`, `Command::AdjustRock` | **extend**: `Rock.remaining_milli` and `initial_milli` (replace the unit fields); `RockField.regrow`. W2. |
| Ships, holds, orders | `Ship`, `Hold`, `OrderQueue` | **extend**: `Hold` gains `origins: BTreeMap<WareId, BTreeMap<AnyId, u32>>` (how much of each ware came from which field or station; `cargo` stays the total) and `consigned: BTreeMap<WareId, u32>`. `Ship` gains `mining_accumulator_milli`. `OrderQueue` gains W2's progress fields, including a patient sell's `waiting`, `blocked` and `pulses_waited`. W2. |
| Player wallet, flags, traits, snapshots | `Player` | exists (credits become centicredits, above) |
| Fittings, hull ranks, saved loops | none | **new** on `Player`: `fittings: BTreeSet<UpgradeId>`, `hull_records: BTreeMap<HullId, HullRecord { rank, dockings, units_sold, jumps, transfers, far_dockings }>`, `loops: BTreeMap<LoopName, Vec<Order>>` (at most 3). W2 (loops), W4 (fittings, ranks). |
| Standing | `Faction.standing`, `Command::AdjustStanding` | exists. Regard is derived from it, never stored. |
| NPC craft and tenders | `Ship` with `Owner::Faction`, `Owner::Npc` (Little Pail, Magpie) or `Owner::Player` (Thimble, Button) | exists |
| Rumor | `Rumor` (`is_true`, `planted_by`, `heard_at`, `heard_by_player`, `source`) | **extend**: `Rumor` gains `told_by: NpcId`, `source_craft: Option<ShipId>`, `site: Vec2`, `followed: bool`, `resolved: bool`. W4 (core beat), read by W5. |
| Ledgers and dispute tallies | none | **new** store `ledgers: Store<LedgerId, Ledger { station, totals_milli: BTreeMap<WareId, i64> }>`, command `AdjustLedger { ledger, ware, delta_milli }`. W4. |
| Situations (shortage, dispute) | none | **new** store `situations: Store<SituationId, Situation { beat, status: dormant / active / resolved, outcome, params }>`. `params` is the frozen copy of its tunables taken at start. Command `PutSituation`. W4. |
| Tunables | none | **new** `tunables: BTreeMap<TunableId, Tunable>` with `Tunable = Number(i64 fixed-point) / Id(AnyId) / Ids(Vec<AnyId>) / Override(BTreeMap<String, Tunable>) / Absent`, loaded from `pacing.toml`. Each entry also records `set_by: Option<CauseId>` (empty for pack data), which is how a beat knows it was `tuned`. Command `SetTunable { id, value }` (reversible, so a gm script's write rolls back exactly). W1 and W4. |
| Beats | none | **new** store `beats: Store<BeatId, BeatState { status, step, started: FactId, completed: Option<FactId> }>`. W4. |
| Canon graph | none | **new** store `canon: Store<CanonId, CanonNode { status, statement, opener, edges }>`, command `SetCanonStatus`. `opener` is `Beat { beat, step }` (core), `Pack` (any dependent pack, claimed through its manifest's `opens` list) or absent. Core data never names a pack ID. W5. |
| Map zones and props | none | **new** stores `zones: Store<ZoneId, Zone { faction: Option<FactionId>, center, radius, style }>` and `props: Store<PropId, Prop { position, kind, readable: Option<String> }>`, commands `PutZone`, `PutProp`. W3 (zones, map), W2 (props in space). |
| NPC lines used | none | Stored as history (`npc_line` facts); no store. |

## 4. Events and facts

### Engine events (native)

| Spec fact | Contract | Status | Change |
| --- | --- | --- | --- |
| `dock`, `undock` | `Event::Dock`, `Event::Undock` | extend | `Dock` gains `counted: bool`; only counted dockings bump counters and make pulses. W2. |
| `mine` | `Event::Mine` | extend | One event per mining run (start to stop on one rock), with `field` added. W2. |
| `rock_depleted` | `Event::RockDepleted` | extend | Gains `field`. W2. |
| `rock_reformed` | none | new | `Event::RockReformed { rock, field }`. W2. |
| `sell`, `buy` | `Event::Sell`, `Event::Buy` | extend | `unit_price` is replaced by `total` (centicredits) and `avg_price`; `Sell` gains `sat_after` and `origin: Option<AnyId>`. One event per ware, origin and selling burst. W3. |
| `market_flooded` | none | new | `Event::MarketFlooded { ship, station, ware, sat_after }`. W3. |
| `scout` | `Event::Scout` | exists | The snapshot itself is `Command::SetPriceSnapshot`. |
| `jump` | `Event::Jump` | exists | |
| `deliver` | `Event::Deliver` | extend | Gains `ledger: LedgerId`. Emitted by `core` scripts through `Command::Emit`. W1. |
| `paused` | none | new | `Event::Paused { session }`, a player event with no counter bump. The app submits it when the card opens; it commits on the first tick after the card closes. W6 emits, W1 adds. |
| `order_done` | `Event::OrderDone` | extend | Gains `target: Option<AnyId>` and `result` (`done`, `skipped`, `cleared`, `blocked`). W2. |
| `queue_done`, `loop_saved`, `loop_lap`, `loop_stalled` | none | new | One event each. W2. |
| `patient_wait`, `patient_sold`, `patient_blocked`, `patient_unblocked`, `floor_changed` | none | new | One event each, fields as in `SPEC.md`. W2. |
| `tender_acquired`, `tender_assigned` | none | new | W2. |
| `beat_started`, `beat_step`, `beat_completed` | `Event::QuestStep { quest, step }` | extend | Replace `QuestStep` with `BeatStarted { beat }`, `BeatStep { beat, step }`, `BeatCompleted { beat }` (`BeatId`). `QuestId` is dropped. W4. |
| `trait_earned` | `Event::TraitEarned` | extend | Gains `deed_count`. W4. |
| `prestige` | `Event::Prestige` | extend | Gains `fee`. W4. |
| `upgrade_fitted`, `cap_unlocked`, `hull_rank`, `standing_changed`, `regard_changed` | none | new | One event each. W4. |
| `tagged`, `untagged` | none | new | `Event::Tagged { ship, rock }`, `Untagged`. Reserved in the contract, rejected unless `mech.rock_tagging` is set. W2. |
| `npc_line`, `asked_what_changed` | none | new | W5 (talk channel). |
| `session_start` | `Event::SessionStart` | exists | Counts `player.sessions`. |

### Recorded facts (packs)

Written with `Command::RecordFact`. `FactDraft` today has `kind`, `subjects`, `amount` and a free-text `note`, but lines and beats match on fields such as `way`, `truth` and `stations`. **Extend:** `FactDraft` and `FactPayload::Note` gain `fields: BTreeMap<String, FactValue>` (`FactValue` = integer, fixed-point, bool, ID, ID list or short text), and `HistoryQuery` gains `fields: BTreeMap<String, FactValue>` for equality matches. W1, used by W5's line matcher and W4's beat runtime.

| Kind | Pack | Subjects used (`Subjects` field) |
| --- | --- | --- |
| `core.stage_built` | core (stage 1); `gm.stage_built` for stages 2 and 3 | station |
| `core.shortage_started`, `core.shortage_helped`, `core.shortage_resolved` | core | station (Kettle), region, other = the situation |
| `core.dispute_started`, `core.side_chosen`, `core.territory_changed` | core | station (Cinder Hook), faction |
| `core.rumor_heard`, `core.rumor_spread`, `core.rumor_followed`, `core.rumor_resolved` | core | station, npc, other = the rumor |
| `core.fragment_found`, `core.shown` | core | ship, rock / npc, station |
| `core.canon_opened` | core (b15); `gm.canon_opened` for `second_fragment` | other = the canon node |
| `core.mechanic_taught` | core (tender); `gm.mechanic_taught` (rock tagging) | npc, other = the mechanic |
| `gm.rumor_exposed` | gm | faction, other = the rumor |
| `gm.border_moved` | gm | faction, station, other = the zone |
| `gm.station_founded` | gm | station, faction, region |
| `gm.chronicle` | gm | the place and person it names |

A line or beat that cites "`stage_built`" matches both `core.stage_built` and `gm.stage_built`: the matcher treats `<pack>.<kind>` and `<kind>` as the same kind for citing. `core` data only ever names `core.*` and native kinds.

## 5. Counters

| Spec counter | Contract | Status |
| --- | --- | --- |
| `player.dockings`, `player.dockings_at.<station>` | `CounterKind::Dockings`, scopes `Player` / `Station` | extend: count only `counted` dockings |
| `player.jumps` | `Jumps` | exists |
| `player.sessions` | `Sessions` | exists |
| `player.units_mined.<ware>.<region>` | `UnitsMined` | extend: counters gain an optional ware dimension (`CounterScope::PlayerWare`, `RegionWare`) |
| `player.rocks_spent` | `RocksDepleted` | exists |
| `player.units_sold`, `.<ware>` | `Sales` counts transactions, not units | new: `UnitsSold`, with the ware dimension |
| `player.split_sales`, `player.ledger_deliveries`, `player.queues_done`, `player.loop_laps`, `player.stations_visited` | none | new: one `CounterKind` each |
| `hull.<id>.*` | none | new: in `Player.hull_records`, not counters (they reset at prestige) |
| `craft.<id>.laps`, `craft.<id>.patient_sells` | none | new: `CounterScope::Craft { ship }` |
| `station.<id>.pulses`, `region.<id>.pulses` | none | new: world state, see section 3 |
| `since.<fact_id>.<counter>` | `HistoryQuery { after: fact, .. }` counted with `count` | exists |

**Who counts.** Today `Event::counter_bumps` lands in the player scope for every `Owner::Player` ship, so a tender's dockings and sales would count toward the nav chip, Ringwise or Wren ranks. **Change:** the `Player`, `Station` and `Region` scopes count `Player.ship` (the flagship, whatever its hull) only; other player-owned ships bump `CounterScope::Craft { ship }`. `Deliveries` stops counting sales (`Sell` bumps `UnitsSold` instead). W1.

## 6. Orders and verbs

| Spec verb | Contract `Order` | Status | Change |
| --- | --- | --- | --- |
| `fly` | `Fly { to: FlyTarget }` | extend | `FlyTarget` gains `Field { field }` and `Mouth { lane, end }`. |
| `mine` | `Mine { rock }` | extend | `Mine { target: MineTarget }`, `MineTarget = Rock { rock } / Field { field, prefer_tagged: bool }`. |
| `dock` | `Dock { station }` | exists | Routes through lanes when needed (W2). |
| `buy` | `Buy { ware, quantity }` | extend | Gains `max_price: Option<u32>`. `Quantity::All` is the spec's `fill`. |
| `sell` | `Sell { ware, quantity }` | extend | Gains `floor: Option<u32>`. |
| `wait` | `Wait { until: Condition }` | extend | `Condition` gains `ShipAt { ship, station }` (the spec's `tender_at`). The rest exist. |
| `loop` | `Loop { orders, until }` | extend | Gains `name: Option<LoopName>`; a saved loop is copied into the order when assigned. |
| `transfer` | none | new | `Transfer { ware, quantity, to: ShipId }`; `VerbKind::Transfer`. |
| `tag` | none | new | `Tag { rock }`; `VerbKind::Tag`. Reserved; rejected unless `mech.rock_tagging` is set. |
| The patient sell | none | W2 behaviour | A floored `Sell` on a tender at a standing berth; floors are revalidated when `MarketLine.base_factor` changes. |

`PlayerInput` and `ClickTarget` gain `Field` and `Mouth` to match, and `Click` gains the `T` and right-click tag slots once tagging is taught (W2, W6).

## 7. What each piece uses

Hooks are the `contract::api::Hook` namespaces. Commands are `contract::command::Command` variants; "Emit" means `Command::Emit` of the events named.

| Piece | Hooks | Commands | Reads | Writes (events and facts) |
| --- | --- | --- | --- | --- |
| Flight, docking, mining, lanes (W2, native) | none | `SetShipMotion`, `SetShipDocked`, `SetShipRegion`, `AdjustCargo`, `AdjustRock`, Emit | input, `rules.toml` | `dock`, `undock`, `mine`, `rock_depleted`, `rock_reformed`, `jump` |
| Market, pulses, snapshots (W3, native) | none | `SetMarketLine`, `AdjustCredits`, `AdjustCargo`, `SetPriceSnapshot`, Emit | `dock`, `jump` events | `buy`, `sell`, `market_flooded`, `scout` |
| Orders, queues, loops, tenders, patient sell (W2, native) | none | `SetOrders`, the above, Emit | `Station.pulses_milli`, market lines | `order_done`, `queue_done`, `loop_*`, `patient_*`, `floor_changed`, `tender_*` |
| Beat runtime, standing, traits, ranks, prestige, upgrades (W4, native) | Beats (as host) | `AdjustStanding`, `SetFlag`, `SetTrait`, `AdjustCredits`, `PutShip`, `PutHull`, Emit | counters, history, tunables | `beat_*`, `standing_changed`, `regard_changed`, `trait_earned`, `hull_rank`, `prestige`, `upgrade_fitted`, `cap_unlocked` |
| `core` b01 to b08, b11, b14, b16, b17 (Luau) | Beats, Events, Counters, History, Talk, Stores | `AdjustLedger`, `AdjustCargo` (Ilse's consigned plate), `SetStationStage`, `SetStationPart`, Emit `deliver` | counters, `pacing.toml` tunables | beat events, `deliver`, `core.stage_built` |
| `core` b09 dispute | Beats, Events, Stores, Market, Kit, Talk, History | `PutSituation`, `AdjustLedger`, `SetStationOwner`, `SetStationPart` (banner), `SetMarketLine` (base factors), `AdjustStanding`, `AdjustCredits` | `sell` at the Hook, NPC `dock` at the Hook, `dispute.hook.*` | `core.dispute_started`, `core.side_chosen`, `core.territory_changed` |
| `core` b10 rumor | Beats, Events, Stores, History, Mind (spread), Talk | `PutRumor`, `PutField`, `PutRock`, `PutProp` | newest NPC `dock` at Brassmouth, `rumor.ember_shoals.*` (including the override) | `core.rumor_*` |
| `core` b12 shortage | Beats, Events, Stores, Market, Kit, Talk, Mind (Even Keel's menu) | `PutSituation`, `SetMarketLine`, `SetStationPart`, `AdjustLedger`, `AdjustStanding` | `field.rimewater` remaining, counters, `shortage.rimewater_ice.*` | `core.shortage_*`, `deliver` |
| `core` b13 tender | Beats, Orders (Little Pail), Events, Talk, Stores | `PutShip` (Thimble), `AdjustCargo` (Little Pail's ice), `SetOrders` (Little Pail), `AdjustCredits` | Kettle's ice `sat`, `patient_*` events, `tender_demo.*` | beat events, `core.mechanic_taught`, `tender_acquired` |
| `core` b15 fragment | Beats, Events, Canon, Kit, Talk, Stores | `AdjustCargo` (the fragment), `SetCanonStatus`, `SetStationPart` (the case), `AdjustCredits`, `AdjustStanding` | `rock_depleted` (glassite), counters, `jump` | `core.fragment_found`, `core.shown`, `core.canon_opened` |
| `gm.kettle_growth` | Events, Stores, Market, Kit, Talk | `AdjustLedger`, `SetStationStage`, `SetStationPart`, `SetMarketLine`, `PutStation` (ring radius) | `sell` at Kettle | `gm.stage_built` |
| `gm.shortage_tuning` | Events, History, Stores | `SetTunable` | `sell{ware = ice, origin = field.rimewater}` | none (`core` writes `shortage_started{tuned = true}`) |
| `gm.territory_map` | Events, Stores, Talk (map) | `PutZone` | `core.territory_changed` | `gm.border_moved` |
| `gm.compact_plant` | Events, History, Stores, Mind (Ninefold's menu), Talk | `SetTunable`, `PutProp`, `AdjustStanding`, `AdjustCredits` | tallies, `core.side_chosen`, `core.rumor_followed` | `gm.rumor_exposed` |
| `gm.rock_tagging` | Mechanics, Orders (Magpie), Events, Talk | `SetFlag`, `SetOrders` (Magpie) | counters, `dock` at the Hook | `gm.mechanic_taught`, `tagged` |
| `gm.ancients_bearing` | Canon, Events, Stores, Talk (map) | `SetCanonStatus`, `PutZone` (the bearing, drawn as a zone with `style = bearing`) | `beat_completed{b15}` | `gm.canon_opened` |
| `gm.sixth_station` | Events, History, Stores, Kit, Market, Mind, Talk | `PutStation`, `PutMarket`, `PutNpc`, `SetStationPart` | anchor facts, counters | `gm.station_founded` |
| `gm.chronicle` | Events, Talk (chronicle) | none | the kinds listed in `06-game-master.md` | `gm.chronicle` (a `TalkLine` on `Channel::Chronicle`) |

Talk lines are `contract::talk::TalkLine` (`cites` never empty), on `Channel::DockTalk`, `MapNote` or `Chronicle`; `TalkLine` gains `line: Option<LineId>` and `use: ambient / beat`. W5.

## 8. Extensions by workstream

In the order the content first needs them.

| Workstream | Extension | First needed by |
| --- | --- | --- |
| W1 | Centicredits and milli-unit fixed point in `Player`, `MarketLine`, `Rock`, events and conditions; `WORLD_FORMAT_VERSION` 1 → 2 (no saves exist yet) | Act 1 (b04's first sale) |
| W1 | Flagship-only player counters; `CounterScope::Craft`; ware dimension; `UnitsSold` and the new counter kinds | b06 (the nav chip's docking count) |
| W1 | `Event::Paused` | b05 |
| W1 | `FactDraft.fields`, `HistoryQuery.fields`; `CauseKind::Script.script` | b03 (`deliver` to a ledger), every line |
| W1 | `DATA_FILES` and loader for the new files; the ID renames | The `core` data handoff |
| W1, W4 | `tunables`, `SetTunable`; `ledgers`, `AdjustLedger`; `situations`; `beats`; `Rumor` fields | b03 (ledger), b09, b10, b12 |
| W2 | `Dock.counted`; per-run `Mine`; `RockReformed`; regrowth from region pulses; authored rock positions; `Hold.origins` and `consigned` | b02, b07, b12 |
| W2 | Order extensions (`max_price`, `floor`, `MineTarget`, `FlyTarget::Field/Mouth`, `ShipAt`, `Transfer`, `Tag`); queue limit 8; saved loops; order results; queue, loop and patient events; floor revalidation | b11, b13 |
| W3 | `MarketLine` model, levers as factors, pulses as world state, `MarketFlooded`, `Sell` totals and origin, zones | b04, b08, b12 |
| W4 | Beat events replacing `QuestStep`; standing, regard, traits, ranks, prestige, upgrade and capability events; fittings and hull records | b03 onward |
| W5 | Canon store and `SetCanonStatus`; line matcher with ambient and beat use; `npc_line`; the `gm` pack validator rule that `core` never names `gm` | b15, b14, b17 |
| W6 | Emitting `paused`; showing blocked tender orders, the bearing and zones on the map | b05, b13 |
