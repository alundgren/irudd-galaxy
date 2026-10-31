//! What a pack must declare in `uses` to cause each [`Command`].
//!
//! The `ctx` hooks are gated one by one (an undeclared function is absent), but a
//! pack also writes through routes that aren't a single hook: a beat's transition and
//! rewards, a trigger's bookkeeping, its data files, a buoy's reward the pilot claims
//! later. [`COMMAND_NEEDS`] is the one table every one of those routes answers to: the
//! script host checks each batch a pack causes against it before submitting it, and
//! `galaxy pack check` checks what a pack's data and beat rewards would write. A command
//! whose capability the manifest doesn't list rejects the whole batch as a fault.
//!
//! Rules hold whichever [`Route`] a batch takes and whichever [`PackLayer`] its pack
//! runs as. **Provenance**: the facts and events the journey reads can't be forged.
//!
//! - A pack records facts only in its own namespace, `<pack-id>.*` (the whole ID, so
//!   `core.probe` can't write `core.*` and `gm.a` can't write `gm.b.*`).
//! - Engine and journey events (docking, trades, mining, `beat_started`, `quest_step`,
//!   `beat_completed` ...) never come from a pack's hooks. The beat runtime writes the
//!   beat events ([`Route::Beat`], for a core-layer pack only); the few events a hook
//!   may emit must directly follow the change they report (`deliver` its cargo,
//!   `standing_changed` its standing).
//! - A fact that reports a checked change follows that change in the same batch:
//!   `<pack-id>.canon_opened` its `set_canon_status` (`ctx.canon:open`, gated by the
//!   manifest's `opens`), `<pack-id>.mechanic_taught` its `mech.*` flag
//!   (`ctx.mechanics:teach`), `market_base_changed` its `set_market_base`. So
//!   `history.write` never stands in for opening canon or teaching.
//!
//! **Adding a command**: give it a row here. `every_command_has_a_row` fails until it
//! has one, so a new command can't reach the world from a pack unchecked.

use crate::command::Command;
use crate::events::Event;
use crate::ids::PackId;
use crate::pack::PackLayer;
use crate::talk::Channel;

/// What causing a command takes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Needs {
    /// Changes nothing (an `Expect*` check). Any pack may carry one.
    Nothing,
    /// The host's bookkeeping of the calling pack's own state (its stores, its
    /// triggers). Allowed for that pack only.
    Own,
    /// Every clause, each satisfied by any one of its capabilities.
    Uses(&'static [&'static [&'static str]]),
    /// Only the beat runtime's transition ([`Route::Beat`]) of a core-layer pack,
    /// with every clause, as [`Needs::Uses`].
    Beat(&'static [&'static [&'static str]]),
    /// Depends on what the command carries; the row's note says how ([`needs`]).
    Payload,
    /// A pack may never cause it: only the engine, native systems, the player's verbs,
    /// the host itself and rollbacks write it.
    Never,
}

/// One row of [`COMMAND_NEEDS`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CommandNeeds {
    /// The command's `op` tag ([`Command::op`]).
    pub op: &'static str,
    pub needs: Needs,
    /// Where a pack writes it from, and the payload rule for [`Needs::Payload`].
    pub note: &'static str,
}

const fn row(op: &'static str, needs: Needs, note: &'static str) -> CommandNeeds {
    CommandNeeds { op, needs, note }
}

/// Where a batch a pack causes comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Route {
    /// The `ctx` hooks: a handler, an `after.` trigger, or a beat's callback.
    Hooks,
    /// The beat runtime's own transition for a beat the pack registers (`G.beat`): the
    /// beat's state, its `beat_started` / `quest_step` / `beat_completed` and its
    /// rewards. Built by the host, never by a script; a game-master layer has none.
    Beat,
    /// The pack's data files, installed when it loads or is applied.
    Data,
}

/// Who causes a batch: the pack, what its manifest declares, and the layer it runs as
/// (`PackState::layer` as recorded in the world, or as the host loaded it).
#[derive(Clone, Copy, Debug)]
pub struct Caller<'a> {
    pub pack: &'a PackId,
    pub uses: &'a [String],
    pub layer: PackLayer,
}

