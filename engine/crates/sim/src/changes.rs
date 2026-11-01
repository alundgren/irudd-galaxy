//! What changed in a world from one tick to the next, in a line each, for the tools that
//! report it: an input (`drift.order_dock down`), an order given, changed or dropped (an
//! entity's `engine.steer`) and an arrival (`engine.arrived`). Films keep them beside
//! their frames; `galaxy-gm recording <rec> orders` lists them for a whole session.

use std::collections::BTreeMap;

use galaxy_api::engine::{ARRIVED, STEER};
use serde_json::Value;

use crate::session::Input;
use crate::world::World;

/// Every entity's order, as `mode target` (or `mode x,y`), empty for none.
pub fn orders(world: &World) -> BTreeMap<String, String> {
    world
        .entities
        .iter()
        .filter_map(|(id, c)| {
            let f = &c.get(STEER)?.fields;
            let text = |k: &str| f.get(k).and_then(Value::as_str).unwrap_or("").to_owned();
            let (mode, target) = (text("mode"), text("target"));
            let order = match (mode.is_empty(), target.is_empty()) {
                (true, _) => String::new(),
                (false, false) => format!("{mode} {target}"),
                (false, true) => {
                    let n = |k: &str| f.get(k).and_then(Value::as_f64).unwrap_or(0.0);
                    format!("{mode} {:.1},{:.1}", n("x"), n("y"))
                }
            };
            Some((id.clone(), order))
        })
        .collect()
}

/// An input as one short line: `drift.order_dock down`.
pub fn input(input: &Input) -> String {
    match input.payload.get("down").and_then(Value::as_bool) {
        Some(true) if input.payload.len() == 1 => format!("{} down", input.name),
        Some(false) if input.payload.len() == 1 => format!("{} up", input.name),
        _ if input.payload.is_empty() => input.name.clone(),
        _ => format!("{} {}", input.name, Value::Object(input.payload.clone())),
    }
}

/// The orders and arrivals between two moments: `before` (from [`orders`] on the earlier
/// world) and `world` now, whose facts from `first_fact` on are new. Each is (kind, what).
pub fn since(
    before: &BTreeMap<String, String>,
    world: &World,
    first_fact: usize,
) -> Vec<(&'static str, String)> {
    let mut out = Vec::new();
    for (id, order) in orders(world) {
        if before.get(&id).map_or("", String::as_str) != order {
            let order = if order.is_empty() {
                "no order".to_owned()
            } else {
                order
            };
            out.push(("order", format!("{id}: {order}")));
        }
    }
    for f in world
        .facts
        .iter()
        .skip(first_fact)
        .filter(|f| f.kind == ARRIVED)
    {
        let text = |k: &str| f.payload.get(k).and_then(Value::as_str).unwrap_or("");
        out.push((
            "arrived",
            format!("{}: {} {}", text("entity"), text("mode"), text("target")),
        ));
    }
    out
}
