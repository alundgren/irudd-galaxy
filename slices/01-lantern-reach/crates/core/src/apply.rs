//! Applying one command, and the undo record that inverts it.
//!
//! This is the only code that mutates [`WorldState`] after a world is built. Adding a
//! command means adding a `Command` variant in the contract and an arm here that
//! checks its inputs, makes the change and returns the inverse.

use std::fmt::Display;

use galaxy_contract::history::FactDraft;
use galaxy_contract::prelude::*;

/// Why a command was refused.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum CommandError {
    #[error("unknown {kind} `{id}`")]
    Unknown { kind: &'static str, id: String },
    #[error("{0}")]
    Invalid(String),
}

/// Effects that only happen if the whole batch commits.
#[derive(Debug, Default)]
pub struct Staged {
    pub events: Vec<Event>,
    pub facts: Vec<FactDraft>,
}

fn unknown(kind: &'static str, id: &impl Display) -> CommandError {
    CommandError::Unknown {
        kind,
        id: id.to_string(),
    }
}

fn find<'a, K: Ord + Display, V>(
    store: &'a mut Store<K, V>,
    id: &K,
    kind: &'static str,
) -> Result<&'a mut V, CommandError> {
    store.get_mut(id).ok_or_else(|| unknown(kind, id))
}

/// Whether a command's references to other records are checked. Forward commands
/// check them; undoing a just-applied command restores the captured values as they
/// were, even if they referred to something that didn't exist yet (the batch is being
/// rejected precisely because the world it left behind was broken).
#[derive(Clone, Copy)]
struct Refs {
    check: bool,
}

impl Refs {
    fn require<K: Ord + Display, V>(
        self,
        store: &Store<K, V>,
        id: &K,
        kind: &'static str,
    ) -> Result<(), CommandError> {
        if !self.check || store.contains(id) {
            Ok(())
        } else {
            Err(unknown(kind, id))
        }
    }
}

fn adjust_u32(value: u32, delta: i64, what: &str) -> Result<u32, CommandError> {
    i64::from(value)
        .checked_add(delta)
        .and_then(|next| u32::try_from(next).ok())
        .ok_or_else(|| CommandError::Invalid(format!("{what} can't change by {delta}")))
}

/// The inverse delta, refused before anything changes if it doesn't exist
/// (`i64::MIN`, `i32::MIN`).
fn negate<T: Copy + Display>(delta: T, neg: impl Fn(T) -> Option<T>) -> Result<T, CommandError> {
    neg(delta).ok_or_else(|| CommandError::Invalid(format!("delta {delta} has no inverse")))
}

fn put<V: Record + Clone>(
    store: &mut Store<V::Id, V>,
    id: &V::Id,
    value: &Option<Box<V>>,
    kind: &'static str,
) -> Result<Option<Box<V>>, CommandError>
where
    V::Id: Display,
{
    let old = match value {
        Some(v) => {
            if v.id() != id {
                return Err(CommandError::Invalid(format!(
                    "{kind} record id `{}` doesn't match `{id}`",
                    v.id()
                )));
            }
            store.insert(id.clone(), (**v).clone())
        }
        None => store.remove(id),
    };
    Ok(old.map(Box::new))
}

/// Adds to a tally, dropping it when it returns to zero so undo restores the map
/// exactly.
fn adjust_tally<K: Ord + Clone>(
    map: &mut std::collections::BTreeMap<K, ActivityTally>,
    key: &K,
    delta: &ActivityTally,
) {
    let tally = map.entry(key.clone()).or_default();
    tally.add(delta);
    if tally.is_zero() {
        map.remove(key);
    }
}

fn invert(command: Command) -> Result<UndoRecord, CommandError> {
    Ok(UndoRecord::Invert { command })
}

/// Applies `cmd` to `world`. Events and facts go to `staged` and only take effect when
/// the batch commits. On error nothing has changed.
pub fn apply(
    world: &mut WorldState,
    cmd: &Command,
    staged: &mut Staged,
) -> Result<UndoRecord, CommandError> {
    apply_with(world, cmd, staged, Refs { check: true })
}

