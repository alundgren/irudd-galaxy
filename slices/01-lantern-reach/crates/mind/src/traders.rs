//! The economy's complete utility choices, executed by the ship's order runner.
//! No per-tick decisions: TraderMenus invokes this only on a committed docking.

use galaxy_contract::prelude::*;
use galaxy_economy::{TraderProfile, rank_trades};

use crate::menu::{PlanOption, PlanSource};

pub struct EconomyPlans;

impl PlanSource for EconomyPlans {
    fn plans(&self, world: &WorldState, id: &ShipId) -> Vec<PlanOption> {
        let Some(def) = world.traders.get(id) else {
            return Vec::new();
        };
        let Some(ship) = world.ships.get(id) else {
            return Vec::new();
        };
        if ship.docked_at.is_none() || !ship.orders.orders.is_empty() {
            return Vec::new();
        }
        let mut profile = TraderProfile {
            ship: id.clone(),
            stations: def.stations.clone(),
            wares: def.wares.clone(),
            max_units_per_ware: def.max_units_per_ware,
            max_wares_per_docking: def.max_wares_per_docking,
            cost_per_distance: def.cost_per_distance,
        };
        for p in &def.preferences {
            if world.player.flags.contains(&p.when_flag) {
                profile.stations.insert(p.station.clone());
            }
        }
        let Ok(plans) = rank_trades(world, &profile) else {
            return Vec::new();
        };
        plans
            .into_iter()
            .filter_map(|p| {
                let destination = p.destination?;
                let mut score = p.score;
                if destination == def.home && p.carry.iter().all(|c| c.expected_credits == 0) {
                    score += def.return_home_bonus;
                }
                for pref in &def.preferences {
                    if pref.station == destination && world.player.flags.contains(&pref.when_flag) {
                        score += pref.bonus;
                    }
                }
                let mut orders: Vec<Order> = p
                    .trades
                    .into_iter()
                    .map(|t| {
                        let quantity = Quantity::Units { n: t.quantity };
                        match t.side {
                            TradeSide::Buy => Order::Buy {
                                ware: t.ware,
                                quantity,
                            },
                            TradeSide::Sell => Order::Sell {
                                ware: t.ware,
                                quantity,
                                floor: None,
                            },
                        }
                    })
                    .collect();
                orders.push(Order::Undock {});
                orders.push(Order::Dock {
                    station: destination.clone(),
                });
                Some(PlanOption {
                    summary: p.summary,
                    score,
                    refs: vec![AnyId::new(destination.as_str()).expect("station ID")],
                    commands: vec![Command::UpdateOrders {
                        ship: id.clone(),
                        from: Box::new(ship.orders.clone()),
                        to: Box::new(OrderQueue {
                            orders,
                            ..OrderQueue::default()
                        }),
                    }],
                })
            })
            .collect()
    }
}
