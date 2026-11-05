//! The collision service: solid things (`engine.body`) never stay inside each other.
//!
//! After everything has moved, each pair of bodies that overlaps is pushed apart along
//! the direction of least overlap ([`crate::shape::overlap`]), split by mass: a light
//! thing moves more than a heavy one, and a fixed body (or one with no `engine.motion`)
//! doesn't move at all. Their velocities lose what was carrying them into each other,
//! with a little bounce, so a ship slides along a rock instead of sticking to it, and a
//! rock it flies into drifts off. Things that don't fly lose `drag` of their speed per
//! second, so a pushed rock comes to rest. A few passes settle a pile-up; nothing takes
//! damage. When a module sets `contact_speed`, a pair that starts touching at least that
//! fast records `engine.contact`, once until the two come apart.

use std::collections::BTreeMap;

use galaxy_api::engine::{BODY, CONTACT, FLIGHT, MODEL, MOTION, POSITION, TICK_SECONDS};
use serde_json::{Map, Value};

use crate::shape::{Outline, Shapes, gap, overlap};
use crate::world::World;

/// Passes over the pairs each tick.
const PASSES: usize = 4;
/// How much of the closing speed comes back as bounce.
const BOUNCE: f64 = 0.2;
/// Below this speed a drifting thing is stopped.
const REST: f64 = 0.01;
/// A body with radius 0 and no model it can be measured by.
const FALLBACK_RADIUS: f64 = 0.5;
/// Two bodies still count as touching while the gap between them is under this share of
/// the smaller one's reach, so a pair resting together or sliding along each other is one
/// contact, not a new one each time a push leaves them a hair apart.
const CONTACT_GAP: f64 = 0.02;
/// Ticks a pair that came apart still counts as in contact: half a second.
const CONTACT_HOLD: u64 = 15;

fn number(f: &Map<String, Value>, k: &str) -> f64 {
    f.get(k)
        .and_then(Value::as_f64)
        .filter(|v| v.is_finite())
        .unwrap_or(0.0)
}

/// The outline of `entity` where it is now, if it has a body.
pub fn outline(world: &World, shapes: &Shapes, entity: &str) -> Option<Outline> {
    outline_facing(world, shapes, entity, None)
}

/// The outline of `entity` turned to `angle` (its own angle when `None`), if it has a
/// body.
pub fn outline_facing(
    world: &World,
    shapes: &Shapes,
    entity: &str,
    angle: Option<f64>,
) -> Option<Outline> {
    let body = &world.component(entity, BODY)?.fields;
    let radius = number(body, "radius");
    if radius > 0.0 {
        return Some(Outline::circle(radius));
    }
    let model = world.component(entity, MODEL).map(|m| &m.fields);
    let profile = model
        .and_then(|m| m.get("model"))
        .and_then(Value::as_str)
        .and_then(|id| shapes.models.get(id));
    match (model, profile) {
        (Some(m), Some(profile)) => {
            let angle = angle.unwrap_or_else(|| {
                world
                    .component(entity, POSITION)
                    .map_or(0.0, |p| number(&p.fields, "angle"))
            });
            let scale = m.get("scale").and_then(Value::as_f64).unwrap_or(1.0);
            let scale = if scale.is_finite() && scale > 0.0 {
                scale
            } else {
                1.0
            };
            Some(Outline::of_profile(
                profile,
                angle,
                scale,
                number(m, "morph"),
            ))
        }
        _ => Some(Outline::circle(FALLBACK_RADIUS)),
    }
}

struct Body {
    entity: String,
    place: String,
    outline: Outline,
    bound: f64,
    x: f64,
    y: f64,
    vx: f64,
    vy: f64,
    /// 0 for a body that never moves.
    inverse_mass: f64,
    /// Speed lost per second after a push; 0 for things that fly.
    drag: f64,
    /// Whether its pairs are tested: every body's in [`collide`]; in [`separate`] only
    /// those of bodies that were set down or pushed.
    stirred: bool,
}