/// Every command a pack might put in a batch, and what it takes. Keyed by `op`.
pub const COMMAND_NEEDS: &[CommandNeeds] = &[
    // --- Ships ---
    row("set_ship_motion", Needs::Never, "Flight."),
    row("set_ship_docked", Needs::Never, "Docking."),
    row("expect_docked", Needs::Nothing, "A check."),
    row("expect_ship", Needs::Nothing, "A check."),
    row("set_ship_hull", Needs::Never, "Prestige."),
    row("set_ship_region", Needs::Never, "Lane jumps."),
    row(
        "adjust_cargo",
        Needs::Uses(&[&["beats.write"]]),
        "`ctx.beats:deliver` and `ctx.beats:load`.",
    ),
    row(
        "set_consignment",
        Needs::Uses(&[&["beats.write"]]),
        "`ctx.beats:consign` and `ctx.beats:deliver` of a consignment.",
    ),
    row(
        "set_orders",
        Needs::Uses(&[&["orders.write"]]),
        "`ctx.orders:give`, `append`, `clear`.",
    ),
    row("update_orders", Needs::Never, "The ship system."),
    row("set_ship_activity", Needs::Never, "The ship system."),
    // --- Player ---
    row(
        "adjust_credits",
        Needs::Uses(&[&["beats.write"]]),
        "`ctx.beats:pay` and a beat's `rewards.credits`.",
    ),
    row(
        "set_flag",
        Needs::Payload,
        "A `mech.*` flag needs `mechanics.write` (`ctx.mechanics:set`, `teach`) or \
         `beats.write` (a beat's `rewards.flags`); any other flag comes only from a \
         beat's rewards (the beat runtime, `beats.write`).",
    ),
    row("set_trait", Needs::Never, "Traits are earned natively."),
    row("set_price_snapshot", Needs::Never, "Scouting."),
    row("set_loop", Needs::Never, "The player's loops."),
    // --- Rocks ---
    row("adjust_rock", Needs::Never, "Mining."),
    row("set_rock_growth", Needs::Never, "Regrowth."),
    // --- Stations and factions ---
    row(
        "set_station_stage",
        Needs::Uses(&[&["stations.write"]]),
        "`ctx.stations:set_stage`.",
    ),
    row(
        "set_station_part",
        Needs::Uses(&[&["stations.write"]]),
        "`ctx.stations:attach`, `detach`.",
    ),
    row(
        "set_station_owner",
        Needs::Uses(&[&["stations.write", "territory.write"]]),
        "`ctx.stations:set_owner`, `ctx.territory:set_station_owner`.",
    ),
    row(
        "adjust_standing",
        Needs::Uses(&[&["standing.write", "factions.write"]]),
        "`ctx.standing:deed`, `ctx.factions:adjust_standing`, a beat's `rewards.standing`.",
    ),
    row("expect_standing", Needs::Nothing, "A check."),
    // --- Markets ---
    row(
        "set_market_line",
        Needs::Uses(&[&["market.lines"]]),
        "Adds or removes a line: `ctx.market:add_line`, `remove_line`, and the \
         `add_line` / `remove_line` levers.",
    ),
    row("update_market_line", Needs::Never, "Trades and pulses."),
    row(
        "set_market_rates",
        Needs::Uses(&[&["market.levers"]]),
        "Producer and consumer levers.",
    ),
    row(
        "set_market_base",
        Needs::Uses(&[&["market.levers"]]),
        "`set_base` / `scale_base` levers.",
    ),
    row(
        "set_market_stock",
        Needs::Uses(&[&["market.levers"]]),
        "`ctx.market:set_stock` and the `set_stock` lever.",
    ),
    row("adjust_activity", Needs::Never, "The economy's tallies."),
    row("set_dock_track", Needs::Never, "Counted dockings."),
    row("set_split_stack", Needs::Never, "Split sales."),
    // --- Whole records ---
    row(
        "found_station",
        Needs::Payload,
        "`ctx.kit:station`: `kit.write`, plus `market.lines` if it carries a market.",
    ),
    row(
        "close_founded_station",
        Needs::Never,
        "A founding's inverse: rollbacks only.",
    ),
    row(
        "set_founded_station",
        Needs::Payload,
        "`ctx.kit:station` reopening its pack's scaffold: `kit.write`, plus \
         `market.lines` if it opens a market.",
    ),
    row(
        "update_survey_buoy",
        Needs::Never,
        "The pilot reading a buoy or claiming its reward.",
    ),
    row("expect_flag", Needs::Nothing, "A check."),
    row("set_rock_tag", Needs::Never, "The player's rock lamps."),
    row(
        "put_survey_buoy",
        Needs::Payload,
        "`ctx.kit:buoy`: `kit.write`; a reward paying credits also needs \
         `beats.write`, one moving standing `standing.write`. Never placed read or \
         claimed.",
    ),
    row(
        "set_canon_status",
        Needs::Uses(&[&["canon.write"]]),
        "`ctx.canon:open`.",
    ),
    row(
        "put_station_recipe",
        Needs::Never,
        "Recipes come with `found_station`.",
    ),
    row(
        "put_station_part",
        Needs::Uses(&[&["stations.write"]]),
        "`ctx.parts:put`, `ctx.parts:remove` (only while the player is docked).",
    ),
    row(
        "put_happening",
        Needs::Uses(&[&["happenings.write"]]),
        "`ctx.happenings:put`, `ctx.happenings:remove` (upcoming happenings only).",
    ),
    row(
        "put_map_overlay",
        Needs::Uses(&[&["kit.write"]]),
        "`ctx.kit:overlay`.",
    ),
    row(
        "put_region",
        Needs::Uses(&[&["territory.write"]]),
        "`ctx.territory:set_owner`.",
    ),
    row("put_lane", Needs::Never, "Authored data."),
    row(
        "put_station",
        Needs::Never,
        "Authored data; packs found stations with `found_station`.",
    ),
    row(
        "put_market",
        Needs::Uses(&[&["market.lines"]]),
        "A first line at a station without a market (`add_line` lever, `ctx.kit:station`).",
    ),
    row("put_ware", Needs::Never, "Authored data."),
    row("put_hull", Needs::Never, "Authored data."),
    row("put_rock_type", Needs::Never, "Authored data."),
    row(
        "put_field",
        Needs::Uses(&[&["space.write"]]),
        "`ctx.space:put_field`, `set_regrow`.",
    ),
    row(
        "put_rock",
        Needs::Uses(&[&["space.write"]]),
        "`ctx.space:put_field`.",
    ),
    row(
        "put_ship",
        Needs::Uses(&[&["space.write"]]),
        "`ctx.space:put_craft`.",
    ),
    row("put_faction", Needs::Never, "Authored data."),
    row(
        "put_npc",
        Needs::Uses(&[&["npcs.write"]]),
        "`ctx.npcs:put`, `move`, `ctx.kit:station`'s dockmaster, and `npcs.toml` in the \
         pack's `data`.",
    ),
    row(
        "put_rumor",
        Needs::Uses(&[&["rumors.write"]]),
        "`ctx.rumors:put`, `set_truth`, `spread`.",
    ),
    row("set_rumor_heard", Needs::Never, "Craft carrying rumors."),
    row(
        "put_trader",
        Needs::Never,
        "Authored trader routes (`craft.toml`).",
    ),
    // --- History and events ---
    row(
        "emit",
        Needs::Payload,
        "By event. `beat_started`, `quest_step`, `beat_completed`, `upgrade_fitted` \
         and `cap_unlocked` come only from the beat runtime (`beats.write`, core layer). \
         From the hooks: `deliver` (`beats.write`) right after the `adjust_cargo` that \
         hands its cargo over (or the `set_consignment` that hands over its ledger's \
         consignment); `standing_changed` (`standing.write` or \
         `factions.write`) right after the `adjust_standing` (or, for no change, the \
         `expect_standing`) of its faction, and `regard_changed` right after that \
         `standing_changed`; `talk_requested` (`talk.pick`). Packs never emit any other \
         event: docking, trades, mining and the rest come from the engine.",
    ),
    row(
        "record_fact",
        Needs::Payload,
        "Only in the pack's own namespace, `<pack-id>.*` (`history.write`), except \
         `<pack-id>.canon_opened` (`canon.write`, right after the `set_canon_status` \
         that opens the node it names: `ctx.canon:open`) and \
         `<pack-id>.mechanic_taught` (`mechanics.write`, right after its `mech.*` flag \
         and slots: `ctx.mechanics:teach`); and `market_base_changed` \
         (`market.levers`, right after the `set_market_base` it reports). Packs never \
         record any other kind.",
    ),
    // --- Scripts ---
    row(
        "set_script_value",
        Needs::Own,
        "`ctx.store:set` on the script's declared stores.",
    ),
    row(
        "set_trigger_fired",
        Needs::Own,
        "The host marking a `G.after` trigger fired.",
    ),
    row("set_script_health", Needs::Never, "The host's quarantine."),
    row("set_revision_retired", Needs::Never, "Rollbacks."),
    row(
        "set_pack_off",
        Needs::Never,
        "Rollbacks and the game master applying a revision.",
    ),
    row(
        "set_pack_layer",
        Needs::Never,
        "The host and the bridge, when a pack loads or is applied.",
    ),
    // --- Progress ---
    row(
        "put_beat",
        Needs::Beat(&[&["beats.write"]]),
        "Beat transitions (`G.beat`), core layer only.",
    ),
    row(
        "set_fitting",
        Needs::Beat(&[&["beats.write"]]),
        "A beat's `rewards.fit`, core layer only.",
    ),
    row("expect_fitting", Needs::Nothing, "A check."),
    row("set_locker", Needs::Never, "Hullwright services."),
    row("set_part_order", Needs::Never, "Hullwright services."),
    row("put_upgrade", Needs::Never, "Authored data."),
    row("set_fit_rules", Needs::Never, "Authored data."),
    row("set_hull_record", Needs::Never, "Hull ranks and prestige."),
    row("expect_hull_record", Needs::Nothing, "A check."),
    row(
        "set_tunable",
        Needs::Uses(&[&["pacing.write"]]),
        "`ctx.pacing:set`.",
    ),
    row(
        "bind_slot",
        Needs::Uses(&[&["mechanics.write"]]),
        "`ctx.mechanics:bind`, `unbind`, `teach`.",
    ),
    // --- Legibility ---
    row(
        "say",
        Needs::Payload,
        "By channel: dock talk needs `talk.write`, a map note `map.write`, the \
         chronicle `chronicle.write`.",
    ),
    row("put_talk", Needs::Never, "`say`'s inverse: rollbacks only."),
];