/// Applies the inverse of a command that was just applied, without checking that the
/// values it restores refer to existing records. Used to undo a rejected batch.
pub(crate) fn restore(world: &mut WorldState, inverse: &Command) -> Result<(), CommandError> {
    apply_with(
        world,
        inverse,
        &mut Staged::default(),
        Refs { check: false },
    )
    .map(drop)
}

fn apply_with(
    world: &mut WorldState,
    cmd: &Command,
    staged: &mut Staged,
    refs: Refs,
) -> Result<UndoRecord, CommandError> {
    use Command as C;
    match cmd {
        C::SetShipMotion {
            ship,
            position,
            velocity,
            heading,
        } => {
            let s = find(&mut world.ships, ship, "ship")?;
            let old = C::SetShipMotion {
                ship: ship.clone(),
                position: s.position,
                velocity: s.velocity,
                heading: s.heading,
            };
            s.position = *position;
            s.velocity = *velocity;
            s.heading = *heading;
            invert(old)
        }
        C::SetShipDocked { ship, station } => {
            if let Some(st) = station {
                refs.require(&world.stations, st, "station")?;
            }
            let s = find(&mut world.ships, ship, "ship")?;
            let old = std::mem::replace(&mut s.docked_at, station.clone());
            invert(C::SetShipDocked {
                ship: ship.clone(),
                station: old,
            })
        }
        C::SetShipRegion { ship, region } => {
            refs.require(&world.regions, region, "region")?;
            let s = find(&mut world.ships, ship, "ship")?;
            let old = std::mem::replace(&mut s.region, region.clone());
            invert(C::SetShipRegion {
                ship: ship.clone(),
                region: old,
            })
        }
        C::AdjustCargo { ship, ware, delta } => {
            refs.require(&world.wares, ware, "ware")?;
            let inverse = negate(*delta, i64::checked_neg)?;
            let s = find(&mut world.ships, ship, "ship")?;
            let next = adjust_u32(s.hold.count(ware), *delta, &format!("{ware} in {ship}"))?;
            if next == 0 {
                s.hold.cargo.remove(ware);
            } else {
                s.hold.cargo.insert(ware.clone(), next);
            }
            invert(C::AdjustCargo {
                ship: ship.clone(),
                ware: ware.clone(),
                delta: inverse,
            })
        }
        C::SetOrders { ship, orders } => {
            let s = find(&mut world.ships, ship, "ship")?;
            let old = std::mem::replace(&mut s.orders, orders.clone());
            invert(C::SetOrders {
                ship: ship.clone(),
                orders: old,
            })
        }
        C::AdjustCredits { delta } => {
            let inverse = negate(*delta, i64::checked_neg)?;
            let next = world
                .player
                .credits
                .checked_add(*delta)
                .filter(|&c| c >= 0)
                .ok_or_else(|| CommandError::Invalid("not enough credits".into()))?;
            world.player.credits = next;
            invert(C::AdjustCredits { delta: inverse })
        }
        C::SetFlag { flag, on } => {
            let was = if *on {
                !world.player.flags.insert(flag.clone())
            } else {
                world.player.flags.remove(flag)
            };
            invert(C::SetFlag {
                flag: flag.clone(),
                on: was,
            })
        }
        C::SetTrait { trait_id, on } => {
            let was = if *on {
                !world.player.traits.insert(trait_id.clone())
            } else {
                world.player.traits.remove(trait_id)
            };
            invert(C::SetTrait {
                trait_id: trait_id.clone(),
                on: was,
            })
        }
        C::SetPriceSnapshot { station, snapshot } => {
            refs.require(&world.stations, station, "station")?;
            let snaps = &mut world.player.price_snapshots;
            let old = match snapshot {
                Some(s) => snaps.insert(station.clone(), s.clone()),
                None => snaps.remove(station),
            };
            invert(C::SetPriceSnapshot {
                station: station.clone(),
                snapshot: old,
            })
        }
        C::AdjustRock { rock, delta } => {
            let inverse = negate(*delta, i64::checked_neg)?;
            let r = find(&mut world.rocks, rock, "rock")?;
            r.remaining = adjust_u32(r.remaining, *delta, &format!("{rock} remaining"))?;
            invert(C::AdjustRock {
                rock: rock.clone(),
                delta: inverse,
            })
        }
        C::SetStationStage { station, stage } => {
            let s = find(&mut world.stations, station, "station")?;
            let old = std::mem::replace(&mut s.build.stage, *stage);
            invert(C::SetStationStage {
                station: station.clone(),
                stage: old,
            })
        }
        C::SetStationPart {
            station,
            part,
            attached,
        } => {
            let s = find(&mut world.stations, station, "station")?;
            let was = if *attached {
                !s.build.parts.insert(part.clone())
            } else {
                s.build.parts.remove(part)
            };
            invert(C::SetStationPart {
                station: station.clone(),
                part: part.clone(),
                attached: was,
            })
        }
        C::SetStationOwner { station, owner } => {
            if let Some(f) = owner {
                refs.require(&world.factions, f, "faction")?;
            }
            let s = find(&mut world.stations, station, "station")?;
            let old = std::mem::replace(&mut s.owner, owner.clone());
            invert(C::SetStationOwner {
                station: station.clone(),
                owner: old,
            })
        }
        C::AdjustStanding { faction, delta } => {
            let inverse = negate(*delta, i32::checked_neg)?;
            let f = find(&mut world.factions, faction, "faction")?;
            f.standing = f
                .standing
                .checked_add(*delta)
                .ok_or_else(|| CommandError::Invalid("standing overflow".into()))?;
            invert(C::AdjustStanding {
                faction: faction.clone(),
                delta: inverse,
            })
        }
        C::SetMarketLine {
            station,
            ware,
            line,
        } => {
            if line.is_some() {
                refs.require(&world.wares, ware, "ware")?;
            }
            let m = find(&mut world.markets, station, "market")?;
            let old = match line {
                Some(l) => m.lines.insert(ware.clone(), l.clone()),
                None => m.lines.remove(ware),
            };
            invert(C::SetMarketLine {
                station: station.clone(),
                ware: ware.clone(),
                line: old,
            })
        }
        C::UpdateMarketLine {
            station,
            ware,
            from,
            to,
        } => {
            let m = find(&mut world.markets, station, "market")?;
            let line = m
                .lines
                .get_mut(ware)
                .ok_or_else(|| unknown("market line", &format!("{station}/{ware}")))?;
            if *line != **from {
                return Err(CommandError::Invalid(format!(
                    "{ware} at {station} changed since this update was worked out"
                )));
            }
            *line = (**to).clone();
            invert(C::UpdateMarketLine {
                station: station.clone(),
                ware: ware.clone(),
                from: to.clone(),
                to: from.clone(),
            })
        }
        C::SetMarketRates {
            station,
            ware,
            rates,
        } => {
            let scales = [rates.production_scale, rates.consumption_scale];
            if scales.iter().any(|s| !s.is_finite() || *s < 0.0) {
                return Err(CommandError::Invalid(format!(
                    "rate scales must be finite and not negative: {scales:?}"
                )));
            }
            let m = find(&mut world.markets, station, "market")?;
            let line = m
                .lines
                .get_mut(ware)
                .ok_or_else(|| unknown("market line", &format!("{station}/{ware}")))?;
            let old = MarketRates {
                production: line.production,
                consumption: line.consumption,
                production_scale: line.production_scale,
                consumption_scale: line.consumption_scale,
            };
            line.production = rates.production;
            line.consumption = rates.consumption;
            line.production_scale = rates.production_scale;
            line.consumption_scale = rates.consumption_scale;
            invert(C::SetMarketRates {
                station: station.clone(),
                ware: ware.clone(),
                rates: old,
            })
        }
        C::AdjustActivity { scope, delta } => {
            let eco = &mut world.economy;
            match scope {
                ActivityScope::Station { station } => {
                    require(&world.stations, station, "station")?;
                    adjust_tally(&mut eco.stations, station, delta);
                }
                ActivityScope::Region { region } => {
                    require(&world.regions, region, "region")?;
                    adjust_tally(&mut eco.regions, region, delta);
                }
            }
            invert(C::AdjustActivity {
                scope: scope.clone(),
                delta: delta.negated(),
            })
        }
        C::SetDockTrack { ship, track } => {
            if let Some(t) = track {
                require(&world.stations, &t.station, "station")?;
            }
            let docks = &mut world.economy.docks;
            let old = match track {
                Some(t) => docks.insert(ship.clone(), t.clone()),
                None => docks.remove(ship),
            };
            invert(C::SetDockTrack {
                ship: ship.clone(),
                track: old,
            })
        }
        C::PutRegion { id, value } => {
            let old = put(&mut world.regions, id, value, "region")?;
            invert(C::PutRegion {
                id: id.clone(),
                value: old,
            })
        }
        C::PutLane { id, value } => {
            let old = put(&mut world.lanes, id, value, "lane")?;
            invert(C::PutLane {
                id: id.clone(),
                value: old,
            })
        }
        C::PutStation { id, value } => {
            let old = put(&mut world.stations, id, value, "station")?;
            invert(C::PutStation {
                id: id.clone(),
                value: old,
            })
        }
        C::PutMarket { id, value } => {
            let old = put(&mut world.markets, id, value, "market")?;
            invert(C::PutMarket {
                id: id.clone(),
                value: old,
            })
        }
        C::PutWare { id, value } => {
            let old = put(&mut world.wares, id, value, "ware")?;
            invert(C::PutWare {
                id: id.clone(),
                value: old,
            })
        }
        C::PutHull { id, value } => {
            let old = put(&mut world.hulls, id, value, "hull")?;
            invert(C::PutHull {
                id: id.clone(),
                value: old,
            })
        }
        C::PutRockType { id, value } => {
            let old = put(&mut world.rock_types, id, value, "rock type")?;
            invert(C::PutRockType {
                id: id.clone(),
                value: old,
            })
        }
        C::PutField { id, value } => {
            let old = put(&mut world.fields, id, value, "field")?;
            invert(C::PutField {
                id: id.clone(),
                value: old,
            })
        }
        C::PutRock { id, value } => {
            let old = put(&mut world.rocks, id, value, "rock")?;
            invert(C::PutRock {
                id: id.clone(),
                value: old,
            })
        }
        C::PutShip { id, value } => {
            let old = put(&mut world.ships, id, value, "ship")?;
            invert(C::PutShip {
                id: id.clone(),
                value: old,
            })
        }
        C::PutFaction { id, value } => {
            let old = put(&mut world.factions, id, value, "faction")?;
            invert(C::PutFaction {
                id: id.clone(),
                value: old,
            })
        }
        C::PutNpc { id, value } => {
            let old = put(&mut world.npcs, id, value, "npc")?;
            invert(C::PutNpc {
                id: id.clone(),
                value: old,
            })
        }
        C::PutRumor { id, value } => {
            let old = put(&mut world.rumors, id, value, "rumor")?;
            invert(C::PutRumor {
                id: id.clone(),
                value: old,
            })
        }
        C::Emit { event } => {
            staged.events.push(event.clone());
            Ok(UndoRecord::Compensate {
                note: format!("`{}` event was emitted", event.kind().as_str()),
            })
        }
        C::RecordFact { fact } => {
            staged.facts.push((**fact).clone());
            Ok(UndoRecord::Compensate {
                note: format!("`{}` fact was recorded", fact.kind),
            })
        }
    }
}