/// The solid bodies in `world`, where they are now.
fn bodies(world: &World, shapes: &Shapes) -> Vec<Body> {
    let mut bodies: Vec<Body> = Vec::new();
    for (entity, components) in &world.entities {
        let (Some(body), Some(pos)) = (components.get(BODY), components.get(POSITION)) else {
            continue;
        };
        let Some(outline) = outline(world, shapes, entity) else {
            continue;
        };
        let motion = components.get(MOTION).map(|m| &m.fields);
        let fixed = body
            .fields
            .get("fixed")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let mass = number(&body.fields, "mass");
        let inverse_mass =
            if fixed || motion.is_none() || mass <= 0.0 || crate::places::walking(world, entity) {
                0.0
            } else {
                1.0 / mass
            };
        let drag = if components.contains_key(FLIGHT) {
            0.0
        } else {
            number(&body.fields, "drag").max(0.0)
        };
        bodies.push(Body {
            entity: entity.clone(),
            place: crate::places::place_of(world, entity).to_owned(),
            bound: outline.bound(),
            outline,
            x: number(&pos.fields, "x"),
            y: number(&pos.fields, "y"),
            vx: motion.map_or(0.0, |m| number(m, "vx")),
            vy: motion.map_or(0.0, |m| number(m, "vy")),
            inverse_mass,
            drag,
            stirred: true,
        });
    }
    bodies
}

/// Pushes overlapping pairs with a stirred body in them apart, over [`PASSES`] passes; a
/// body that is pushed is stirred from then on. With `bounce`, their velocities also lose
/// what was carrying them into each other. Returns each pair that overlapped (by index,
/// lower first) with its closing speed when first found.
fn push_apart(bodies: &mut [Body], bounce: bool) -> BTreeMap<(usize, usize), f64> {
    let mut touched = BTreeMap::new();
    for _ in 0..PASSES {
        // Sweep along x: only bodies whose spans on x meet can touch.
        let mut order: Vec<usize> = (0..bodies.len()).collect();
        order.sort_by(|&a, &b| {
            (bodies[a].x - bodies[a].bound)
                .total_cmp(&(bodies[b].x - bodies[b].bound))
                .then(a.cmp(&b))
        });
        let mut pushed = false;
        for (n, &i) in order.iter().enumerate() {
            for &j in &order[n + 1..] {
                if bodies[j].x - bodies[j].bound > bodies[i].x + bodies[i].bound {
                    break;
                }
                let (a, b) = (&bodies[i], &bodies[j]);
                if a.place != b.place || !(a.stirred || b.stirred) {
                    continue;
                }
                let total = a.inverse_mass + b.inverse_mass;
                if total == 0.0 {
                    continue;
                }
                let Some((depth, (nx, ny))) =
                    overlap(&a.outline, (a.x, a.y), &b.outline, (b.x, b.y))
                else {
                    continue;
                };
                let (ia, ib) = (a.inverse_mass / total, b.inverse_mass / total);
                let closing = (b.vx - a.vx) * nx + (b.vy - a.vy) * ny;
                touched
                    .entry((i.min(j), i.max(j)))
                    .or_insert((-closing).max(0.0));
                let impulse = if bounce && closing < 0.0 {
                    -(1.0 + BOUNCE) * closing
                } else {
                    0.0
                };
                {
                    let a = &mut bodies[i];
                    a.x -= nx * depth * ia;
                    a.y -= ny * depth * ia;
                    a.vx -= nx * impulse * ia;
                    a.vy -= ny * impulse * ia;
                    a.stirred |= ia > 0.0;
                }
                {
                    let b = &mut bodies[j];
                    b.x += nx * depth * ib;
                    b.y += ny * depth * ib;
                    b.vx += nx * impulse * ib;
                    b.vy += ny * impulse * ib;
                    b.stirred |= ib > 0.0;
                }
                pushed = true;
            }
        }
        if !pushed {
            break;
        }
    }
    touched
}

