//! What a ship wears: the world's fitted parts as render looks.
//!
//! The one place that maps fittings to looks. Today fittings belong to the pilot
//! (`Player.fittings`), so only the flagship wears them; repoint [`look_of`] and
//! [`ship_look`] when parts move onto the hull's sockets.

use galaxy_contract::prelude::*;
use galaxy_contract::progress::Modifier;
use render::kit::{LookId, ShipFitLook, Socket};

/// The look an upgrade gives, and where: the mining lens on the nose, any hold upgrade
/// as pods on the belly. Parts named for a look (`…long_drill`, `…fins`, `…seam_lamp`)
/// wear it. Control modules and the rest have no outside look.
pub fn look_of(world: &WorldState, upgrade: &UpgradeId) -> Option<(Socket, LookId)> {
    let id = upgrade.as_str();
    let adds_hold = world.progress.rules.upgrades.get(upgrade).is_some_and(|d| {
        d.modifiers
            .iter()
            .any(|m| matches!(m, Modifier::Hold { .. }))
    });
    let look = if id == "upgrade.mining_lens" {
        LookId::Lens
    } else if adds_hold || id.contains("hold") {
        LookId::Pods
    } else if id.contains("long_drill") || id.contains("long_throw") {
        LookId::LongDrill
    } else if id.contains("fins") {
        LookId::Fins
    } else if id.contains("seam_lamp") {
        LookId::SeamLamp
    } else {
        return None;
    };
    Some((look.socket(), look))
}

/// The look for a set of upgrades on a ship that needn't be in the world: the cradle's
/// preview of a fit before it's made. Add a part with [`ShipFitLook::with`].
pub fn look_for<'a>(
    world: &WorldState,
    upgrades: impl IntoIterator<Item = &'a UpgradeId>,
) -> ShipFitLook {
    ShipFitLook::from_pairs(upgrades.into_iter().filter_map(|u| look_of(world, u)))
}

/// What `ship` wears in the world. The pilot's fittings ride on the flagship; every
/// other craft flies bare.
pub fn ship_look(world: &WorldState, ship: &Ship) -> ShipFitLook {
    if world.player.ship.as_ref() != Some(&ship.id) {
        return ShipFitLook::default();
    }
    look_for(world, &world.player.fittings)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ship(id: &'static str) -> Ship {
        Ship {
            id: ShipId::from_static(id),
            name: "Wren".into(),
            hull: HullId::from_static("hull.wren"),
            owner: Owner::Player,
            region: RegionId::from_static("region.lantern_shallows"),
            position: Vec2::ZERO,
            velocity: Vec2::ZERO,
            heading: 0.0,
            docked_at: None,
            hold: Hold::default(),
            orders: Default::default(),
            activity: Default::default(),
        }
    }

    #[test]
    fn the_flagship_wears_the_pilots_fittings() {
        let mut world = WorldState::default();
        let flagship = ship("ship.player");
        let tender = ship("ship.thimble");
        world.player.ship = Some(flagship.id.clone());
        assert!(ship_look(&world, &flagship).is_empty());
        for id in [
            "upgrade.mining_lens",
            "upgrade.hold_extender",
            "upgrade.nav_chip",
            "upgrade.lane_chart",
        ] {
            world.player.fittings.insert(UpgradeId::new(id).unwrap());
        }
        let look = ship_look(&world, &flagship);
        assert_eq!(look.nose, Some(LookId::Lens));
        assert_eq!(look.belly, Some(LookId::Pods));
        assert_eq!(look.utility, None, "control modules have no outside look");
        assert_eq!(look.tail, None);
        assert!(
            ship_look(&world, &tender).is_empty(),
            "fittings are the pilot's"
        );
    }

    #[test]
    fn a_ship_not_in_the_world_previews_a_part() {
        let world = WorldState::default();
        let lens = UpgradeId::from_static("upgrade.mining_lens");
        let preview = look_for(&world, [&lens]).with(Socket::Tail, LookId::Fins);
        assert_eq!(preview.nose, Some(LookId::Lens));
        assert_eq!(preview.tail, Some(LookId::Fins));
        let named = UpgradeId::from_static("upgrade.swift_fins");
        assert_eq!(look_of(&world, &named), Some((Socket::Tail, LookId::Fins)));
    }
}
