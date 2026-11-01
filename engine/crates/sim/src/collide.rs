//! The collision service: solid things (`engine.body`) never stay inside each other.
//!
//! After everything has moved, each pair of bodies that overlaps is pushed apart along
//! the direction of least overlap ([`crate::shape::overlap`]), split by mass: a light
//! thing moves more than a heavy one, and a fixed body (or one with no `engine.motion`)
//! doesn't move at all. Their velocities lose what was carrying them into each other,
//! with a little bounce, so a ship slides along a rock instead of sticking to it, and a
//! rock it flies into drifts off. Things that don't fly lose `drag` of their speed per
//! second, so a pushed rock comes to rest. A few passes settle a pile-up; nothing takes
//! damage.

use std::collections::BTreeMap;

use galaxy_api::engine::{BODY, FLIGHT, MODEL, MOTION, POSITION, TICK_SECONDS};
use serde_json::{Map, Value};

use crate::shape::{Outline, Shapes, overlap};
use crate::world::World;

/// Passes over the pairs each tick.
const PASSES: usize = 4;
/// How much of the closing speed comes back as bounce.
const BOUNCE: f64 = 0.2;
/// Below this speed a drifting thing is stopped.
const REST: f64 = 0.01;
/// A body with radius 0 and no model it can be measured by.
const FALLBACK_RADIUS: f64 = 0.5;

fn number(f: &Map<String, Value>, k: &str) -> f64 {
    f.get(k)
        .and_then(Value::as_f64)
        .filter(|v| v.is_finite())
        .unwrap_or(0.0)
}

/// The outline of `entity` where it is now, if it has a body.
pub fn outline(world: &World, shapes: &Shapes, entity: &str) -> Option<Outline> {
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
            let angle = world
                .component(entity, POSITION)
                .map_or(0.0, |p| number(&p.fields, "angle"));
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
}

/// Pushes overlapping bodies apart and damps drifting ones.
pub fn collide(world: &mut World, shapes: &Shapes) {
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
        let inverse_mass = if fixed || motion.is_none() || mass <= 0.0 {
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
            bound: outline.bound(),
            outline,
            x: number(&pos.fields, "x"),
            y: number(&pos.fields, "y"),
            vx: motion.map_or(0.0, |m| number(m, "vx")),
            vy: motion.map_or(0.0, |m| number(m, "vy")),
            inverse_mass,
            drag,
        });
    }
    if bodies.is_empty() {
        return;
    }
    let before: Vec<(f64, f64, f64, f64)> = bodies.iter().map(|b| (b.x, b.y, b.vx, b.vy)).collect();
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
                let impulse = if closing < 0.0 {
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
                }
                {
                    let b = &mut bodies[j];
                    b.x += nx * depth * ib;
                    b.y += ny * depth * ib;
                    b.vx += nx * impulse * ib;
                    b.vy += ny * impulse * ib;
                }
                pushed = true;
            }
        }
        if !pushed {
            break;
        }
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
    let changed: BTreeMap<&str, &Body> = bodies
        .iter()
        .zip(&before)
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
