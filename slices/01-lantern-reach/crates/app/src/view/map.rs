//! The map (M): regions, lanes, stations, faction colours and borders, and the prices
//! the player knows with their age in words. Drawn from `galaxy_economy::MapView`.

use galaxy_contract::prelude::*;
use galaxy_economy::{Control, MapView, PriceKnowledge};
use render::glam::{Mat4, Vec2 as G2, Vec3};
use render::{Align, Camera, Glow, Line, MeshInstance, Panel, Rect, Scene, Text, palette};

use super::{View, g, recipes};
use crate::trades::credits;

/// The camera that fits every region on screen.
pub fn map_camera(map: &MapView, viewport: G2) -> Camera {
    let (mut lo, mut hi) = (G2::splat(f32::MAX), G2::splat(f32::MIN));
    for r in &map.regions {
        let c = g(r.center);
        lo = lo.min(c - G2::splat(r.radius));
        hi = hi.max(c + G2::splat(r.radius));
    }
    if map.regions.is_empty() {
        (lo, hi) = (G2::splat(-1000.0), G2::splat(1000.0));
    }
    let size = hi - lo;
    let aspect = viewport.x / viewport.y.max(1.0);
    let height = size.y.max(size.x / aspect) * 1.12;
    // A little room at the top for the title.
    Camera::top_down((lo + hi) * 0.5 + G2::new(0.0, height * 0.03), height)
}

fn control_color(c: &Control) -> render::Color {
    match c {
        Control::Held { faction } => recipes::faction_color(faction),
        Control::Contested { .. } => palette::BLOSSOM,
        Control::Unclaimed => palette::CREAM,
    }
}

fn ware_name(world: &WorldState, ware: &WareId) -> String {
    world
        .wares
        .get(ware)
        .map_or_else(|| ware.to_string(), |w| w.name.clone())
}

/// The price rows for one station, and the line under them (the age, or nothing).
pub fn price_rows(
    world: &WorldState,
    knowledge: &PriceKnowledge,
    station: &StationId,
) -> (Vec<(String, String)>, Option<String>) {
    let row = |p: &galaxy_economy::scout::KnownPrice| {
        let verb = match p.role {
            MarketRole::Buys => "pays",
            MarketRole::Sells => "asks",
        };
        (
            ware_name(world, &p.ware),
            format!("{verb} {}", credits(i64::from(p.price))),
        )
    };
    match knowledge {
        PriceKnowledge::NoMarket => (Vec::new(), None),
        PriceKnowledge::Unknown => {
            let rows = world
                .markets
                .get(station)
                .map(|m| {
                    m.lines
                        .keys()
                        .map(|w| (ware_name(world, w), "?".to_owned()))
                        .collect()
                })
                .unwrap_or_default();
            (rows, Some("not visited yet".into()))
        }
        PriceKnowledge::Live { prices } => (prices.iter().map(row).collect(), Some("live".into())),
        PriceKnowledge::Seen { prices, label, .. } => {
            (prices.iter().map(row).collect(), Some(label.clone()))
        }
    }
}

fn overlap(a: &Rect, b: &Rect) -> f32 {
    let w = (a.x + a.w).min(b.x + b.w) - a.x.max(b.x);
    let h = (a.y + a.h).min(b.y + b.h) - a.y.max(b.y);
    w.max(0.0) * h.max(0.0)
}

/// A spot for a `size` panel next to the point `at`, clear of everything in `placed`
/// and on screen if it can be: the preferred side first, then the others.
pub fn place(placed: &[Rect], at: G2, size: G2, prefer_left: bool, viewport: G2) -> Rect {
    let gap = 16.0;
    let right = Rect::new(at.x + gap, at.y - size.y * 0.5, size.x, size.y);
    let left = Rect::new(at.x - gap - size.x, at.y - size.y * 0.5, size.x, size.y);
    let below = Rect::new(at.x - size.x * 0.5, at.y + gap, size.x, size.y);
    let above = Rect::new(at.x - size.x * 0.5, at.y - gap - size.y, size.x, size.y);
    let shifted = |r: Rect, dy: f32| Rect { y: r.y + dy, ..r };
    let (first, second) = if prefer_left {
        (left, right)
    } else {
        (right, left)
    };
    let candidates = [
        first,
        second,
        below,
        above,
        shifted(first, size.y * 0.5 - gap),
        shifted(first, -size.y * 0.5 + gap),
        shifted(second, size.y * 0.5 - gap),
        shifted(second, -size.y * 0.5 + gap),
    ];
    let screen = Rect::new(0.0, 0.0, viewport.x, viewport.y);
    let cost = |r: &Rect| {
        let off = r.w * r.h - overlap(r, &screen);
        placed.iter().map(|p| overlap(r, p)).sum::<f32>() + off * 2.0
    };
    candidates
        .iter()
        .copied()
        .min_by(|a, b| cost(a).total_cmp(&cost(b)))
        .unwrap_or(first)
}

