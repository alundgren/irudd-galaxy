//! Which kit recipe draws which station, which hull mesh a hull uses, which rock kind a
//! rock type is, and each region's backdrop.

use galaxy_contract::prelude::*;
use render::kit::{BackdropParams, HullColors, HullParams, RockKind, StationRecipe};
use render::{ArtBible, Color, palette};

/// The recipe for a station at its current build stage. Unknown stations (the game
/// master's sixth) get a small generic recipe until they carry their own.
pub fn station_recipe(station: &Station) -> StationRecipe {
    let stage = u8::try_from(station.build.stage.min(3)).unwrap_or(3);
    let mut recipe = StationRecipe::for_station(station.id.as_str(), stage);
    recipe.name = station.name.clone();
    recipe
}

/// The mesh parameters for a hull.
pub fn hull_params(hull: &HullId) -> HullParams {
    match hull.as_str() {
        "hull.tandem" | "hull.compact_barge" => HullParams::tandem(),
        "hull.heron" | "hull.lamp_launch" => HullParams::heron(),
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
        "hull.wren" | "hull.coop_skiff" => HullParams::wren(),
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
/// The faction mark rides on the hull as a sigil and the pennant stays.
pub fn npc_hull_params(hull: &HullId, faction: Option<&FactionId>) -> HullParams {
    let mut p = hull_params(hull);
    match faction.and_then(|id| ArtBible::bundled().faction(id.as_str())) {
        Some(style) => {
            p.colors.body = style.primary;
            p.mark = style.mark.clone();
        }
        None => p.colors.body = palette::SAGE,
    }
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

/// A faction's colour on the map and on its craft, from the art bible.
pub fn faction_color(faction: &FactionId) -> Color {
    ArtBible::bundled()
        .faction(faction.as_str())
        .map(|style| style.primary)
        .unwrap_or(palette::SAGE)
}

/// Each region's sky, from the art bible. Region tint stays on the backdrop.
pub fn backdrop(region: Option<&RegionId>) -> BackdropParams {
    let base = BackdropParams::default();
    let Some(id) = region else {
        return base;
    };
    let Some(style) = ArtBible::bundled().region(id.as_str()) else {
        return base;
    };
    BackdropParams {
        top: style.sky_top,
        bottom: style.sky_bottom,
        cloud_colors: style.clouds.clone(),
        cloud_alpha: style.cloud_alpha,
        ..base
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
