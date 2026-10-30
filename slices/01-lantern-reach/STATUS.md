# The Lantern Reach: end-of-run status

A release candidate for the first vertical slice (issue #10, decisions on PR #11). The
whole arc is in and playable, from undocking the Wren to the Hearthbound choice and home
again. After the first playtest's fixes (below), the journey bot reaches prestige in about
two hours of game time on authored data, as the design intends. Read "What doesn't"
before you play.

## Checks at this release candidate

| Where | What ran | Result |
|---|---|---|
| Windows 11 | `cargo xtask check` (fetch-luau, fmt, clippy `-D warnings`, tests) | Pass: 512 tests, 0 failed, 19 ignored (after the playtest fixes; 509 at the release candidate) |
| Windows 11 | `cargo test -p render -- --ignored` (GPU) | Pass: 9 of 9 |
| Windows 11 | `galaxy play` for 40,000 frames, plus the live game-master steps | 0 render errors |
| Linux (WSL 2, Ubuntu 26.04) | `cargo xtask check`, then `cargo build --release` (thin LTO) | Pass: 509 tests, 0 failed, 19 ignored; release builds |
| Linux (WSL 2) | headless `galaxy shot` of a new game, mining | Renders the same frame as Windows |
| Linux (WSL 2) | render GPU tests on lavapipe (Mesa's software Vulkan) | Pass: 9 of 9 with `VK_ICD_FILENAMES` pinned to lavapipe. With WSL's default driver list they crash with SIGSEGV in the driver |

Linux had no real GPU, no desktop session and no audio device. macOS is untested.

### The journey bot on the authored pacing

After the playtest fixes (Wren at 72 u/s, the Glass Drift regrowing 1.5 a pulse instead
of 0.15): Run A 17 of 17 beats at tick 222,255 (2.06 h), 141 dockings, prestige at
215,871 (2.0 h), 8 of 8 game-master change types `checked`; Run B 17 of 17 at tick
238,441 (2.21 h), 144 dockings, prestige at 232,057 (2.15 h). One tender stall per run,
no script faults. Full table in `content/07-pacing.md`. The table below is the
integration pass, before the fixes.

New saves with seed 1, `galaxy sim run --bot journey`, release build:

| | Run A: core + game-master packs | Run B: core only |
|---|---|---|
| Beats | 17 of 17, `ok` | 17 of 17, `ok` |
| Final tick (game time) | 857,483 (7.9 h) | 699,400 (6.5 h) |
| Player dockings | 1,296 | 1,191 |
| Act 1 done (b05) | tick 5,287 (3 min) | tick 5,287 (3 min) |
| Act 3 opens (b11) | tick 97,672 (54 min) | tick 92,309 (51 min) |
| First tender (b13) | tick 688,053 (6.4 h) | tick 154,744 (1.4 h) |
| Prestige (b16) | tick 851,099 (7.9 h) | tick 693,016 (6.4 h) |
| Game-master change types | 8 of 8 `checked` | all `not_installed` |
| Game-master facts / causes | 11 / 401 | 0 / 0 |
| Script aborts or quarantines | 0 | 0 |
| Rejected batches | 33 (23 market pulses losing a compare-and-set, 6 ship, 4 verb) | 58 (48 / 6 / 4) |
| Tender `loop_stalled` facts | 119 | 168 |
| Wall-clock run time | 9 min 12 s | 2 min 49 s |

The numbers above come from the merged branch before this PR. Run A again after this PR's
fixes: 17 of 17 beats, 8 of 8 change types `checked`, tick 799,961 (7.4 h), 1,136
dockings, 31 rejected batches, 78 tender stalls, no script faults. Hesper's border line
now first comes at tick 33,170 (b09), and Odo's tuning at tick 58,689 (after b11 starts).
The default test suite covers both journeys on a fast data profile.

## What works

By system:

- **Sim core, saves, history:** a fixed-step, deterministic, headless sim with one write
  path (cause-tagged command batches), SQLite saves, an append-only history with queries,
  activity counters, rollback by cause, a determinism hash and golden replay.
- **Flight, controls and orders:** WASD and Space, then click to fly, mine and dock, map
  travel, queues and saved loops. The player and every craft use one verb set.
- **Mining and rocks:** three rock types, visible shrinking, depletion that persists,
  regrowth from activity.
- **Docking, market and map:** ring docking, six wares, per-unit prices that move with
  every unit, saturation, split sales, and a map of prices dated by your dockings.
- **Progress:** 17 beats in the core pack, standing and regard with three factions, pilot
  traits, hull ranks, the Tandem and Heron prestige, and hullwright upgrades that say
  what they do.
- **People:** about ten named NPCs. Dock talk cites real facts, and replies are buttons.
  Four named traders fly real routes and are chosen on docking, never on a timer.
- **The tender:** Tamsin's demonstration, then your own tender running your saved loop,
  with a patient sell that waits for a price floor.
- **Scripts:** a sandboxed Luau `--!strict` host with budgets, hot reload, strict
  typecheck, `pack check`, `pack test`, and quarantine.
- **Game master:** eight example packs. The live socket answers `state get`, `history`,
  `watch`, `gm digest`, `shot --live`, and `gm propose` and `gm rollback` with a sandbox
  run. Checked live in this pass: `gm propose packs/gm-examples/kettle_growth --live
  --wait` passed every step and applied at once, because the player was docked (a calm
  moment).
- **Fast-model layer:** Jev picks among authored lines and trader plans through
  OpenRouter, with a seeded fake in tests. Without a key, the authored default is used.
- **Presentation:** a cel-shaded 3D-ready renderer with ink outlines, panels that scale
  from 960×540 to 2560×1080, the F3 overlay, five soft sound cues, `galaxy shot`, and
  Linux and Windows launch scripts.

### The ten questions the slice must prove

Each pass signal is something a player feels while playing, so these notes say what the
build makes possible, not whether it passes.

| # | Question | Honest note |
|---|---|---|
| 1 | Does a hands-on first ship stay satisfying while it works toward clear goals? | Every beat has a one-line goal and Act 1 runs in minutes. The first playtest found the Space confirm to undock, a low start speed and a mining lens with no perceptible effect. All three are fixed: flying off undocks, the Wren is 20% faster, and the lens says what it does and shows its rate while you mine. |
| 2 | Do control unlocks feel like progression, from WASD and Space to queues and loops? | Every control tier unlocks in order through beats (nav chip b06, map travel b07, order slate and loop log b11), and the bot uses each. The flight model is on hold pending a separate design exploration. |
| 3 | Does first automation feel like a new game opening? | The tender runs your own loop with the same verbs and does a patient sell no other craft does. It stalled often on authored data (119 to 168 stalled laps per run) while the glassite belt was dead; after the regrowth fix the bot's tender stalls once per run. The player still can't tell it apart in flight because it has no label. |
| 4 | Do scouted living prices with saturation create real choices? | The mechanics are there: dated snapshots, "?" for unvisited stations, per-unit price drops, flood facts and split sales. Whether they create choices is untested with a human. |
| 5 | Does the Hearthbound choice read as a calling, not paying to lose work? | The card lists what stays and what resets (it was unreadable until this PR), and both hulls are reachable. The 8,000 cr fee now comes at about two hours of the bot's game time. |
| 6 | Can the game master change systems, places and people live? | Yes: eight change types ship as scripts, including a sixth station built at runtime, a taught verb, moved borders and retuned shortages. All eight were `checked` in Run A, and a live propose applied. Two packs spoke too early in Act 1; fixed here. |
| 7 | Where is the line between the base engine and the live scripting layer? | Every game-master change ships as a script. What needed a base change is listed below. |
| 8 | Do slow pilot growth and factions add depth or clutter? | Traits (such as Rimesight), standing and regard reach the player through people's lines, not a stat screen. Untested with a human. |
| 9 | Does a fast-model layer make the world feel alive while text stays important or absent? | Words are always authored or real fact values, and Jev only picks. A failed or slow call falls back to the authored line. This pass ran without a key, so it tested the fallback path only; the live calls were measured in PR #19. |
| 10 | When does the AI game master enter the project? | The slice includes it: a live socket, a digest, propose with a sandbox, calm-moment staging, rollback and an inbox, with GM-AGENTS.md as the agent guide. |

## What doesn't, and known issues

1. **Pacing is measured on the bot only.** The bot now reaches prestige in 2.0 to 2.15
   hours of game time (it was 6.4 to 7.9: the Glass Drift took about 870 Cinder Drift
   pulses to reform a rock, and the bot shuttled for hours). The Glass Drift regrows 1.5
   a pulse now; a human's two hours are unmeasured. Tuning: `packs/core/data/rocks.toml`
   (`regrow`) and `ship_rules.toml` (`reform_share`).
2. **The PR #11 playtest list:** undocking by flying off, +20% start speed, the nav-chip
   hint and a smaller goal line, names before use (the Wren, Orrin, Kettle Dock, Rook,
   the tender, Rimewater), and upgrades that say what they do are fixed. Rock shading in
   blocks and beam clipping are a separate renderer pass. The flight model, station UI
   redesign, audio, music and the map are on hold for design.
3. **The chronicle lowercases names:** "At Cinder Hook, hesper redrew the border."
4. **No end card at b17.** After coming home the goal line goes blank, and the world
   keeps running.
5. **Layout:** at 1280×720 the map title runs under the goal panel. Trader labels can
   sit under station cards on the map ("Little Pail"). The talk panel can overlap the
   edge of a long dock screen and the bottom hint.
6. **Tender stalls:** "a whole lap moved nothing" fires often on authored prices (see
   the bot table). The player's own tender has no label.
7. **Order contract gaps:** orders target a rock, not a field, and `Order::Buy` has no
   maximum price.
8. **Tooling:** `shot --keys :buy:hull.*` prestiges without the confirm card, and `shot`
   can't show F3 (use `shot --live`).
9. **Platforms:** Linux was checked only in WSL on software rendering; in WSL the GPU
   tests need `VK_ICD_FILENAMES` pinned to lavapipe. macOS is untested.

### Fixed in this pass

- **The prestige card was unreadable.** The dock screen's text drew over the card,
  including its Trade up and Not yet buttons, because every text draws after every panel.
  The card is modal, so the frame now leaves the talk and dock screen out while it's
  open. Test: `nothing_shows_through_the_prestige_card`.
- **Two game-master packs spoke in Act 1.** A new game opened on Hesper's line about a
  contested Cinder Hook border, and Odo announced a relief-ice quota long before the
  shortage. `territory_map` now waits for the Hook dispute (b09) and `shortage_tuning`
  for Act 3 (b11). Test: `game_master_layers_wait_for_their_beats_in_act_one`.

## Needed a base change

The core pack is written through the same hooks a game master uses. These are the things
the hooks couldn't do, which became native Rust instead (PR numbers in brackets).

- **Hook API (`ctx`):** opened `market.levers`, `orders.write`, `beats.write` and
  `mechanics.write`; added the `beats`, `orders`, `mechanics`, `standing`, `traits` and
  `pacing` namespaces, and the host drives beats after handlers [#21].
  `ctx.space` (craft, fields, regrowth) as reversible `Put*` commands,
  `ctx.market:pulses`, `ctx.beats:pay` (rewards were static), loads found in space, and
  `G.near` [#26]. Rumor-following through `G.test` cost a fresh Luau state per tick, so
  Act 2 took 42 s; with `G.near` it takes 2.5 s [#26].
  `ctx.kit` for stations, overlays and buoys, `ctx.canon` with `canon.toml`, and
  manifest `observes` [#31]. The `talk.pick` hook was bridged into the mind [#28].
- **Commands and contract:** dock answers (`TalkLine.choices`, `PlayerInput::replies`,
  `on.reply`) [#26]; `Event::Paused` [#20]; `Lever::SetBase` and `ScaleBase`, because
  levers could only move supply [#26]; `split_sale` and `far_docking` [#25];
  `Order::Transfer` for the Tandem [#25]; the hold invariant reading modifiers [#25];
  prestige refusing an over-full hold [#26].
- **Native by design:** rock lamps, tagging and the six-lamp cap; survey-proof rewards,
  so earned credits survive a rollback [#31]; the talk citation check, which needs
  history that `apply` can't see [#18]; and `galaxy-kit`, which split recipes and art
  validation out of the renderer so scripts don't pull in wgpu [#31].
- **Rules:** game-master packs can't register journey beats, judged by how a pack is
  loaded, not by its ID [#31]. One capability table (`COMMAND_NEEDS`) covers every write
  route [#31].
- **Still open, worked around in content:** `FactDraft.fields`, ledger and situation
  stores, hold origins, a props store and a `lines.toml` matcher [#18, #26].

## How it was built

- **22 pull requests** into `slice/01-lantern-reach`: #12 to #33 except #30 (a flight lab
  on `main`), plus this one. PR #11 proposes the branch for `main` and is never merged by
  an agent.
- **Process:** one branch per topic, each reviewed by an agent that didn't build it:
  Codex (GPT-6.1 Sol) on every PR. That was a different model family for everything
  except #28, #29, #31 and #32, which Codex (GPT-6 Astra) built. Most PRs needed one to
  three re-checks; #31 needed four and #19 six. For #18 (the sandbox), Claude Opus 5.5
  did the re-checks, because Codex's safety filter refused sandbox reviews. The orchestrator merged after a local
  `cargo xtask check`. There was no CI. Questions went to PR #11 and were answered there.
- **Builders:** Claude Code built most PRs, Grok Build the art pass (#23), and Codex
  (GPT-6 Astra) dock-talk picks, NPC traders, the game-master examples and the harness
  (#28, #29, #31, #32). This integration pass was Claude Opus 5.5.
- **In the game:** TypeSafe Jev (`typesafe/jev-1.14`) picks, and `openai/gpt-oss-20b`
  phrases lines when opted in, both through OpenRouter. Free phrasing is off by default.
