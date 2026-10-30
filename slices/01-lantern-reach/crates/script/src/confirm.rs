//! No silent changes to the player's stuff: a pack changes the player's hold, credits
//! or fitted parts only in a transaction the player confirmed.
//!
//! The rule the host enforces on every batch a pack causes (a handler, a trigger, a
//! beat's transition with its rewards and callback): a batch that adds or takes the
//! player's cargo (their own or a consignment), pays or charges credits, or fits a part
//! must cite, in its cause, the player's own answer at the dock (a `reply` fact by a
//! player ship). The one exception is something found while mining: cargo that only
//! goes *into* the hold may cite the player's own `mine` or `rock_depleted` instead,
//! the deliberate act that found it. A batch that breaks the rule is a script fault
//! (`unconfirmed_change`), so none of it applies.
//!
//! So a beat can't pay a reward or fit a gift on its own: the hand-in that earns it
//! happens in a reply handler (Deliver, Accept), which pays in the same batch and says
//! what was given and what came back. Trades, services and survey-buoy claims are the
//! player's own verbs and native systems, not packs, and never pass through here.

use galaxy_contract::prelude::*;
use galaxy_core::History;

/// The player's flagship or one of their own craft.
fn players(world: &WorldState, ship: &ShipId) -> bool {
    world.player.ship.as_ref() == Some(ship)
        || world
            .ships
            .get(ship)
            .is_some_and(|s| s.owner == Owner::Player)
}

/// What a command changes of the player's (and whether it only adds cargo), or `None`.
fn touches(world: &WorldState, c: &Command) -> Option<(&'static str, bool)> {
    match c {
        Command::AdjustCargo { ship, delta, .. } if players(world, ship) => Some(("hold", *delta > 0)),
        Command::SetConsignment { ship, .. } if players(world, ship) => Some(("hold", false)),
        Command::AdjustCredits { .. } => Some(("credits", false)),
        Command::SetFitting { .. } => Some(("fitted parts", false)),
        _ => None,
    }
}

/// How a cited fact confirms: `Some(true)` for anything (the player's reply),
/// `Some(false)` only for finds (the player's own mining), `None` not at all.
fn confirms(world: &WorldState, history: &History, fact: FactId) -> Option<bool> {
    let f = history.fact(fact)?;
    let FactPayload::Event(event) = &f.payload else {
        return None;
    };
    match event {
        Event::Reply { ship, .. } if players(world, ship) => Some(true),
        Event::Mine { ship, .. } if world.player.ship.as_ref() == Some(ship) => Some(false),
        Event::RockDepleted {
            ship: Some(ship), ..
        } if world.player.ship.as_ref() == Some(ship) => Some(false),
        _ => None,
    }
}

/// Whether `batch` keeps the rule. `Err` names the first change the player didn't
/// confirm.
pub fn confirmed(world: &WorldState, history: &History, batch: &CommandBatch) -> Result<(), String> {
    let mut changes = batch
        .commands
        .iter()
        .enumerate()
        .filter_map(|(i, c)| touches(world, c).map(|t| (i, c, t)))
        .peekable();
    if changes.peek().is_none() {
        return Ok(());
    }
    let mut reply = false;
    let mut find = false;
    for fact in &batch.cause.cites {
        match confirms(world, history, *fact) {
            Some(true) => reply = true,
            Some(false) => find = true,
            None => {}
        }
    }
    for (i, c, (what, gain)) in changes {
        if reply || (find && gain) {
            continue;
        }
        return Err(format!(
            "command {i} (`{}`) changes the player's {what}, but the batch doesn't cite \
             the player's `reply` agreeing to it{}: offer a choice (Deliver, Accept) and \
             make the change in the reply handler, saying what was given and what came back",
            c.op(),
            if gain {
                " (or, for a find, their own `mine` or `rock_depleted`)"
            } else {
                ""
            }
        ));
    }
    Ok(())
}
