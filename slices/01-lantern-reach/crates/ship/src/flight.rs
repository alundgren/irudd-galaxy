//! Arcade flight: one step of a ship's motion.
//!
//! The wanted velocity comes from WASD or from an order's target. The ship reaches it
//! quickly (`accel`) and sheds speed faster still (`brake`), so letting go stops it in
//! a few ticks with no drift. Its heading swings toward the wanted direction at the
//! hull's turn rate, and while it points away from where it goes it loses some speed
//! (`turn_slowdown`): nimble hulls feel nimble without anything coasting.

use std::f32::consts::{PI, TAU};

use galaxy_contract::prelude::*;
use galaxy_contract::ship::FlightRules;

/// Below this speed a braking ship is stopped.
const REST: f32 = 0.01;

/// A hull's handling under the current rules.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Feel {
    /// Top speed, world units per second.
    pub speed: f32,
    /// World units per second squared toward the wanted velocity.
    pub accel: f32,
    /// World units per second squared when slowing down.
    pub brake: f32,
    /// Radians per second.
    pub turn: f32,
    /// Share of top speed lost while moving directly away from the heading.
    pub slowdown: f32,
}

impl Feel {
    pub fn of(rules: &FlightRules, hull: &Hull) -> Feel {
        let speed = hull.max_speed.max(0.0);
        let accel = hull
            .accel
            .unwrap_or_else(|| speed / rules.accel_time.max(f32::EPSILON))
            .max(0.0);
        Feel {
            speed,
            accel,
            brake: accel * rules.brake.max(1.0),
            turn: hull
                .turn_rate
                .unwrap_or(rules.turn_rate)
                .max(0.0)
                .to_radians(),
            slowdown: rules.turn_slowdown.clamp(0.0, 1.0),
        }
    }

    /// The feel with the ship's speed and turn modifiers applied.
    pub fn modified(self, m: &galaxy_contract::modifiers::Modifiers) -> Feel {
        let speed = (m.speed as f32).max(0.0);
        let turn = (m.turn_rate as f32).max(0.0);
        Feel {
            speed: self.speed * speed,
            accel: self.accel * speed,
            brake: self.brake * speed,
            turn: self.turn * turn,
            slowdown: self.slowdown,
        }
    }
}

/// Where a ship is and how it moves.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Motion {
    pub position: Vec2,
    pub velocity: Vec2,
    /// Radians, 0 = +x, counter-clockwise, kept in `-PI..=PI`.
    pub heading: f32,
}

impl Motion {
    pub fn of(ship: &Ship) -> Motion {
        Motion {
            position: ship.position,
            velocity: ship.velocity,
            heading: ship.heading,
        }
    }
}

/// What the ship wants to do this tick.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Steer {
    /// Stop.
    Brake,
    /// Move this way; the length (0..=1) is the share of top speed.
    Thrust(Vec2),
    /// Come to rest at this point without overshooting.
    Arrive(Vec2),
}

/// One flight step of `dt` seconds.
pub fn step(feel: &Feel, m: Motion, steer: Steer, dt: f32) -> Motion {
    let (dir, mut speed) = match steer {
        Steer::Brake => (Vec2::ZERO, 0.0),
        Steer::Thrust(t) => {
            let t = t.clamp_length(1.0);
            (t.normalize_or_zero(), feel.speed * t.length())
        }
        Steer::Arrive(at) => {
            let to = at - m.position;
            let dist = to.length();
            // Fast enough to get there, slow enough to stop on the spot.
            let stop = (2.0 * feel.brake * dist).sqrt();
            let speed = feel.speed.min(stop).min(dist / dt.max(f32::EPSILON));
            (to.normalize_or_zero(), speed)
        }
    };

    let mut heading = m.heading;
    if dir != Vec2::ZERO {
        let want = dir.y.atan2(dir.x);
        heading = turn_toward(heading, want, feel.turn * dt);
        let off = angle_diff(want, heading);
        speed *= 1.0 - feel.slowdown * (1.0 - off.cos()) * 0.5;
    }
    let wanted = dir * speed;

    let v = m.velocity;
    // Slowing along the current motion (stopping, reversing, turning) uses the brake.
    let rate = if wanted.dot(v) < v.dot(v) {
        feel.brake
    } else {
        feel.accel
    };
    let mut velocity = v + (wanted - v).clamp_length(rate * dt);
    if wanted == Vec2::ZERO && velocity.length() < REST {
        velocity = Vec2::ZERO;
    }
    Motion {
        position: m.position + velocity * dt,
        velocity,
        heading,
    }
}

/// `from` turned toward `to` by at most `max` radians.
pub fn turn_toward(from: f32, to: f32, max: f32) -> f32 {
    let d = angle_diff(to, from);
    wrap(if d.abs() <= max {
        to
    } else {
        from + max.copysign(d)
    })
}

/// `a - b` wrapped into `-PI..=PI`.
pub fn angle_diff(a: f32, b: f32) -> f32 {
    wrap(a - b)
}

fn wrap(a: f32) -> f32 {
    let mut a = a % TAU;
    if a > PI {
        a -= TAU;
    } else if a < -PI {
        a += TAU;
    }
    a
}

trait Dot {
    fn dot(self, o: Self) -> f32;
}

impl Dot for Vec2 {
    fn dot(self, o: Vec2) -> f32 {
        self.x * o.x + self.y * o.y
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wren() -> Feel {
        Feel {
            speed: 60.0,
            accel: 600.0,
            brake: 900.0,
            turn: 270f32.to_radians(),
            slowdown: 0.5,
        }
    }

    #[test]
    fn reaches_top_speed_quickly_and_stops_on_release() {
        let feel = wren();
        let dt = TICK_SECONDS;
        let mut m = Motion {
            position: Vec2::ZERO,
            velocity: Vec2::ZERO,
            heading: 0.0,
        };
        for _ in 0..6 {
            m = step(&feel, m, Steer::Thrust(Vec2::new(1.0, 0.0)), dt);
        }
        assert!((m.velocity.length() - 60.0).abs() < 0.01, "{m:?}");
        let mut ticks = 0;
        while m.velocity != Vec2::ZERO {
            m = step(&feel, m, Steer::Brake, dt);
            ticks += 1;
            assert!(ticks < 10, "still moving: {m:?}");
        }
        assert!(ticks <= 3, "took {ticks} ticks to stop");
    }

    #[test]
    fn arrive_stops_on_the_point() {
        let feel = wren();
        let target = Vec2::new(100.0, -40.0);
        let mut m = Motion {
            position: Vec2::ZERO,
            velocity: Vec2::ZERO,
            heading: 0.0,
        };
        for _ in 0..300 {
            m = step(&feel, m, Steer::Arrive(target), TICK_SECONDS);
        }
        assert!(m.position.distance(target) < 0.5, "{m:?}");
        assert_eq!(m.velocity, Vec2::ZERO);
    }

    #[test]
    fn turns_at_the_turn_rate() {
        assert!((turn_toward(0.0, PI, 0.5) - 0.5).abs() < 1e-6);
        assert!((turn_toward(3.0, -3.0, 1.0) - (-3.0)).abs() < 1e-6);
        assert!((angle_diff(-3.0, 3.0) - (TAU - 6.0)).abs() < 1e-5);
    }
}
