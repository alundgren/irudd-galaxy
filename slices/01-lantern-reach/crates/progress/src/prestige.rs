//! Hullwright prestige (Hearthbound): trade the hull up for a fee.
//!
//! Only hull ranks reset: the new hull starts at rank 0 with its own rank window.
//! The world, credits minus the fee, standing, history, depletion, pilot traits,
//! tenders, capabilities, every fitting and the hull's locker stay: the flagship keeps
//! its ship ID, so its parts and locker move with the hull's lineage. The old hull is
//! berthed at the hullwright's station as its own ship and can't be flown again.

use galaxy_contract::prelude::*;
use galaxy_core::History;

use crate::eval::{Eval, player_ship};
use crate::services::{Refusal, SERVICES};

/// The prestige offer as the hullwright makes it.
#[derive(Clone, Debug, PartialEq)]
pub struct PrestigeOffer {
    pub station: StationId,
    pub seller: Option<NpcId>,
    /// Centicredits.
    pub fee: i64,
    pub from: HullId,
    pub from_name: String,
    pub to: Vec<HullId>,
    pub berth_as: ShipId,
}

/// Whether a ship is a hull the player left at prestige.
pub fn is_berthed(world: &WorldState, ship: &ShipId) -> bool {
    world.player.is_berthed(ship)
}

/// The offer, if its requirement holds for the hull the player flies (whether or not
/// they're docked there or can pay).
pub fn offer(world: &WorldState, history: &History) -> Option<PrestigeOffer> {
    let rules = world.progress.rules.prestige.as_ref()?;
    let ship = player_ship(world)?;
    let branch = rules.branches.iter().find(|b| b.from == ship.hull)?;
    if !Eval::new(world, history).holds_data(&rules.requires) {
        return None;
    }
    Some(PrestigeOffer {
        station: rules.station.clone(),
        seller: rules.seller.clone(),
        fee: rules.fee,
        from: ship.hull.clone(),
        from_name: world
            .hulls
            .get(&ship.hull)
            .map_or_else(|| ship.hull.to_string(), |h| h.name.clone()),
        to: branch.to.clone(),
        berth_as: branch.berth_as.clone(),
    })
}

/// What stays and what resets, one line each, for the confirm screen.
pub fn summary(world: &WorldState, offer: &PrestigeOffer) -> (Vec<String>, Vec<String>) {
    let stays = vec![
        "The world: stations, territory, depletion, rumors and who remembers you".into(),
        format!("Credits, minus the {} cr fee", show_credits(offer.fee)),
        "Standing with every faction".into(),
        "Every pilot trait".into(),
        "Tenders and their loops, every capability, every fitted module".into(),
        format!("The {} stays berthed here", offer.from_name),
    ];
    let resets = vec![format!(
        "Hull ranks: the new hull starts unstriped ({} keeps {})",
        offer.from_name,
        crate::growth::stripes(world, &offer.from).len()
    )];
    (stays, resets)
}

pub fn check(world: &WorldState, history: &History, to: &HullId) -> Result<(), Refusal> {
    let o = offer(world, history).ok_or(Refusal::Locked)?;
    if !o.to.contains(to) {
        return Err(Refusal::NoBranch);
    }
    let ship = player_ship(world).ok_or(Refusal::NoShip)?;
    if ship.docked_at.as_ref() != Some(&o.station) {
        return Err(Refusal::NotDocked(o.station));
    }
    if world.ships.contains(&o.berth_as) {
        return Err(Refusal::AlreadyOwned);
    }
    // The cargo moves across with the pilot: the new hull must hold it, with the
    // fittings and traits that carry across (its own ranks start over).
    let room = galaxy_contract::modifiers::hold_capacity_after_prestige(world, to);
    if world.hulls.contains(to) && ship.hold.total() > room {
        return Err(Refusal::HoldTooFull {
            holds: ship.hold.total(),
            room: u32::try_from(room).unwrap_or(u32::MAX),
        });
    }
    if world.player.credits < o.fee {
        return Err(Refusal::Credits {
            need: show_credits(o.fee),
            have: show_credits(world.player.credits),
        });
    }
    Ok(())
}

