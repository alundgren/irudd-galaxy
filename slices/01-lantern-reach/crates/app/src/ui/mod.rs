//! The screen's panels, minimal and warm: one goal line, the hold and credits, a hint
//! for what Space does when something is in reach, the dock screen, the talk panel and
//! the PAUSED card. No timers, no progress bars; text is important or absent.
//!
//! Layout is fixed logical pixels from the viewport, so it needs no renderer and can
//! be tested headless. Each function adds to an [`Overlay`] and returns the buttons it
//! drew, for the window's mouse hit test.

pub mod dock;

use galaxy_contract::prelude::*;
use render::glam::Vec2 as G2;
use render::{Align, Camera, Color, Overlay, Panel, PausedCard, Rect, Text, palette};

use crate::hooks;
use crate::trades::credits;
use crate::view::g;
pub use dock::{Amount, DockLine, dock_lines, trade_request};

/// The dock screen's width in logical pixels (at most; it shrinks on narrow windows).
pub const DOCK_PANEL_WIDTH: f32 = 600.0;

/// What the screen is showing besides the world.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct UiState {
    pub map_open: bool,
    /// The selected dock line.
    pub selected: usize,
    /// The mouse, in logical pixels.
    pub cursor: Option<G2>,
}

/// What a button does.
#[derive(Clone, Debug, PartialEq)]
pub enum UiAction {
    Trade { line: usize, amount: Amount },
    Select(usize),
    Undock,
    CloseMap,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Button {
    pub rect: Rect,
    pub action: UiAction,
}

fn contains(r: &Rect, p: G2) -> bool {
    p.x >= r.x && p.x <= r.x + r.w && p.y >= r.y && p.y <= r.y + r.h
}

/// The button under `p`, topmost first.
pub fn hit(buttons: &[Button], p: G2) -> Option<&UiAction> {
    buttons
        .iter()
        .rev()
        .find(|b| contains(&b.rect, p))
        .map(|b| &b.action)
}

/// A rough text width for layout, so panels fit their text without a renderer.
pub fn text_width(text: &str, size: f32) -> f32 {
    text.chars().count() as f32 * size * 0.52
}

fn button(
    o: &mut Overlay,
    ui: &UiState,
    rect: Rect,
    label: &str,
    enabled: bool,
    action: UiAction,
) -> Button {
    let hover = ui.cursor.is_some_and(|c| contains(&rect, c));
    let fill = if !enabled {
        palette::PARCHMENT.lerp(palette::INK, 0.12)
    } else if hover {
        palette::LANTERN
    } else {
        palette::MUSTARD
    };
    o.panels.push(Panel {
        radius: 8.0,
        fill,
        border_px: 2.0,
        shadow: false,
        ..Panel::new(rect)
    });
    o.texts.push(
        Text::new(
            label,
            G2::new(rect.center().x, rect.y + rect.h * 0.5 - 9.0),
            15.0,
        )
        .weight(600)
        .color(if enabled {
            palette::INK
        } else {
            palette::INK.with_alpha(0.45)
        })
        .align(Align::Center),
    );
    Button { rect, action }
}

/// The goal line, centred at the top. Nothing when there's no goal.
pub fn goal(o: &mut Overlay, world: &WorldState, viewport: G2) {
    let Some(line) = hooks::goal_line(world) else {
        return;
    };
    let size = 24.0;
    let w = text_width(&line, size) + 56.0;
    let rect = Rect::new((viewport.x - w) * 0.5, 18.0, w, 54.0);
    o.panels.push(Panel::new(rect));
    o.texts.push(
        Text::new(line, G2::new(viewport.x * 0.5, 31.0), size)
            .weight(600)
            .align(Align::Center),
    );
}

/// Credits and the hold, top right.
pub fn hold(o: &mut Overlay, world: &WorldState, viewport: G2) {
    let Some(ship) = world.player.ship.as_ref().and_then(|s| world.ships.get(s)) else {
        return;
    };
    let cap = world.hulls.get(&ship.hull).map_or(0, |h| h.hold_capacity);
    let wares: Vec<String> = ship
        .hold
        .cargo
        .iter()
        .filter(|(_, n)| **n > 0)
        .map(|(w, n)| {
            let name = world.wares.get(w).map_or(w.as_str(), |w| w.name.as_str());
            format!("{name} {n}")
        })
        .collect();
    let w = 200.0;
    let h = 78.0 + wares.len() as f32 * 20.0;
    let rect = Rect::new(viewport.x - w - 18.0, 18.0, w, h);
    o.panels.push(Panel {
        fill: palette::INK.with_alpha(0.88),
        border: palette::MUSTARD,
        ..Panel::new(rect)
    });
    let x = rect.x + 18.0;
    o.texts.push(
        Text::new(
            credits(world.player.credits),
            G2::new(x, rect.y + 10.0),
            24.0,
        )
        .weight(700)
        .color(palette::LANTERN),
    );
    o.texts.push(
        Text::new(
            format!("Hold {} / {cap}", ship.hold.total()),
            G2::new(x, rect.y + 44.0),
            18.0,
        )
        .weight(600)
        .color(palette::CREAM),
    );
    for (i, line) in wares.into_iter().enumerate() {
        o.texts.push(
            Text::new(
                line,
                G2::new(x + 6.0, rect.y + 68.0 + i as f32 * 20.0),
                16.0,
            )
            .weight(500)
            .color(palette::CREAM.with_alpha(0.85)),
        );
    }
}

/// What Space would do right now, if anything.
pub fn context_hint(world: &WorldState) -> Option<String> {
    let ship = world
        .player
        .ship
        .as_ref()
        .and_then(|s| world.ships.get(s))?;
    if ship.docked_at.is_some() || ship.activity.transit.is_some() {
        return None;
    }
    if let Some(station) = galaxy_ship::space::ring_at(world, &ship.region, ship.position) {
        return Some(format!("Space  ·  dock at {}", station.name));
    }
    if let Some(way) = galaxy_ship::space::mouth_at(world, &ship.region, ship.position) {
        let to = world
            .regions
            .get(&way.to)
            .map_or_else(|| way.to.to_string(), |r| r.name.clone());
        return Some(format!("Space  ·  ride the lantern current to {to}"));
    }
    if ship.activity.mining.is_some() {
        return None;
    }
    let rock = galaxy_ship::space::rock_in_reach(world, &ship.region, ship.position)?;
    let cap = world
        .hulls
        .get(&ship.hull)
        .map_or(0, |h| u64::from(h.hold_capacity));
    if ship.hold.total() >= cap {
        return Some("Your hold is full".into());
    }
    let ware = world
        .rock_types
        .get(&rock.rock_type)
        .and_then(|t| world.wares.get(&t.ware))
        .map_or_else(|| "it".to_owned(), |w| w.name.to_lowercase());
    Some(format!("Hold Space  ·  mine {ware}"))
}

/// The Space hint, low in the middle of the screen.
pub fn hint(o: &mut Overlay, world: &WorldState, viewport: G2) {
    let Some(text) = context_hint(world) else {
        return;
    };
    let size = 19.0;
    let w = text_width(&text, size) + 40.0;
    let rect = Rect::new((viewport.x - w) * 0.5, viewport.y - 84.0, w, 44.0);
    o.panels.push(Panel {
        fill: palette::INK.with_alpha(0.85),
        border: palette::LANTERN,
        radius: 12.0,
        ..Panel::new(rect)
    });
    o.texts.push(
        Text::new(text, G2::new(viewport.x * 0.5, rect.y + 10.0), size)
            .weight(600)
            .color(palette::CREAM)
            .align(Align::Center),
    );
}

/// Each station's name under its ring, when it's on screen and you aren't docked there.
pub fn station_labels(o: &mut Overlay, world: &WorldState, camera: &Camera, viewport: G2) {
    let docked = world
        .player
        .ship
        .as_ref()
        .and_then(|s| world.ships.get(s))
        .and_then(|s| s.docked_at.clone());
    for s in world.stations.values() {
        if docked.as_ref() == Some(&s.id) {
            continue;
        }
        let below = g(s.position) - G2::new(0.0, s.dock_radius * 1.15);
        let Some(p) = camera.world_to_screen(below.extend(0.0), viewport) else {
            continue;
        };
        if p.x < -100.0 || p.y < -20.0 || p.x > viewport.x + 100.0 || p.y > viewport.y + 20.0 {
            continue;
        }
        o.texts.push(
            Text::new(s.name.clone(), p, 17.0)
                .weight(600)
                .color(palette::CREAM.with_alpha(0.9))
                .align(Align::Center),
        );
    }
}

/// Small markers at the screen edge for stations out of view in the region you're in,
/// and for the goal's station wherever it is.
pub fn edge_markers(o: &mut Overlay, world: &WorldState, camera: &Camera, viewport: G2) {
    let Some(ship) = world.player.ship.as_ref().and_then(|s| world.ships.get(s)) else {
        return;
    };
    let goal = hooks::goal_place(world);
    let inset = 34.0;
    for s in world.stations.values() {
        let is_goal = goal.as_ref() == Some(&s.id);
        if s.region != ship.region && !is_goal {
            continue;
        }
        let Some(p) = camera.world_to_screen(g(s.position).extend(0.0), viewport) else {
            continue;
        };
        // Any part of the station on screen counts as seen.
        let r = s.dock_radius * 1.6 / camera.world_per_pixel(viewport).max(1e-3);
        let on_screen = p.x > -r && p.y > -r && p.x < viewport.x + r && p.y < viewport.y + r;
        if on_screen {
            continue;
        }
        let mid = viewport * 0.5;
        let d = p - mid;
        let half = mid - G2::splat(inset);
        let k = (half.x / d.x.abs().max(1e-3)).min(half.y / d.y.abs().max(1e-3));
        let at = mid + d * k;
        let color = if is_goal {
            palette::MUSTARD
        } else {
            palette::CREAM.with_alpha(0.75)
        };
        o.panels.push(Panel {
            fill: color,
            border_px: 2.0,
            radius: 7.0,
            shadow: false,
            ..Panel::new(Rect::new(at.x - 7.0, at.y - 7.0, 14.0, 14.0))
        });
        let size = if is_goal { 17.0 } else { 15.0 };
        let w = text_width(&s.name, size);
        let (align, tx) = if d.x > 0.0 {
            (Align::Right, at.x - 14.0)
        } else {
            (Align::Left, at.x + 14.0)
        };
        let ty = (at.y - size * 0.6).clamp(6.0, viewport.y - size - 6.0);
        let _ = w;
        o.texts.push(
            Text::new(s.name.clone(), G2::new(tx, ty), size)
                .weight(if is_goal { 700 } else { 500 })
                .color(color)
                .align(align),
        );
    }
}

/// The latest talk lines, bottom left, each naming its speaker. Nothing when nobody
/// has spoken.
pub fn talk(o: &mut Overlay, world: &WorldState, viewport: G2) {
    let lines: Vec<_> = hooks::talk_lines(world, 3)
        .into_iter()
        .filter(|l| hooks::is_talk(l) && l.validate().is_ok())
        .collect();
    if lines.is_empty() {
        return;
    }
    let w = 440.0;
    let row = 46.0;
    let h = 20.0 + lines.len() as f32 * row;
    let rect = Rect::new(18.0, viewport.y - h - 18.0, w, h);
    o.panels.push(Panel {
        fill: palette::PARCHMENT.with_alpha(0.95),
        ..Panel::new(rect)
    });
    for (i, line) in lines.iter().enumerate() {
        let y = rect.y + 12.0 + i as f32 * row;
        let who = hooks::speaker(world, line).unwrap_or_else(|| "Someone".into());
        o.texts.push(
            Text::new(who, G2::new(rect.x + 16.0, y), 14.0)
                .weight(700)
                .color(palette::TERRACOTTA),
        );
        o.texts.push(
            Text::new(line.text.clone(), G2::new(rect.x + 16.0, y + 17.0), 16.0)
                .weight(500)
                .wrap(w - 32.0),
        );
    }
}

/// The dock screen: the station's name, its market lines (price per unit, stock, your
/// hold, buy or sell), any refusal in one calm line, the station's other services, and
/// Undock.
pub fn dock_panel(
    o: &mut Overlay,
    world: &WorldState,
    station: &StationId,
    ui: &UiState,
    notice: Option<&str>,
    viewport: G2,
) -> Vec<Button> {
    let mut buttons = Vec::new();
    let Some(st) = world.stations.get(station) else {
        return buttons;
    };
    let lines = dock_lines(world, station);
    let services = hooks::station_services(world, station);
    let w = DOCK_PANEL_WIDTH.min(viewport.x - 40.0);
    let row = 44.0;
    let table_h = if lines.is_empty() {
        30.0
    } else {
        26.0 + lines.len() as f32 * row
    };
    let notice_h = if notice.is_some() { 34.0 } else { 0.0 };
    let services_h = if services.is_empty() { 0.0 } else { 58.0 };
    let h = 74.0 + table_h + notice_h + services_h + 70.0;
    let x0 = viewport.x - w - 18.0;
    let y0 = (viewport.y - h)
        .clamp(110.0, 140.0)
        .min(viewport.y - h - 10.0)
        .max(90.0);
    o.panels.push(Panel {
        radius: 16.0,
        border_px: 3.5,
        ..Panel::new(Rect::new(x0, y0, w, h))
    });
    // A coloured band under the title in the owner's colour.
    let owner_color = st
        .owner
        .as_ref()
        .map_or(palette::TERRACOTTA, crate::view::recipes::faction_color);
    o.panels.push(Panel {
        fill: owner_color,
        border_px: 0.0,
        radius: 3.0,
        shadow: false,
        ..Panel::new(Rect::new(x0 + 24.0, y0 + 58.0, w - 48.0, 4.0))
    });
    o.texts
        .push(Text::new(st.name.clone(), G2::new(x0 + 24.0, y0 + 16.0), 30.0).weight(700));
    if let Some(f) = st.owner.as_ref().and_then(|f| world.factions.get(f)) {
        o.texts.push(
            Text::new(f.name.clone(), G2::new(x0 + w - 24.0, y0 + 28.0), 15.0)
                .weight(600)
                .color(palette::TERRACOTTA)
                .align(Align::Right),
        );
    }

    let col_ware = x0 + 24.0;
    let col_price = x0 + 190.0;
    let col_stock = x0 + 330.0;
    let col_hold = x0 + 386.0;
    let col_btn = x0 + w - 24.0 - 64.0 - 8.0 - 78.0;
    let mut y = y0 + 74.0;
    if lines.is_empty() {
        o.texts.push(
            Text::new("No market here.", G2::new(col_ware, y), 17.0)
                .weight(500)
                .color(palette::INK.with_alpha(0.7)),
        );
        y += 30.0;
    } else {
        let head = |o: &mut Overlay, t: &str, x: f32, align: Align| {
            o.texts.push(
                Text::new(t, G2::new(x, y), 13.0)
                    .weight(600)
                    .color(palette::TERRACOTTA)
                    .align(align),
            );
        };
        head(o, "WARE", col_ware, Align::Left);
        head(o, "PRICE EACH", col_price, Align::Left);
        head(o, "STOCK", col_stock + 30.0, Align::Right);
        head(o, "HOLD", col_hold + 30.0, Align::Right);
        y += 24.0;
        for (i, line) in lines.iter().enumerate() {
            let selected = i == ui.selected.min(lines.len() - 1);
            let row_rect = Rect::new(x0 + 12.0, y - 2.0, w - 24.0, row - 4.0);
            if selected {
                o.panels.push(Panel {
                    fill: palette::MUSTARD.with_alpha(0.28),
                    border: palette::MUSTARD,
                    border_px: 1.5,
                    radius: 9.0,
                    shadow: false,
                    ..Panel::new(row_rect)
                });
            }
            buttons.push(Button {
                rect: row_rect,
                action: UiAction::Select(i),
            });
            let ty = y + 9.0;
            o.texts
                .push(Text::new(line.name.clone(), G2::new(col_ware, ty), 18.0).weight(600));
            let (verb, color) = match line.side {
                TradeSide::Sell => ("pays", Color::hex(0x2f7a4a)),
                TradeSide::Buy => ("asks", palette::INK),
            };
            let price = line.unit.map_or_else(
                || "—".to_owned(),
                |c| format!("{verb} {}", credits(i64::from(c))),
            );
            o.texts.push(
                Text::new(price, G2::new(col_price, ty), 17.0)
                    .weight(600)
                    .color(color),
            );
            o.texts.push(
                Text::new(line.stock.to_string(), G2::new(col_stock + 30.0, ty), 17.0)
                    .weight(500)
                    .align(Align::Right),
            );
            o.texts.push(
                Text::new(line.in_hold.to_string(), G2::new(col_hold + 30.0, ty), 17.0)
                    .weight(if line.in_hold > 0 { 700 } else { 400 })
                    .align(Align::Right),
            );
            let can = match line.side {
                TradeSide::Sell => line.in_hold > 0 && line.unit.is_some(),
                TradeSide::Buy => line.stock > 0 && line.unit.is_some(),
            };
            let (one, most) = match line.side {
                TradeSide::Sell => ("Sell 1", "Sell all"),
                TradeSide::Buy => ("Buy 1", "Buy max"),
            };
            let by = y + 3.0;
            buttons.push(button(
                o,
                ui,
                Rect::new(col_btn, by, 64.0, 32.0),
                one,
                can,
                UiAction::Trade {
                    line: i,
                    amount: Amount::One,
                },
            ));
            buttons.push(button(
                o,
                ui,
                Rect::new(col_btn + 72.0, by, 78.0, 32.0),
                most,
                can,
                UiAction::Trade {
                    line: i,
                    amount: Amount::Most,
                },
            ));
            y += row;
        }
    }
    if let Some(n) = notice {
        o.texts.push(
            Text::new(n, G2::new(col_ware, y + 6.0), 16.0)
                .weight(600)
                .color(palette::TERRACOTTA)
                .wrap(w - 48.0),
        );
        y += notice_h;
    }
    if !services.is_empty() {
        y += 6.0;
        o.texts.push(
            Text::new("SERVICES", G2::new(col_ware, y), 13.0)
                .weight(600)
                .color(palette::TERRACOTTA),
        );
        let mut x = col_ware;
        for s in &services {
            let label = match &s.keeper {
                Some(k) => format!("{} · {k}", s.name),
                None => s.name.clone(),
            };
            let bw = text_width(&label, 15.0) + 26.0;
            o.panels.push(Panel {
                fill: if s.open {
                    palette::SAGE
                } else {
                    palette::PARCHMENT.lerp(palette::INK, 0.08)
                },
                border: palette::INK.with_alpha(0.5),
                border_px: 1.5,
                radius: 8.0,
                shadow: false,
                ..Panel::new(Rect::new(x, y + 20.0, bw, 28.0))
            });
            o.texts.push(
                Text::new(label, G2::new(x + 13.0, y + 25.0), 15.0)
                    .weight(500)
                    .color(palette::INK.with_alpha(if s.open { 1.0 } else { 0.6 })),
            );
            x += bw + 10.0;
        }
        y += services_h;
    }
    let foot_y = y0 + h - 52.0;
    let _ = y;
    o.texts.push(
        Text::new(
            "W/S choose  ·  E trade one  ·  Shift+E trade all",
            G2::new(col_ware, foot_y + 12.0),
            14.0,
        )
        .weight(500)
        .color(palette::INK.with_alpha(0.6)),
    );
    buttons.push(button(
        o,
        ui,
        Rect::new(x0 + w - 24.0 - 150.0, foot_y, 150.0, 36.0),
        "Undock  ·  Space",
        true,
        UiAction::Undock,
    ));
    buttons
}

/// The map's own furniture: a title and how to close it.
pub fn map_furniture(o: &mut Overlay, ui: &UiState, viewport: G2) -> Vec<Button> {
    o.texts.push(
        Text::new("The Lantern Reach", G2::new(28.0, 22.0), 30.0)
            .weight(700)
            .color(palette::CREAM),
    );
    vec![button(
        o,
        ui,
        Rect::new(viewport.x - 168.0, viewport.y - 58.0, 150.0, 38.0),
        "Back  ·  M",
        true,
        UiAction::CloseMap,
    )]
}

/// The big PAUSED card.
pub fn paused_card() -> PausedCard {
    PausedCard {
        title: "PAUSED".into(),
        hint: "Nothing moves until you're back. Press P or Esc to carry on.".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hit_finds_the_topmost_button() {
        let b = vec![
            Button {
                rect: Rect::new(0.0, 0.0, 100.0, 100.0),
                action: UiAction::Select(0),
            },
            Button {
                rect: Rect::new(10.0, 10.0, 20.0, 20.0),
                action: UiAction::Undock,
            },
        ];
        assert_eq!(hit(&b, G2::new(15.0, 15.0)), Some(&UiAction::Undock));
        assert_eq!(hit(&b, G2::new(50.0, 50.0)), Some(&UiAction::Select(0)));
        assert_eq!(hit(&b, G2::new(150.0, 50.0)), None);
    }
}
