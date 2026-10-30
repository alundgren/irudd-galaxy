# Game-master examples

Core carries all 17 beats alone. These eight independently removable packs only
vary its dormant tunables, draw its results, or add optional opportunities.

| Pack | Change | Person and place |
| --- | --- | --- |
| kettle_growth | More greenhouse rings from deliveries | Odo at Kettle Dock |
| shortage_tuning | FIFO mining history selects ice buyers and relief quota | Odo at Kettle Dock |
| territory_map | Contested borders become the depot winner's border | Hesper at Cinder Hook |
| compact_plant | A false Compact survey and a readable proof buoy | Pip at Brassmouth; Rook at the Hook |
| rock_tagging | Magpie demonstrates lamps before Juno teaches T/right-click and tagged field orders | Juno at Cinder Hook |
| ancients_bearing | Manifest-declared revelation after b15, with a direction-only map bearing | Nell at Glass Drift |
| sixth_station | A new station assembled from a pack-authored recipe by the player's ice workings | Wynn at Meltwater Halt |
| chronicle | Cited entries and Ilse's answer to What changed? | Ilse at Lantern Wharf |

`bad_script` and `bad_sandbox` contain intentionally rejected examples.

Run `galaxy pack check packs/gm-examples/NAME --save SAVE`, then
`galaxy gm propose packs/gm-examples/NAME --save SAVE`. Proposals run strict types,
canon checks, and paired sandbox journeys. The bearing proposal requires b15 to
be complete; its bundled copy can wait for that condition on a new save.

Changes check the current docking before writing. The bridge stages installations
at calm moments. Rolling back retires the revision and says its manifest's line
in-world. History and chronicle entries remain, with a compensating entry.
A founded station closes to a scaffold, preserving a berth for any ship already
there. Its market and new person are removed; orders skip its closed ring. A later
revision of the same pack reopens the scaffold in place (`SetFoundedStation`), and
the one-new-station cap counts only open rings.

## Needed a base change

- `galaxy-kit`: recipe data, palettes, colour validation, and art-bible checks moved
  out of `render`; the renderer still builds meshes and lighting.
- `ctx.kit.station`: atomic station, recipe, resident dockmaster, and market, with
  player-history placement checks; a closed-scaffold rollback. It also needs
  `npcs.write` and `market.lines`.
- `ctx.kit.overlay` and `ctx.kit.buoy`: saved borders/bearings and a readable survey
  log, drawn by the app's small runtime drawing adapter.
- `canon.toml`, manifest `opens`/`asserts`, `ctx.canon`, reversible revelation,
  and the bridge's formerly skipped canon validation step. These checks enforce
  structured node assertions and opening rights, not arbitrary natural-language
  truth in free prose.
- Native rock lamps, T/right-click, `tag` and `mine_field { prefer_tagged = true }`
  orders, six-lamp cap, and the fallback to ordinary richest-rock mining.
- Optional dependency records in manifest `observes`, for records core creates later.
- Native survey-proof rewards claimed through dock replies, so rollback keeps earned
  credits and standing. `galaxy-progress` pays them, standing as a deed. The host
  gates a reward on the pack's `beats.write` (credits) and `standing.write`
  (standing), and only the pilot sets a buoy read or a reward claimed. Reads and
  claims are compare-and-set (`UpdateSurveyBuoy`), and lamps carry `ExpectFlag`, so
  a removal or retirement queued in the same tick wins.
- Reject game-master packs that register journey beats, judged by how the pack is
  loaded (`ScriptHost::load_layer`, `CheckOptions::game_master`), not by its ID. The
  bundled examples load as layers too, and each pack's layer is saved in the world
  (`PackState::layer`), so every later load (a reload, `play`, `sim run`, the bridge,
  the sandbox) keeps the restriction.
- Facts in each pack's own namespace, its whole ID: `gm.kettle_growth.stage_built`,
  `gm.shortage_tuning.shortage_tuned`, `gm.territory_map.border_moved`,
  `gm.compact_plant.rumor_exposed`, `gm.rock_tagging.mechanic_taught`,
  `gm.ancients_bearing.canon_opened`, `gm.sixth_station.station_founded`. The
  chronicle reads each by its full kind.
- One capability table for every route a pack writes through
  (`galaxy_contract::needs::COMMAND_NEEDS`): the host checks every batch a pack causes,
  beat rewards and data installs included, and `pack check` checks what a pack's data
  and beat rewards imply. `kettle_growth` (and `core`) now declare `npcs.write` for
  their `npcs.toml`; the stock lever writes `SetMarketStock`, so only adding or removing
  a line needs `market.lines`.
- Keep repeated dockmaster questions from replaying the same chronicle page.

Validation lives in `crates/app/tests/gm_examples.rs` (real CLI proposals and
rollback), `crates/script/tests/live_layers.rs` (types, canon and art rejection, and the
PR #31 review regressions),
and `crates/script/tests/journey_gm.rs` (the existing journey acts with all layers).
The viewed [sixth-station shot](../../docs/screenshots/gm-sixth-station.png) is 212 KB.

The station example demonstrates Meltwater Halt; the design's other candidate kinds
are not instantiated. The Ninefold buy-limit variation and adding the sixth station
to a trader route await the separate NPC-trader integration. The chronicle uses the
existing dockmaster answer and talk-log presentation; this change adds no new screen.
