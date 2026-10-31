//! Station services: what hullwrights and dockmasters fit and build, and the
//! hullwright's prestige. Ship parts, the locker, Make and Friends go to
//! [`crate::hullwright`]. Each purchase is one atomic batch: if any part of it can't
//! apply (not enough credits, undocked earlier in the tick, already bought by an
//! earlier request in the same tick), none of it does and nothing is charged.

use std::collections::BTreeSet;

use galaxy_contract::prelude::*;
use galaxy_contract::progress::{CraftDef, UpgradeDef};
use galaxy_core::History;

use crate::eval::{Eval, player_ship};

/// The system name purchases are caused by.
pub const SERVICES: &str = "progress.services";

/// Why a purchase can't happen. Every reason is calm: nothing is lost.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum Refusal {
    #[error("no such offer `{0}`")]
    Unknown(String),
    #[error("the player has no ship")]
    NoShip,
    #[error("dock at {0} for that")]
    NotDocked(StationId),
    #[error("that isn't on offer yet")]
    Locked,
    #[error("already fitted")]
    AlreadyFitted,
    #[error("already built")]
    AlreadyOwned,
    #[error("it costs {need}; you have {have}")]
    Credits { need: String, have: String },
    #[error("only a beat can give that")]
    NotSold,
    #[error("the hullwright has no hull for that here")]
    NoBranch,
    #[error("the new hull holds {room}; sell or deliver down from {holds} first")]
    HoldTooFull { holds: u64, room: u32 },
    /// The hullwright can't fit, take off, make or order that part.
    #[error("{0}")]
    Fit(crate::hullwright::FitRefusal),
}

/// One line on a seller's list.
#[derive(Clone, Debug, PartialEq)]
pub struct Offer {
    pub request: ServiceRequest,
    pub name: String,
    /// One line: what it does.
    pub effect: String,
    /// Centicredits (0: a gift).
    pub cost: i64,
    pub seller: Option<NpcId>,
    /// Whether the player can pay for it now.
    pub affordable: bool,
}

/// Everything on offer for credits at `station` right now: unlocked, not yet owned.
/// Parts made or ordered from a friend, and the hull's locker, are in the wharf's
/// room ([`crate::hullwright::wharf`]).
pub fn offers(world: &WorldState, history: &History, station: &StationId) -> Vec<Offer> {
    let eval = Eval::new(world, history);
    let rules = &world.progress.rules;
    let mut out = Vec::new();
    for u in rules.upgrades.values() {
        if u.station.as_ref() == Some(station)
            && u.source.is_ready()
            && !galaxy_contract::fit::owned(world, &u.id)
            && eval.holds_data(&u.unlock)
        {
            out.push(Offer {
                request: ServiceRequest::Fit {
                    upgrade: u.id.clone(),
                },
                name: u.name.clone(),
                effect: u.effect.clone(),
                cost: u.cost,
                seller: u.seller.clone(),
                affordable: world.player.credits >= u.cost,
            });
        }
    }
    for c in rules.crafts.values() {
        if &c.station == station && !world.ships.contains(&c.id) && eval.holds_data(&c.unlock) {
            out.push(Offer {
                request: ServiceRequest::Build {
                    craft: c.id.clone(),
                },
                name: c.name.clone(),
                effect: c.effect.clone(),
                cost: c.cost,
                seller: c.seller.clone(),
                affordable: world.player.credits >= c.cost,
            });
        }
    }
    if let Some(p) = crate::prestige::offer(world, history)
        && &p.station == station
    {
        for to in p.to {
            let name = world
                .hulls
                .get(&to)
                .map_or_else(|| to.to_string(), |h| h.name.clone());
            out.push(Offer {
                request: ServiceRequest::Prestige { to },
                effect: format!("Trade the {} up into a {name}", p.from_name),
                name,
                cost: p.fee,
                seller: p.seller.clone(),
                affordable: world.player.credits >= p.fee,
            });
        }
    }
    out
}

fn docked_at<'w>(world: &'w WorldState, station: &StationId) -> Result<&'w Ship, Refusal> {
    let ship = player_ship(world).ok_or(Refusal::NoShip)?;
    if ship.docked_at.as_ref() != Some(station) {
        return Err(Refusal::NotDocked(station.clone()));
    }
    Ok(ship)
}