/// The prestige batch: the fee, the old hull berthed as its own ship, the player's
/// ship on the new hull, the frozen and fresh rank records, and a `prestige` fact.
///
/// Worked out from the tick's starting world, so everything it relies on is checked
/// against the committed state when it applies: still docked, still flying the old
/// hull, no berth yet, both rank records unchanged. A second request in the same tick,
/// or a rank earned first, makes it fail whole (nothing charged). It never writes the
/// flown ship whole: only its hull and name change, so cargo, credits and activity
/// committed earlier in the tick stay.
pub fn batch(world: &WorldState, history: &History, to: &HullId) -> Result<CommandBatch, Refusal> {
    check(world, history, to)?;
    let o = offer(world, history).ok_or(Refusal::Locked)?;
    let ship = player_ship(world).ok_or(Refusal::NoShip)?;
    let head = history.last_fact_id();
    let to_name = world
        .hulls
        .get(to)
        .map_or_else(|| to.to_string(), |h| h.name.clone());
    let mut b = CommandBatch::new(Cause::system(
        SERVICES,
        format!(
            "the {} became a {to_name} for {} cr",
            o.from_name,
            show_credits(o.fee)
        ),
    ));
    b.push(Command::ExpectDocked {
        ship: ship.id.clone(),
        station: o.station.clone(),
    });
    b.push(Command::ExpectShip {
        ship: ship.id.clone(),
        hull: Some(o.from.clone()),
    });
    b.push(Command::ExpectShip {
        ship: o.berth_as.clone(),
        hull: None,
    });
    if o.fee > 0 {
        b.push(Command::AdjustCredits { delta: -o.fee });
    }
    // The berth is a new ship: empty, idle and docked where the trade happens.
    let mut berth = ship.clone();
    berth.id = o.berth_as.clone();
    berth.velocity = Vec2::ZERO;
    berth.hold = Hold::default();
    berth.orders = OrderQueue::default();
    berth.activity = ShipActivity::default();
    b.push(Command::PutShip {
        id: o.berth_as.clone(),
        value: Some(Box::new(berth)),
    });
    b.push(Command::SetShipHull {
        ship: ship.id.clone(),
        hull: to.clone(),
        name: to_name,
    });
    // Fitted parts come across; one the new hull can't take goes to its locker.
    b.commands
        .extend(crate::hullwright::prestige_commands(world, ship, to));
    if !ship.orders.orders.is_empty() {
        b.push(Command::UpdateOrders {
            ship: ship.id.clone(),
            from: Box::new(ship.orders.clone()),
            to: Box::default(),
        });
    }
    // A stripe that comes due this tick is painted before the hull is frozen (the
    // rank system's own batch for it then fails on the changed record).
    let old = world.player.hull_records.get(&o.from).cloned();
    let due = crate::growth::due_rank(world, history);
    let mut frozen = old.clone().unwrap_or_default();
    if let Some(rank) = due {
        frozen.rank = rank;
    }
    b.push(Command::ExpectHullRecord {
        hull: o.from.clone(),
        record: old,
    });
    b.push(Command::SetHullRecord {
        hull: o.from.clone(),
        record: Some(HullRecord {
            until: head,
            berthed_as: Some(o.berth_as.clone()),
            ..frozen
        }),
    });
    if let Some(rank) = due {
        b.push(Command::Emit {
            event: Event::HullRank {
                hull: o.from.clone(),
                rank,
            },
        });
    }
    b.push(Command::ExpectHullRecord {
        hull: to.clone(),
        record: world.player.hull_records.get(to).cloned(),
    });
    b.push(Command::SetHullRecord {
        hull: to.clone(),
        record: Some(HullRecord {
            rank: 0,
            since: head,
            until: None,
            berthed_as: None,
        }),
    });
    b.push(Command::Emit {
        event: Event::Prestige {
            from_hull: o.from.clone(),
            to_hull: to.clone(),
            fee: o.fee,
        },
    });
    Ok(b)
}
