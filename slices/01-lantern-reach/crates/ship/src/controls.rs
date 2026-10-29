//! Control tiers as progression.
//!
//! Tier 0 is WASD plus the context action, and it ignores clicks. Each further input
//! mode is a [`Capability`], unlocked by a per-save flag in `Player::flags` whose name
//! is data (`ship_rules.controls`). Tiers group capabilities for presentation; they
//! aren't a strict ladder. The rules for *when* a capability unlocks belong to the
//! progression pack: it sets the flag with [`set_capability`].
//!
//! The loop API ([`save_loop`], [`run_loop`], [`assign_loop`]) builds the command
//! batches the app or a script submits when the player saves, runs or hands out a loop.

use std::collections::BTreeSet;

use galaxy_contract::prelude::*;
use galaxy_contract::ship::ControlRules;

use crate::space;

/// One unlockable input mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Capability {
    /// Click a point near the ship: fly there and stop.
    ClickFly,
    /// Click a rock: fly into reach and mine until full or spent.
    ClickMine,
    /// Click a station: fly into the ring and dock.
    ClickDock,
    /// Click anything on the map, at any distance; routes through lanes.
    MapTravel,
    /// Shift-click appends to a queue of orders.
    Queues,
    /// Save a queue as a named loop and run it.
    Loops,
    /// The player's automated craft run saved loops.
    Automation,
}

impl Capability {
    pub const ALL: [Capability; 7] = [
        Capability::ClickFly,
        Capability::ClickMine,
        Capability::ClickDock,
        Capability::MapTravel,
        Capability::Queues,
        Capability::Loops,
        Capability::Automation,
    ];

    /// The flag that unlocks it, from data.
    pub fn flag(self, rules: &ControlRules) -> &FlagId {
        match self {
            Capability::ClickFly => &rules.click_fly,
            Capability::ClickMine => &rules.click_mine,
            Capability::ClickDock => &rules.click_dock,
            Capability::MapTravel => &rules.map_travel,
            Capability::Queues => &rules.queues,
            Capability::Loops => &rules.loops,
            Capability::Automation => &rules.automation,
        }
    }

    /// The presentation tier it belongs to (0 is hands on: WASD and Space).
    pub fn tier(self) -> u8 {
        match self {
            Capability::ClickFly => 1,
            Capability::ClickMine | Capability::ClickDock => 2,
            Capability::MapTravel => 3,
            Capability::Queues => 4,
            Capability::Loops => 5,
            Capability::Automation => 6,
        }
    }
}

/// The capabilities the player has unlocked in this save.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Controls {
    caps: BTreeSet<Capability>,
}

impl Controls {
    pub fn of(world: &WorldState) -> Controls {
        let rules = &world.ship_rules.controls;
        Controls {
            caps: Capability::ALL
                .into_iter()
                .filter(|c| world.player.flags.contains(c.flag(rules)))
                .collect(),
        }
    }

    pub fn has(&self, cap: Capability) -> bool {
        self.caps.contains(&cap)
    }

    pub fn all(&self) -> impl Iterator<Item = Capability> + '_ {
        self.caps.iter().copied()
    }

    /// The highest tier with anything unlocked; 0 when only WASD and Space.
    pub fn tier(&self) -> u8 {
        self.caps.iter().map(|c| c.tier()).max().unwrap_or(0)
    }
}

/// The command that sets or clears a capability's flag.
pub fn set_capability(world: &WorldState, cap: Capability, on: bool) -> Command {
    Command::SetFlag {
        flag: cap.flag(&world.ship_rules.controls).clone(),
        on,
    }
}

/// What a click does under the unlocked controls: the order it makes, and whether it
/// appends to the queue. `None` when the click does nothing (tier 0, out of range,
/// locked mode).
pub fn click_order(
    world: &WorldState,
    controls: &Controls,
    ship: &Ship,
    click: &Click,
) -> Option<(Order, bool)> {
    let rules = &world.ship_rules.controls;
    let map = controls.has(Capability::MapTravel);
    let near = |pos: Vec2, region: Option<&RegionId>| {
        region.is_none_or(|r| r == &ship.region)
            && ship.position.distance(pos) <= rules.click_fly_range
    };
    let fly_near = |pos, region| controls.has(Capability::ClickFly) && near(pos, region);
    let order = match &click.target {
        ClickTarget::Point { at } => {
            let region = space::region_at(world, *at);
            (fly_near(*at, region) || map).then_some(Order::Fly {
                to: FlyTarget::Point { at: *at },
            })
        }
        ClickTarget::Rock { rock } => {
            let r = world.rocks.get(rock)?;
            let region = space::rock_region(world, r)?;
            let local = region == &ship.region;
            if controls.has(Capability::ClickMine) && (local || map) {
                Some(Order::Mine { rock: rock.clone() })
            } else {
                (fly_near(r.position, Some(region)) || map).then(|| Order::Fly {
                    to: FlyTarget::Rock { rock: rock.clone() },
                })
            }
        }
        ClickTarget::Station { station } => {
            let s = world.stations.get(station)?;
            let local = s.region == ship.region;
            if controls.has(Capability::ClickDock) && (local || map) {
                Some(Order::Dock {
                    station: station.clone(),
                })
            } else {
                (fly_near(s.position, Some(&s.region)) || map).then(|| Order::Fly {
                    to: FlyTarget::Station {
                        station: station.clone(),
                    },
                })
            }
        }
        ClickTarget::Region { region } => map.then(|| Order::Fly {
            to: FlyTarget::Region {
                region: region.clone(),
            },
        }),
    }?;
    Some((order, click.queue && controls.has(Capability::Queues)))
}

