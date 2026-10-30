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
//! **Adding a command**: give it a row here. `every_command_has_a_row` fails until it
//! has one, so a new command can't reach the world from a pack unchecked.

use crate::command::Command;
use crate::events::Event;
use crate::ids::PackId;
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
         `beats.write` (a beat's `rewards.flags`); any other flag needs `beats.write`.",
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
        "By event: `deliver`, `beat_started`, `quest_step`, `beat_completed`, \
         `upgrade_fitted` and `cap_unlocked` need `beats.write`; `talk_requested` needs \
         `talk.pick`; `standing_changed` and `regard_changed` need `standing.write` or \
         `factions.write`. Packs never emit any other event.",
    ),
    row(
        "record_fact",
        Needs::Payload,
        "Kinds with the pack's prefix need `history.write` (`<prefix>.canon_opened` \
         also `canon.write`, `<prefix>.mechanic_taught` also `mechanics.write`); \
         `market_base_changed` needs `market.levers`. Packs never record any other kind.",
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
    // --- Progress ---
    row(
        "put_beat",
        Needs::Uses(&[&["beats.write"]]),
        "Beat transitions (`G.beat`).",
    ),
    row(
        "set_fitting",
        Needs::Uses(&[&["beats.write"]]),
        "A beat's `rewards.fit`.",
    ),
    row("expect_fitting", Needs::Nothing, "A check."),
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

/// What `cmd`, caused by `pack`, needs: `Ok` with the clauses (each satisfied by any
/// one of its capabilities; empty for nothing), or `Err` saying why a pack may never
/// cause it.
pub fn needs(cmd: &Command, pack: &PackId) -> Result<Vec<&'static [&'static str]>, String> {
    let op = cmd.op();
    let Some(r) = row_for(op) else {
        return Err(format!("`{op}` has no row in the capability table"));
    };
    match r.needs {
        Needs::Nothing => Ok(Vec::new()),
        Needs::Uses(clauses) => Ok(clauses.to_vec()),
        Needs::Never => Err(format!(
            "a pack never causes `{op}` ({})",
            r.note.trim_end_matches('.')
        )),
        Needs::Own => match cmd {
            Command::SetScriptValue { pack: p, .. } | Command::SetTriggerFired { pack: p, .. }
                if p == pack =>
            {
                Ok(Vec::new())
            }
            _ => Err(format!("`{op}` writes another pack's state")),
        },
        Needs::Payload => payload(cmd, pack),
    }
}

fn payload(cmd: &Command, pack: &PackId) -> Result<Vec<&'static [&'static str]>, String> {
    const KIT: &[&str] = &["kit.write"];
    const LINES: &[&str] = &["market.lines"];
    const BEATS: &[&str] = &["beats.write"];
    const STANDING: &[&str] = &["standing.write", "factions.write"];
    let op = cmd.op();
    Ok(match cmd {
        Command::SetFlag { flag, .. } if flag.as_str().starts_with("mech.") => {
            vec![&["mechanics.write", "beats.write"]]
        }
        Command::SetFlag { .. } => vec![BEATS],
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
            Event::Deliver { .. }
            | Event::BeatStarted { .. }
            | Event::QuestStep { .. }
            | Event::BeatCompleted { .. }
            | Event::UpgradeFitted { .. }
            | Event::CapUnlocked { .. } => vec![BEATS],
            Event::TalkRequested { .. } => vec![&["talk.pick"]],
            Event::StandingChanged { .. } | Event::RegardChanged { .. } => vec![STANDING],
            other => {
                return Err(format!(
                    "a pack never emits `{}` events",
                    other.kind().as_str()
                ));
            }
        },
        Command::RecordFact { fact } => {
            let kind = fact.kind.as_str();
            let prefix = pack.as_str().split('.').next().unwrap_or("");
            match kind.strip_prefix(prefix).and_then(|k| k.strip_prefix('.')) {
                Some("canon_opened") => vec![&["history.write", "canon.write"]],
                Some("mechanic_taught") => vec![&["history.write", "mechanics.write"]],
                Some(_) => vec![&["history.write"]],
                None if kind == "market_base_changed" => vec![&["market.levers"]],
                None => {
                    return Err(format!(
                        "a pack records only kinds with its prefix (`{prefix}.`), not `{kind}`"
                    ));
                }
            }
        }
        Command::Say { line } => match line.channel {
            Channel::DockTalk => vec![&["talk.write"]],
            Channel::MapNote => vec![&["map.write"]],
            Channel::Chronicle => vec![&["chronicle.write"]],
        },
        _ => return Err(format!("`{op}` has no payload rule")),
    })
}

/// The first command in `commands` that `pack` may not cause with `uses`, as a
/// diagnostic naming the command (by index and `op`) and what it lacks.
pub fn check(commands: &[Command], pack: &PackId, uses: &[String]) -> Result<(), String> {
    for (i, c) in commands.iter().enumerate() {
        let op = c.op();
        let clauses = needs(c, pack).map_err(|why| format!("command {i} (`{op}`): {why}"))?;
        for clause in clauses {
            if !clause.iter().any(|cap| uses.iter().any(|u| u == cap)) {
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
            let Needs::Uses(clauses) = r.needs else {
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
