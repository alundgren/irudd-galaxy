//! A stand-in player ship so the kernel's own tests can be driven by `PlayerInput`
//! without depending on `galaxy-ship` (which depends on this crate). The game uses
//! `galaxy_ship::systems(..)` instead.
//!
//! Arcade flight: velocity is thrust times the hull's top speed, so the ship stops
//! when the keys are released. Thrust while docked undocks; holding the action while
//! stopped inside a station's dock radius docks. Stateless, as systems must be.

use galaxy_contract::prelude::*;

use crate::system::{System, TickCtx};

pub struct PlaceholderFlight;

impl System for PlaceholderFlight {
    fn name(&self) -> &'static str {
        "placeholder_flight"
    }

    fn step(&self, ctx: &mut TickCtx<'_>) {
        let world = ctx.world;
        let Some(ship) = world
            .player
            .ship
            .as_ref()
            .and_then(|id| world.ships.get(id))
        else {
            return;
        };
        let Some(hull) = world.hulls.get(&ship.hull) else {
            return;
        };
        let thrust = ctx.input.thrust.clamp_length(1.0);
        let moving = thrust != Vec2::ZERO;

        if let Some(station) = &ship.docked_at {
            if moving {
                ctx.submit(
                    CommandBatch::new(Cause::verb(Actor::Player, VerbKind::Undock, "undock"))
                        .with(Command::SetShipDocked {
                            ship: ship.id.clone(),
                            station: None,
                        })
                        .with(Command::Emit {
                            event: Event::Undock {
                                ship: ship.id.clone(),
                                station: station.clone(),
                            },
                        }),
                );
            }
            return;
        }

        if !moving && ctx.input.action {
            let ring = world
                .stations
                .values()
                .find(|s| s.position.distance(ship.position) <= s.dock_radius);
            if let Some(station) = ring {
                ctx.submit(
                    CommandBatch::new(Cause::verb(Actor::Player, VerbKind::Dock, "dock"))
                        .with(Command::SetShipMotion {
                            ship: ship.id.clone(),
                            position: ship.position,
                            velocity: Vec2::ZERO,
                            heading: ship.heading,
                        })
                        .with(Command::SetShipDocked {
                            ship: ship.id.clone(),
                            station: Some(station.id.clone()),
                        })
                        .with(Command::Emit {
                            event: Event::Dock {
                                ship: ship.id.clone(),
                                station: station.id.clone(),
                            },
                        }),
                );
                return;
            }
        }

        let velocity = thrust * hull.max_speed;
        if velocity == Vec2::ZERO && ship.velocity == Vec2::ZERO {
            return;
        }
        let heading = if moving {
            thrust.y.atan2(thrust.x)
        } else {
            ship.heading
        };
        ctx.submit(
            CommandBatch::new(Cause::system(self.name(), "manual flight")).with(
                Command::SetShipMotion {
                    ship: ship.id.clone(),
                    position: ship.position + velocity * TICK_SECONDS,
                    velocity,
                    heading,
                },
            ),
        );
    }
}
