# 02 Wares and the living market

Back to [SPEC.md](SPEC.md). Prices are in credits (cr). Every number here is `core` pack data (`wares.toml`; the market lines in `stations.toml`; `traders.toml`). Credits are whole centicredits in the sim; the decimals below are real, not rounding noise (`08-contract.md`, "Numbers").

## Wares

| ID | Name | Source | Notes |
| --- | --- | --- | --- |
| `ware.ice` | Ice | Mined (Rimewater, Hookside); sold by Brassmouth | Life support and greenhouse water. Every station except Brassmouth drinks it. |
| `ware.iron_ore` | Iron ore | Mined (Tinroot, Cinder Belt); sold by Cinder Hook | Smelted into plate at the Wharf and Brassmouth. |
| `ware.plate` | Refined plate | Sold by Lantern Wharf and Brassmouth | Building material. Cinder Hook and Kettle Dock want it badly. |
| `ware.glassite` | Glassite | Mined (Glass Drift; Ember Shoals if the rumor is true); sold by Cinder Hook | Lens glass. Brassmouth pays best, until it is flooded. |
| `ware.rations` | Rations | Sold by Lantern Wharf, and by Kettle Dock from stage 2 | Low margin and slow to saturate; NPC traders move most of it. |
| `ware.fragment` | Ancients fragment | Found once (see `01-galaxy.md`) | Not for sale anywhere: no market lists it, and the sell screen shows it as "not for sale". It takes one hold slot. |

