//! Applying one command, and the undo record that inverts it.
//!
//! This is the only code that mutates [`WorldState`] after a world is built. Adding a
//! command means adding a `Command` variant in the contract and an arm here that
//! checks its inputs, makes the change and returns the inverse.

use std::fmt::Display;

use galaxy_contract::history::FactDraft;
use galaxy_contract::pack::{PackState, ScriptHealth};
use galaxy_contract::prelude::*;
use galaxy_contract::talk::{Channel, SaidLine};

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

/// Drops a pack's entry once it holds nothing, so undo restores the world exactly.
fn prune_pack(world: &mut WorldState, pack: &PackId) {
    if world
        .packs
        .get(pack)
        .is_some_and(|p| *p == PackState::default())
    {
        world.packs.remove(pack);
    }
}

/// Adds to a tally, dropping it when it returns to zero so undo restores the map
/// exactly. Every field is checked before anything changes.
fn adjust_tally<K: Ord + Clone + Display>(
    map: &mut std::collections::BTreeMap<K, ActivityTally>,
    key: &K,
    delta: &ActivityTally,
) -> Result<(), CommandError> {
    let next = map
        .get(key)
        .cloned()
        .unwrap_or_default()
        .checked_add(delta)
        .ok_or_else(|| CommandError::Invalid(format!("activity at {key} would overflow")))?;
    if next.is_zero() {
        map.remove(key);
    } else {
        map.insert(key.clone(), next);
    }
    Ok(())
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
        C::ExpectDocked { ship, station } => {
            if refs.check {
                let s = world.ships.get(ship).ok_or_else(|| unknown("ship", ship))?;
                if s.docked_at.as_ref() != Some(station) {
                    return Err(CommandError::Invalid(format!(
                        "{ship} isn't docked at {station}"
                    )));
                }
            }
            invert(cmd.clone())
        }
        C::ExpectShip { ship, hull } => {
            if refs.check {
                match (world.ships.get(ship), hull) {
                    (None, None) => {}
                    (Some(s), Some(h)) if s.hull == *h => {}
                    (Some(_), None) => {
                        return Err(CommandError::Invalid(format!("{ship} already exists")));
                    }
                    (Some(s), Some(h)) => {
                        return Err(CommandError::Invalid(format!(
                            "{ship} flies {}, not {h}",
                            s.hull
                        )));
                    }
                    (None, Some(_)) => return Err(unknown("ship", ship)),
                }
            }
            invert(cmd.clone())
        }
        C::SetShipHull { ship, hull, name } => {
            refs.require(&world.hulls, hull, "hull")?;
            let s = find(&mut world.ships, ship, "ship")?;
            let old_hull = std::mem::replace(&mut s.hull, hull.clone());
            let old_name = std::mem::replace(&mut s.name, name.clone());
            invert(C::SetShipHull {
                ship: ship.clone(),
                hull: old_hull,
                name: old_name,
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
        C::SetConsignment {
            ship,
            ledger,
            consignment,
        } => {
            if let Some(c) = consignment {
                refs.require(&world.wares, &c.ware, "ware")?;
                refs.require(&world.npcs, &c.owner, "npc")?;
                refs.require(&world.npcs, &c.to, "npc")?;
                refs.require(&world.stations, &c.station, "station")?;
                if c.quantity == 0 {
                    return Err(CommandError::Invalid(format!(
                        "consignment {ledger} carries nothing"
                    )));
                }
            }
            let s = find(&mut world.ships, ship, "ship")?;
            let old = match consignment {
                Some(c) => {
                    if refs.check && s.hold.consigned.contains_key(ledger) {
                        return Err(CommandError::Invalid(format!(
                            "{ship} already carries consignment {ledger}"
                        )));
                    }
                    s.hold.consigned.insert(ledger.clone(), (**c).clone())
                }
                None => {
                    let old = s.hold.consigned.remove(ledger);
                    if refs.check && old.is_none() {
                        return Err(CommandError::Invalid(format!(
                            "{ship} carries no consignment {ledger}"
                        )));
                    }
                    old
                }
            };
            invert(C::SetConsignment {
                ship: ship.clone(),
                ledger: ledger.clone(),
                consignment: old.map(Box::new),
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
        C::UpdateOrders { ship, from, to } => {
            let s = find(&mut world.ships, ship, "ship")?;
            if s.orders != **from {
                return Err(CommandError::Invalid(format!(
                    "{ship}'s orders changed since this update was worked out"
                )));
            }
            s.orders = (**to).clone();
            invert(C::UpdateOrders {
                ship: ship.clone(),
                from: to.clone(),
                to: from.clone(),
            })
        }
        C::SetShipActivity { ship, activity } => {
            if let Some(t) = &activity.transit {
                refs.require(&world.lanes, &t.lane, "lane")?;
                refs.require(&world.regions, &t.from, "region")?;
                refs.require(&world.regions, &t.to, "region")?;
            }
            if let Some(m) = &activity.mining {
                refs.require(&world.rocks, &m.rock, "rock")?;
            }
            let s = find(&mut world.ships, ship, "ship")?;
            let old = std::mem::replace(&mut s.activity, (**activity).clone());
            invert(C::SetShipActivity {
                ship: ship.clone(),
                activity: Box::new(old),
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
        C::SetLoop { name, orders } => {
            if name.trim().is_empty() {
                return Err(CommandError::Invalid("a loop needs a name".into()));
            }
            let loops = &mut world.player.loops;
            let old = match orders {
                Some(o) => loops.insert(name.clone(), o.clone()),
                None => loops.remove(name),
            };
            invert(C::SetLoop {
                name: name.clone(),
                orders: old,
            })
        }
        C::SetRockGrowth { rock, growth } => {
            let r = find(&mut world.rocks, rock, "rock")?;
            let old = std::mem::replace(&mut r.growth, growth.clone());
            invert(C::SetRockGrowth {
                rock: rock.clone(),
                growth: old,
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
        C::ExpectStanding { faction, min, max } => {
            if refs.check {
                let f = world
                    .factions
                    .get(faction)
                    .ok_or_else(|| unknown("faction", faction))?;
                if !(*min..=*max).contains(&f.standing) {
                    return Err(CommandError::Invalid(format!(
                        "{faction}'s standing changed since this deed was worked out"
                    )));
                }
            }
            invert(cmd.clone())
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
        C::SetMarketBase {
            station,
            ware,
            base,
        } => {
            if base.base_price == 0
                || base.min_price > base.max_price
                || base.price < base.min_price
                || base.price > base.max_price
            {
                return Err(CommandError::Invalid(format!(
                    "a base price needs a band holding its price: {base:?}"
                )));
            }
            let m = find(&mut world.markets, station, "market")?;
            let line = m
                .lines
                .get_mut(ware)
                .ok_or_else(|| unknown("market line", &format!("{station}/{ware}")))?;
            let old = MarketBase {
                base_price: line.base_price,
                min_price: line.min_price,
                max_price: line.max_price,
                price: line.price,
            };
            line.base_price = base.base_price;
            line.min_price = base.min_price;
            line.max_price = base.max_price;
            line.price = base.price;
            invert(C::SetMarketBase {
                station: station.clone(),
                ware: ware.clone(),
                base: old,
            })
        }
        C::SetMarketStock {
            station,
            ware,
            stock,
        } => {
            if stock.min_price > stock.max_price
                || stock.price < stock.min_price
                || stock.price > stock.max_price
            {
                return Err(CommandError::Invalid(format!(
                    "a stock needs a band holding its price: {stock:?}"
                )));
            }
            let m = find(&mut world.markets, station, "market")?;
            let line = m
                .lines
                .get_mut(ware)
                .ok_or_else(|| unknown("market line", &format!("{station}/{ware}")))?;
            let old = MarketStock {
                stock: line.stock,
                stock_carry: line.stock_carry,
                min_price: line.min_price,
                max_price: line.max_price,
                price: line.price,
            };
            line.stock = stock.stock;
            line.stock_carry = stock.stock_carry;
            line.min_price = stock.min_price;
            line.max_price = stock.max_price;
            line.price = stock.price;
            invert(C::SetMarketStock {
                station: station.clone(),
                ware: ware.clone(),
                stock: old,
            })
        }
        C::AdjustActivity { scope, delta } => {
            let inverse = delta
                .checked_neg()
                .ok_or_else(|| CommandError::Invalid(format!("delta {delta:?} has no inverse")))?;
            let eco = &mut world.economy;
            match scope {
                ActivityScope::Station { station } => {
                    refs.require(&world.stations, station, "station")?;
                    adjust_tally(&mut eco.stations, station, delta)?;
                }
                ActivityScope::Region { region } => {
                    refs.require(&world.regions, region, "region")?;
                    adjust_tally(&mut eco.regions, region, delta)?;
                }
            }
            invert(C::AdjustActivity {
                scope: scope.clone(),
                delta: inverse,
            })
        }
        C::SetDockTrack { ship, track } => {
            if let Some(t) = track {
                refs.require(&world.stations, &t.station, "station")?;
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
        C::SetSplitStack { ware, station } => {
            refs.require(&world.wares, ware, "ware")?;
            if let Some(s) = station {
                refs.require(&world.stations, s, "station")?;
            }
            let stacks = &mut world.economy.split_stacks;
            let old = match station {
                Some(s) => stacks.insert(ware.clone(), s.clone()),
                None => stacks.remove(ware),
            };
            invert(C::SetSplitStack {
                ware: ware.clone(),
                station: old,
            })
        }
        C::FoundStation {
            station,
            recipe,
            market,
        } => {
            if refs.check && world.stations.contains(&station.id) {
                return Err(CommandError::Invalid("station exists".into()));
            }
            recipe
                .validate_art(galaxy_kit::art_bible::ArtBible::bundled())
                .map_err(|e| CommandError::Invalid(format!("invalid recipe {e:?}")))?;
            world
                .stations
                .insert(station.id.clone(), (**station).clone());
            world
                .station_recipes
                .insert(station.id.clone(), (**recipe).clone());
            world.markets.insert(
                station.id.clone(),
                market.as_deref().cloned().unwrap_or_else(|| Market {
                    station: station.id.clone(),
                    lines: Default::default(),
                }),
            );
            invert(C::CloseFoundedStation {
                id: station.id.clone(),
            })
        }
        C::CloseFoundedStation { id } => {
            let market = world.markets.get(id).cloned().map(Box::new);
            let old = world
                .stations
                .get(id)
                .cloned()
                .ok_or_else(|| unknown("station", id))?;
            let recipe = world
                .station_recipes
                .get(id)
                .cloned()
                .ok_or_else(|| unknown("recipe", id))?;
            if refs.check {
                let station = world.stations.get_mut(id).unwrap();
                station.services.clear();
                station.build.stage = 0;
                world.markets.remove(id);
                let mut frame = recipe.clone();
                frame.parts = vec![galaxy_kit::Part::new(
                    galaxy_kit::PartKind::Scaffold {
                        radius: 12.0,
                        height: 6.0,
                    },
                    Default::default(),
                )];
                frame.decals.clear();
                world.station_recipes.insert(id.clone(), frame);
            } else {
                world.stations.remove(id);
                world.station_recipes.remove(id);
                world.markets.remove(id);
            }
            invert(C::FoundStation {
                station: Box::new(old),
                recipe: Box::new(recipe),
                market,
            })
        }
        C::SetFoundedStation {
            station,
            recipe,
            market,
        } => {
            let id = &station.id;
            let old = world
                .stations
                .get(id)
                .cloned()
                .ok_or_else(|| unknown("station", id))?;
            let old_recipe = world
                .station_recipes
                .get(id)
                .cloned()
                .ok_or_else(|| unknown("recipe", id))?;
            if refs.check {
                if old.region != station.region || old.position != station.position {
                    return Err(CommandError::Invalid(format!(
                        "{id} keeps its region and position when its ring changes"
                    )));
                }
                let open = world.markets.contains(id);
                if market.is_some() == open || (!open && !old.services.is_empty()) {
                    return Err(CommandError::Invalid(format!(
                        "{id}'s ring is already {}",
                        if open { "open" } else { "closed" }
                    )));
                }
                if market.as_ref().is_some_and(|m| &m.station != id) {
                    return Err(CommandError::Invalid(format!(
                        "market for {id} names another station"
                    )));
                }
                // A shut ring is a bare scaffold frame; only an open one is a station.
                if market.is_some() {
                    recipe
                        .validate_art(galaxy_kit::art_bible::ArtBible::bundled())
                        .map_err(|e| CommandError::Invalid(format!("invalid recipe {e:?}")))?;
                }
            }
            let old_market = match market {
                Some(m) => world.markets.insert(id.clone(), (**m).clone()),
                None => world.markets.remove(id),
            };
            world.stations.insert(id.clone(), (**station).clone());
            world.station_recipes.insert(id.clone(), (**recipe).clone());
            invert(C::SetFoundedStation {
                station: Box::new(old),
                recipe: Box::new(old_recipe),
                market: old_market.map(Box::new),
            })
        }
        C::UpdateSurveyBuoy { id, from, to } => {
            if !to.radius.is_finite()
                || to.radius <= 0.0
                || !to.position.x.is_finite()
                || !to.position.y.is_finite()
            {
                return Err(CommandError::Invalid("invalid buoy".into()));
            }
            let buoy = world
                .survey_buoys
                .get_mut(id)
                .ok_or_else(|| CommandError::Invalid(format!("no survey buoy {id}")))?;
            if *buoy != **from {
                return Err(CommandError::Invalid(format!(
                    "survey buoy {id} changed since this update was worked out"
                )));
            }
            *buoy = (**to).clone();
            invert(C::UpdateSurveyBuoy {
                id: id.clone(),
                from: to.clone(),
                to: from.clone(),
            })
        }
        C::ExpectFlag { flag, on } => {
            if world.player.flags.contains(flag) != *on {
                return Err(CommandError::Invalid(format!(
                    "{flag} is no longer {}",
                    if *on { "set" } else { "clear" }
                )));
            }
            invert(cmd.clone())
        }
        C::SetRockTag { rock, on } => {
            refs.require(&world.rocks, rock, "rock")?;
            let old = world.rock_tags.contains(rock);
            if *on && !old && world.rock_tags.len() >= 6 {
                return Err(CommandError::Invalid("at most six lamps".into()));
            }
            if *on {
                world.rock_tags.insert(rock.clone());
            } else {
                world.rock_tags.remove(rock);
            }
            invert(C::SetRockTag {
                rock: rock.clone(),
                on: old,
            })
        }
        C::PutSurveyBuoy { id, value } => {
            if let Some(v) = value
                && (!v.radius.is_finite()
                    || v.radius <= 0.0
                    || !v.position.x.is_finite()
                    || !v.position.y.is_finite())
            {
                return Err(CommandError::Invalid("invalid buoy".into()));
            }
            let old = match value {
                Some(v) => world.survey_buoys.insert(id.clone(), (**v).clone()),
                None => world.survey_buoys.remove(id),
            };
            invert(C::PutSurveyBuoy {
                id: id.clone(),
                value: old.map(Box::new),
            })
        }
        C::SetCanonStatus { id, status } => {
            let n = world
                .canon
                .nodes
                .get_mut(id)
                .ok_or_else(|| CommandError::Invalid(format!("no canon node {id}")))?;
            let old = n.status;
            n.status = *status;
            invert(C::SetCanonStatus {
                id: id.clone(),
                status: old,
            })
        }
        C::PutStationRecipe { id, value } => {
            if refs.check
                && let Some(v) = value
            {
                let errors = v.validate_art(galaxy_kit::art_bible::ArtBible::bundled());
                if let Err(errors) = errors {
                    return Err(CommandError::Invalid(format!(
                        "invalid station recipe: {errors:?}"
                    )));
                }
            }
            let old = match value {
                Some(v) => world.station_recipes.insert(id.clone(), (**v).clone()),
                None => world.station_recipes.remove(id),
            };
            invert(C::PutStationRecipe {
                id: id.clone(),
                value: old.map(Box::new),
            })
        }
        C::PutStationPart { id, value } => {
            if let Some(v) = value {
                if &v.id != id {
                    return Err(CommandError::Invalid(format!(
                        "station part `{}` stored under `{id}`",
                        v.id
                    )));
                }
                if refs.check {
                    crate::parts::check(world, v).map_err(|e| {
                        CommandError::Invalid(format!("station part {id}: {}", e.join("; ")))
                    })?;
                }
            }
            let old = match value {
                Some(v) => world.station_parts.insert(id.clone(), (**v).clone()),
                None => world.station_parts.remove(id),
            };
            invert(C::PutStationPart {
                id: id.clone(),
                value: old.map(Box::new),
            })
        }
        C::PutMapOverlay { id, value } => {
            if let Some(v) = value
                && (!v.radius.is_finite()
                    || v.radius <= 0.0
                    || !v.center.x.is_finite()
                    || !v.center.y.is_finite())
            {
                return Err(CommandError::Invalid("invalid map overlay".into()));
            }
            let old = match value {
                Some(v) => world.map_overlays.insert(id.clone(), (**v).clone()),
                None => world.map_overlays.remove(id),
            };
            invert(C::PutMapOverlay {
                id: id.clone(),
                value: old.map(Box::new),
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
        C::PutTrader { id, value } => {
            let old = match value {
                Some(v) => {
                    if &v.id != id {
                        return Err(CommandError::Invalid(format!(
                            "trader record id `{}` doesn't match `{id}`",
                            v.id
                        )));
                    }
                    world.traders.insert(id.clone(), (**v).clone())
                }
                None => world.traders.remove(id),
            };
            invert(C::PutTrader {
                id: id.clone(),
                value: old.map(Box::new),
            })
        }
        C::SetRumorHeard {
            rumor,
            station,
            heard,
        } => {
            refs.require(&world.stations, station, "station")?;
            let r = find(&mut world.rumors, rumor, "rumor")?;
            let changed = if *heard {
                r.heard_at.insert(station.clone())
            } else {
                r.heard_at.remove(station)
            };
            if !changed {
                return Err(CommandError::Invalid(if *heard {
                    format!("{rumor} has already reached {station}")
                } else {
                    format!("{rumor} hasn't reached {station}")
                }));
            }
            invert(C::SetRumorHeard {
                rumor: rumor.clone(),
                station: station.clone(),
                heard: !heard,
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
        C::SetScriptValue {
            pack,
            script,
            store,
            key,
            value,
        } => {
            let p = world.packs.entry(pack.clone()).or_default();
            let stores = p.stores.entry(script.clone()).or_default();
            let entries = stores.entry(store.clone()).or_default();
            let old = match value {
                Some(v) => entries.insert(key.clone(), v.clone()),
                None => entries.remove(key),
            };
            if entries.is_empty() {
                stores.remove(store);
            }
            if stores.is_empty() {
                p.stores.remove(script);
            }
            prune_pack(world, pack);
            invert(C::SetScriptValue {
                pack: pack.clone(),
                script: script.clone(),
                store: store.clone(),
                key: key.clone(),
                value: old,
            })
        }
        C::SetTriggerFired {
            pack,
            trigger,
            fired,
        } => {
            let p = world.packs.entry(pack.clone()).or_default();
            let old = if *fired == 0 {
                p.triggers.remove(trigger)
            } else {
                p.triggers.insert(trigger.clone(), *fired)
            };
            prune_pack(world, pack);
            invert(C::SetTriggerFired {
                pack: pack.clone(),
                trigger: trigger.clone(),
                fired: old.unwrap_or(0),
            })
        }
        C::SetScriptHealth {
            pack,
            script,
            health,
        } => {
            let p = world.packs.entry(pack.clone()).or_default();
            let old = if *health == ScriptHealth::default() {
                p.health.remove(script)
            } else {
                p.health.insert(script.clone(), health.clone())
            };
            prune_pack(world, pack);
            invert(C::SetScriptHealth {
                pack: pack.clone(),
                script: script.clone(),
                health: old.unwrap_or_default(),
            })
        }
        C::SetRevisionRetired {
            pack,
            revision,
            retired,
        } => {
            let p = world.packs.entry(pack.clone()).or_default();
            let was = if *retired {
                !p.retired.insert(revision.clone())
            } else {
                p.retired.remove(revision)
            };
            prune_pack(world, pack);
            invert(C::SetRevisionRetired {
                pack: pack.clone(),
                revision: revision.clone(),
                retired: was,
            })
        }
        C::SetPackOff { pack, off } => {
            let p = world.packs.entry(pack.clone()).or_default();
            let was = std::mem::replace(&mut p.off, *off);
            prune_pack(world, pack);
            invert(C::SetPackOff {
                pack: pack.clone(),
                off: was,
            })
        }
        C::SetPackLayer { pack, layer } => {
            let was = world.packs.get(pack).and_then(|p| p.layer);
            // Unrecording (an inverse) is allowed; switching layers never is.
            if refs.check && was.is_some() && layer.is_some() && was != *layer {
                return Err(CommandError::Invalid(format!(
                    "{pack} already runs as {was:?}; a pack's layer never changes"
                )));
            }
            world.packs.entry(pack.clone()).or_default().layer = *layer;
            prune_pack(world, pack);
            invert(C::SetPackLayer {
                pack: pack.clone(),
                layer: was,
            })
        }
        C::PutBeat { id, value } => {
            let beats = &mut world.progress.beats;
            let old = match value {
                Some(v) => beats.insert(id.clone(), (**v).clone()),
                None => beats.remove(id),
            };
            invert(C::PutBeat {
                id: id.clone(),
                value: old.map(Box::new),
            })
        }
        C::SetFitting { upgrade, fitted } => {
            if *fitted && refs.check && !world.progress.rules.upgrades.contains_key(upgrade) {
                return Err(unknown("upgrade", upgrade));
            }
            let was = if *fitted {
                !world.player.fittings.insert(upgrade.clone())
            } else {
                world.player.fittings.remove(upgrade)
            };
            invert(C::SetFitting {
                upgrade: upgrade.clone(),
                fitted: was,
            })
        }
        C::ExpectFitting { upgrade, fitted } => {
            if refs.check && world.player.fittings.contains(upgrade) != *fitted {
                return Err(CommandError::Invalid(if *fitted {
                    format!("{upgrade} isn't fitted")
                } else {
                    format!("{upgrade} is already fitted")
                }));
            }
            invert(cmd.clone())
        }
        C::ExpectHullRecord { hull, record } => {
            if refs.check && world.player.hull_records.get(hull) != record.as_ref() {
                return Err(CommandError::Invalid(format!(
                    "{hull}'s rank record changed since this was worked out"
                )));
            }
            invert(cmd.clone())
        }
        C::SetHullRecord { hull, record } => {
            refs.require(&world.hulls, hull, "hull")?;
            let records = &mut world.player.hull_records;
            let old = match record {
                Some(r) => records.insert(hull.clone(), r.clone()),
                None => records.remove(hull),
            };
            invert(C::SetHullRecord {
                hull: hull.clone(),
                record: old,
            })
        }
        C::SetTunable { id, value } => {
            let tunables = &mut world.progress.tunables;
            let old = match value {
                Some(v) => tunables.insert(id.clone(), (**v).clone()),
                None => tunables.remove(id),
            };
            invert(C::SetTunable {
                id: id.clone(),
                value: old.map(Box::new),
            })
        }
        C::BindSlot { slot, mechanic } => {
            if mechanic.is_some() && refs.check && !world.progress.rules.slots.contains_key(slot) {
                return Err(unknown("slot", slot));
            }
            let slots = &mut world.progress.slots;
            let old = match mechanic {
                Some(m) => slots.insert(slot.clone(), m.clone()),
                None => slots.remove(slot),
            };
            invert(C::BindSlot {
                slot: slot.clone(),
                mechanic: old,
            })
        }
        C::Say { line } => {
            line.validate()
                .map_err(|e| CommandError::Invalid(e.into()))?;
            if let Some(npc) = &line.speaker {
                refs.require(&world.npcs, npc, "npc")?;
            }
            if let Some(station) = &line.place {
                refs.require(&world.stations, station, "station")?;
            }
            let log = &mut world.talk;
            let id = TalkId(log.last + 1);
            log.last = id.0;
            log.lines.insert(
                id,
                SaidLine {
                    id,
                    tick: world.meta.tick,
                    line: (**line).clone(),
                },
            );
            if line.channel == Channel::Chronicle {
                Ok(UndoRecord::Compensate {
                    note: format!("chronicle entry {id} was written"),
                })
            } else {
                invert(C::PutTalk { id, value: None })
            }
        }
        C::PutTalk { id, value } => {
            let log = &mut world.talk;
            let old = match value {
                Some(v) => {
                    if v.id != *id {
                        return Err(CommandError::Invalid(format!(
                            "line record id `{}` doesn't match `{id}`",
                            v.id
                        )));
                    }
                    log.last = log.last.max(id.0);
                    log.lines.insert(*id, (**v).clone())
                }
                None => {
                    let old = log.lines.remove(id);
                    // Withdrawing the newest line frees its number again (exact undo).
                    if old.is_some() && log.last == id.0 {
                        log.last -= 1;
                    }
                    old
                }
            };
            invert(C::PutTalk {
                id: *id,
                value: old.map(Box::new),
            })
        }
    }
}
