//! Which kit recipe draws which station, which hull mesh a hull uses, which rock kind a
//! rock type is, and each region's backdrop.

use galaxy_contract::prelude::*;
use render::kit::{BackdropParams, HullColors, HullParams, RockKind, StationRecipe};
use render::{Color, palette};

/// The recipe for a station at its current build stage. Unknown stations (the game
/// master's sixth) get a small generic recipe until they carry their own.
pub fn station_recipe(station: &Station) -> StationRecipe {
    let stage = u8::try_from(station.build.stage.min(3)).unwrap_or(3);
    let mut recipe = match station.id.as_str() {
        "station.lantern_wharf" => StationRecipe::lantern_wharf(),
        "station.kettle_dock" => StationRecipe::kettle_dock(stage),
        "station.brassmouth_exchange" => StationRecipe::brassmouth(),
        "station.cinder_hook" => StationRecipe::cinder_hook(),
        "station.glass_drift_observatory" => StationRecipe::glass_drift_observatory(),
        _ => {
            let mut r = StationRecipe::kettle_dock(stage);
            r.parts.retain(|p| {
                !matches!(p.kind, render::kit::PartKind::Greenhouse { .. }) || stage > 0
            });
            r
        }
    };
    recipe.name = station.name.clone();
    recipe
}

/// The mesh parameters for a hull.
pub fn hull_params(hull: &HullId) -> HullParams {
    match hull.as_str() {
        "hull.tandem" => HullParams::tandem(),
        "hull.heron" => HullParams::heron(),
        "hull.tender" => HullParams {
            length: 5.0,
            width: 3.4,
            wing_span: 4.0,
            cargo_pods: 1,
            fin: 0.6,
            colors: HullColors {
                body: palette::MUSTARD,
                nose: palette::HULL_CREAM,
                trim: palette::TEAL,
                ..HullColors::default()
            },
            ..HullParams::tandem()
        },
        "hull.wren" => HullParams::wren(),
        _ => HullParams {
            colors: HullColors {
                body: palette::SAGE,
                ..HullColors::default()
            },
            ..HullParams::tandem()
        },
    }
}

/// NPC craft are painted by who owns them, so the player's coral Wren stands out.
pub fn npc_hull_params(hull: &HullId, faction: Option<&FactionId>) -> HullParams {
    let mut p = hull_params(hull);
    p.colors.body = faction.map_or(palette::SAGE, faction_color);
    p
}

/// The rock kind for a rock type, from the ware it yields.
pub fn rock_kind(world: &WorldState, rock_type: &RockTypeId) -> RockKind {
    let ware = world.rock_types.get(rock_type).map(|t| t.ware.as_str());
    match ware {
        Some("ware.ice") => RockKind::Ice,
        Some("ware.glassite") => RockKind::Glassite,
        _ => RockKind::Iron,
    }
}

/// A faction's colour on the map and on its craft.
pub fn faction_color(faction: &FactionId) -> Color {
    match faction.as_str() {
        "faction.wharf_cooperative" => palette::TEAL,
        "faction.tollway_compact" => palette::MUSTARD,
        "faction.lamplighter_society" => Color::hex(0xb58ae0),
        _ => palette::SAGE,
    }
}

/// Each region's sky: the Shallows calm teal and violet, the Cinder Drift ember and
/// rose, Brassmouth brassy violet.
pub fn backdrop(region: Option<&RegionId>) -> BackdropParams {
    let base = BackdropParams::default();
    match region.map(RegionId::as_str) {
        Some("region.lantern_shallows") => BackdropParams {
            top: Color::hex(0x3a3470),
            bottom: Color::hex(0x1d1a3e),
            cloud_colors: vec![
                palette::NEBULA_TEAL,
                palette::NEBULA_VIOLET,
                Color::hex(0x4a7fb0),
            ],
            ..base
        },
        Some("region.cinder_drift") => BackdropParams {
            top: Color::hex(0x55304a),
            bottom: Color::hex(0x24162a),
            cloud_colors: vec![
                palette::NEBULA_AMBER,
                palette::NEBULA_ROSE,
                Color::hex(0x9a4a3a),
            ],
            cloud_alpha: 0.24,
            ..base
        },
        Some("region.brassmouth_narrows") => BackdropParams {
            top: Color::hex(0x4e3672),
            bottom: Color::hex(0x2a1f48),
            cloud_colors: vec![
                palette::NEBULA_VIOLET,
                Color::hex(0xc98f4a),
                palette::NEBULA_ROSE,
            ],
            ..base
        },
        _ => base,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_authored_station_has_a_valid_recipe() {
        for (id, name) in [
            ("station.lantern_wharf", "Lantern Wharf"),
            ("station.kettle_dock", "Kettle Dock"),
            ("station.cinder_hook", "Cinder Hook"),
            ("station.glass_drift_observatory", "Glass Drift Observatory"),
            ("station.brassmouth_exchange", "Brassmouth Exchange"),
            ("gm.sixth_station", "Sixth"),
        ] {
            let s = Station {
                id: StationId::new(id).unwrap(),
                name: name.into(),
                region: RegionId::from_static("region.lantern_shallows"),
                position: Vec2::ZERO,
                dock_radius: 50.0,
                owner: None,
                services: Default::default(),
                build: BuildState::default(),
            };
            let r = station_recipe(&s);
            assert_eq!(r.name, name);
            r.validate().unwrap_or_else(|e| panic!("{id}: {e:?}"));
        }
    }

    #[test]
    fn kettle_follows_its_build_stage() {
        let mut s = Station {
            id: StationId::from_static("station.kettle_dock"),
            name: "Kettle Dock".into(),
            region: RegionId::from_static("region.lantern_shallows"),
            position: Vec2::ZERO,
            dock_radius: 50.0,
            owner: None,
            services: Default::default(),
            build: BuildState::default(),
        };
        let stage_of = |r: &StationRecipe| {
            r.parts.iter().find_map(|p| match p.kind {
                render::kit::PartKind::Greenhouse { stage, .. } => Some(stage),
                _ => None,
            })
        };
        assert_eq!(stage_of(&station_recipe(&s)), Some(0));
        s.build.stage = 2;
        assert_eq!(stage_of(&station_recipe(&s)), Some(2));
        s.build.stage = 9;
        assert_eq!(stage_of(&station_recipe(&s)), Some(3));
    }
}