Cargo is held in **stacks**. Each stack records its ware, quantity and **origin** (the field or station it came from). The shortage beat uses origin. Consigned cargo (Ilse's plate in b07) is a stack flagged `consigned` that can't be sold or jettisoned. The slice has no jettison at all.

## Price model

Each station lists the wares it **buys** (consumes) and the wares it **sells** (produces). A station never buys a ware it produces, or sells one it consumes. Per station and ware, the market keeps `stock` and, for wares it buys, `sat` (saturation, 0 to 0.7).

```
stock_factor = clamp(1 + 0.5 * (1 - stock / target), 0.7, 1.3)

price the station pays, per unit   = base * stock_factor * (1 - sat)
price the station asks, per unit   = base * stock_factor * spread
spread                             = 1.15 (see regard, 05-people.md: 1.20 Cold, 1.15 Stranger or Known, 1.10 Trusted, 1.06 Kin)
```

- **Per unit.** A sale of N units is N single-unit trades. Each unit sold adds 1 to `stock` and `sat_per_unit` to `sat` (capped at 0.7), then the next unit's price is recalculated. The sell screen shows the price falling unit by unit (beat b04 teaches this).
- **Buying** removes stock, which raises the asking price through `stock_factor`. Saturation doesn't affect asking prices.
- **On each station pulse** (see `SPEC.md`): producers add `rate` to stock (only while their inputs are in stock, and only up to 2 x target); consumers remove `rate` from stock (down to 0); every bought ware's `sat` falls by `sat_recover` (down to 0). Saturation never recovers on its own; only activity at or near the station clears it.
- **Floors and ceilings:** the lowest a station ever pays is `0.21 * base` (fully stocked and fully saturated). The highest is `1.3 * base`.
- Any ship's sales count: the player's, a tender's and NPC craft's. NPC craft sell at most 10 units of a ware per docking, so they nudge prices rather than crash them.

### Production chains

- Lantern Wharf's smelter makes 2 plate per pulse and uses 3 iron ore per pulse. With no ore in stock, plate production stops and the ore price sits at its ceiling.
- Brassmouth Exchange's foundry works the same way (2 plate per pulse from 3 ore).
- From stage 2, Kettle Dock's greenhouse makes 2 rations per pulse (3 from stage 3) and uses 1 extra ice per pulse on top of its normal drink.

## Market tables

`role`: B = the station buys it, S = the station sells it. `rate`: stock change per station pulse (+ produce, − consume). `sat/unit` and `recover` apply to bought wares only. "Opening price" is what the player sees on the first docking (pays for B, asks at the Stranger spread for S).

### Lantern Wharf (`station.lantern_wharf`, Wharf Cooperative)

| Ware | Role | Base | Target | Start stock | Rate | sat/unit | Recover | Opening price |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| Ice | B | 10 | 160 | 150 | −2 | 0.02 | 0.05 | pays 10.3 |
| Iron ore | B | 14 | 200 | 190 | −3 (smelter) | 0.015 | 0.05 | pays 14.3 |
| Plate | S | 30 | 120 | 120 | +2 (needs ore) | | | asks 34.5 |
| Glassite | B | 24 | 80 | 70 | −1 | 0.03 | 0.04 | pays 25.5 |
| Rations | S | 14 | 150 | 150 | +2 | | | asks 16.1 |

### Kettle Dock (`station.kettle_dock`, Wharf Cooperative)

| Ware | Role | Base | Target | Start stock | Rate | sat/unit | Recover | Opening price |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| Ice | B | 12 | 160 | 120 | −3 (−4 from stage 2) | 0.025 | 0.05 | pays 13.5 |
| Plate | B | 38 | 60 | 40 | −1 | 0.03 | 0.04 | pays 44.3 |
| Rations | S from stage 2 | 15 | 120 | 0 at stage 2 | +2 (+3 at stage 3) | | | asks up to 22.4 while empty |
| Glassite | B from stage 3 | 30 | 40 | 20 | −1 | 0.04 | 0.04 | pays 37.5 when it opens |

### Cinder Hook (`station.cinder_hook`, contested)

| Ware | Role | Base | Target | Start stock | Rate | sat/unit | Recover | Opening price |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| Ice | B | 14 | 100 | 70 | −2 | 0.03 | 0.05 | pays 16.1 |
| Iron ore | S | 6 | 300 | 300 | +4 | | | asks 6.9 |
| Plate | B | 55 | 100 | 80 | −2 | 0.02 | 0.05 | pays 60.5 |
| Glassite | S | 16 | 100 | 80 | +1 | | | asks 20.2 |
| Rations | B | 26 | 120 | 80 | −2 | 0.01 | 0.05 | pays 30.3 |

The dispute changes some of these bases once the depot changes hands (see `04-journey.md`, b09).

### Glass Drift Observatory (`station.glass_drift_observatory`, Lamplighter Society)

| Ware | Role | Base | Target | Start stock | Rate | sat/unit | Recover | Opening price |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| Ice | B | 13 | 60 | 40 | −1 | 0.03 | 0.05 | pays 15.2 |
| Glassite | B | 36 | 60 | 50 | −1 | 0.04 | 0.04 | pays 39.0 |
| Rations | B | 24 | 80 | 60 | −1 | 0.015 | 0.05 | pays 27.0 |

A small market: it buys a little of everything a research tower needs, and saturates fast.

### Brassmouth Exchange (`station.brassmouth_exchange`, Tollway Compact)

| Ware | Role | Base | Target | Start stock | Rate | sat/unit | Recover | Opening price |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| Ice | S | 8 | 200 | 200 | +3 | | | asks 9.2 |
| Iron ore | B | 16 | 200 | 180 | −3 (foundry) | 0.015 | 0.05 | pays 16.8 |
| Plate | S | 26 | 150 | 150 | +2 (needs ore) | | | asks 29.9 |
| Glassite | B | 48 | 120 | 110 | −2 | 0.03 | 0.05 | pays 50.0 |
| Rations | B | 20 | 200 | 180 | −2 | 0.01 | 0.06 | pays 21.0 |

## Where the gaps are

Margins per unit at opening prices, before saturation. A full Wren hold is 20 units (30 with the hold extender). See `07-pacing.md` for what saturation does to these.

| Route | Buy | Sell | Gap per unit | Notes |
| --- | --- | --- | --- | --- |
| Mined glassite to Brassmouth | free (slow: hardness 0.3) | 50.0 | 50 | The best price in the slice, and the fastest to saturate |
| Mined glassite to the Observatory | free | 39.0 | 39 | 7 s from the Glass Drift field; saturates even faster |
| Cinder Hook glassite to Brassmouth | 20.2 | 50.0 | 29.8 | No mining needed; one lane |
| Brassmouth plate to Cinder Hook | 29.9 | 60.5 | 30.6 | The dispute route; one lane |
| Wharf plate to Cinder Hook | 34.5 | 60.5 | 26.0 | Ilse's route |
| Wharf plate to Kettle Dock | 34.5 | 44.3 | 9.8 | Short hop; also builds Kettle's stages |
| Cinder Hook ore to Brassmouth | 6.9 | 16.8 | 9.9 | Cheap to fill; plenty of stock |
| Mined Cinder Belt ore to Brassmouth | free | 16.8 | 16.8 | 60-unit rocks |
| Wharf rations to Cinder Hook | 16.1 | 30.3 | 14.2 | Slow to saturate (0.01 per unit) |
| Brassmouth ice to Kettle Dock | 9.2 | 13.5 | 4.3 | Thin, until the ice shortage (then up to 21.8) |
| Mined Rimewater ice to Kettle Dock | free (fast: hardness 1.0) | 13.5 | 13.5 | The Act 1 staple; it drains Rimewater |
| Mined Tinroot ore to the Wharf | free | 14.3 | 14.3 | The Act 1 staple |

A natural circuit opens in Act 2: Wharf plate to Cinder Hook, Cinder Hook glassite to Brassmouth, Brassmouth plate back to Cinder Hook. Each leg earns. Repeating any one leg blindly saturates its buyer.

## Snapshots and price reach

- Docking at a station shows its live prices and stores a **snapshot** (a `scout` fact) of every ware there.
- Away from a station, the map and market screens show the last snapshot, labelled by how many of the player's dockings ago it was taken: "seen 6 dockings ago". A station never docked at shows "?" for every ware. There are no live remote prices at the home dock.
- **Price reach** widens what is live:
  - Wren and Tandem: live prices only while docked.
  - `upgrade.price_glass` (any hull): also live prices for every station within 1200 u of the ship.
  - Heron ("Long glass"): live prices for every station in the current region; entering a lane mouth refreshes the snapshots of the stations in the destination region.
- A snapshot's age resets only through scouting. Nothing ever expires.

## NPC traders

Four authored craft keep the markets breathing and generate pulses. They use the same verbs as the player (fly, dock, buy, sell) and the same market rules. Each docking, the craft's utility AI scores the trades it is allowed from its own snapshots and keeps the top 3. The fast-model layer (TypeSafe Jev) picks one, and the craft's captain may get a dock-talk line. If the call fails or is slow, the craft takes the top-scored option. Test runs use a seeded fake, so they stay deterministic.

| ID | Name | Faction | Hold | Speed | Stations it may visit | Wares it may trade | Character |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `craft.little_mercy` | Little Mercy | Wharf Cooperative | 30 | 40 | Lantern Wharf, Kettle Dock, Cinder Hook | ice, plate, rations, iron ore | A patched co-op skiff that never leaves anyone short of rations |
| `craft.ninefold` | Ninefold | Tollway Compact | 60 | 35 | Cinder Hook, Brassmouth Exchange | iron ore, plate, glassite, rations | A wide brass barge working the Cinder ore trade |
| `craft.even_keel` | Even Keel | Tollway Compact | 60 | 35 | Brassmouth Exchange, Lantern Wharf, Kettle Dock | ice, plate, rations, iron ore | A tidy barge bringing Compact ice and plate into the Shallows |
| `craft.quiet_lamp` | Quiet Lamp | Lamplighter Society | 20 | 55 | Glass Drift Observatory, Lantern Wharf, Cinder Hook | rations, ice, glassite | A slim launch that also tends the lane lamps |

Rules: at most 10 units of a ware per docking, at most 2 wares per docking, and every trade writes `buy` or `sell` with the craft as actor. Estimated traffic: each craft makes a counted docking about every 50 to 70 seconds of play (which gives the pulse rates in `SPEC.md`). Scripts may add craft or widen their menus through the fast-model budget hook; they may never steer one directly.
