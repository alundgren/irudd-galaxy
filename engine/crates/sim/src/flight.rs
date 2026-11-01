//! The flight service: things with `engine.flight` fly, by hand (`engine.helm`) or by an
//! order (`engine.steer`).
//!
//! **By hand** the ship has tank controls with inertia, carried over from the slice and
//! its flight lab: `turn` swings the nose at the turn rate, `thrust` pushes along it.
//! Inertia decides how much of the old velocity survives a change: sideways speed swings
//! round into the new heading instead of vanishing, and coasting bleeds off. Letting go of
//! thrust starts the auto-brake, which stops the ship in `stop_time` from any speed.
//! Thrust from rest goes straight along the nose, so a ship leaves a dock in a straight
//! line.
//!
//! **By order** the ship steers straight for where it goes: the wanted velocity comes from
//! the target, reached at `accel` and shed at `brake`, slow enough to stop on the spot,
//! and the nose swings toward the way it goes at the turn rate. Touching the helm sets the
//! order aside: the player always has the controls.
//!
//! Every number is `f64` with plain arithmetic or `libm`, so replays match on every OS.

use std::f64::consts::{PI, TAU};

use galaxy_api::engine::{ARRIVED, FLIGHT, HELM, MOTION, POSITION, STEER, TICK_SECONDS, WAYPOINT};
use serde_json::{Map, Value};

use crate::collide::outline;
use crate::services::set;
use crate::shape::{Shapes, touching};
use crate::world::World;

/// Below this speed a ship nobody pushes is stopped.
const REST: f64 = 0.01;
/// An order is there when this close to its point and this slow.
const THERE: f64 = 0.05;
const STILL: f64 = 0.2;
/// And, when it must face a way, turned to within this (radians).
const FACING: f64 = 0.02;
/// `follow` passes a waypoint this close to it, unless the order's range is wider.
const PASS: f64 = 1.0;

fn number(f: &Map<String, Value>, k: &str) -> f64 {
    f.get(k)
        .and_then(Value::as_f64)
        .filter(|v| v.is_finite())
        .unwrap_or(0.0)
}

fn text<'a>(f: &'a Map<String, Value>, k: &str) -> &'a str {
    f.get(k).and_then(Value::as_str).unwrap_or("")
}

fn boolean(f: &Map<String, Value>, k: &str) -> bool {
    f.get(k).and_then(Value::as_bool).unwrap_or(false)
}

/// How a thing handles, from its `engine.flight`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Feel {
    pub speed: f64,
    pub accel: f64,
    pub brake: f64,
    /// Radians per second.
    pub turn: f64,
    pub slowdown: f64,
    pub boost: f64,
    /// Seconds for leftover velocity to fall to 1/e: 0 for no inertia, infinite for none
    /// lost.
    pub coast: f64,
    /// Ticks the auto-brake takes to stop; `None` when off.
    pub stop_ticks: Option<u32>,
}

/// Seconds for leftover velocity to fall to 1/e at `inertia` (0..1): the flight lab's
/// curve, so its tuning carries over.
pub fn coast_time(inertia: f64) -> f64 {
    if inertia <= 0.0 {
        0.0
    } else if inertia >= 1.0 {
        f64::INFINITY
    } else {
        0.35 * libm::pow(inertia / (1.0 - inertia), 1.3)
    }
}

impl Feel {
    pub fn of(f: &Map<String, Value>) -> Feel {
        let stop = number(f, "stop_time").max(0.0);
        Feel {
            speed: number(f, "speed").max(0.0),
            accel: number(f, "accel").max(0.0),
            brake: number(f, "brake").max(0.0),
            turn: number(f, "turn").max(0.0).to_radians(),
            slowdown: number(f, "slowdown").clamp(0.0, 1.0),
            boost: number(f, "boost").max(1.0),
            coast: coast_time(number(f, "inertia")),
            stop_ticks: (stop > 0.0)
                .then(|| libm::round(stop / TICK_SECONDS).clamp(1.0, 1e6) as u32),
        }
    }

    /// The share of velocity that survives `dt` seconds of turning or coasting.
    fn keep(&self, dt: f64) -> f64 {
        if self.coast <= 0.0 {
            0.0
        } else if self.coast.is_infinite() {
            1.0
        } else {
            libm::exp(-dt / self.coast)
        }
    }
}

