//! Station parts drawn as modules (issue #47, 3.1): each part is a kit module bolted
//! onto its anchor on the station's recipe, with a strut back to the station. A closed
//! or building part wears a scaffold. Hovered at the dock, a module lifts a little and
//! gets an orange outline; its sign is UI ([`crate::ui::station`]).
//!
//! Everything here is pure geometry over the world, so the UI can hit-test modules and
//! talk bubbles can rise from them without a renderer.

use galaxy_contract::parts::{PartState, StationPart};
use galaxy_contract::prelude::*;
use render::glam::{Mat4, Vec2 as G2, Vec3};
use render::kit::{self, StationRecipe};
use render::{Line, Scene};

use super::station_transform;

/// How far a hovered module lifts, in world units, and how much bigger it draws.
pub const LIFT: f32 = 10.0;
pub const LIFT_SCALE: f32 = 1.08;
/// A module's reach from its anchor, in recipe units, before level.
const MODULE_REACH: f32 = 5.0;
/// How far a station's own art reaches around its centre, in recipe units: a module's
/// strut reaches back to it.
const CORE: f32 = 12.0;

/// The orange of a hovered module's outline (the concept's hover accent).
pub use crate::ui::talk::ORANGE;

/// Where `part`'s anchor is: module space to world, the station's scale, and the
/// anchor's distance from the station's centre in recipe units.
fn anchored(world: &WorldState, part: &StationPart) -> Option<(Mat4, f32, f32)> {
    let station = world.stations.get(&part.station)?;
    let recipe: StationRecipe = galaxy_core::parts::recipe_of(world, station);
    let anchor = kit::anchors(part.station.as_str(), &recipe)
        .into_iter()
        .find(|a| a.name == part.anchor)?;
    let to_world = station_transform(station, &recipe);
    Some((
        to_world * anchor.at.matrix(),
        to_world.x_axis.truncate().length(),
        anchor.at.distance,
    ))
}

/// The strut from a module back to its station's art, in recipe units.
fn strut(distance: f32) -> f32 {
    (distance - CORE).clamp(3.0, 16.0)
}

/// Module space to world for `part`: its station's transform times its anchor.
pub fn module_transform(world: &WorldState, part: &StationPart) -> Option<Mat4> {
    anchored(world, part).map(|(m, ..)| m)
}

/// Where `part`'s module sits on the plane, and about how far it reaches, in world
/// units. `None` if its station or anchor is gone.
pub fn module_point(world: &WorldState, part: &StationPart) -> Option<(G2, f32)> {
    let (m, scale, _) = anchored(world, part)?;
    let grow = 1.0 + 0.12 * part.level.saturating_sub(1).min(4) as f32;
    Some((m.w_axis.truncate().truncate(), MODULE_REACH * grow * scale))
}

/// Where a talk bubble from `part`'s keeper rises: the module's top.
pub fn bubble_point(world: &WorldState, part: &StationPart) -> Option<Vec2> {
    let (p, _) = module_point(world, part)?;
    Some(Vec2::new(p.x, p.y))
}

/// The mesh key: what changes a module's look.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct ModuleKey {
    pub part: StationPartId,
    pub module: String,
    pub level: u32,
    pub scaffold: bool,
    pub stage: u32,
    pub anchor: String,
}

impl ModuleKey {
    pub(super) fn of(world: &WorldState, part: &StationPart) -> Self {
        Self {
            part: part.id.clone(),
            module: part.module.clone(),
            level: part.level,
            scaffold: part.state != PartState::Open,
            stage: world
                .stations
                .get(&part.station)
                .map_or(0, |s| s.build.stage),
            anchor: part.anchor.clone(),
        }
    }
}

/// The module's mesh in its station's paint.
pub(super) fn mesh(world: &WorldState, part: &StationPart) -> Option<render::MeshData> {
    let station = world.stations.get(&part.station)?;
    let recipe = galaxy_core::parts::recipe_of(world, station);
    let kind = kit::module_part(&part.module, part.level)?;
    let (.., distance) = anchored(world, part)?;
    Some(kit::module(
        &kind,
        recipe.palette,
        recipe.faction.clone(),
        strut(distance),
        part.state != PartState::Open,
    ))
}

/// The hovered module's outline: an orange ring around it, a little above the deck.
pub fn outline(scene: &mut Scene, world: &WorldState, part: &StationPart) {
    let Some((p, r)) = module_point(world, part) else {
        return;
    };
    scene
        .lines
        .push(Line::circle(p.extend(LIFT + 4.0), r * 1.35, 4.0, ORANGE));
}

/// The transform a module draws with, lifted when hovered.
pub fn draw_transform(world: &WorldState, part: &StationPart, hovered: bool) -> Option<Mat4> {
    let m = module_transform(world, part)?;
    if !hovered {
        return Some(m);
    }
    Some(Mat4::from_translation(Vec3::Z * LIFT) * m * Mat4::from_scale(Vec3::splat(LIFT_SCALE)))
}

#[cfg(test)]
mod tests {
    use super::super::g;
    use super::*;

    #[test]
    fn every_authored_part_has_a_module_outside_its_station_core() {
        let dir =
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../packs/core/data");
        let state = galaxy_core::new_game(&dir, 1).unwrap();
        let w = &state.world;
        for part in w.station_parts.values() {
            let (p, r) = module_point(w, part).expect("anchored");
            let st = w.stations.get(&part.station).unwrap();
            let d = p.distance(g(st.position));
            assert!(r > 5.0, "{}: reach {r}", part.id);
            assert!(
                d < st.dock_radius * 4.0,
                "{}: {d} from its station",
                part.id
            );
            assert!(mesh(w, part).is_some(), "{}", part.id);
        }
        // Two parts of one station never share a spot.
        for a in w.station_parts.values() {
            for b in w.station_parts.values() {
                if a.id < b.id && a.station == b.station {
                    let (pa, ra) = module_point(w, a).unwrap();
                    let (pb, _) = module_point(w, b).unwrap();
                    assert!(pa.distance(pb) > ra, "{} and {} overlap", a.id, b.id);
                }
            }
        }
    }
}