impl View {
    /// The map scene. `player_heading` turns the player's marker.
    pub fn map_scene(&self, world: &WorldState, time: f32, viewport: G2) -> Scene {
        let map = MapView::build(world);
        let camera = map_camera(&map, viewport);
        let mut scene = Scene::new(camera);
        scene.time = time;
        scene.backdrop = self.backdrop_at(world, Vec2::new(-1.0e9, -1.0e9));
        let wpp = camera.world_per_pixel(viewport);
        let screen = |p: G2| camera.world_to_screen(p.extend(0.0), viewport);

        // Screen space already taken: the goal line, the hold, the back button, then each
        // label and panel as it's placed.
        let mut placed = vec![
            Rect::new(viewport.x * 0.5 - 260.0, 0.0, 520.0, 84.0),
            Rect::new(viewport.x - 240.0, 0.0, 240.0, 140.0),
            Rect::new(viewport.x - 190.0, viewport.y - 74.0, 190.0, 74.0),
            Rect::new(0.0, 0.0, 330.0, 70.0),
        ];
        for s in &map.stations {
            if let Some(p) = screen(g(s.position)) {
                placed.push(Rect::new(p.x - 10.0, p.y - 10.0, 20.0, 20.0));
            }
        }
        for lane in &map.lanes {
            let (a, b) = (screen(g(lane.a_gate)), screen(g(lane.b_gate)));
            if let (Some(a), Some(b)) = (a, b) {
                let n = (a.distance(b) / 24.0).ceil().max(1.0) as usize;
                for k in 0..=n {
                    let p = a.lerp(b, k as f32 / n as f32);
                    placed.push(Rect::new(p.x - 6.0, p.y - 6.0, 12.0, 12.0));
                }
            }
        }
        if let Some(p) = map.player.as_ref().and_then(|m| screen(g(m.position))) {
            placed.push(Rect::new(p.x - 22.0, p.y - 22.0, 44.0, 44.0));
        }

        // Regions: a soft wash of the holder's colour and an inked edge.
        for r in &map.regions {
            let c = g(r.center);
            let color = control_color(&r.control);
            scene.glows.push(Glow {
                position: c.extend(-40.0),
                radius: r.radius * 1.05,
                color: color.with_alpha(0.16),
            });
            let mut edge =
                Line::circle(c.extend(-30.0), r.radius, 3.5 * wpp, color.with_alpha(0.8))
                    .wobbly(10.0);
            if matches!(r.control, Control::Contested { .. } | Control::Unclaimed) {
                edge = edge.dashed(60.0, 40.0);
            }
            scene.lines.push(edge);
            if let Some(p) = screen(c - G2::new(0.0, r.radius * 0.8)) {
                let holder = r
                    .control
                    .holder()
                    .and_then(|f| world.factions.get(f))
                    .map(|f| f.name.clone())
                    .unwrap_or_else(|| match r.control {
                        Control::Contested { .. } => "contested".into(),
                        _ => "unclaimed".into(),
                    });
                let w = crate::ui::text_width(&r.name, 22.0);
                placed.push(Rect::new(p.x - w * 0.5, p.y - 4.0, w, 44.0));
                scene.overlay.texts.push(
                    Text::new(r.name.clone(), p + G2::new(0.0, -4.0), 22.0)
                        .weight(700)
                        .color(palette::CREAM)
                        .align(Align::Center),
                );
                scene.overlay.texts.push(
                    Text::new(holder, p + G2::new(0.0, 22.0), 14.0)
                        .weight(500)
                        .color(color)
                        .align(Align::Center),
                );
            }
        }

        // Lanes and their mouths; a border tick where the two ends are held apart.
        for lane in &map.lanes {
            let (a, b) = (g(lane.a_gate), g(lane.b_gate));
            scene.lines.push(
                Line::path(
                    vec![a.extend(-10.0), b.extend(-10.0)],
                    4.0 * wpp,
                    palette::MUSTARD.with_alpha(0.9),
                )
                .dashed(14.0 * wpp, 9.0 * wpp)
                .wobbly(2.0 * wpp),
            );
            for gate in [a, b] {
                scene.glows.push(Glow {
                    position: gate.extend(0.0),
                    radius: 9.0 * wpp,
                    color: palette::LANTERN.with_alpha(0.9),
                });
            }
        }
        for border in &map.borders {
            let Some(lane) = map.lanes.iter().find(|l| l.id == border.lane) else {
                continue;
            };
            let (a, b) = (g(lane.a_gate), g(lane.b_gate));
            let mid = a.lerp(b, 0.5);
            let n = (b - a).perp().normalize_or_zero() * 14.0 * wpp;
            scene.lines.push(Line::path(
                vec![(mid - n).extend(-5.0), (mid + n).extend(-5.0)],
                5.0 * wpp,
                palette::CORAL,
            ));
        }

        // Stations and what the player knows of their prices.
        for s in &map.stations {
            let pos = g(s.position);
            let color = s
                .owner
                .as_ref()
                .map_or(palette::CREAM, recipes::faction_color);
            scene.glows.push(Glow {
                position: pos.extend(0.0),
                radius: 12.0 * wpp,
                color: color.with_alpha(0.95),
            });
            scene.lines.push(Line::circle(
                pos.extend(1.0),
                8.0 * wpp,
                2.5 * wpp,
                palette::INK,
            ));
            let Some(p) = screen(pos) else { continue };
            let region_center = map.region(&s.region).map_or(pos, |r| g(r.center));
            let left = pos.x < region_center.x - 1.0;
            let (rows, foot) = price_rows(world, &s.prices, &s.id);
            let w = 200.0;
            let h = 32.0 + rows.len() as f32 * 17.0 + if foot.is_some() { 20.0 } else { 0.0 };
            let rect = place(&placed, p, G2::new(w, h), left, viewport);
            placed.push(rect);
            let x = rect.x;
            scene.overlay.panels.push(Panel {
                fill: palette::PARCHMENT.with_alpha(0.94),
                radius: 10.0,
                border_px: 2.5,
                ..Panel::new(rect)
            });
            scene
                .overlay
                .texts
                .push(Text::new(s.name.clone(), G2::new(x + 12.0, rect.y + 8.0), 16.0).weight(700));
            for (i, (ware, price)) in rows.iter().enumerate() {
                let y = rect.y + 30.0 + i as f32 * 17.0;
                scene
                    .overlay
                    .texts
                    .push(Text::new(ware.clone(), G2::new(x + 12.0, y), 14.0).weight(500));
                scene.overlay.texts.push(
                    Text::new(price.clone(), G2::new(x + w - 12.0, y), 14.0)
                        .weight(600)
                        .align(Align::Right),
                );
            }
            if let Some(foot) = foot {
                scene.overlay.texts.push(
                    Text::new(foot, G2::new(x + 12.0, rect.y + h - 22.0), 13.0)
                        .weight(400)
                        .color(palette::TERRACOTTA),
                );
            }
        }

        // The player, a little Wren that's easy to find.
        if let Some(me) = &map.player
            && let Some(ship) = world.ships.get(&me.ship)
            && let Some(&mesh) = self.hulls.get(&(ship.hull.clone(), None))
        {
            let pos = g(me.position);
            scene.glows.push(Glow {
                position: pos.extend(0.0),
                radius: 22.0 * wpp,
                color: palette::CORAL.with_alpha(0.6 + 0.2 * (time * 2.0).sin()),
            });
            let scale = 4.0 * wpp;
            let mut inst = MeshInstance::at(mesh, pos, ship.heading, scale);
            inst.transform = Mat4::from_translation(Vec3::Z * 10.0 * wpp) * inst.transform;
            scene.meshes.push(inst);
        }
        scene
    }
}