/// Where a thing is and how it moves.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Motion {
    pub x: f64,
    pub y: f64,
    pub vx: f64,
    pub vy: f64,
    /// Radians, counter-clockwise from +x, kept in `-PI..=PI`.
    pub heading: f64,
}

/// The hands on the controls.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Hands {
    /// -1..=1, positive turns left.
    pub turn: f64,
    /// -1..=1 along the nose.
    pub thrust: f64,
    pub brake: bool,
    pub boost: bool,
}

impl Hands {
    fn of(f: &Map<String, Value>) -> Hands {
        Hands {
            turn: number(f, "turn").clamp(-1.0, 1.0),
            thrust: number(f, "thrust").clamp(-1.0, 1.0),
            brake: boolean(f, "brake"),
            boost: boolean(f, "boost"),
        }
    }

    fn idle(&self) -> bool {
        self.turn == 0.0 && self.thrust == 0.0
    }
}

pub fn wrap(a: f64) -> f64 {
    let mut a = a % TAU;
    if a > PI {
        a -= TAU;
    } else if a < -PI {
        a += TAU;
    }
    a
}

fn turn_toward(from: f64, to: f64, max: f64) -> f64 {
    let d = wrap(to - from);
    wrap(if d.abs() <= max {
        to
    } else {
        from + max.copysign(d)
    })
}

fn length(x: f64, y: f64) -> f64 {
    libm::sqrt(x * x + y * y)
}

/// One tick flown by hand. `stopping` is the auto-brake's ticks left; returns the new
/// motion and ticks left.
pub fn fly(feel: &Feel, m: Motion, stopping: u32, hands: Hands, dt: f64) -> (Motion, u32) {
    let heading = wrap(m.heading + hands.turn * feel.turn * dt);
    let (fx, fy) = (libm::cos(heading), libm::sin(heading));
    let mult = if hands.boost { feel.boost } else { 1.0 };
    let (top, accel) = (feel.speed * mult, feel.accel * mult);
    let keep = feel.keep(dt);
    let (mut vx, mut vy) = (m.vx, m.vy);
    let mut stopping = stopping;
    if hands.thrust != 0.0 {
        stopping = 0;
        let (dx, dy) = if hands.thrust > 0.0 {
            (fx, fy)
        } else {
            (-fx, -fy)
        };
        let target = top * hands.thrust.abs();
        let mut along = vx * dx + vy * dy;
        let (sx, sy) = (vx - dx * along, vy - dy * along);
        if along >= 0.0 {
            // Grip: the sideways speed inertia doesn't keep swings round into the new
            // heading instead of vanishing.
            along = libm::sqrt(along * along + (sx * sx + sy * sy) * (1.0 - keep * keep));
        } else {
            along *= keep;
        }
        along = if along < target {
            (along + accel * dt).min(target)
        } else {
            target.max((along - accel * dt).min(target + (along - target) * keep))
        };
        vx = dx * along + sx * keep;
        vy = dy * along + sy * keep;
    } else if let Some(ticks) = feel.stop_ticks.filter(|_| !hands.brake) {
        // The auto-brake: speed falls in a straight line to zero over the stop, from
        // whatever it was when thrust was let go.
        if stopping == 0 && (vx != 0.0 || vy != 0.0) {
            stopping = ticks;
        }
        if stopping == 0 {
            (vx, vy) = (0.0, 0.0);
        } else {
            let k = f64::from(stopping - 1) / f64::from(stopping);
            (vx, vy) = (vx * k, vy * k);
            stopping -= 1;
        }
    } else {
        stopping = 0;
        (vx, vy) = (vx * keep, vy * keep);
    }
    if hands.brake && feel.brake > 0.0 {
        let speed = length(vx, vy);
        if speed > 0.0 {
            let k = (speed - feel.brake * dt).max(0.0) / speed;
            (vx, vy) = (vx * k, vy * k);
        }
    }
    let cap = feel.speed * feel.boost;
    let speed = length(vx, vy);
    if speed > cap && speed > 0.0 {
        (vx, vy) = (vx * cap / speed, vy * cap / speed);
    }
    if hands.thrust == 0.0 && length(vx, vy) < REST {
        (vx, vy) = (0.0, 0.0);
        stopping = 0;
    }
    let motion = Motion {
        x: m.x,
        y: m.y,
        vx,
        vy,
        heading,
    };
    (motion, stopping)
}

