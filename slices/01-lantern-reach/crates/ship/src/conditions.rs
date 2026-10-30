//! World conditions for `wait` orders and `loop ... until`. Only world state, never
//! time: a wait ends when something in the world changes.

use galaxy_contract::prelude::*;

/// Whether `cond` holds now. Hold conditions read `ship`'s hold (false without a ship).
pub fn holds(world: &WorldState, ship: Option<&Ship>, cond: &Condition) -> bool {
    let price = |station: &StationId, ware: &WareId| {
        world.markets.get(station).and_then(|m| m.lines.get(ware))
    };
    match cond {
        Condition::HoldFull => ship.is_some_and(|s| {
            world
                .hulls
                .get(&s.hull)
                .is_some_and(|h| s.hold.total() >= u64::from(h.hold_capacity))
        }),
        Condition::HoldEmpty => ship.is_some_and(|s| s.hold.total() == 0),
        Condition::PriceAtLeast {
            station,
            ware,
            price: p,
        } => price(station, ware).is_some_and(|l| l.price >= *p),
        Condition::PriceAtMost {
            station,
            ware,
            price: p,
        } => price(station, ware).is_some_and(|l| l.price <= *p),
        Condition::StockAtLeast {
            station,
            ware,
            stock,
        } => price(station, ware).is_some_and(|l| l.stock >= *stock),
        Condition::CounterAtLeast {
            counter,
            scope,
            value,
        } => world.counters.get(*counter, scope) >= *value,
        Condition::FlagSet { flag } => world.player.flags.contains(flag),
        Condition::All { of } => of.iter().all(|c| holds(world, ship, c)),
        Condition::Any { of } => of.iter().any(|c| holds(world, ship, c)),
        Condition::Not { cond } => !holds(world, ship, cond),
    }
}
