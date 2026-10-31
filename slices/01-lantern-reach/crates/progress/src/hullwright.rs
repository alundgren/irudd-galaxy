//! The hullwright (#47 Phase 3, the sim side of "The fit model"): fitting parts to the
//! flagship's sockets, the hull's parts locker, the three sources (Ready, Make,
//! Friends), blueprints found in the world, and the read API the hullwright room
//! builds on. The data shapes and pure reads are in [`galaxy_contract::fit`].
//!
//! # Rules
//!
//! - A **part** sits in a socket that holds one; a **control module** sits in the
//!   control rack, uses no budget and is never taken off. Fitting a part into a socket
//!   that holds one already puts the old part in the hull's locker, in the same batch.
//! - **Power**: a fit that would draw more than the hull's power cap is refused
//!   ([`FitRefusal::OverPower`]), so everything fitted always runs.
//! - **Mass** past the hull's free allowance slows top speed and turn rate, through
//!   [`galaxy_contract::modifiers::for_ship`].
//! - Parts are fitted and taken off only at a **wharf** that works on their socket
//!   (`ship_parts.toml`); control modules are fitted by their seller, as before.
//! - The **locker** belongs to the hull (keyed by ship). Nothing in it is ever lost; it
//!   can be refitted at any wharf that works on the part's socket.
//! - **Make** consumes materials from the flagship's hold and needs the blueprint
//!   (a flag the progress system sets the first time the blueprint's `found` holds).
//! - **Friends**: an order is paid when placed and arrives in the locker after the
//!   flagship's next docking at the friend's station (counted in dockings).
//!
//! Every change is a player service (`PlayerInput::services`, one atomic batch each,
//! [`crate::services::batch`]) except a friend's part arriving and a blueprint being
//! found, which the progress system does as the consequence of what the player did.
//! Each writes a fact whose note is the log line: `part_fitted`, `part_removed`,
//! `part_refitted`, `part_made`, `part_ordered`, `part_arrived`, `blueprint_found`.
//!
//! # Read API for the room
//!
//! Pure functions over the world (and history, for unlocks):
//!
//! - [`wharf`]: what a wharf offers, per tab ([`WharfView`]: `ready`, `make`,
//!   `friends`, each a [`PartOffer`] with its service request and what blocks it), the
//!   hull's locker and the parts in transit.
//! - [`sockets`]: the flagship's sockets, what's in each, and which light up as drop
//!   targets for a part held over the cradle ([`SocketView::takes`]).
//! - [`budget`]: power and mass in use against the hull's budgets.
//! - [`preview`]: the stat preview of fitting a part (top speed, turn, hold, mining
//!   rate and reach, before and after), with the budgets and what it would refuse.
//! - [`locker`] and [`in_transit`]: the hull's locker contents and the Friends orders
//!   on their way.
//! - [`galaxy_contract::fit::looks`] (what the ship wears, for drawing it) and
//!   [`galaxy_contract::fit::seam_readings`] (what a fitted seam lamp shows).

use std::collections::BTreeSet;

use galaxy_contract::fit::{self, Budget, PartOrder, PartSource};
use galaxy_contract::prelude::*;
use galaxy_contract::progress::UpgradeDef;
use galaxy_core::History;

use crate::eval::{Eval, player_ship};
use crate::services::{Refusal, SERVICES, fit_commands};

/// Why the hullwright can't do it. Each is one calm sentence; nothing is lost.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum FitRefusal {
    #[error("The {hull} has no {socket}.")]
    NoSocket { hull: String, socket: String },
    #[error("Nobody here works on the {socket}.")]
    NoWharf { socket: String },
    #[error("That draws {need} power; the {hull} has {cap}.")]
    OverPower { need: u32, cap: u32, hull: String },
    #[error("Without it your hold takes {room}. You carry {holds}: sell or deliver some first.")]
    HoldFull { holds: u64, room: u64 },
    #[error("Dock at a wharf for that.")]
    NotAtWharf,
    #[error("Controls stay fitted.")]
    Module,
    #[error("It isn't fitted.")]
    NotFitted,
    #[error("It's in your locker already.")]
    InLocker,
    #[error("It isn't in your locker.")]
    NotInLocker,
    #[error("It's on its way already.")]
    OnOrder,
    #[error("That comes from the {tab} tab.")]
    OtherTab { tab: &'static str },
    #[error("You haven't found the {0} yet.")]
    NoBlueprint(String),
    #[error("Making it takes {0}.")]
    Materials(String),
}

impl From<FitRefusal> for Refusal {
    fn from(r: FitRefusal) -> Self {
        Refusal::Fit(r)
    }
}

// --- Shared reads ----------------------------------------------------------------------

fn socket_name(world: &WorldState, socket: &SocketId) -> String {
    world
        .progress
        .rules
        .fit
        .sockets
        .get(socket)
        .map_or_else(|| socket.to_string(), |s| s.name.to_lowercase())
}