fn afford(world: &WorldState, cost: i64) -> Result<(), Refusal> {
    if world.player.credits < cost {
        return Err(Refusal::Credits {
            need: show_credits(cost),
            have: show_credits(world.player.credits),
        });
    }
    Ok(())
}

/// Whether the player could buy this now.
pub fn check(world: &WorldState, history: &History, req: &ServiceRequest) -> Result<(), Refusal> {
    if crate::hullwright::handles(world, req) {
        return crate::hullwright::check(world, history, req);
    }
    let eval = Eval::new(world, history);
    let rules = &world.progress.rules;
    match req {
        ServiceRequest::Fit { upgrade } => {
            let u = rules
                .upgrades
                .get(upgrade)
                .ok_or_else(|| Refusal::Unknown(upgrade.to_string()))?;
            let station = u.station.as_ref().ok_or(Refusal::NotSold)?;
            docked_at(world, station)?;
            if world.player.fittings.contains(upgrade) {
                return Err(Refusal::AlreadyFitted);
            }
            if !eval.holds_data(&u.unlock) {
                return Err(Refusal::Locked);
            }
            afford(world, u.cost)
        }
        ServiceRequest::Build { craft } => {
            let c = rules
                .crafts
                .get(craft)
                .ok_or_else(|| Refusal::Unknown(craft.to_string()))?;
            docked_at(world, &c.station)?;
            if world.ships.contains(craft) {
                return Err(Refusal::AlreadyOwned);
            }
            if !eval.holds_data(&c.unlock) {
                return Err(Refusal::Locked);
            }
            afford(world, c.cost)
        }
        ServiceRequest::Prestige { to } => crate::prestige::check(world, history, to),
        ServiceRequest::Remove { .. }
        | ServiceRequest::Refit { .. }
        | ServiceRequest::Make { .. }
        | ServiceRequest::Order { .. } => crate::hullwright::check(world, history, req),
    }
}

/// The `reply` event for an answer the player gives at the dock: only when the
/// player's ship is docked where that person is. `None` otherwise (dropped).
pub fn reply(world: &WorldState, reply: &Reply) -> Option<CommandBatch> {
    let ship = player_ship(world)?;
    let station = ship.docked_at.clone()?;
    let npc = world.npcs.get(&reply.npc)?;
    if npc.location.as_ref() != Some(&station) {
        return None;
    }
    let mut b = CommandBatch::new(Cause::system(
        SERVICES,
        format!("the player answered {}: {}", npc.name, reply.choice),
    ));
    // Checked when it applies: an undock earlier in the tick drops the answer.
    b.push(Command::ExpectDocked {
        ship: ship.id.clone(),
        station: station.clone(),
    });
    b.push(Command::Emit {
        event: Event::Reply {
            ship: ship.id.clone(),
            npc: npc.id.clone(),
            station,
            choice: reply.choice.clone(),
        },
    });
    Some(b)
}

