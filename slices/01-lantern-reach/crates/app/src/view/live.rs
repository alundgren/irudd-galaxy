//! Runtime borders, bearings, claim buoys and rock lamps.
use super::{g, recipes};
use galaxy_contract::prelude::*;
use render::{Glow, Line, Scene, palette};
pub fn draw(scene: &mut Scene, world: &WorldState) {
    for (id, overlay) in &world.map_overlays {
        let color = overlay
            .owner
            .as_ref()
            .map_or(palette::MUSTARD, recipes::faction_color)
            .with_alpha(0.5);
        let mut at = g(overlay.center).extend(-3.0);
        if id == "gm.ancients_bearing.song"
            && let Some(ship) = world
                .player
                .ship
                .as_ref()
                .and_then(|id| world.ships.get(id))
                .filter(|s| s.hold.count(&"ware.ancients_fragment".parse().unwrap()) > 0)
        {
            at = g(ship.position).extend(-3.0);
        }
        if let Some(d) = overlay.direction {
            let d = if id == "gm.ancients_bearing.song" {
                world
                    .stations
                    .get("station.brassmouth_exchange")
                    .map_or(d, |s| s.position - Vec2::new(at.x, at.y))
            } else {
                d
            };
            let end = at + g(d).normalize_or_zero().extend(0.0) * overlay.radius;
            scene
                .lines
                .push(Line::path(vec![at, end], 3.0, color).dashed(18.0, 12.0));
        } else {
            let spread = if id == "gm.territory_map.hook" && !overlay.contested {
                world
                    .talk
                    .on(galaxy_contract::talk::Channel::MapNote)
                    .filter(|l| {
                        l.line
                            .speaker
                            .as_ref()
                            .is_some_and(|n| n.as_str() == "npc.hesper_lune")
                            && l.line
                                .place
                                .as_ref()
                                .is_some_and(|s| s.as_str() == "station.cinder_hook")
                    })
                    .last()
                    .map_or(1.0, |l| {
                        (world.meta.tick.0.saturating_sub(l.tick.0) as f32 / 90.0).clamp(0.1, 1.0)
                    })
            } else {
                1.0
            };
            let line = Line::circle(at, overlay.radius * spread, 5.0, color);
            if overlay.contested {
                let along = render::glam::Vec3::new(1.0, 1.0, 0.0).normalize();
                let across = render::glam::Vec3::new(-along.y, along.x, 0.0);
                for n in -5..=5 {
                    let offset = n as f32 * overlay.radius / 6.0;
                    let half = (overlay.radius * overlay.radius - offset * offset).sqrt();
                    let center = at + across * offset;
                    scene.lines.push(Line::path(
                        vec![center - along * half, center + along * half],
                        2.0,
                        color.with_alpha(0.16),
                    ));
                }
            }
            scene.lines.push(if overlay.contested {
                line.dashed(20.0, 14.0)
            } else {
                line
            });
        }
    }
    for buoy in world.survey_buoys.values() {
        let at = g(buoy.position).extend(2.0);
        scene
            .lines
            .push(Line::circle(at, 12.0, 3.0, palette::MUSTARD));
        scene.glows.push(Glow {
            position: at,
            radius: 24.0,
            color: palette::MUSTARD.with_alpha(0.5),
        });
    }
    if !world.rock_tags.is_empty() {
        for id in &world.rock_tags {
            if let Some(r) = world.rocks.get(id) {
                let at = g(r.position).extend(4.0);
                scene
                    .lines
                    .push(Line::circle(at, 18.0, 3.0, palette::LANTERN));
                scene.glows.push(Glow {
                    position: at,
                    radius: 25.0,
                    color: palette::LANTERN.with_alpha(0.5),
                });
            }
        }
    }
}