/// Applies a click to a queue: replace it, or append within the queue limit. Returns
/// false if the click changed nothing.
pub fn apply_click(world: &WorldState, queue: &mut OrderQueue, order: Order, append: bool) -> bool {
    let active = queue.current < queue.orders.len();
    if append && active {
        if queue.orders.len() >= world.ship_rules.controls.queue_limit as usize {
            return false;
        }
        queue.orders.push(order);
    } else {
        *queue = OrderQueue {
            orders: vec![order],
            ..OrderQueue::default()
        };
    }
    true
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ControlError {
    #[error("{0:?} isn't unlocked yet")]
    Locked(Capability),
    #[error("the player has no ship")]
    NoShip,
    #[error("unknown ship {0}")]
    UnknownShip(ShipId),
    #[error("{0} isn't the player's")]
    NotPlayers(ShipId),
    #[error("there are no orders to save")]
    NothingQueued,
    #[error("a loop holds at most {0} orders")]
    TooLong(u32),
    #[error("at most {0} loops can be saved")]
    TooMany(u32),
    #[error("a loop can't hold another loop")]
    Nested,
    #[error("no saved loop named {0:?}")]
    UnknownLoop(String),
    #[error("a loop needs a name")]
    NoName,
    #[error("{0} is flown by hand, not automated")]
    Flagship(ShipId),
}

fn need(world: &WorldState, cap: Capability) -> Result<(), ControlError> {
    if Controls::of(world).has(cap) {
        Ok(())
    } else {
        Err(ControlError::Locked(cap))
    }
}

fn loop_cause(summary: String) -> Cause {
    Cause::verb(Actor::Player, VerbKind::Loop, summary)
}

/// Saves the player's current queue as the loop `name` (replacing one of that name).
pub fn save_loop(world: &WorldState, name: &str) -> Result<CommandBatch, ControlError> {
    need(world, Capability::Loops)?;
    let name = name.trim();
    if name.is_empty() {
        return Err(ControlError::NoName);
    }
    let rules = &world.ship_rules.controls;
    let ship = world
        .player
        .ship
        .as_ref()
        .and_then(|id| world.ships.get(id))
        .ok_or(ControlError::NoShip)?;
    let orders = ship.orders.orders.clone();
    if orders.is_empty() {
        return Err(ControlError::NothingQueued);
    }
    if orders.iter().any(|o| matches!(o, Order::Loop { .. })) {
        return Err(ControlError::Nested);
    }
    if orders.len() > rules.loop_limit as usize {
        return Err(ControlError::TooLong(rules.loop_limit));
    }
    let saved = &world.player.loops;
    if !saved.contains_key(name) && saved.len() >= rules.saved_loop_limit as usize {
        return Err(ControlError::TooMany(rules.saved_loop_limit));
    }
    Ok(
        CommandBatch::new(loop_cause(format!("saved the loop {name:?}"))).with(Command::SetLoop {
            name: name.to_owned(),
            orders: Some(orders),
        }),
    )
}

fn saved<'w>(world: &'w WorldState, name: &str) -> Result<&'w Vec<Order>, ControlError> {
    world
        .player
        .loops
        .get(name)
        .ok_or_else(|| ControlError::UnknownLoop(name.to_owned()))
}

fn players_ship<'w>(world: &'w WorldState, ship: &ShipId) -> Result<&'w Ship, ControlError> {
    let s = world
        .ships
        .get(ship)
        .ok_or_else(|| ControlError::UnknownShip(ship.clone()))?;
    if s.owner == Owner::Player {
        Ok(s)
    } else {
        Err(ControlError::NotPlayers(ship.clone()))
    }
}

fn loop_queue(orders: Vec<Order>) -> OrderQueue {
    OrderQueue {
        orders: vec![Order::Loop {
            orders,
            until: None,
        }],
        ..OrderQueue::default()
    }
}

/// Starts the saved loop `name` on one of the player's ships. The flagship needs the
/// loops capability; any other ship (a tender) needs automation.
pub fn run_loop(
    world: &WorldState,
    ship: &ShipId,
    name: &str,
) -> Result<CommandBatch, ControlError> {
    let s = players_ship(world, ship)?;
    let flagship = world.player.ship.as_ref() == Some(&s.id);
    need(
        world,
        if flagship {
            Capability::Loops
        } else {
            Capability::Automation
        },
    )?;
    let orders = saved(world, name)?.clone();
    Ok(
        CommandBatch::new(loop_cause(format!("{ship} runs the loop {name:?}"))).with(
            Command::SetOrders {
                ship: ship.clone(),
                orders: loop_queue(orders),
            },
        ),
    )
}

/// Hands the saved loop `name` to an automated craft. Assigning copies the loop, so
/// later edits to the player's loop don't change the craft until assigned again.
/// `floor` (if any) becomes the floor of every sell in it: a craft with a standing
/// berth then waits docked until the price is back at the floor (the patient sell).
pub fn assign_loop(
    world: &WorldState,
    craft: &ShipId,
    name: &str,
    floor: Option<u32>,
) -> Result<CommandBatch, ControlError> {
    need(world, Capability::Automation)?;
    let s = players_ship(world, craft)?;
    if world.player.ship.as_ref() == Some(&s.id) {
        return Err(ControlError::Flagship(craft.clone()));
    }
    let mut orders = saved(world, name)?.clone();
    if floor.is_some() {
        for o in &mut orders {
            if let Order::Sell { floor: f, .. } = o {
                *f = floor;
            }
        }
    }
    Ok(
        CommandBatch::new(loop_cause(format!("{craft} was given the loop {name:?}"))).with(
            Command::SetOrders {
                ship: craft.clone(),
                orders: loop_queue(orders),
            },
        ),
    )
}