/// The row for `op`.
pub fn row_for(op: &str) -> Option<&'static CommandNeeds> {
    COMMAND_NEEDS.iter().find(|r| r.op == op)
}

/// What `cmd`, caused by `caller` along `route`, needs: `Ok` with the clauses (each
/// satisfied by any one of its capabilities; empty for nothing), or `Err` saying why
/// the pack may never cause it there. `before` is what the batch carries ahead of it
/// on the same route (the provenance rules read the commands just before).
pub fn needs(
    cmd: &Command,
    before: &[Command],
    caller: &Caller<'_>,
    route: Route,
) -> Result<Vec<&'static [&'static str]>, String> {
    let op = cmd.op();
    let Some(r) = row_for(op) else {
        return Err(format!("`{op}` has no row in the capability table"));
    };
    if route == Route::Beat && caller.layer == PackLayer::GameMaster {
        return Err(
            "a game-master layer never drives journey beats or pays their rewards".to_owned(),
        );
    }
    match r.needs {
        Needs::Nothing => Ok(Vec::new()),
        Needs::Uses(clauses) => Ok(clauses.to_vec()),
        Needs::Beat(clauses) => {
            beat_only(route, op)?;
            Ok(clauses.to_vec())
        }
        Needs::Never => Err(format!(
            "a pack never causes `{op}` ({})",
            r.note.trim_end_matches('.')
        )),
        Needs::Own => match cmd {
            Command::SetScriptValue { pack: p, .. } | Command::SetTriggerFired { pack: p, .. }
                if p == caller.pack =>
            {
                Ok(Vec::new())
            }
            _ => Err(format!("`{op}` writes another pack's state")),
        },
        Needs::Payload => payload(cmd, before, caller, route),
    }
}

