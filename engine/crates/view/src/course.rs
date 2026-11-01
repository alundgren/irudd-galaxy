//! The course: where the followed thing's order is taking it.
//!
//! Round dots run from the thing to where it is going, through the waypoints left on a
//! route, with a ring round where it ends. The dot pattern is measured from the far end,
//! so the dots stay still in the world and the ship flies over them, eating them, rather
//! than the pattern sliding along with it. The dots nearest the ship fade out instead of
//! popping. The leg being flown is brightest; later legs are fainter and smaller, and each
//! waypoint left has a small ring.
//!
//! It is at full strength for a few seconds after an order is given, then settles faint.
//! On arrival the ring closes in a little and fades; when the player takes the controls
//! the whole course fades out quickly, so it is clear the order is gone. Sizes are in
//! logical pixels, so it reads the same at every zoom. Timing uses presentation time
//! only; nothing here touches the world.

use galaxy_render::glam::Vec3;
use galaxy_render::style::Course;
use galaxy_render::{Line, Scene};

/// Seconds the course takes to settle from full strength to faint.
const SETTLE: f32 = 0.6;
/// Seconds the ring takes to close and fade on arrival.
const ARRIVE: f32 = 0.45;
/// Seconds the course takes to fade when the order is set aside.
const DROP: f32 = 0.2;
/// Logical pixels over which the dots nearest the ship fade.
const NEAR_FADE: f32 = 16.0;

/// Where an order is going, this frame.
#[derive(Clone, Debug, PartialEq)]
pub struct Plan {
    /// Which order this is: a new key is a new order.
    pub key: String,
    /// The thing, then each point it will pass, ending where it stops.
    pub points: Vec<Vec3>,
    /// A ring radius for each point after the first (world units).
    pub rings: Vec<f32>,
}

enum Ending {
    Arrived,
    Dropped,
}

struct Shown {
    plan: Plan,
    since: f32,
}

struct Fading {
    plan: Plan,
    since: f32,
    strength: f32,
    ending: Ending,
}

/// The course over time: the order shown now, and one fading out.
#[derive(Default)]
pub struct Courses {
    /// A course first seen from now on starts this many seconds old (an audit drawing a
    /// note's moment again).
    pub start_age: Option<f32>,
    shown: Option<Shown>,
    fading: Option<Fading>,
}

/// How strong a course is `age` seconds after its order.
fn strength(style: &Course, age: f32) -> f32 {
    let faint = style.faint.clamp(0.0, 1.0);
    let t = ((age - style.bright.max(0.0)) / SETTLE).clamp(0.0, 1.0);
    1.0 + (faint - 1.0) * t * t * (3.0 - 2.0 * t)
}

impl Courses {
    /// No course shown yet; the first one seen starts `age` seconds old.
    pub fn starting_at(age: Option<f32>) -> Courses {
        Courses {
            start_age: age,
            ..Courses::default()
        }
    }

    /// How long the course shown now has been up, at `time`.
    pub fn age(&self, time: f32) -> Option<f32> {
        self.shown.as_ref().map(|s| time - s.since)
    }