/// One tick steered toward the point (`to_x`, `to_y`), stopping on it; or braking to a
/// stop when `to` is `None`. The nose turns toward the way it goes, or to `face` once
/// there.
pub fn steer(feel: &Feel, m: Motion, to: Option<(f64, f64)>, face: Option<f64>, dt: f64) -> Motion {
    let (mut dx, mut dy, mut speed) = (0.0, 0.0, 0.0);
    if let Some((tx, ty)) = to {
        let (ox, oy) = (tx - m.x, ty - m.y);
        let dist = length(ox, oy);
        if dist > 1e-9 {
            (dx, dy) = (ox / dist, oy / dist);
            // Fast enough to get there, slow enough to stop on the spot: the speed that
            // braking one tick at a time can still shed before the point (it moves a
            // tick at this speed first).
            let b = feel.brake * dt;
            let stop = libm::sqrt(b * b + 2.0 * feel.brake * dist) - b;
            speed = feel.speed.min(stop).min(dist / dt);
        }
    }
    let mut heading = m.heading;
    if speed > 0.0 && dist_ok(dx, dy) {
        let want = libm::atan2(dy, dx);
        heading = turn_toward(heading, want, feel.turn * dt);
        let off = wrap(want - heading);
        speed *= 1.0 - feel.slowdown * (1.0 - libm::cos(off)) * 0.5;
    } else if let Some(face) = face {
        heading = turn_toward(heading, face, feel.turn * dt);
    }
    let (wx, wy) = (dx * speed, dy * speed);
    let (vx, vy) = (m.vx, m.vy);
    // Slowing along the current motion (stopping, turning) uses the brake.
    let rate = if wx * vx + wy * vy < vx * vx + vy * vy {
        feel.brake
    } else {
        feel.accel
    };
    let (mut cx, mut cy) = (wx - vx, wy - vy);
    let change = length(cx, cy);
    let most = rate * dt;
    if change > most && change > 0.0 {
        (cx, cy) = (cx * most / change, cy * most / change);
    }
    let (mut nx, mut ny) = (vx + cx, vy + cy);
    if speed == 0.0 && length(nx, ny) < REST {
        (nx, ny) = (0.0, 0.0);
    }
    Motion {
        vx: nx,
        vy: ny,
        heading,
        ..m
    }
}

fn dist_ok(dx: f64, dy: f64) -> bool {
    dx != 0.0 || dy != 0.0
}

/// What one entity's flight changed, to write back.
struct Flown {
    entity: String,
    motion: Motion,
    stopping: u32,
    /// The order's new fields, when they changed.
    steer: Option<Map<String, Value>>,
    arrived: Option<Map<String, Value>>,
}

/// An order's goal this tick: where to stop, which way to face there, and the next
/// target when it is a waypoint passed.
struct Goal {
    to: (f64, f64),
    face: Option<f64>,
    /// `follow` only: the waypoint after this one, if any.
    next: Option<String>,
}

fn goal(
    world: &World,
    shapes: &Shapes,
    entity: &str,
    m: &Motion,
    order: &Map<String, Value>,
) -> Option<Goal> {
    let mode = text(order, "mode");
    let target = text(order, "target");
    let range = number(order, "range").max(0.0);
    let pose = |id: &str| {
        let p = &world.component(id, POSITION)?.fields;
        Some((number(p, "x"), number(p, "y"), number(p, "angle")))
    };
    let (tx, ty, tangle) = if target.is_empty() {
        (number(order, "x"), number(order, "y"), 0.0)
    } else {
        pose(target)?
    };
    match mode {
        "arrive" | "hold" => {
            let (ox, oy) = (tx - m.x, ty - m.y);
            let dist = length(ox, oy);
            let toward = libm::atan2(oy, ox);
            // Stop where the outlines are `range` apart along the line between centres.
            let apart = match (target.is_empty(), outline(world, shapes, entity)) {
                (false, Some(own)) => {
                    let theirs = outline(world, shapes, target)
                        .unwrap_or_else(|| crate::shape::Outline::circle(0.0));
                    touching(&own, &theirs, toward) + range
                }
                _ => range,
            };
            let back = (dist - apart).max(0.0);
            let to = if dist > 1e-9 {
                (m.x + ox / dist * back, m.y + oy / dist * back)
            } else {
                (m.x, m.y)
            };
            let face = (mode == "hold").then_some(toward);
            Some(Goal {
                to,
                face,
                next: None,
            })
        }
        "dock" => Some(Goal {
            to: (tx, ty),
            face: Some(tangle),
            next: None,
        }),
        "follow" => {
            let next = world
                .component(target, WAYPOINT)
                .map(|w| text(&w.fields, "next").to_owned())
                .filter(|n| !n.is_empty() && world.entities.contains_key(n));
            Some(Goal {
                to: (tx, ty),
                face: None,
                next,
            })
        }
        _ => None,
    }
}

