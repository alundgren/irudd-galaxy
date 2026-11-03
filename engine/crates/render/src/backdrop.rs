//! A calm space backdrop: a deep gradient, a few soft nebula clouds and sparse stars.
//!
//! The backdrop lives on a square tile measured in screen pixels that repeats forever and
//! scrolls with the camera at a small parallax, so it never competes with the play field.
//! Stars can lie in several layers, each scrolling at its own rate, so the far ones creep
//! and the near ones slide: depth without competing either.

use crate::defaults;
use crate::rng::Rng;
use galaxy_mesh::Color;
use glam::Vec2;
use serde::{Deserialize, Serialize};

/// Parameters for [`backdrop`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BackdropParams {
    /// Edge of the repeating tile in logical pixels.
    pub tile: f32,
    pub stars: u32,
    pub clouds: u32,
    /// Gradient colour at the top of the screen.
    pub top: Color,
    /// Gradient colour at the bottom of the screen.
    pub bottom: Color,
    /// Nebula colours; clouds pick from these.
    pub cloud_colors: Vec<Color>,
    /// Peak opacity of a cloud. Keep it low so the backdrop stays quiet.
    pub cloud_alpha: f32,
    /// Each star layer's parallax: logical pixels the layer scrolls per world unit the
    /// camera moves. Stars are shared out among the layers; nearer (faster) layers have
    /// slightly bigger stars.
    pub star_layers: Vec<f32>,
}

/// The star parallax with one layer, logical pixels per world unit.
pub const STAR_PARALLAX: f32 = 0.6;
/// The most star layers a sky has.
pub const MAX_STAR_LAYERS: usize = 4;
/// The fastest a star layer scrolls, logical pixels per world unit.
pub const MAX_STAR_PARALLAX: f32 = 20.0;

impl Default for BackdropParams {
    fn default() -> Self {
        Self {
            tile: 1400.0,
            stars: 260,
            clouds: 16,
            top: defaults::SKY_TOP,
            bottom: defaults::SKY_BOTTOM,
            cloud_colors: vec![
                defaults::CLOUD_ROSE,
                defaults::CLOUD_VIOLET,
                defaults::CLOUD_TEAL,
                defaults::CLOUD_AMBER,
            ],
            cloud_alpha: 0.2,
            star_layers: vec![STAR_PARALLAX],
        }
    }
}

/// A soft round dot: a star or a nebula puff.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Blob {
    /// Position inside the tile, in logical pixels.
    pub pos: Vec2,
    /// Radius in logical pixels.
    pub radius: f32,
    /// Colour; alpha is peak opacity.
    pub color: Color,
    /// How fast it scrolls: logical pixels per world unit the camera moves (stars; clouds
    /// share one rate).
    pub parallax: f32,
}

/// A generated backdrop, ready for [`crate::Renderer::upload_backdrop`].
#[derive(Clone, Debug, PartialEq)]
pub struct Backdrop {
    pub tile: f32,
    pub top: Color,
    pub bottom: Color,
    pub clouds: Vec<Blob>,
    pub stars: Vec<Blob>,
}

