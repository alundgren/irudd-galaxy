//! Engine services: the per-tick work that runs in Rust, never in Luau.
//!
//! - **flight** ([`crate::flight`]): things with `engine.flight` turn and set their
//!   velocity from the helm or an order.
//! - **motion**: everything with an `engine.position` and an `engine.motion` moves by its
//!   velocity each tick.
//! - **ground**: everything with an `engine.position` gets the height of the floor under
//!   it on its layer: the highest `engine.surface` on that layer that covers its point, or
//!   0 where none does.
//! - **collide** ([`crate::collide`]): solid things pushed apart, after motion and
//!   before ground.
//!
//! Each service changes components directly with its own cause, and only when a value
//! actually changes, so the cause on a component says who last moved it.

use galaxy_api::engine::{MOTION, POSITION, SURFACE, TICK_SECONDS};
use serde_json::Value;

use crate::world::World;

/// Sets one field of an existing component for an engine service, only when it changes,
/// with the service's cause, so the cause on a component says who last changed it.
pub(crate) fn set(
    world: &mut World,
    entity: &str,
    component: &str,
    field: &str,
    value: Value,
    cause: u64,
) {
    let tick = world.tick;
    let Some(c) = world
        .entities
        .get_mut(entity)
        .and_then(|c| c.get_mut(component))
    else {
        return;
    };
    if c.fields.get(field) == Some(&value) {
        return;
    }
    c.fields.insert(field.to_owned(), value);
    c.cause = cause;
    c.tick = tick;
}

fn number(fields: &serde_json::Map<String, Value>, name: &str) -> f64 {
    fields.get(name).and_then(Value::as_f64).unwrap_or(0.0)
}

fn int(fields: &serde_json::Map<String, Value>, name: &str) -> i64 {
    fields.get(name).and_then(Value::as_i64).unwrap_or(0)
}

/// Moves everything with a position and a motion.
pub fn motion(world: &mut World) {
    let moving: Vec<(String, f64, f64)> = world
        .entities
        .iter()
        .filter_map(|(entity, components)| {
            components.get(POSITION)?;
            let motion = &components.get(MOTION)?.fields;
            let (vx, vy) = (number(motion, "vx"), number(motion, "vy"));
            (vx != 0.0 || vy != 0.0).then(|| (entity.clone(), vx, vy))
        })
        .collect();
    if moving.is_empty() {
        return;
    }
    let cause = world.service_cause("motion");
    let tick = world.tick;
    for (entity, vx, vy) in moving {
        let position = world
            .entities
            .get_mut(&entity)
            .and_then(|c| c.get_mut(POSITION))
            .expect("found above");
        let x = number(&position.fields, "x") + vx * TICK_SECONDS;
        let y = number(&position.fields, "y") + vy * TICK_SECONDS;
        position.fields.insert("x".into(), Value::from(x));
        position.fields.insert("y".into(), Value::from(y));
        position.cause = cause;
        position.tick = tick;
    }
}

/// A walkable rectangle on a layer.
struct Surface {
    layer: i64,
    min_x: f64,
    min_y: f64,
    max_x: f64,
    max_y: f64,
    height: f64,
    rise_x: f64,
    rise_y: f64,
}

impl Surface {
    fn height_at(&self, x: f64, y: f64) -> Option<f64> {
        let inside = x >= self.min_x && x <= self.max_x && y >= self.min_y && y <= self.max_y;
        inside.then_some(
            self.height + self.rise_x * (x - self.min_x) + self.rise_y * (y - self.min_y),
        )
    }
}

/// The floor height under (`x`, `y`) on `layer`.
fn floor(surfaces: &[Surface], layer: i64, x: f64, y: f64) -> f64 {
    surfaces
        .iter()
        .filter(|s| s.layer == layer)
        .filter_map(|s| s.height_at(x, y))
        .reduce(f64::max)
        .unwrap_or(0.0)
}

/// Sets each position's height to the floor under it.
pub fn ground(world: &mut World) {
    let surfaces: Vec<Surface> = world
        .entities
        .values()
        .filter_map(|c| c.get(SURFACE))
        .map(|s| {
            let f = &s.fields;
            Surface {
                layer: int(f, "layer"),
                min_x: number(f, "min_x"),
                min_y: number(f, "min_y"),
                max_x: number(f, "max_x"),
                max_y: number(f, "max_y"),
                height: number(f, "height"),
                rise_x: number(f, "rise_x"),
                rise_y: number(f, "rise_y"),
            }
        })
        .collect();
    let changed: Vec<(String, f64)> = world
        .entities
        .iter()
        .filter_map(|(entity, components)| {
            let p = &components.get(POSITION)?.fields;
            let height = floor(&surfaces, int(p, "layer"), number(p, "x"), number(p, "y"));
            (p.get("height").and_then(Value::as_f64) != Some(height))
                .then(|| (entity.clone(), height))
        })
        .collect();
    if changed.is_empty() {
        return;
    }
    let cause = world.service_cause("ground");
    let tick = world.tick;
    for (entity, height) in changed {
        let position = world
            .entities
            .get_mut(&entity)
            .and_then(|c| c.get_mut(POSITION))
            .expect("found above");
        position.fields.insert("height".into(), Value::from(height));
        position.cause = cause;
        position.tick = tick;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_highest_surface_on_the_layer_is_the_floor() {
        let ramp = Surface {
            layer: 0,
            min_x: 10.0,
            min_y: 0.0,
            max_x: 20.0,
            max_y: 5.0,
            height: 0.0,
            rise_x: 0.2,
            rise_y: 0.0,
        };
        let deck = Surface {
            layer: 1,
            min_x: 20.0,
            min_y: 0.0,
            max_x: 40.0,
            max_y: 5.0,
            height: 2.0,
            rise_x: 0.0,
            rise_y: 0.0,
        };
        let shelf = Surface {
            layer: 0,
            min_x: 14.0,
            min_y: 0.0,
            max_x: 16.0,
            max_y: 5.0,
            height: 1.5,
            rise_x: 0.0,
            rise_y: 0.0,
        };
        let all = [ramp, deck, shelf];
        assert_eq!(floor(&all, 0, 5.0, 1.0), 0.0);
        assert_eq!(floor(&all, 0, 12.5, 1.0), 0.5);
        assert_eq!(floor(&all, 0, 15.0, 1.0), 1.5);
        assert_eq!(floor(&all, 0, 20.0, 1.0), 2.0);
        assert_eq!(floor(&all, 1, 30.0, 1.0), 2.0);
        assert_eq!(floor(&all, 1, 12.0, 1.0), 0.0);
    }
}