fn hull_name(world: &WorldState, ship: &Ship) -> String {
    world
        .hulls
        .get(&ship.hull)
        .map_or_else(|| ship.hull.to_string(), |h| h.name.clone())
}

fn ware_name(world: &WorldState, ware: &WareId) -> String {
    world
        .wares
        .get(ware)
        .map_or_else(|| ware.to_string(), |w| w.name.to_lowercase())
}

fn npc_name(world: &WorldState, npc: &NpcId) -> String {
    world
        .npcs
        .get(npc)
        .map_or_else(|| npc.to_string(), |n| n.name.clone())
}

fn first_name(world: &WorldState, npc: &NpcId) -> String {
    let name = npc_name(world, npc);
    name.split_whitespace().next().unwrap_or(&name).to_owned()
}

fn station_name(world: &WorldState, station: &StationId) -> String {
    world
        .stations
        .get(station)
        .map_or_else(|| station.to_string(), |s| s.name.clone())
}

/// "the Wren's locker".
fn locker_name(ship: &Ship) -> String {
    format!("the {}'s locker", ship.name)
}

/// A part (not a control module) by ID.
fn part_def<'w>(world: &'w WorldState, id: &UpgradeId) -> Result<&'w UpgradeDef, Refusal> {
    let u = world
        .progress
        .rules
        .upgrades
        .get(id)
        .ok_or_else(|| Refusal::Unknown(id.to_string()))?;
    if fit::is_module(world, id) {
        return Err(FitRefusal::Module.into());
    }
    Ok(u)
}

fn socket_of(u: &UpgradeDef) -> &SocketId {
    u.socket.as_ref().expect("a part has a socket")
}

/// The flagship docked at `station`.
fn docked_at<'w>(world: &'w WorldState, station: &StationId) -> Result<&'w Ship, Refusal> {
    let ship = player_ship(world).ok_or(Refusal::NoShip)?;
    if ship.docked_at.as_ref() != Some(station) {
        return Err(Refusal::NotDocked(station.clone()));
    }
    Ok(ship)
}

/// The flagship docked anywhere, and where.
fn docked(world: &WorldState) -> Result<(&Ship, &StationId), Refusal> {
    let ship = player_ship(world).ok_or(Refusal::NoShip)?;
    let station = ship.docked_at.as_ref().ok_or(FitRefusal::NotAtWharf)?;
    Ok((ship, station))
}

/// Whether the wharf at `station` works on `socket`.
fn works_on(world: &WorldState, station: &StationId, socket: &SocketId) -> bool {
    world
        .progress
        .rules
        .fit
        .wharves
        .get(station)
        .is_some_and(|w| w.sockets.contains(socket))
}

/// The flagship's hold capacity with `fittings` fitted.
fn hold_with(world: &WorldState, ship: &Ship, fittings: &BTreeSet<UpgradeId>) -> u64 {
    let base = world
        .hulls
        .get(&ship.hull)
        .map_or(0, |h| u64::from(h.hold_capacity));
    base + u64::from(galaxy_contract::modifiers::flagship_with(world, fittings).hold)
}

/// The fittings after `part` goes into its socket, and the part it puts in the
/// locker, if any.
fn with_part(world: &WorldState, part: &UpgradeDef) -> (BTreeSet<UpgradeId>, Option<UpgradeId>) {
    let mut after = world.player.fittings.clone();
    let out = fit::occupant(world, &world.player.fittings, socket_of(part)).cloned();
    if let Some(o) = &out {
        after.remove(o);
    }
    after.insert(part.id.clone());
    (after, out)
}