/// Generates a backdrop. Clouds gather around a few centres so they form shapes instead
/// of an even fog; stars are mostly tiny with a handful of brighter ones.
pub fn backdrop(params: &BackdropParams, seed: u64) -> Backdrop {
    let tile = if params.tile.is_finite() {
        params.tile.clamp(256.0, 8192.0)
    } else {
        1400.0
    };
    let rng = Rng::new(seed);
    let mut c = rng.fork(1);
    let colors = if params.cloud_colors.is_empty() {
        vec![defaults::CLOUD_VIOLET]
    } else {
        params.cloud_colors.clone()
    };
    let centres: Vec<(Vec2, usize)> = (0..3)
        .map(|i| {
            (
                Vec2::new(c.f32(), c.f32()) * tile,
                (i + c.below(colors.len() as u32) as usize) % colors.len(),
            )
        })
        .collect();
    let alpha = params.cloud_alpha.clamp(0.0, 1.0);
    let clouds = (0..params.clouds.min(64))
        .map(|i| {
            let (centre, ci) = centres[i as usize % centres.len()];
            let off = Vec2::new(c.signed(), c.signed()) * tile * 0.16;
            // Neighbouring clouds in a cluster drift through related colours.
            let color = colors[(ci + (c.below(3) as usize).min(1)) % colors.len()];
            Blob {
                pos: wrap(centre + off, tile),
                radius: tile * c.range(0.08, 0.2),
                color: color.with_alpha(alpha * c.range(0.5, 1.0)),
                parallax: 0.0,
            }
        })
        .collect();
    let mut s = rng.fork(2);
    let star_colors = [
        Color::hex(0xfff4dd),
        Color::hex(0xffd8c2),
        Color::hex(0xcff3ee),
        Color::hex(0xffe9a8),
    ];
    let mut layers: Vec<f32> = params
        .star_layers
        .iter()
        .copied()
        .filter(|p| p.is_finite())
        .map(|p| p.clamp(0.0, MAX_STAR_PARALLAX))
        .take(MAX_STAR_LAYERS)
        .collect();
    if layers.is_empty() {
        layers.push(STAR_PARALLAX);
    }
    layers.sort_by(f32::total_cmp);
    let n = layers.len();
    let stars = (0..params.stars.min(4000))
        .map(|i| {
            let bright = s.f32() < 0.06;
            // Far layers have smaller stars, near ones bigger; one layer is unchanged.
            let k = i as usize % n;
            let size = if n > 1 {
                0.8 + 0.4 * k as f32 / (n - 1) as f32
            } else {
                1.0
            };
            Blob {
                pos: Vec2::new(s.f32(), s.f32()) * tile,
                radius: size
                    * if bright {
                        s.range(2.2, 3.2)
                    } else {
                        s.range(0.8, 1.6)
                    },
                color: star_colors[s.below(star_colors.len() as u32) as usize]
                    .with_alpha(if bright { 1.0 } else { s.range(0.35, 0.8) }),
                parallax: layers[k],
            }
        })
        .collect();
    Backdrop {
        tile,
        top: params.top,
        bottom: params.bottom,
        clouds,
        stars,
    }
}

fn wrap(p: Vec2, tile: f32) -> Vec2 {
    Vec2::new(p.x.rem_euclid(tile), p.y.rem_euclid(tile))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deterministic_and_inside_the_tile() {
        let p = BackdropParams::default();
        let a = backdrop(&p, 4);
        assert_eq!(a, backdrop(&p, 4));
        assert_ne!(a, backdrop(&p, 5));
        assert_eq!(a.stars.len(), p.stars as usize);
        assert_eq!(a.clouds.len(), p.clouds as usize);
        for b in a.stars.iter().chain(&a.clouds) {
            assert!(b.pos.x >= 0.0 && b.pos.x < a.tile && b.pos.y >= 0.0 && b.pos.y < a.tile);
            assert!(b.radius > 0.0 && b.color.a <= 1.0);
        }
    }

    #[test]
    fn star_layers_share_the_stars_and_scroll_at_their_own_rates() {
        let one = backdrop(&BackdropParams::default(), 4);
        assert!(one.stars.iter().all(|s| s.parallax == STAR_PARALLAX));
        let p = BackdropParams {
            star_layers: vec![1.2, 0.2, 0.6],
            ..BackdropParams::default()
        };
        let three = backdrop(&p, 4);
        let count = |rate: f32| three.stars.iter().filter(|s| s.parallax == rate).count();
        assert_eq!((count(0.2), count(0.6), count(1.2)), (87, 87, 86));
        // The same positions as one layer: only the rates and sizes differ.
        for (a, b) in one.stars.iter().zip(&three.stars) {
            assert_eq!(a.pos, b.pos);
        }
        let mean = |rate: f32| {
            let r: Vec<f32> = three
                .stars
                .iter()
                .filter(|s| s.parallax == rate)
                .map(|s| s.radius)
                .collect();
            r.iter().sum::<f32>() / r.len() as f32
        };
        assert!(mean(0.2) < mean(0.6) && mean(0.6) < mean(1.2));
    }

    #[test]
    fn stays_quiet() {
        let a = backdrop(&BackdropParams::default(), 1);
        // Clouds are faint and few stars are bright: the backdrop must not be busy.
        assert!(a.clouds.iter().all(|c| c.color.a <= 0.25));
        let bright = a.stars.iter().filter(|s| s.radius > 2.0).count();
        assert!(bright * 8 < a.stars.len());
    }
}