/// Only the beat runtime writes `what`.
fn beat_only(route: Route, what: &str) -> Result<(), String> {
    if route == Route::Beat {
        Ok(())
    } else {
        Err(format!(
            "only the beat runtime writes `{what}` (a core-layer pack's `G.beat`), never a \
             hook or data"
        ))
    }
}

/// Names in a pack's own namespace that report a checked change: only the hook that
/// makes the change records them (`ctx.canon:open`, `ctx.mechanics:teach`), never
/// `ctx.history:record`. The happening runtime's `happening_started` and
/// `happening_ended` are the engine's alone, so no pack forges one under its prefix.
pub const PROTECTED_FACTS: [&str; 4] = [
    "canon_opened",
    "mechanic_taught",
    "happening_started",
    "happening_ended",
];

fn payload(
    cmd: &Command,
    before: &[Command],
    caller: &Caller<'_>,
    route: Route,
) -> Result<Vec<&'static [&'static str]>, String> {
    const KIT: &[&str] = &["kit.write"];
    const LINES: &[&str] = &["market.lines"];
    const BEATS: &[&str] = &["beats.write"];
    const STANDING: &[&str] = &["standing.write", "factions.write"];
    let op = cmd.op();
    let last = before.last();
    Ok(match cmd {
        Command::SetFlag { flag, .. } if flag.as_str().starts_with("mech.") => {
            vec![&["mechanics.write", "beats.write"]]
        }
        Command::SetFlag { flag, .. } => {
            beat_only(route, &format!("set_flag {flag}"))?;
            vec![BEATS]
        }
        Command::FoundStation { market, .. } | Command::SetFoundedStation { market, .. } => {
            if market.is_some() {
                vec![KIT, LINES]
            } else {
                vec![KIT]
            }
        }
        Command::PutSurveyBuoy { value, .. } => {
            let mut out = vec![KIT];
            if let Some(buoy) = value {
                if buoy.read {
                    return Err("a pack never places a buoy already read".to_owned());
                }
                if let Some(reward) = &buoy.reward {
                    if reward.claimed {
                        return Err("a pack never places a buoy reward already claimed".to_owned());
                    }
                    if reward.credits > 0 {
                        out.push(BEATS);
                    }
                    if !reward.standing.is_empty() {
                        out.push(&["standing.write"]);
                    }
                }
            }
            out
        }
        Command::Emit { event } => match event {
            Event::BeatStarted { .. }
            | Event::QuestStep { .. }
            | Event::BeatCompleted { .. }
            | Event::UpgradeFitted { .. }
            | Event::CapUnlocked { .. } => {
                beat_only(route, event.kind().as_str())?;
                vec![BEATS]
            }
            Event::Deliver {
                ship,
                ware,
                quantity,
                ledger,
                ..
            } => {
                let handed = match last {
                    Some(Command::AdjustCargo {
                        ship: s,
                        ware: w,
                        delta,
                    }) => s == ship && w == ware && *delta == -i64::from(*quantity),
                    Some(Command::SetConsignment {
                        ship: s,
                        ledger: l,
                        consignment: None,
                    }) => s == ship && ledger.as_ref() == Some(l),
                    _ => false,
                };
                if !handed {
                    return Err(
                        "a `deliver` event follows the `adjust_cargo` that hands its \
                                cargo over, or the `set_consignment` that hands over its \
                                ledger's consignment (`ctx.beats:deliver_consignment`)"
                            .to_owned(),
                    );
                }
                vec![BEATS]
            }
            Event::TalkRequested { .. } => vec![&["talk.pick"]],
            Event::StandingChanged { faction, delta, .. } => {
                let moved = match last {
                    Some(Command::AdjustStanding {
                        faction: f,
                        delta: d,
                    }) => f == faction && d == delta,
                    Some(Command::ExpectStanding { faction: f, .. }) => f == faction && *delta == 0,
                    _ => false,
                };
                if !moved {
                    return Err(format!(
                        "a `standing_changed` event follows the change to {faction}'s standing \
                         it reports (a deed)"
                    ));
                }
                vec![STANDING]
            }
            Event::RegardChanged { faction, .. } => {
                let after = matches!(last, Some(Command::Emit { event: Event::StandingChanged { faction: f, .. } })
                    if f == faction);
                if !after {
                    return Err(format!(
                        "a `regard_changed` event follows the `standing_changed` of {faction}"
                    ));
                }
                vec![STANDING]
            }
            other => {
                return Err(format!(
                    "a pack never emits `{}` events (only the engine does)",
                    other.kind().as_str()
                ));
            }
        },
        Command::RecordFact { fact } => record(fact, before, caller)?,
        Command::Say { line } => match line.channel {
            Channel::DockTalk => vec![&["talk.write"]],
            Channel::MapNote => vec![&["map.write"]],
            Channel::Chronicle => vec![&["chronicle.write"]],
        },
        _ => return Err(format!("`{op}` has no payload rule")),
    })
}