/// Runs flight for everything with `engine.flight`, `engine.position` and `engine.motion`.
pub fn flight(world: &mut World, shapes: &Shapes) {
    let dt = TICK_SECONDS;
    let mut flown = Vec::new();
    for (entity, components) in &world.entities {
        let (Some(feel), Some(pos), Some(mot)) = (
            components.get(FLIGHT),
            components.get(POSITION),
            components.get(MOTION),
        ) else {
            continue;
        };
        let feel = Feel::of(&feel.fields);
        let m = Motion {
            x: number(&pos.fields, "x"),
            y: number(&pos.fields, "y"),
            vx: number(&mot.fields, "vx"),
            vy: number(&mot.fields, "vy"),
            heading: wrap(number(&pos.fields, "angle")),
        };
        let helm = components.get(HELM).map(|h| &h.fields);
        let hands = helm.map(Hands::of).unwrap_or_default();
        let stopping = helm
            .and_then(|h| h.get("stopping"))
            .and_then(Value::as_u64)
            .unwrap_or(0) as u32;
        let order = components.get(STEER).map(|s| &s.fields);
        let active = order.filter(|o| !text(o, "mode").is_empty() && !boolean(o, "done"));
        let Some(order) = active else {
            let (motion, stopping) = fly(&feel, m, stopping, hands, dt);
            flown.push(Flown {
                entity: entity.clone(),
                motion,
                stopping,
                steer: None,
                arrived: None,
            });
            continue;
        };
        if !hands.idle() {
            // The player takes the controls: the order is set aside.
            let mut cleared = order.clone();
            cleared.insert("mode".into(), Value::from(""));
            let (motion, stopping) = fly(&feel, m, stopping, hands, dt);
            flown.push(Flown {
                entity: entity.clone(),
                motion,
                stopping,
                steer: Some(cleared),
                arrived: None,
            });
            continue;
        }
        let Some(goal) = goal(world, shapes, entity, &m, order) else {
            // The target is gone or the mode unknown: stop.
            let motion = steer(&feel, m, None, None, dt);
            flown.push(Flown {
                entity: entity.clone(),
                motion,
                stopping: 0,
                steer: None,
                arrived: None,
            });
            continue;
        };
        let mode = text(order, "mode").to_owned();
        let target = text(order, "target").to_owned();
        let range = number(order, "range").max(PASS);
        let (gx, gy) = goal.to;
        let away = length(gx - m.x, gy - m.y);
        if mode == "follow"
            && away <= range
            && let Some(next) = goal.next
        {
            // Passed this waypoint: on to the next, without stopping.
            let mut moved = order.clone();
            moved.insert("target".into(), Value::from(next));
            let motion = steer(&feel, m, Some(goal.to), None, dt);
            flown.push(Flown {
                entity: entity.clone(),
                motion,
                stopping: 0,
                steer: Some(moved),
                arrived: None,
            });
            continue;
        }
        let motion = steer(&feel, m, Some(goal.to), goal.face, dt);
        let faced = goal
            .face
            .is_none_or(|f| wrap(f - motion.heading).abs() <= FACING);
        let there = away <= THERE && length(motion.vx, motion.vy) <= STILL && faced;
        if there {
            let mut done = order.clone();
            done.insert("done".into(), Value::Bool(true));
            let mut payload = Map::new();
            payload.insert("entity".into(), Value::from(entity.clone()));
            payload.insert("mode".into(), Value::from(mode));
            payload.insert("target".into(), Value::from(target));
            flown.push(Flown {
                entity: entity.clone(),
                motion: Motion {
                    vx: 0.0,
                    vy: 0.0,
                    ..motion
                },
                stopping: 0,
                steer: Some(done),
                arrived: Some(payload),
            });
        } else {
            flown.push(Flown {
                entity: entity.clone(),
                motion,
                stopping: 0,
                steer: None,
                arrived: None,
            });
        }
    }
    if flown.is_empty() {
        return;
    }
    let cause = world.service_cause("flight");
    for f in flown {
        let e = f.entity.as_str();
        set(world, e, MOTION, "vx", Value::from(f.motion.vx), cause);
        set(world, e, MOTION, "vy", Value::from(f.motion.vy), cause);
        set(
            world,
            e,
            POSITION,
            "angle",
            Value::from(f.motion.heading),
            cause,
        );
        set(world, e, HELM, "stopping", Value::from(f.stopping), cause);
        if let Some(steer) = f.steer {
            for (k, v) in steer {
                set(world, e, STEER, &k, v, cause);
            }
        }
        if let Some(payload) = f.arrived {
            world.record(cause, ARRIVED, payload);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feel() -> Feel {
        Feel {
            speed: 14.0,
            accel: 21.0,
            brake: 16.0,
            turn: 180f64.to_radians(),
            slowdown: 0.5,
            boost: 2.0,
            coast: coast_time(0.4),
            stop_ticks: Some(30),
        }
    }

    fn rest() -> Motion {
        Motion {
            x: 0.0,
            y: 0.0,
            vx: 0.0,
            vy: 0.0,
            heading: 0.0,
        }
    }

    fn hands(turn: f64, thrust: f64) -> Hands {
        Hands {
            turn,
            thrust,
            ..Hands::default()
        }
    }

    fn run(m: Motion, h: Hands, ticks: u32) -> (Motion, u32) {
        let mut state = (m, 0);
        for _ in 0..ticks {
            let (mut next, stopping) = fly(&feel(), state.0, state.1, h, TICK_SECONDS);
            next.x += next.vx * TICK_SECONDS;
            next.y += next.vy * TICK_SECONDS;
            state = (next, stopping);
        }
        state
    }

    #[test]
    fn it_turns_half_a_circle_a_second() {
        let (m, _) = run(rest(), hands(1.0, 0.0), 15);
        assert!((m.heading - PI / 2.0).abs() < 1e-9, "{}", m.heading);
        let (m, _) = run(rest(), hands(-1.0, 0.0), 15);
        assert!((m.heading + PI / 2.0).abs() < 1e-9);
    }

    #[test]
    fn thrust_from_rest_goes_straight_along_the_nose() {
        let start = Motion {
            heading: 0.7,
            ..rest()
        };
        let (m, _) = run(start, hands(0.0, 1.0), 30);
        let along = libm::atan2(m.y, m.x);
        assert!((along - 0.7).abs() < 1e-9, "{along}");
        assert!(
            (length(m.vx, m.vy) - 14.0).abs() < 1e-6,
            "top speed after a second"
        );
    }

    #[test]
    fn letting_go_coasts_to_a_stop_in_the_stop_time() {
        let (moving, _) = run(rest(), hands(0.0, 1.0), 30);
        let (after, stopping) = run(moving, hands(0.0, 0.0), 30);
        assert_eq!((after.vx, after.vy, stopping), (0.0, 0.0, 0));
        assert!(
            after.x > moving.x + 3.0,
            "it coasted on: {} {}",
            moving.x,
            after.x
        );
    }

    #[test]
    fn turning_at_speed_carries_some_sideways_speed_round() {
        let (moving, _) = run(rest(), hands(0.0, 1.0), 30);
        let (turned, _) = run(moving, hands(1.0, 1.0), 15);
        // Facing +y now; still some +x speed left over, less than before.
        assert!(turned.vx > 0.0 && turned.vx < moving.vx, "{}", turned.vx);
        assert!(turned.vy > 0.0);
    }

    #[test]
    fn an_order_stops_on_its_point_without_overshooting() {
        let mut m = rest();
        let mut furthest: f64 = 0.0;
        for _ in 0..300 {
            m = steer(&feel(), m, Some((20.0, 0.0)), None, TICK_SECONDS);
            m.x += m.vx * TICK_SECONDS;
            m.y += m.vy * TICK_SECONDS;
            furthest = furthest.max(m.x);
        }
        assert!((m.x - 20.0).abs() < THERE, "{}", m.x);
        assert!(furthest < 20.0 + THERE, "overshot to {furthest}");
        assert_eq!((m.vx, m.vy), (0.0, 0.0));
    }
}