/// Records `engine.contact` for each pair that overlapped this tick, closed at least
/// `least` fast, and isn't already in contact; then keeps the pairs in contact, each with
/// the last tick it touched. A pair stays in contact while it overlaps or nearly touches,
/// and for [`CONTACT_HOLD`] ticks after it last did, so a bounce that comes straight back
/// is one bump.
fn contacts(
    world: &mut World,
    bodies: &[Body],
    touched: &BTreeMap<(usize, usize), f64>,
    least: f64,
) {
    let tick = world.tick;
    let name = |i: usize, j: usize| {
        let (a, b) = (&bodies[i].entity, &bodies[j].entity);
        if a < b {
            (a.clone(), b.clone())
        } else {
            (b.clone(), a.clone())
        }
    };
    let before: BTreeMap<(String, String), u64> = std::mem::take(&mut world.contacts)
        .into_iter()
        .map(|(a, b, t)| ((a, b), t))
        .collect();
    let index: BTreeMap<&str, usize> = bodies
        .iter()
        .enumerate()
        .map(|(i, b)| (b.entity.as_str(), i))
        .collect();
    let mut now: BTreeMap<(String, String), u64> = BTreeMap::new();
    for (pair, &last) in &before {
        let (Some(&i), Some(&j)) = (index.get(pair.0.as_str()), index.get(pair.1.as_str())) else {
            continue;
        };
        let (a, b) = (&bodies[i], &bodies[j]);
        if a.place != b.place {
            continue;
        }
        if gap(&a.outline, (a.x, a.y), &b.outline, (b.x, b.y)) <= CONTACT_GAP * a.bound.min(b.bound)
        {
            now.insert(pair.clone(), tick);
        } else if tick.saturating_sub(last) < CONTACT_HOLD {
            now.insert(pair.clone(), last);
        }
    }
    let mut new: Vec<(String, String, f64)> = Vec::new();
    for (&(i, j), &speed) in touched {
        let pair = name(i, j);
        if speed >= least && !before.contains_key(&pair) {
            // The one that moves more is the entity: the ship, not the rock it hit.
            let (a, b) = (&bodies[i], &bodies[j]);
            let first = a.inverse_mass > b.inverse_mass
                || (a.inverse_mass == b.inverse_mass && a.entity < b.entity);
            let (p, q) = if first { (a, b) } else { (b, a) };
            new.push((p.entity.clone(), q.entity.clone(), speed));
        }
        now.insert(pair, tick);
    }
    new.sort_by(|a, b| (&a.0, &a.1).cmp(&(&b.0, &b.1)));
    if !new.is_empty() {
        let cause = world.service_cause("collide");
        for (entity, other, speed) in new {
            let mut payload = Map::new();
            payload.insert("entity".into(), Value::from(entity));
            payload.insert("other".into(), Value::from(other));
            payload.insert("speed".into(), Value::from(speed));
            world.record(cause, CONTACT, payload);
        }
    }
    world.contacts = now.into_iter().map(|((a, b), t)| (a, b, t)).collect();
}

/// Writes back the bodies that moved or changed speed since `before`.
fn write(world: &mut World, bodies: &[Body], before: &[(f64, f64, f64, f64)]) {
    let changed: BTreeMap<&str, &Body> = bodies
        .iter()
        .zip(before)
        .filter(|(b, was)| (b.x, b.y, b.vx, b.vy) != **was)
        .map(|(b, _)| (b.entity.as_str(), b))
        .collect();
    if changed.is_empty() {
        return;
    }
    let cause = world.service_cause("collide");
    for (entity, b) in changed {
        crate::services::set(world, entity, POSITION, "x", Value::from(b.x), cause);
        crate::services::set(world, entity, POSITION, "y", Value::from(b.y), cause);
        if world.component(entity, MOTION).is_some() {
            crate::services::set(world, entity, MOTION, "vx", Value::from(b.vx), cause);
            crate::services::set(world, entity, MOTION, "vy", Value::from(b.vy), cause);
        }
    }
}

fn state(bodies: &[Body]) -> Vec<(f64, f64, f64, f64)> {
    bodies.iter().map(|b| (b.x, b.y, b.vx, b.vy)).collect()
}

/// Pushes overlapping bodies apart and damps drifting ones.
pub fn collide(world: &mut World, shapes: &Shapes) {
    let mut bodies = bodies(world, shapes);
    if bodies.is_empty() {
        world.contacts.clear();
        return;
    }
    let before = state(&bodies);
    let touched = push_apart(&mut bodies, true);
    match shapes.contact_speed {
        Some(least) => contacts(world, &bodies, &touched, least),
        None => world.contacts.clear(),
    }
    for b in &mut bodies {
        if b.drag > 0.0 && b.inverse_mass > 0.0 {
            let keep = libm::exp(-b.drag * TICK_SECONDS);
            (b.vx, b.vy) = (b.vx * keep, b.vy * keep);
            if libm::sqrt(b.vx * b.vx + b.vy * b.vy) < REST {
                (b.vx, b.vy) = (0.0, 0.0);
            }
        }
    }
    write(world, &bodies, &before);
}

/// Pushes `moved`, bodies set down after [`collide`] ran this tick, out of whatever they
/// now overlap, and anything they push out of what that overlaps, so no tick ends with
/// solid bodies inside each other. Positions only: [`collide`] settled speeds and drag,
/// and pairs where neither body moved stay as it left them.
pub fn separate(world: &mut World, shapes: &Shapes, moved: &[String]) {
    let mut bodies = bodies(world, shapes);
    for b in &mut bodies {
        b.stirred = moved.contains(&b.entity);
    }
    if !bodies.iter().any(|b| b.stirred) {
        return;
    }
    let before = state(&bodies);
    push_apart(&mut bodies, false);
    write(world, &bodies, &before);
}