/// The rule for `record_fact`: the pack's own namespace, and the protected names only
/// right after the change they report.
fn record(
    fact: &crate::history::FactDraft,
    before: &[Command],
    caller: &Caller<'_>,
) -> Result<Vec<&'static [&'static str]>, String> {
    let kind = fact.kind.as_str();
    let other = fact.subjects.other.as_ref().map(|o| o.as_str());
    let own = kind
        .strip_prefix(caller.pack.as_str())
        .and_then(|k| k.strip_prefix('.'));
    Ok(match own {
        Some("canon_opened") => {
            let opened = matches!(before.last(), Some(Command::SetCanonStatus { id, status })
                if Some(id.as_str()) == other && *status == crate::canon::CanonStatus::Open);
            if !opened {
                return Err(format!(
                    "`{kind}` is recorded only by `ctx.canon:open`, right after the \
                     `set_canon_status` that opens the node it names"
                ));
            }
            vec![&["canon.write"]]
        }
        Some("mechanic_taught") => {
            let flag = before
                .iter()
                .rev()
                .find(|c| !matches!(c, Command::BindSlot { .. }));
            let taught = matches!(flag, Some(Command::SetFlag { flag, on: true })
                if Some(flag.as_str()) == other && flag.as_str().starts_with("mech."));
            if !taught {
                return Err(format!(
                    "`{kind}` is recorded only by `ctx.mechanics:teach`, right after the \
                     `mech.*` flag it teaches"
                ));
            }
            vec![&["mechanics.write"]]
        }
        // Kinds match by their last segment when lines cite them, so a protected name
        // can't hide deeper in the namespace either (`gm.a.b.canon_opened`).
        Some(name)
            if name
                .rsplit('.')
                .next()
                .is_some_and(|last| PROTECTED_FACTS.contains(&last)) =>
        {
            return Err(format!(
                "`{kind}`: a protected name is recorded only as `{}.<name>`, by its hook",
                caller.pack
            ));
        }
        Some(_) => vec![&["history.write"]],
        None if kind == "market_base_changed" => {
            let moved = matches!(before.last(), Some(Command::SetMarketBase { station, ware, base })
                if fact.subjects.station.as_ref() == Some(station)
                    && fact.subjects.ware.as_ref() == Some(ware)
                    && fact.amount == Some(i64::from(base.base_price)));
            if !moved {
                return Err(
                    "`market_base_changed` is recorded only right after the `set_market_base` \
                     it reports"
                        .to_owned(),
                );
            }
            vec![&["market.levers"]]
        }
        None => {
            return Err(format!(
                "a pack records facts only in its own namespace (`{}.`), not `{kind}`",
                caller.pack
            ));
        }
    })
}