    /// Draws the course for this frame. `plan` is the order under way, if any; `arrived`
    /// says whether an order that just ended got where it was going. `per_pixel` is world
    /// units per logical pixel.
    pub fn draw(
        &mut self,
        plan: Option<Plan>,
        arrived: bool,
        style: &Course,
        time: f32,
        per_pixel: f32,
        scene: &mut Scene,
    ) {
        match plan {
            Some(plan) => {
                let same = self.shown.as_ref().is_some_and(|s| s.plan.key == plan.key);
                if same {
                    self.shown.as_mut().expect("checked").plan = plan;
                } else {
                    let age = self.start_age.unwrap_or(0.0);
                    self.shown = Some(Shown {
                        plan,
                        since: time - age,
                    });
                    self.fading = None;
                }
            }
            None => {
                if let Some(old) = self.shown.take() {
                    self.fading = Some(Fading {
                        strength: strength(style, time - old.since),
                        plan: old.plan,
                        since: time,
                        ending: if arrived {
                            Ending::Arrived
                        } else {
                            Ending::Dropped
                        },
                    });
                }
            }
        }
        if !style.show {
            return;
        }
        if let Some(s) = &self.shown {
            let a = strength(style, time - s.since);
            lines(&s.plan, style, a, per_pixel, scene);
            rings(&s.plan, style, a, 1.0, true, per_pixel, scene);
        }
        let mut done = false;
        if let Some(f) = &self.fading {
            let age = time - f.since;
            match f.ending {
                Ending::Arrived => {
                    let e = (age / ARRIVE).clamp(0.0, 1.0);
                    // The final ring closes in a little as it fades; nothing else stays.
                    let a = (1.0 - e) * f.strength.max(0.6);
                    rings(&f.plan, style, a, 1.0 - 0.25 * e, false, per_pixel, scene);
                    done = e >= 1.0;
                }
                Ending::Dropped => {
                    let e = (age / DROP).clamp(0.0, 1.0);
                    let a = (1.0 - e) * f.strength;
                    lines(&f.plan, style, a, per_pixel, scene);
                    rings(&f.plan, style, a, 1.0, true, per_pixel, scene);
                    done = e >= 1.0;
                }
            }
        }
        if done {
            self.fading = None;
        }
    }
}

/// The dotted legs. Each leg runs from its far end back toward the thing, with its
/// pattern starting where the next leg's ends, so the whole route shares one still
/// pattern measured from where it stops.
fn lines(plan: &Plan, style: &Course, strength: f32, per_pixel: f32, scene: &mut Scene) {
    let n = plan.points.len();
    if n < 2 || strength <= 0.0 {
        return;
    }
    let dot = style.dot.max(1.0) * per_pixel;
    let spacing = style.spacing.max(style.dot.max(1.0) * 1.5) * per_pixel;
    let mut points = plan.points.clone();
    // The last leg stops at the final ring rather than running over the target.
    let end_ring = plan.rings.last().copied().unwrap_or(0.0);
    let (a, b) = (points[n - 2], points[n - 1]);
    if (b - a).length() > end_ring {
        points[n - 1] = b - (b - a).normalize() * end_ring;
    } else {
        points.truncate(n - 1);
    }
    let mut from_end = 0.0;
    for i in (0..points.len().saturating_sub(1)).rev() {
        let (near, far) = (points[i], points[i + 1]);
        let first = i == 0;
        let (alpha, size) = if first {
            (strength, dot)
        } else {
            (strength * 0.6, dot * 0.75)
        };
        let fade = if first { NEAR_FADE * per_pixel } else { 0.0 };
        scene.lines.push(
            Line::path(vec![far, near], size, style.color.with_alpha(alpha))
                .dotted(spacing, fade)
                .with_phase(from_end),
        );
        from_end += (far - near).length();
    }
}

