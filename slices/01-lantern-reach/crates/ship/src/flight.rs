//! Flight: one step of a ship's motion.
//!
//! By hand ([`fly`]) the ship has tank controls and inertia: A/D turn it at the hull's
//! turn rate, W/S thrust along its heading, Space brakes and Shift boosts. Inertia
//! decides how much of the old velocity survives a change: sideways speed swings
//! round into the new heading instead of vanishing, and coasting bleeds off. Letting
//! go of thrust starts the auto-brake, which stops the ship in the same time from any
//! speed.
//!
//! Orders ([`step`]) steer straight to where they go: the wanted velocity comes from
//! the order's target, the ship reaches it at `accel` and sheds speed at `brake`, and
//! its heading swings toward the wanted direction at the hull's turn rate. While it
//! points away from where it goes it loses some speed (`turn_slowdown`).
//!
//! Every number is data (`hulls.toml`, `ship_rules.toml`), and every maths call that
//! isn't exact IEEE arithmetic goes through `galaxy_contract::geom` or `libm`, so the
//! same inputs give the same bits on every OS.

use std::f32::consts::{PI, TAU};

use galaxy_contract::geom;
use galaxy_contract::modifiers::Modifiers;
use galaxy_contract::prelude::*;
use galaxy_contract::ship::FlightRules;

/// Below this speed a ship nobody pushes is stopped.
const REST: f32 = 0.01;

/// A hull's handling under the current rules.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Feel {
    /// Top speed, world units per second.
    pub speed: f32,
    /// World units per second squared of thrust.
    pub accel: f32,
    /// World units per second squared when braking.
    pub brake: f32,
    /// Radians per second.
    pub turn: f32,
    /// Share of top speed an order loses while moving directly away from the heading.
    pub slowdown: f32,
    /// Shift multiplies top speed and thrust by this.
    pub boost: f32,
    /// Seconds for leftover velocity to fall to 1/e while turning or coasting:
    /// 0 for no inertia, infinite for none lost.
    pub coast: f32,
    /// Ticks the auto-brake takes to stop the ship once thrust is let go; `None`: off.
    pub stop_ticks: Option<u32>,
}

impl Feel {
    pub fn of(rules: &FlightRules, hull: &Hull) -> Feel {
        let speed = hull.max_speed.max(0.0);
        let accel = hull
            .accel
            .unwrap_or_else(|| speed / rules.accel_time.max(f32::EPSILON))
            .max(0.0);
        let stop_ticks = rules.auto_brake.then(|| {
            let ticks = rules.stop_time.max(0.0) * TICKS_PER_SECOND as f32;
            ticks.round().min(u32::MAX as f32) as u32
        });
        Feel {
            speed,
            accel,
            brake: hull.brake.unwrap_or(accel * rules.brake).max(0.0),
            turn: hull
                .turn_rate
                .unwrap_or(rules.turn_rate)
                .max(0.0)
                .to_radians(),
            slowdown: rules.turn_slowdown.clamp(0.0, 1.0),
            boost: rules.boost.max(1.0),
            coast: coast_time(rules.inertia),
            stop_ticks,
        }
    }

    /// The feel with the ship's speed and turn modifiers applied.
    pub fn modified(self, m: &Modifiers) -> Feel {
        let speed = (m.speed as f32).max(0.0);
        let turn = (m.turn_rate as f32).max(0.0);
        Feel {
            speed: self.speed * speed,
            accel: self.accel * speed,
            brake: self.brake * speed,
            turn: self.turn * turn,
            ..self
        }
    }

    /// The feel of `ship` as it flies now: its hull under the world's rules, with its
    /// modifiers. `None` if its hull is unknown.
    pub fn of_ship(world: &WorldState, ship: &Ship) -> Option<Feel> {
        let hull = world.hulls.get(&ship.hull)?;
        let mods = galaxy_contract::modifiers::for_ship(world, &ship.id);
        Some(Feel::of(&world.ship_rules.flight, hull).modified(&mods))
    }