/// The first command in `commands` (one route's share of a batch) that `caller` may
/// not cause along `route`, as a diagnostic naming the command (by index and `op`)
/// and what it lacks.
pub fn check(commands: &[Command], caller: &Caller<'_>, route: Route) -> Result<(), String> {
    for (i, c) in commands.iter().enumerate() {
        let op = c.op();
        let clauses = needs(c, &commands[..i], caller, route)
            .map_err(|why| format!("command {i} (`{op}`): {why}"))?;
        for clause in clauses {
            if !clause
                .iter()
                .any(|cap| caller.uses.iter().any(|u| u == cap))
            {
                let want = clause
                    .iter()
                    .map(|c| format!("`{c}`"))
                    .collect::<Vec<_>>()
                    .join(" or ");
                return Err(format!(
                    "command {i} (`{op}`) needs {want} in the manifest's `uses`"
                ));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use schemars::schema_for;

    use super::*;

    /// Every `op` the `Command` schema has (one per variant).
    fn schema_ops() -> BTreeSet<String> {
        let schema = serde_json::to_value(schema_for!(Command)).unwrap();
        let variants = schema["oneOf"].as_array().expect("Command is a oneOf");
        variants
            .iter()
            .map(|v| {
                v["properties"]["op"]["enum"][0]
                    .as_str()
                    .or_else(|| v["properties"]["op"]["const"].as_str())
                    .unwrap_or_else(|| panic!("a variant without an op: {v}"))
                    .to_owned()
            })
            .collect()
    }

    #[test]
    fn every_command_has_a_row() {
        let ops = schema_ops();
        let rows: BTreeSet<String> = COMMAND_NEEDS.iter().map(|r| r.op.to_owned()).collect();
        assert_eq!(rows.len(), COMMAND_NEEDS.len(), "an op has two rows");
        let missing: Vec<_> = ops.difference(&rows).collect();
        assert!(
            missing.is_empty(),
            "commands without a row in COMMAND_NEEDS: {missing:?}"
        );
        let stale: Vec<_> = rows.difference(&ops).collect();
        assert!(stale.is_empty(), "rows for no command: {stale:?}");
    }

    #[test]
    fn every_capability_named_is_available() {
        let all = crate::api::uses();
        for r in COMMAND_NEEDS {
            let (Needs::Uses(clauses) | Needs::Beat(clauses)) = r.needs else {
                continue;
            };
            for cap in clauses.iter().flat_map(|c| c.iter()) {
                let u = all
                    .iter()
                    .find(|u| u.id == *cap)
                    .unwrap_or_else(|| panic!("{}: unknown capability {cap}", r.op));
                assert!(u.available, "{}: {cap} is reserved", r.op);
            }
        }
    }

    #[test]
    fn expectations_need_nothing() {
        for r in COMMAND_NEEDS.iter().filter(|r| r.op.starts_with("expect_")) {
            assert_eq!(r.needs, Needs::Nothing, "{}", r.op);
        }
    }
}
