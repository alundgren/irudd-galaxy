//! World geometry. Positions are world coordinates on the sim plane, in world units.
//! The sim never knows how anything is drawn; a renderer maps these into its own
//! (3D-ready) scene.

use std::ops::{Add, AddAssign, Mul, Neg, Sub};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Trig for the sim. `f32::sin` and friends call the platform's C maths library, which
/// gives slightly different results on macOS than on Windows or Linux, so a replay
/// recorded on one drifts on another. These are the same on every platform.
pub fn sin(a: f32) -> f32 {
    libm::sinf(a)
}
pub fn cos(a: f32) -> f32 {
    libm::cosf(a)
}
pub fn atan2(y: f32, x: f32) -> f32 {
    libm::atan2f(y, x)
}

/// A point or vector on the sim plane.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Vec2 {
    pub x: f32,
    pub y: f32,
}

impl Vec2 {
    pub const ZERO: Vec2 = Vec2 { x: 0.0, y: 0.0 };

    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }

    pub fn length(self) -> f32 {
        libm::hypotf(self.x, self.y)
    }

    pub fn distance(self, other: Vec2) -> f32 {
        (self - other).length()
    }

    /// Unit vector in the same direction, or zero for a zero vector.
    pub fn normalize_or_zero(self) -> Vec2 {
        let len = self.length();
        if len > 0.0 {
            self * (1.0 / len)
        } else {
            Vec2::ZERO
        }
    }

    /// Clamps the length to at most `max`.
    pub fn clamp_length(self, max: f32) -> Vec2 {
        let len = self.length();
        if len > max && len > 0.0 {
            self * (max / len)
        } else {
            self
        }
    }
}

impl Add for Vec2 {
    type Output = Vec2;
    fn add(self, o: Vec2) -> Vec2 {
        Vec2::new(self.x + o.x, self.y + o.y)
    }
}
impl AddAssign for Vec2 {
    fn add_assign(&mut self, o: Vec2) {
        *self = *self + o;
    }
}
impl Sub for Vec2 {
    type Output = Vec2;
    fn sub(self, o: Vec2) -> Vec2 {
        Vec2::new(self.x - o.x, self.y - o.y)
    }
}
impl Mul<f32> for Vec2 {
    type Output = Vec2;
    fn mul(self, k: f32) -> Vec2 {
        Vec2::new(self.x * k, self.y * k)
    }
}
impl Neg for Vec2 {
    type Output = Vec2;
    fn neg(self) -> Vec2 {
        Vec2::new(-self.x, -self.y)
    }
}