    /// The share of velocity that survives `dt` seconds of turning or coasting.
    pub fn keep(&self, dt: f32) -> f32 {
        if self.coast <= 0.0 {
            0.0
        } else if self.coast.is_infinite() {
            1.0
        } else {
            libm::expf(-dt / self.coast)
        }
    }
}

/// Seconds for leftover velocity to fall to 1/e at `inertia` (0..=1): 0 at 0, never
/// at 1. The curve is the flight lab's, so the numbers tuned there carry over.
pub fn coast_time(inertia: f32) -> f32 {
    let i = if inertia.is_finite() { inertia } else { 0.0 };
    if i <= 0.0 {
        0.0
    } else if i >= 1.0 {
        f32::INFINITY
    } else {
        0.35 * libm::powf(i / (1.0 - i), 1.3)
    }
}

/// The camera zoom for `ship`: the rules' zoom times its zoom modifiers. 1.0 is the
/// game's standard view; below 1 shows more.
pub fn camera_zoom(world: &WorldState, ship: Option<&Ship>) -> f32 {
    let base = world.ship_rules.flight.zoom;
    let factor = ship.map_or(1.0, |s| {
        galaxy_contract::modifiers::for_ship(world, &s.id).zoom as f32
    });
    let zoom = base * factor;
    if zoom.is_finite() && zoom > 0.0 {
        zoom
    } else {
        1.0
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

    /// The unit vector the nose points along.
    pub fn forward(&self) -> Vec2 {
        Vec2::new(geom::cos(self.heading), geom::sin(self.heading))
    }
}

/// The player's hands on the controls for a tick.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Hands {
    /// -1..=1: +1 turns clockwise (D), -1 counter-clockwise (A).
    pub turn: f32,
    /// -1..=1: +1 thrusts forward (W), -1 backward (S). The size is the share of top
    /// speed wanted.
    pub thrust: f32,
    /// Space: brake.
    pub brake: bool,
    /// Shift: boost.
    pub boost: bool,
}

impl Hands {
    /// The flight keys of `input`. Space brakes unless the caller used it for
    /// something else this tick.
    pub fn of(input: &PlayerInput) -> Hands {
        let axis = |a: f32| if a.is_finite() { a.clamp(-1.0, 1.0) } else { 0.0 };
        Hands {
            turn: axis(input.thrust.x),
            thrust: axis(input.thrust.y),
            brake: input.action,
            boost: input.boost,
        }
    }
}

/// One step of `dt` seconds flown by hand. `stopping` is the auto-brake's ticks left
/// (`ShipActivity::stopping`); returns the new motion and ticks left.
pub fn fly(feel: &Feel, m: Motion, stopping: u32, hands: Hands, dt: f32) -> (Motion, u32) {
    let heading = wrap(m.heading - hands.turn.clamp(-1.0, 1.0) * feel.turn * dt);
    let forward = Vec2::new(geom::cos(heading), geom::sin(heading));
    let mult = if hands.boost { feel.boost } else { 1.0 };
    let (top, accel) = (feel.speed * mult, feel.accel * mult);
    let keep = feel.keep(dt);
    let push = hands.thrust.clamp(-1.0, 1.0);
    let mut v = m.velocity;
    let mut stopping = stopping;
    if push != 0.0 {
        stopping = 0;
        let dir = if push > 0.0 { forward } else { -forward };
        let target = top * push.abs();
        let mut along = v.dot(dir);
        let side = v - dir * along;
        if along >= 0.0 {
            // Grip: the sideways speed inertia doesn't keep swings round into the
            // new heading instead of vanishing.
            along = (along * along + side.dot(side) * (1.0 - keep * keep)).sqrt();
        } else {
            along *= keep;
        }
        along = if along < target {
            (along + accel * dt).min(target)
        } else {
            target.max((along - accel * dt).min(target + (along - target) * keep))
        };
        v = dir * along + side * keep;
    } else if let Some(ticks) = feel.stop_ticks.filter(|_| !hands.brake) {
        // The auto-brake: speed falls in a straight line to zero over the stop, from
        // whatever it was when thrust was let go.
        if stopping == 0 && v != Vec2::ZERO {
            stopping = ticks;
        }
        if stopping == 0 {
            v = Vec2::ZERO;
        } else {
            v = v * ((stopping - 1) as f32 / stopping as f32);
            stopping -= 1;
        }
    } else {
        stopping = 0;
        v = v * keep;
    }
    if hands.brake && feel.brake > 0.0 {
        let speed = v.length();
        if speed > 0.0 {
            v = v * ((speed - feel.brake * dt).max(0.0) / speed);
        }
    }
    v = v.clamp_length(feel.speed * feel.boost.max(1.0));
    if push == 0.0 && v.length() < REST {
        v = Vec2::ZERO;
        stopping = 0;
    }
    let motion = Motion {
        position: m.position + v * dt,
        velocity: v,
        heading,
    };
    (motion, stopping)
}