/// The batch that carries out a purchase, after [`check`].
pub fn batch(
    world: &WorldState,
    history: &History,
    req: &ServiceRequest,
) -> Result<CommandBatch, Refusal> {
    if crate::hullwright::handles(world, req) {
        return crate::hullwright::batch(world, history, req);
    }
    check(world, history, req)?;
    let rules = &world.progress.rules;
    match req {
        ServiceRequest::Fit { upgrade } => {
            let u = &rules.upgrades[upgrade];
            let mut b = CommandBatch::new(Cause::system(
                SERVICES,
                format!("fitted the {} for {} cr", u.name, show_credits(u.cost)),
            ));
            let ship = player_ship(world).ok_or(Refusal::NoShip)?;
            if let Some(station) = &u.station {
                b.push(Command::ExpectDocked {
                    ship: ship.id.clone(),
                    station: station.clone(),
                });
            }
            // Checked when it applies, so a second request for it this tick fails
            // whole instead of charging again.
            b.push(Command::ExpectFitting {
                upgrade: upgrade.clone(),
                fitted: false,
            });
            if u.cost > 0 {
                b.push(Command::AdjustCredits { delta: -u.cost });
            }
            b.commands
                .extend(fit_commands(world, u, &mut BTreeSet::new()));
            Ok(b)
        }
        ServiceRequest::Build { craft } => {
            let c = &rules.crafts[craft];
            let mut b = CommandBatch::new(Cause::system(
                SERVICES,
                format!("had {} built for {} cr", c.name, show_credits(c.cost)),
            ));
            let ship = player_ship(world).ok_or(Refusal::NoShip)?;
            b.push(Command::ExpectDocked {
                ship: ship.id.clone(),
                station: c.station.clone(),
            });
            b.push(Command::ExpectShip {
                ship: craft.clone(),
                hull: None,
            });
            if c.cost > 0 {
                b.push(Command::AdjustCredits { delta: -c.cost });
            }
            b.commands.extend(build_commands(world, c, ship)?);
            Ok(b)
        }
        ServiceRequest::Prestige { to } => crate::prestige::batch(world, history, to),
        ServiceRequest::Remove { .. }
        | ServiceRequest::Refit { .. }
        | ServiceRequest::Make { .. }
        | ServiceRequest::Order { .. } => crate::hullwright::batch(world, history, req),
    }
}

/// Commands that fit an upgrade (no payment): the fitting, `upgrade_fitted`, and each
/// flag it grants not set yet (`cap_unlocked` for capabilities). `seen` holds flags
/// already granted earlier in the same batch.
pub fn fit_commands(
    world: &WorldState,
    u: &UpgradeDef,
    seen: &mut BTreeSet<FlagId>,
) -> Vec<Command> {
    let hull = player_ship(world)
        .map(|s| s.hull.clone())
        .unwrap_or_else(|| HullId::from_static("hull.none"));
    let mut out = vec![
        Command::SetFitting {
            upgrade: u.id.clone(),
            fitted: true,
        },
        Command::Emit {
            event: Event::UpgradeFitted {
                upgrade: u.id.clone(),
                hull,
            },
        },
    ];
    out.extend(grant_commands(world, &u.grants, seen));
    out
}

/// Sets each flag not set yet; capabilities also write `cap_unlocked`.
pub fn grant_commands(
    world: &WorldState,
    flags: &[FlagId],
    seen: &mut BTreeSet<FlagId>,
) -> Vec<Command> {
    let mut out = Vec::new();
    for f in flags {
        if world.player.flags.contains(f) || !seen.insert(f.clone()) {
            continue;
        }
        out.push(Command::SetFlag {
            flag: f.clone(),
            on: true,
        });
        if world.progress.rules.capabilities.contains_key(f) {
            out.push(Command::Emit {
                event: Event::CapUnlocked { cap: f.clone() },
            });
        }
    }
    out
}

/// Commands that build a craft at its station (no payment).
fn build_commands(world: &WorldState, c: &CraftDef, like: &Ship) -> Result<Vec<Command>, Refusal> {
    let station = world
        .stations
        .get(&c.station)
        .ok_or_else(|| Refusal::Unknown(c.station.to_string()))?;
    // Built from a copy of the player's (docked) ship so every runtime field the ship
    // crate keeps starts from a docked, idle state.
    let mut ship = like.clone();
    ship.id = c.id.clone();
    ship.name = c.name.clone();
    ship.hull = c.hull.clone();
    ship.owner = Owner::Player;
    ship.region = station.region.clone();
    ship.position = station.position;
    ship.velocity = Vec2::ZERO;
    ship.heading = 0.0;
    ship.docked_at = Some(station.id.clone());
    ship.hold = Hold::default();
    ship.orders = OrderQueue::default();
    let mut out = vec![
        Command::PutShip {
            id: c.id.clone(),
            value: Some(Box::new(ship)),
        },
        Command::Emit {
            event: Event::TenderAcquired {
                craft: c.id.clone(),
                hull: c.hull.clone(),
                station: c.station.clone(),
            },
        },
    ];
    out.extend(grant_commands(world, &c.grants, &mut BTreeSet::new()));
    Ok(out)
}