/// The rings: the final one, and when `waypoints` a small one on each point passed on
/// the way. `scale` sizes the final ring.
fn rings(
    plan: &Plan,
    style: &Course,
    strength: f32,
    scale: f32,
    waypoints: bool,
    per_pixel: f32,
    scene: &mut Scene,
) {
    if strength <= 0.0 {
        return;
    }
    let width = style.ring.max(1.0) * per_pixel;
    let last = plan.rings.len().saturating_sub(1);
    for (i, (&at, &radius)) in plan.points[1..].iter().zip(&plan.rings).enumerate() {
        if i == last {
            scene.lines.push(Line::circle(
                at,
                radius * scale,
                width,
                style.color.with_alpha(strength),
            ));
        } else if waypoints {
            scene.lines.push(Line::circle(
                at,
                radius * 0.7,
                width * 0.75,
                style.color.with_alpha(strength * 0.6),
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use galaxy_render::{Camera, glam::Vec2};

    fn plan(key: &str) -> Plan {
        Plan {
            key: key.into(),
            points: vec![
                Vec3::ZERO,
                Vec3::new(10.0, 0.0, 0.0),
                Vec3::new(10.0, 10.0, 0.0),
            ],
            rings: vec![1.0, 2.0],
        }
    }

    fn scene() -> Scene {
        Scene::new(Camera::top_down(Vec2::ZERO, 36.0))
    }

    #[test]
    fn the_course_starts_bright_then_settles_faint() {
        let style = Course::default();
        assert_eq!(strength(&style, 0.0), 1.0);
        assert_eq!(strength(&style, style.bright), 1.0);
        assert!((strength(&style, style.bright + SETTLE) - style.faint).abs() < 1e-6);
    }

    #[test]
    fn the_pattern_is_measured_from_where_the_route_stops() {
        let mut c = Courses::default();
        let mut s = scene();
        c.draw(
            Some(plan("a")),
            false,
            &Course::default(),
            0.0,
            0.05,
            &mut s,
        );
        // Two legs, the second drawn first from its far end, then two rings.
        let legs: Vec<&Line> = s.lines.iter().filter(|l| !l.closed).collect();
        assert_eq!(legs.len(), 2);
        assert_eq!(legs[0].phase, 0.0);
        assert_eq!(legs[0].points[1], Vec3::new(10.0, 0.0, 0.0));
        assert!((legs[1].phase - 8.0).abs() < 1e-5, "{}", legs[1].phase);
        assert_eq!(legs[1].points, [Vec3::new(10.0, 0.0, 0.0), Vec3::ZERO]);
        assert_eq!(s.lines.iter().filter(|l| l.closed).count(), 2);
    }

    #[test]
    fn moving_the_ship_keeps_the_dots_still() {
        let style = Course::default();
        let mut c = Courses::default();
        let mut a = scene();
        c.draw(Some(plan("a")), false, &style, 0.0, 0.05, &mut a);
        let mut moved = plan("a");
        moved.points[0] = Vec3::new(3.0, 0.0, 0.0);
        let mut b = scene();
        c.draw(Some(moved), false, &style, 0.1, 0.05, &mut b);
        // The leg near the ship starts at the same place in the pattern either way.
        let near = |s: &Scene| s.lines.iter().filter(|l| !l.closed).nth(1).unwrap().phase;
        assert_eq!(near(&a), near(&b));
    }

    #[test]
    fn a_course_drawn_again_for_a_note_is_as_old_as_it_was() {
        let style = Course::default();
        let mut c = Courses::starting_at(Some(style.bright + SETTLE + 1.0));
        let mut s = scene();
        c.draw(Some(plan("a")), false, &style, 50.0, 0.05, &mut s);
        let ring = s.lines.iter().rev().find(|l| l.closed).unwrap();
        assert!(
            (ring.color.a - style.faint).abs() < 1e-6,
            "{}",
            ring.color.a
        );
    }

    #[test]
    fn arriving_leaves_only_the_ring_and_it_fades() {
        let style = Course::default();
        let mut c = Courses::default();
        c.draw(Some(plan("a")), false, &style, 0.0, 0.05, &mut scene());
        let mut s = scene();
        c.draw(None, true, &style, 1.0, 0.05, &mut s);
        assert_eq!(s.lines.len(), 1);
        assert!(s.lines[0].closed);
        let mut later = scene();
        c.draw(None, true, &style, 1.0 + ARRIVE + 0.01, 0.05, &mut later);
        assert!(later.lines.is_empty());
    }

    #[test]
    fn dropping_an_order_fades_the_whole_course_quickly() {
        let style = Course::default();
        let mut c = Courses::default();
        c.draw(Some(plan("a")), false, &style, 0.0, 0.05, &mut scene());
        let mut s = scene();
        c.draw(None, false, &style, 0.5, 0.05, &mut s);
        assert_eq!(s.lines.len(), 4, "legs and rings, fading");
        let mut gone = scene();
        c.draw(None, false, &style, 0.5 + DROP + 0.01, 0.05, &mut gone);
        assert!(gone.lines.is_empty());
    }
}