/// What an order wants the ship to do this tick.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Steer {
    /// Stop.
    Brake,
    /// Come to rest at this point without overshooting.
    Arrive(Vec2),
    /// The player flies by hand.
    Hands(Hands),
}

/// One step of `dt` seconds steered by an order: straight to the point, no coasting.
/// [`Steer::Hands`] is flown by [`fly`]; here it brakes.
pub fn step(feel: &Feel, m: Motion, steer: Steer, dt: f32) -> Motion {
    let (dir, mut speed) = match steer {
        Steer::Brake | Steer::Hands(_) => (Vec2::ZERO, 0.0),
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
        let want = geom::atan2(dir.y, dir.x);
        heading = turn_toward(heading, want, feel.turn * dt);
        let off = angle_diff(want, heading);
        speed *= 1.0 - feel.slowdown * (1.0 - geom::cos(off)) * 0.5;
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

/// The flight keys that take a ship flown by hand toward `to` and park it within
/// `close`: turn to face it, thrust while there's room to stop, then let go and let
/// the auto-brake stop it. Bots and tests steer with this, through the same input the
/// keyboard gives; it returns zero once the ship is within `close` (it may still be
/// coasting to a stop).
pub fn steer_to(feel: &Feel, m: Motion, to: Vec2, close: f32) -> Vec2 {
    let off = to - m.position;
    let dist = off.length();
    if dist <= close {
        return Vec2::ZERO;
    }
    let want = geom::atan2(off.y, off.x);
    let err = angle_diff(want, m.heading);
    let per_tick = (feel.turn * TICK_SECONDS).max(f32::EPSILON);
    let turn = (-err / per_tick).clamp(-1.0, 1.0);
    // Facing roughly the right way: ask for the speed that still stops in time.
    // Thrusting toward a lower speed sheds it at `accel`, and letting go coasts half
    // the auto-brake's stop at the speed let go of, so this eases in.
    let thrust = if err.abs() < 0.6 {
        let room = (dist - close * 0.5).max(0.0);
        let coast = feel
            .stop_ticks
            .map_or(f32::INFINITY, |t| 2.0 * room / (t as f32 * TICK_SECONDS));
        let speed = (1.2 * feel.accel * room)
            .sqrt()
            .min(coast)
            .min(feel.speed);
        let facing = geom::cos(err);
        (speed / feel.speed.max(f32::EPSILON) * facing).clamp(0.0, 1.0)
    } else {
        0.0
    };
    Vec2::new(turn, thrust)
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

    /// The locked preset (issue #47): the flight lab's numbers at 1 lab px = 0.55 u.
    fn wren() -> Feel {
        let rules = FlightRules::default();
        Feel {
            speed: 165.0,
            accel: 247.5,
            brake: 192.5,
            turn: 180f32.to_radians(),
            slowdown: 0.5,
            boost: 2.0,
            coast: coast_time(rules.inertia),
            stop_ticks: Some(30),
        }
    }

    fn rest() -> Motion {
        Motion {
            position: Vec2::ZERO,
            velocity: Vec2::ZERO,
            heading: 0.0,
        }
    }

    fn hands(turn: f32, thrust: f32) -> Hands {
        Hands {
            turn,
            thrust,
            ..Hands::default()
        }
    }

    /// Flies `n` ticks with `h`, returning the motion and the stop's ticks left.
    fn run(feel: &Feel, mut m: Motion, mut stopping: u32, h: Hands, n: u32) -> (Motion, u32) {
        for _ in 0..n {
            (m, stopping) = fly(feel, m, stopping, h, TICK_SECONDS);
        }
        (m, stopping)
    }

    #[test]
    fn w_reaches_top_speed_and_shift_doubles_it() {
        let feel = wren();
        let (m, _) = run(&feel, rest(), 0, hands(0.0, 1.0), 90);
        assert!((m.velocity.length() - 165.0).abs() < 0.01, "{m:?}");
        assert!(m.velocity.y.abs() < 1e-3 && m.velocity.x > 0.0, "along the nose");
        let boost = Hands {
            boost: true,
            ..hands(0.0, 1.0)
        };
        let (m, _) = run(&feel, m, 0, boost, 90);
        assert!((m.velocity.length() - 330.0).abs() < 0.01, "{m:?}");
    }

    #[test]
    fn letting_go_stops_in_one_second_from_any_speed() {
        let feel = wren();
        for (speed, boost) in [(40.0, false), (165.0, false), (330.0, true)] {
            let m = Motion {
                velocity: Vec2::new(speed, 0.0),
                ..rest()
            };
            // Half a second in, it's at half speed; at one second it's stopped.
            let (half, left) = run(&feel, m, 0, Hands::default(), 15);
            assert!(
                (half.velocity.length() - speed * 0.5).abs() < 0.01,
                "{speed}: {half:?}"
            );
            assert_eq!(left, 15);
            let (end, left) = run(&feel, half, left, Hands::default(), 15);
            assert_eq!(end.velocity, Vec2::ZERO, "{speed} (boost {boost}): {end:?}");
            assert_eq!(left, 0);
            // It coasts about half the release speed's worth of a second.
            let coasted = end.position.x;
            assert!(
                (coasted - speed * 0.5).abs() < speed * 0.04,
                "{speed}: coasted {coasted}"
            );
            // And stays put: no more motion, no stop under way.
            let (still, left) = run(&feel, end, 0, Hands::default(), 5);
            assert_eq!((still, left), (end, 0));
        }
    }

    #[test]
    fn a_d_turn_at_the_turn_rate_and_s_thrusts_backward() {
        let feel = wren();
        // Half a second of D turns 90 degrees clockwise.
        let (m, _) = run(&feel, rest(), 0, hands(1.0, 0.0), 15);
        assert!((m.heading + PI / 2.0).abs() < 1e-4, "{m:?}");
        assert_eq!(m.velocity, Vec2::ZERO, "turning alone doesn't move it");
        let (m, _) = run(&feel, rest(), 0, hands(-1.0, 0.0), 15);
        assert!((m.heading - PI / 2.0).abs() < 1e-4, "A turns the other way");
        let (m, _) = run(&feel, rest(), 0, hands(0.0, -1.0), 90);
        assert!((m.velocity.x + 165.0).abs() < 0.01, "S backs off: {m:?}");
    }

    #[test]
    fn inertia_swings_sideways_speed_round_into_the_new_heading() {
        let feel = wren();
        // Flying +x at top speed, then turned to face +y and thrusting.
        let m = Motion {
            velocity: Vec2::new(165.0, 0.0),
            heading: PI / 2.0,
            ..rest()
        };
        let (m1, _) = run(&feel, m, 0, hands(0.0, 1.0), 1);
        assert!(m1.velocity.x > 0.0 && m1.velocity.x < 165.0, "{m1:?}");
        assert!(m1.velocity.y > 0.0);
        // Within a second the old sideways speed is all but gone.
        let (m2, _) = run(&feel, m1, 0, hands(0.0, 1.0), 30);
        assert!(m2.velocity.x < 2.0 && m2.velocity.y > 160.0, "{m2:?}");
    }

    #[test]
    fn space_brakes_harder_than_coasting() {
        let feel = wren();
        let m = Motion {
            velocity: Vec2::new(165.0, 0.0),
            ..rest()
        };
        let brake = Hands {
            brake: true,
            ..Hands::default()
        };
        let mut braked = m;
        let mut ticks = 0;
        while braked.velocity != Vec2::ZERO {
            (braked, _) = fly(&feel, braked, 0, brake, TICK_SECONDS);
            ticks += 1;
            assert!(ticks < 30, "still moving: {braked:?}");
        }
        assert!(braked.position.x < 165.0 * 0.5, "{braked:?}");
    }

    #[test]
    fn the_same_inputs_give_the_same_bits() {
        let feel = wren();
        let script = [hands(1.0, 1.0), hands(-0.5, 1.0), hands(0.0, 0.0)];
        let fly_it = || {
            let mut m = rest();
            let mut s = 0;
            for (i, h) in script.iter().cycle().take(300).enumerate() {
                let h = Hands {
                    boost: i % 7 == 0,
                    brake: i % 11 == 0,
                    ..*h
                };
                (m, s) = fly(&feel, m, s, h, TICK_SECONDS);
            }
            (m, s)
        };
        let (a, b) = (fly_it(), fly_it());
        assert_eq!(a.0.position.x.to_bits(), b.0.position.x.to_bits());
        assert_eq!(a.0.position.y.to_bits(), b.0.position.y.to_bits());
        assert_eq!(a.1, b.1);
    }

    #[test]
    fn steer_to_parks_near_the_point() {
        let feel = wren();
        let target = Vec2::new(-300.0, 420.0);
        let mut m = rest();
        let mut s = 0;
        let mut ticks = 0;
        loop {
            let axes = steer_to(&feel, m, target, 8.0);
            if axes == Vec2::ZERO && m.velocity == Vec2::ZERO {
                break;
            }
            let h = hands(axes.x, axes.y);
            (m, s) = fly(&feel, m, s, h, TICK_SECONDS);
            ticks += 1;
            assert!(ticks < 900, "never parked: {m:?}");
        }
        assert!(m.position.distance(target) <= 8.0 + 1.0, "{m:?}");
    }

    #[test]
    fn arrive_stops_on_the_point() {
        let feel = wren();
        let target = Vec2::new(100.0, -40.0);
        let mut m = rest();
        for _ in 0..300 {
            m = step(&feel, m, Steer::Arrive(target), TICK_SECONDS);
        }
        assert!(m.position.distance(target) < 0.5, "{m:?}");
        assert_eq!(m.velocity, Vec2::ZERO);
    }

    #[test]
    fn the_preset_reads_from_data() {
        let rules = FlightRules::default();
        let hull = Hull {
            id: HullId::from_static("hull.test"),
            name: "Test".into(),
            tier: 0,
            hold_capacity: 1,
            max_speed: 165.0,
            mining_rate: 1.0,
            accel: Some(247.5),
            turn_rate: Some(180.0),
            brake: Some(192.5),
            standing_berth: false,
            coupling: false,
            price_reach: Default::default(),
        };
        let mut feel = Feel::of(&rules, &hull);
        assert_eq!(feel.stop_ticks, Some(30));
        feel.coast = wren().coast;
        assert_eq!(feel, wren());
        let faster = Feel::of(&rules, &hull).modified(&Modifiers {
            speed: 1.1,
            turn_rate: 0.5,
            ..Modifiers::default()
        });
        assert!((faster.speed - 181.5).abs() < 1e-3);
        assert!((faster.turn - 90f32.to_radians()).abs() < 1e-5);
        let off = FlightRules {
            auto_brake: false,
            ..rules
        };
        assert_eq!(Feel::of(&off, &hull).stop_ticks, None);
    }

    #[test]
    fn turns_at_the_turn_rate() {
        assert!((turn_toward(0.0, PI, 0.5) - 0.5).abs() < 1e-6);
        assert!((turn_toward(3.0, -3.0, 1.0) - (-3.0)).abs() < 1e-6);
        assert!((angle_diff(-3.0, 3.0) - (TAU - 6.0)).abs() < 1e-5);
    }
}