/// Whether the flagship, docked at `station`, can take `part` into its socket: the
/// hull has the socket, the wharf works on it, the power fits and the hold still holds
/// the cargo. `Ok` with the part it would put in the locker.
fn can_fit(
    world: &WorldState,
    ship: &Ship,
    station: &StationId,
    part: &UpgradeDef,
) -> Result<Option<UpgradeId>, Refusal> {
    let socket = socket_of(part);
    let has = fit::hull_fit(world, &ship.hull).is_some_and(|h| h.sockets.contains(socket));
    if !has {
        return Err(FitRefusal::NoSocket {
            hull: hull_name(world, ship),
            socket: socket_name(world, socket),
        }
        .into());
    }
    if !works_on(world, station, socket) {
        return Err(FitRefusal::NoWharf {
            socket: socket_name(world, socket),
        }
        .into());
    }
    let (after, out) = with_part(world, part);
    let b = fit::budget(world, &ship.hull, &after);
    if b.over_power() {
        return Err(FitRefusal::OverPower {
            need: b.power_used,
            cap: b.power_cap,
            hull: hull_name(world, ship),
        }
        .into());
    }
    let room = hold_with(world, ship, &after);
    if ship.hold.total() > room {
        return Err(FitRefusal::HoldFull {
            holds: ship.hold.total(),
            room,
        }
        .into());
    }
    Ok(out)
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

/// Where the player has the part, if anywhere: fitted, in a locker, or on order.
fn held(world: &WorldState, id: &UpgradeId) -> Result<(), Refusal> {
    if world.player.fittings.contains(id) {
        return Err(Refusal::AlreadyFitted);
    }
    if world.progress.fit.lockers.values().any(|l| l.contains(id)) {
        return Err(FitRefusal::InLocker.into());
    }
    if world.progress.fit.orders.contains_key(id) {
        return Err(FitRefusal::OnOrder.into());
    }
    Ok(())
}

/// What making `part` takes from the hold, and how much of each the flagship has.
pub fn make_bill(world: &WorldState, part: &UpgradeId) -> Vec<MaterialView> {
    let Some(PartSource::Make { materials, .. }) =
        world.progress.rules.upgrades.get(part).map(|u| &u.source)
    else {
        return Vec::new();
    };
    let ship = player_ship(world);
    materials
        .iter()
        .map(|m| MaterialView {
            ware: m.ware.clone(),
            name: ware_name(world, &m.ware),
            need: m.units,
            have: ship.map_or(0, |s| s.hold.cargo.get(&m.ware).copied().unwrap_or(0)),
        })
        .collect()
}

/// "8 iron ore and 4 refined plate".
fn bill_words(bill: &[MaterialView]) -> String {
    let parts: Vec<String> = bill
        .iter()
        .map(|m| format!("{} {}", m.need, m.name))
        .collect();
    match parts.as_slice() {
        [] => String::new(),
        [one] => one.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

// --- Checks ----------------------------------------------------------------------------

/// Whether the player could do this hullwright service now. `Fit` here is the Ready
/// tab for a part; control modules go through [`crate::services::check`] as before.
pub fn check(world: &WorldState, history: &History, req: &ServiceRequest) -> Result<(), Refusal> {
    let eval = Eval::new(world, history);
    match req {
        ServiceRequest::Fit { upgrade } => {
            let u = part_def(world, upgrade)?;
            let station = u.station.as_ref().ok_or(Refusal::NotSold)?;
            let ship = docked_at(world, station)?;
            if !u.source.is_ready() {
                return Err(FitRefusal::OtherTab {
                    tab: u.source.tab(),
                }
                .into());
            }
            held(world, upgrade)?;
            if !eval.holds_data(&u.unlock) {
                return Err(Refusal::Locked);
            }
            can_fit(world, ship, station, u)?;
            afford(world, u.cost)
        }
        ServiceRequest::Remove { upgrade } => {
            let u = part_def(world, upgrade)?;
            let (ship, station) = docked(world)?;
            if !world.player.fittings.contains(upgrade) {
                return Err(FitRefusal::NotFitted.into());
            }
            let socket = socket_of(u);
            if !works_on(world, station, socket) {
                return Err(FitRefusal::NoWharf {
                    socket: socket_name(world, socket),
                }
                .into());
            }
            let mut after = world.player.fittings.clone();
            after.remove(upgrade);
            let room = hold_with(world, ship, &after);
            if ship.hold.total() > room {
                return Err(FitRefusal::HoldFull {
                    holds: ship.hold.total(),
                    room,
                }
                .into());
            }
            Ok(())
        }
        ServiceRequest::Refit { upgrade } => {
            let u = part_def(world, upgrade)?;
            let (ship, station) = docked(world)?;
            if world.player.fittings.contains(upgrade) {
                return Err(Refusal::AlreadyFitted);
            }
            if !fit::locker(world, &ship.id).any(|p| p == upgrade) {
                return Err(FitRefusal::NotInLocker.into());
            }
            can_fit(world, ship, station, u).map(|_| ())
        }
        ServiceRequest::Make { upgrade, fit } => {
            let u = part_def(world, upgrade)?;
            let station = u.station.as_ref().ok_or(Refusal::NotSold)?;
            let ship = docked_at(world, station)?;
            let PartSource::Make { blueprint, .. } = &u.source else {
                return Err(FitRefusal::OtherTab {
                    tab: u.source.tab(),
                }
                .into());
            };
            held(world, upgrade)?;
            if !world.player.flags.contains(blueprint) {
                let name = world
                    .progress
                    .rules
                    .fit
                    .blueprints
                    .get(blueprint)
                    .map_or_else(|| format!("{} blueprint", u.name), |b| b.name.clone());
                return Err(FitRefusal::NoBlueprint(name.to_lowercase()).into());
            }
            if !eval.holds_data(&u.unlock) {
                return Err(Refusal::Locked);
            }
            let bill = make_bill(world, upgrade);
            if bill.iter().any(|m| m.have < m.need) {
                return Err(FitRefusal::Materials(bill_words(&bill)).into());
            }
            if *fit {
                can_fit(world, ship, station, u)?;
            }
            Ok(())
        }
        ServiceRequest::Order { upgrade } => {
            let u = part_def(world, upgrade)?;
            let station = u.station.as_ref().ok_or(Refusal::NotSold)?;
            docked_at(world, station)?;
            if !matches!(u.source, PartSource::Friend { .. }) {
                return Err(FitRefusal::OtherTab {
                    tab: u.source.tab(),
                }
                .into());
            }
            held(world, upgrade)?;
            if !eval.holds_data(&u.unlock) {
                return Err(Refusal::Locked);
            }
            afford(world, u.cost)
        }
        ServiceRequest::Build { .. } | ServiceRequest::Prestige { .. } => {
            Err(Refusal::Unknown("not a hullwright service".into()))
        }
    }
}

// --- Batches ---------------------------------------------------------------------------

fn note(
    kind: &'static str,
    ship: &Ship,
    station: &StationId,
    part: &UpgradeId,
    text: String,
) -> Command {
    Command::RecordFact {
        fact: Box::new(FactDraft {
            kind: FactKind::from_static(kind),
            subjects: Subjects {
                ship: Some(ship.id.clone()),
                station: Some(station.clone()),
                other: AnyId::new(part.as_str()).ok(),
                ..Subjects::default()
            },
            amount: None,
            note: text,
        }),
    }
}

/// The swap: the part in the socket comes off into the hull's locker.
fn store_commands(ship: &Ship, out: &UpgradeId) -> [Command; 3] {
    [
        Command::ExpectFitting {
            upgrade: out.clone(),
            fitted: true,
        },
        Command::SetFitting {
            upgrade: out.clone(),
            fitted: false,
        },
        Command::SetLocker {
            ship: ship.id.clone(),
            part: out.clone(),
            stored: true,
        },
    ]
}

/// "; your hold extender went to the Wren's locker".
fn swap_words(world: &WorldState, ship: &Ship, out: Option<&UpgradeId>) -> String {
    out.and_then(|o| world.progress.rules.upgrades.get(o))
        .map_or_else(String::new, |o| {
            format!(
                "; your {} went to {}",
                o.name.to_lowercase(),
                locker_name(ship)
            )
        })
}

/// The batch for a hullwright service, after [`check`].
pub fn batch(
    world: &WorldState,
    history: &History,
    req: &ServiceRequest,
) -> Result<CommandBatch, Refusal> {
    check(world, history, req)?;
    let ship = player_ship(world).ok_or(Refusal::NoShip)?;
    let station = ship.docked_at.clone().ok_or(Refusal::NoShip)?;
    let docked = Command::ExpectDocked {
        ship: ship.id.clone(),
        station: station.clone(),
    };
    match req {
        ServiceRequest::Fit { upgrade } => {
            let u = part_def(world, upgrade)?;
            let out = can_fit(world, ship, &station, u)?;
            let mut b = CommandBatch::new(Cause::system(
                SERVICES,
                format!("fitted the {} for {} cr", u.name, show_credits(u.cost)),
            ));
            b.push(docked);
            b.push(Command::ExpectFitting {
                upgrade: upgrade.clone(),
                fitted: false,
            });
            if u.cost > 0 {
                b.push(Command::AdjustCredits { delta: -u.cost });
            }
            if let Some(o) = &out {
                b.commands.extend(store_commands(ship, o));
            }
            b.commands
                .extend(fit_commands(world, u, &mut BTreeSet::new()));
            let paid = if u.cost > 0 {
                format!(" · −{} cr", show_credits(u.cost))
            } else {
                String::new()
            };
            b.push(note(
                "part_fitted",
                ship,
                &station,
                upgrade,
                format!(
                    "Fitted {}{paid}{}",
                    u.name.to_lowercase(),
                    swap_words(world, ship, out.as_ref())
                ),
            ));
            Ok(b)
        }
        ServiceRequest::Remove { upgrade } => {
            let u = part_def(world, upgrade)?;
            let mut b = CommandBatch::new(Cause::system(
                SERVICES,
                format!("took the {} off into the locker", u.name),
            ));
            b.push(docked);
            b.commands.extend(store_commands(ship, upgrade));
            b.push(note(
                "part_removed",
                ship,
                &station,
                upgrade,
                format!("Removed {} to {}", u.name.to_lowercase(), locker_name(ship)),
            ));
            Ok(b)
        }
        ServiceRequest::Refit { upgrade } => {
            let u = part_def(world, upgrade)?;
            let out = can_fit(world, ship, &station, u)?;
            let mut b = CommandBatch::new(Cause::system(
                SERVICES,
                format!("refitted the {} from the locker", u.name),
            ));
            b.push(docked);
            b.push(Command::SetLocker {
                ship: ship.id.clone(),
                part: upgrade.clone(),
                stored: false,
            });
            if let Some(o) = &out {
                b.commands.extend(store_commands(ship, o));
            }
            b.push(Command::SetFitting {
                upgrade: upgrade.clone(),
                fitted: true,
            });
            b.push(note(
                "part_refitted",
                ship,
                &station,
                upgrade,
                format!(
                    "Refitted {} from {}{}",
                    u.name.to_lowercase(),
                    locker_name(ship),
                    swap_words(world, ship, out.as_ref())
                ),
            ));
            Ok(b)
        }
        ServiceRequest::Make { upgrade, fit } => {
            let u = part_def(world, upgrade)?;
            let bill = make_bill(world, upgrade);
            let mut b = CommandBatch::new(Cause::system(
                SERVICES,
                format!("made the {} from {}", u.name, bill_words(&bill)),
            ));
            b.push(docked);
            b.push(Command::ExpectFitting {
                upgrade: upgrade.clone(),
                fitted: false,
            });
            for m in &bill {
                b.push(Command::AdjustCargo {
                    ship: ship.id.clone(),
                    ware: m.ware.clone(),
                    delta: -i64::from(m.need),
                });
            }
            let used: Vec<String> = bill
                .iter()
                .map(|m| format!("−{} {}", m.need, m.name))
                .collect();
            let text = if *fit {
                let out = can_fit(world, ship, &station, u)?;
                if let Some(o) = &out {
                    b.commands.extend(store_commands(ship, o));
                }
                b.commands
                    .extend(fit_commands(world, u, &mut BTreeSet::new()));
                format!(
                    "Made and fitted {} · {}{}",
                    u.name.to_lowercase(),
                    used.join(", "),
                    swap_words(world, ship, out.as_ref())
                )
            } else {
                b.push(Command::SetLocker {
                    ship: ship.id.clone(),
                    part: upgrade.clone(),
                    stored: true,
                });
                format!(
                    "Made {} · {}; it's in {}",
                    u.name.to_lowercase(),
                    used.join(", "),
                    locker_name(ship)
                )
            };
            b.push(note("part_made", ship, &station, upgrade, text));
            Ok(b)
        }
        ServiceRequest::Order { upgrade } => {
            let u = part_def(world, upgrade)?;
            let PartSource::Friend { npc, station: from } = &u.source else {
                return Err(Refusal::NotSold);
            };
            let mut b = CommandBatch::new(Cause::system(
                SERVICES,
                format!("ordered the {} for {} cr", u.name, show_credits(u.cost)),
            ));
            b.push(docked);
            if u.cost > 0 {
                b.push(Command::AdjustCredits { delta: -u.cost });
            }
            b.push(Command::SetPartOrder {
                part: upgrade.clone(),
                order: Some(Box::new(PartOrder {
                    part: upgrade.clone(),
                    ship: ship.id.clone(),
                    from: npc.clone(),
                    station: from.clone(),
                    since: history.last_fact_id(),
                    paid: u.cost,
                })),
            });
            b.push(note(
                "part_ordered",
                ship,
                &station,
                upgrade,
                format!(
                    "Ordered {} from {} at {} · −{} cr; it comes after your next docking there",
                    u.name.to_lowercase(),
                    first_name(world, npc),
                    station_name(world, from),
                    show_credits(u.cost)
                ),
            ));
            Ok(b)
        }
        ServiceRequest::Build { .. } | ServiceRequest::Prestige { .. } => {
            Err(Refusal::Unknown("not a hullwright service".into()))
        }
    }
}

/// Whether the request is the hullwright's (a part, or a locker, Make or Friends
/// service) rather than a control module, tender or prestige.
pub fn handles(world: &WorldState, req: &ServiceRequest) -> bool {
    match req {
        ServiceRequest::Fit { upgrade } => !fit::is_module(world, upgrade),
        ServiceRequest::Remove { .. }
        | ServiceRequest::Refit { .. }
        | ServiceRequest::Make { .. }
        | ServiceRequest::Order { .. } => true,
        ServiceRequest::Build { .. } | ServiceRequest::Prestige { .. } => false,
    }
}

// --- What the progress system does on its own ----------------------------------------

/// Parts from friends whose sender's station the flagship has docked at since the
/// order: one batch each, taking the order off and putting the part in its locker.
pub fn arrivals(world: &WorldState, history: &History) -> Vec<CommandBatch> {
    let Some(flagship) = player_ship(world) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for (part, o) in &world.progress.fit.orders {
        let q = HistoryQuery {
            kind: Some(FactKind::from_static("dock")),
            ship: Some(flagship.id.clone()),
            station: Some(o.station.clone()),
            after: o.since,
            ..HistoryQuery::default()
        };
        if history.count(&q) == 0 {
            continue;
        }
        let Some(ship) = world.ships.get(&o.ship) else {
            continue;
        };
        let name = world
            .progress
            .rules
            .upgrades
            .get(part)
            .map_or_else(|| part.to_string(), |u| u.name.to_lowercase());
        let who = first_name(world, &o.from);
        let mut b = CommandBatch::new(Cause::system(SERVICES, format!("{who} sent the {name}")));
        b.push(Command::SetPartOrder {
            part: part.clone(),
            order: None,
        });
        b.push(Command::SetLocker {
            ship: ship.id.clone(),
            part: part.clone(),
            stored: true,
        });
        b.push(note(
            "part_arrived",
            ship,
            &o.station,
            part,
            format!("{who} sent the {name}; it's in {}", locker_name(ship)),
        ));
        out.push(b);
    }
    out
}

/// Blueprints whose `found` now holds and that the player hasn't found yet: their
/// flags, each with a `blueprint_found` fact naming where and who can make it.
pub fn found_blueprints(world: &WorldState, history: &History) -> Vec<Command> {
    let eval = Eval::new(world, history);
    let mut out = Vec::new();
    for b in world.progress.rules.fit.blueprints.values() {
        if world.player.flags.contains(&b.id) || !eval.holds_data(&b.found) {
            continue;
        }
        let maker = world.progress.rules.upgrades.get(&b.part).and_then(|u| {
            Some(format!(
                " {} can make it at {}.",
                first_name(world, u.seller.as_ref()?),
                station_name(world, u.station.as_ref()?)
            ))
        });
        out.push(Command::SetFlag {
            flag: b.id.clone(),
            on: true,
        });
        out.push(Command::RecordFact {
            fact: Box::new(FactDraft {
                kind: FactKind::from_static("blueprint_found"),
                subjects: Subjects {
                    ship: world.player.ship.clone(),
                    other: AnyId::new(b.part.as_str()).ok(),
                    ..Subjects::default()
                },
                amount: None,
                note: format!(
                    "Found the {} in {}.{}",
                    b.name.to_lowercase(),
                    b.found_in,
                    maker.unwrap_or_default()
                ),
            }),
        });
    }
    out
}

/// The parts the flagship's new hull can't take at prestige go to its locker: the
/// commands for the prestige batch (none with the authored hulls, which all take
/// what the Wren can).
pub fn prestige_commands(world: &WorldState, ship: &Ship, to: &HullId) -> Vec<Command> {
    fit::left_at_prestige(world, to)
        .iter()
        .flat_map(|u| store_commands(ship, u))
        .collect()
}

// --- Read API --------------------------------------------------------------------------

/// The flagship's budgets as fitted now.
pub fn budget(world: &WorldState) -> Option<Budget> {
    fit::flagship_budget(world)
}

/// What the flagship does, in the numbers the preview compares. Speeds and reaches in
/// world units (a second), turn in degrees a second, mining in units a second at
/// hardness 1.
#[derive(Clone, Debug, PartialEq)]
pub struct Stats {
    pub top_speed: f32,
    pub turn_rate: f32,
    pub hold: u64,
    pub mining_rate: f32,
    pub mining_reach: f32,
}

/// The flagship's stats with `fittings` fitted.
pub fn stats_with(world: &WorldState, fittings: &BTreeSet<UpgradeId>) -> Option<Stats> {
    let ship = player_ship(world)?;
    let hull = world.hulls.get(&ship.hull)?;
    let m = galaxy_contract::modifiers::flagship_with(world, fittings);
    let rules = &world.ship_rules;
    Some(Stats {
        top_speed: hull.max_speed * m.speed as f32,
        turn_rate: hull.turn_rate.unwrap_or(rules.flight.turn_rate) * m.turn_rate as f32,
        hold: u64::from(hull.hold_capacity) + u64::from(m.hold),
        mining_rate: hull.mining_rate * m.mining_rate as f32,
        mining_reach: rules.mining.reach + m.mining_reach,
    })
}

/// The flagship's stats as fitted now.
pub fn stats(world: &WorldState) -> Option<Stats> {
    stats_with(world, &world.player.fittings)
}

/// The stat preview for fitting a part: before and after, with the budgets, the part
/// it would put in the locker, and why it would be refused where the flagship is now
/// (`None`: it would go through).
#[derive(Clone, Debug, PartialEq)]
pub struct Preview {
    pub part: UpgradeId,
    pub socket: SocketId,
    pub before: Stats,
    pub after: Stats,
    pub budget_before: Budget,
    pub budget_after: Budget,
    /// The part already in that socket, which would go to the locker.
    pub swaps_out: Option<UpgradeId>,
    /// The request a drop would send: Ready, Make (and fit), or Refit from the locker.
    pub request: ServiceRequest,
    pub refusal: Option<Refusal>,
}

/// The preview of fitting `part` (a part, not a control module) to the flagship.
pub fn preview(world: &WorldState, history: &History, part: &UpgradeId) -> Option<Preview> {
    let u = part_def(world, part).ok()?;
    let ship = player_ship(world)?;
    let (after, out) = with_part(world, u);
    let request = fit_request(world, u);
    Some(Preview {
        part: part.clone(),
        socket: socket_of(u).clone(),
        before: stats(world)?,
        after: stats_with(world, &after)?,
        budget_before: fit::budget(world, &ship.hull, &world.player.fittings),
        budget_after: fit::budget(world, &ship.hull, &after),
        swaps_out: out,
        refusal: crate::services::check(world, history, &request).err(),
        request,
    })
}

/// The request that fits `u`: from the locker if it's there, made (and fitted) if
/// it's a Make part, else bought from the Ready tab.
fn fit_request(world: &WorldState, u: &UpgradeDef) -> ServiceRequest {
    let in_locker =
        player_ship(world).is_some_and(|s| fit::locker(world, &s.id).any(|p| p == &u.id));
    if in_locker {
        ServiceRequest::Refit {
            upgrade: u.id.clone(),
        }
    } else if matches!(u.source, PartSource::Make { .. }) {
        ServiceRequest::Make {
            upgrade: u.id.clone(),
            fit: true,
        }
    } else {
        ServiceRequest::Fit {
            upgrade: u.id.clone(),
        }
    }
}

/// One of the flagship's sockets in the cradle.
#[derive(Clone, Debug, PartialEq)]
pub struct SocketView {
    pub socket: SocketId,
    pub name: String,
    /// `nose`, `utility`, `belly`, `tail` or `bay`.
    pub place: String,
    /// The control rack: holds every control module.
    pub rack: bool,
    /// What's in it (at most one part; the rack: its modules).
    pub fitted: Vec<UpgradeId>,
    /// The wharf here works on it (the rack: always, its modules come from sellers).
    pub works_here: bool,
    /// The held part fits here now: a drop target (dashed sky-blue ring).
    pub takes: bool,
}

/// The flagship's sockets, in the hull's order, as seen docked at `station`, with
/// `holding` (a part dragged over the cradle) lighting the sockets that would take it
/// there now.
pub fn sockets(
    world: &WorldState,
    history: &History,
    station: &StationId,
    holding: Option<&UpgradeId>,
) -> Vec<SocketView> {
    let Some(ship) = player_ship(world) else {
        return Vec::new();
    };
    let Some(hf) = fit::hull_fit(world, &ship.hull) else {
        return Vec::new();
    };
    let held = holding.and_then(|h| world.progress.rules.upgrades.get(h));
    let takes_held = held.is_some_and(|u| {
        let req = if fit::is_module(world, &u.id) {
            ServiceRequest::Fit {
                upgrade: u.id.clone(),
            }
        } else {
            fit_request(world, u)
        };
        crate::services::check(world, history, &req).is_ok()
    });
    hf.sockets
        .iter()
        .filter_map(|s| {
            let def = world.progress.rules.fit.sockets.get(s)?;
            let fitted = world
                .player
                .fittings
                .iter()
                .filter(
                    |u| match fit::part(world, u).and_then(|d| d.socket.as_ref()) {
                        Some(x) => x == s,
                        None => def.rack,
                    },
                )
                .cloned()
                .collect();
            Some(SocketView {
                socket: s.clone(),
                name: def.name.clone(),
                place: def.place.clone(),
                rack: def.rack,
                fitted,
                works_here: def.rack || works_on(world, station, s),
                takes: takes_held && held.and_then(|u| u.socket.as_ref()) == Some(s),
            })
        })
        .collect()
}

/// Units of one ware a Make takes, and how many the flagship carries.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MaterialView {
    pub ware: WareId,
    pub name: String,
    pub need: u32,
    pub have: u32,
}

/// Where the player has a part already.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Held {
    Fitted,
    Locker,
    OnOrder,
}

/// One part in a wharf's tray.
#[derive(Clone, Debug, PartialEq)]
pub struct PartOffer {
    pub part: UpgradeId,
    pub name: String,
    pub effect: String,
    pub socket: SocketId,
    pub power: u32,
    pub mass: u32,
    pub look: Option<String>,
    /// "Ready", "Make" or "Friends".
    pub tab: &'static str,
    /// Centicredits (Ready, Friends); 0 for Make.
    pub cost: i64,
    /// What making it takes (Make only).
    pub materials: Vec<MaterialView>,
    /// Who sends it and from where (Friends only).
    pub friend: Option<(NpcId, StationId)>,
    /// Already the player's: it shows as theirs ("Fitted", "In the locker", "On its
    /// way") and the section never vanishes.
    pub held: Option<Held>,
    /// What a click or drop sends. For a Make part this makes it into the locker; the
    /// room's confirm shows `materials` first.
    pub request: ServiceRequest,
    /// What stands in the way now, if anything.
    pub blocked: Option<Refusal>,
}

/// A part in the hull's locker.
#[derive(Clone, Debug, PartialEq)]
pub struct LockerItem {
    pub part: UpgradeId,
    pub name: String,
    pub socket: SocketId,
    /// Refit it here.
    pub request: ServiceRequest,
    pub blocked: Option<Refusal>,
}

/// A part on its way from a friend.
#[derive(Clone, Debug, PartialEq)]
pub struct InTransit {
    pub part: UpgradeId,
    pub name: String,
    pub from: NpcId,
    pub from_name: String,
    /// It's sent after the flagship's next docking here.
    pub station: StationId,
    pub station_name: String,
    pub paid: i64,
}

/// A wharf's room: who, which sockets, the three tabs, the locker and what's coming.
#[derive(Clone, Debug, PartialEq)]
pub struct WharfView {
    pub station: StationId,
    pub keeper: NpcId,
    pub name: String,
    pub sockets: Vec<SocketId>,
    /// Bought for credits: parts and the control modules sold here.
    pub ready: Vec<PartOffer>,
    pub make: Vec<PartOffer>,
    pub friends: Vec<PartOffer>,
    pub locker: Vec<LockerItem>,
    pub in_transit: Vec<InTransit>,
}

/// The wharf at `station`, if there is one. Each tab lists the parts sold here whose
/// unlock holds (and the ones the player has already, marked `held`).
pub fn wharf(world: &WorldState, history: &History, station: &StationId) -> Option<WharfView> {
    let w = world.progress.rules.fit.wharves.get(station)?;
    let eval = Eval::new(world, history);
    let mut view = WharfView {
        station: station.clone(),
        keeper: w.keeper.clone(),
        name: w.name.clone(),
        sockets: w.sockets.clone(),
        ready: Vec::new(),
        make: Vec::new(),
        friends: Vec::new(),
        locker: locker(world, history),
        in_transit: in_transit(world),
    };
    for u in world.progress.rules.upgrades.values() {
        if u.station.as_ref() != Some(station) {
            continue;
        }
        let Some(socket) = u.socket.clone() else {
            continue;
        };
        let held = held_where(world, &u.id);
        if held.is_none() && !eval.holds_data(&u.unlock) {
            continue;
        }
        let request = match &u.source {
            PartSource::Ready => ServiceRequest::Fit {
                upgrade: u.id.clone(),
            },
            PartSource::Make { .. } => ServiceRequest::Make {
                upgrade: u.id.clone(),
                fit: false,
            },
            PartSource::Friend { .. } => ServiceRequest::Order {
                upgrade: u.id.clone(),
            },
        };
        let offer = PartOffer {
            part: u.id.clone(),
            name: u.name.clone(),
            effect: u.effect.clone(),
            socket,
            power: u.power,
            mass: u.mass,
            look: u.look.clone(),
            tab: u.source.tab(),
            cost: if matches!(u.source, PartSource::Make { .. }) {
                0
            } else {
                u.cost
            },
            materials: make_bill(world, &u.id),
            friend: match &u.source {
                PartSource::Friend { npc, station } => Some((npc.clone(), station.clone())),
                _ => None,
            },
            held,
            blocked: crate::services::check(world, history, &request).err(),
            request,
        };
        match &u.source {
            PartSource::Ready => view.ready.push(offer),
            PartSource::Make { .. } => view.make.push(offer),
            PartSource::Friend { .. } => view.friends.push(offer),
        }
    }
    Some(view)
}

fn held_where(world: &WorldState, id: &UpgradeId) -> Option<Held> {
    if world.player.fittings.contains(id) {
        Some(Held::Fitted)
    } else if world.progress.fit.lockers.values().any(|l| l.contains(id)) {
        Some(Held::Locker)
    } else if world.progress.fit.orders.contains_key(id) {
        Some(Held::OnOrder)
    } else {
        None
    }
}

/// The flagship's locker, by part ID, each with the refit request and what blocks it
/// where the flagship is now.
pub fn locker(world: &WorldState, history: &History) -> Vec<LockerItem> {
    let Some(ship) = player_ship(world) else {
        return Vec::new();
    };
    fit::locker(world, &ship.id)
        .filter_map(|p| {
            let u = world.progress.rules.upgrades.get(p)?;
            let request = ServiceRequest::Refit { upgrade: p.clone() };
            Some(LockerItem {
                part: p.clone(),
                name: u.name.clone(),
                socket: u.socket.clone()?,
                blocked: crate::services::check(world, history, &request).err(),
                request,
            })
        })
        .collect()
}

/// Parts on their way from friends, by part ID.
pub fn in_transit(world: &WorldState) -> Vec<InTransit> {
    world
        .progress
        .fit
        .orders
        .values()
        .map(|o| InTransit {
            part: o.part.clone(),
            name: world
                .progress
                .rules
                .upgrades
                .get(&o.part)
                .map_or_else(|| o.part.to_string(), |u| u.name.clone()),
            from: o.from.clone(),
            from_name: npc_name(world, &o.from),
            station: o.station.clone(),
            station_name: station_name(world, &o.station),
            paid: o.paid,
        })
        .collect()
}
