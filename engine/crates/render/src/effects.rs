//! Effect styles a module names from its components: beams (`[beams.<name>]`) and
//! particle recipes (`[particles.<name>]`) in `style.toml`. Plain data with checked
//! ranges; the view turns them into [`crate::Line`]s, [`crate::Glow`]s and
//! [`crate::Particle`]s each frame. How it works: `engine/docs/effects.md`.

use galaxy_mesh::Color;
use serde::{Deserialize, Serialize};

use crate::scene::ParticleShape;

/// How a beam is drawn: `engine.beam { look = "<module>.<name>" }`.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct BeamStyle {
    pub color: Color,
    /// Width in world units.
    pub width: f32,
    /// Strength of the soft glow round the beam and of the halos at its ends; 0 leaves
    /// the hot core alone.
    pub glow: f32,
    /// Radius of the halo where the beam lands, world units (the one at its source is
    /// half as big).
    pub glow_size: f32,
    /// How much its brightness flickers, 0 (steady) to 1.
    pub flicker: f32,
    /// How fast it flickers, times a second.
    pub flicker_rate: f32,
    /// Particle recipes (`<module>.<name>`) sprayed where it lands.
    pub impact: Vec<String>,
}

impl Default for BeamStyle {
    fn default() -> Self {
        Self {
            color: Color::hex(0x7ff0d8),
            width: 0.5,
            glow: 1.0,
            glow_size: 1.4,
            flicker: 0.15,
            flicker_rate: 1.5,
            impact: Vec::new(),
        }
    }
}

/// How particles are blended over the scene.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Blend {
    /// Added to what is beneath: sparks, embers and engine glow.
    #[default]
    Add,
    /// Painted over what is beneath: chips and puffs, inked like the models.
    Paint,
}

/// A particle recipe: what an emitter, a burst or a beam's landing sprays.
#[derive(Clone, Copy, Debug, PartialEq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct ParticleRecipe {
    pub shape: ParticleShape,
    pub blend: Blend,
    pub color: Color,
    /// The colour at the end of a particle's life; it fades out as it gets there.
    pub end_color: Color,
    /// Particles in one burst.
    pub count: u32,
    /// Particles a second from an emitter or a beam's landing.
    pub rate: f32,
    /// Seconds a particle lives.
    pub life: f32,
    /// Up to this share of particles' lives is cut short at random.
    pub life_jitter: f32,
    /// Starting speed, world units a second.
    pub speed: f32,
    /// Up to this share of the speed is taken off at random.
    pub speed_jitter: f32,
    /// Which way they fly, degrees from the emitter's facing (0 ahead, 180 behind); from
    /// a beam's landing, from the way back up the beam.
    pub direction: f32,
    /// How wide they fan out round that way, degrees (360 is every way).
    pub spread: f32,
    /// Diameter at birth and at the end of life, world units.
    pub size: f32,
    pub end_size: f32,
    /// Share of speed lost a second (it slows smoothly, never reverses).
    pub drag: f32,
    /// Share of the emitter's own velocity a particle keeps (0 stays where it was born).
    pub inherit: f32,
}

impl Default for ParticleRecipe {
    fn default() -> Self {
        Self {
            shape: ParticleShape::Spark,
            blend: Blend::Add,
            color: Color::hex(0xffd27a),
            end_color: Color::hex(0xff7a3c),
            count: 12,
            rate: 24.0,
            life: 0.5,
            life_jitter: 0.4,
            speed: 6.0,
            speed_jitter: 0.5,
            direction: 0.0,
            spread: 70.0,
            size: 0.18,
            end_size: 0.05,
            drag: 2.0,
            inherit: 0.0,
        }
    }
}

/// The most particles one burst makes, and one emitter makes a second.
pub const MAX_COUNT: u32 = 256;
pub const MAX_RATE: f32 = 400.0;
/// The longest a particle lives, seconds.
pub const MAX_LIFE: f32 = 4.0;

fn within(name: &str, v: f32, lo: f32, hi: f32) -> Result<(), String> {
    if v.is_finite() && (lo..=hi).contains(&v) {
        Ok(())
    } else {
        Err(format!("{name} is {v}; it is {lo} to {hi}"))
    }
}

impl BeamStyle {
    pub(crate) fn check(&self) -> Result<(), String> {
        within("width", self.width, 0.0, 8.0)?;
        within("glow", self.glow, 0.0, 4.0)?;
        within("glow_size", self.glow_size, 0.0, 32.0)?;
        within("flicker", self.flicker, 0.0, 1.0)?;
        within("flicker_rate", self.flicker_rate, 0.0, 30.0)
    }
}

impl ParticleRecipe {
    pub(crate) fn check(&self) -> Result<(), String> {
        if self.count > MAX_COUNT {
            return Err(format!("count is {}; it is 0 to {MAX_COUNT}", self.count));
        }
        within("rate", self.rate, 0.0, MAX_RATE)?;
        within("life", self.life, 0.0, MAX_LIFE)?;
        within("life_jitter", self.life_jitter, 0.0, 1.0)?;
        within("speed", self.speed, 0.0, 200.0)?;
        within("speed_jitter", self.speed_jitter, 0.0, 1.0)?;
        within("direction", self.direction, -360.0, 360.0)?;
        within("spread", self.spread, 0.0, 360.0)?;
        within("size", self.size, 0.0, 16.0)?;
        within("end_size", self.end_size, 0.0, 16.0)?;
        within("drag", self.drag, 0.0, 20.0)?;
        within("inherit", self.inherit, 0.0, 1.0)
    }
}
