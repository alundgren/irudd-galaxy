//! The station art checks decide whether a sim command is accepted, so their maths
//! must give the same bits on every OS (#42). These pin the results bit for bit over
//! inputs where the platform C libraries round differently from `libm`: with std's
//! `powf`, `cbrt` and `hypot` on Windows the sRGB, OKLab and decal folds all change, so
//! a check that slips back to std's float functions fails.

use galaxy_kit::art_bible::{ArtBible, chroma, oklab};
use galaxy_kit::color::Color;
use galaxy_kit::{Decal, DecalKind, Placement, StationRecipe};

/// FNV-1a over the bits, so one pinned number covers many results.
#[derive(Default)]
struct Fold(u64);

impl Fold {
    fn add(&mut self, bits: u32) {
        for b in bits.to_le_bytes() {
            self.0 = (self.0 ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
}

fn fold() -> Fold {
    Fold(0xcbf2_9ce4_8422_2325)
}

/// A spread of sRGB colours: every grey, and a stride through the cube.
fn colours() -> impl Iterator<Item = u32> {
    (0..=255u32)
        .map(|g| g * 0x01_0101)
        .chain((0..0x100_0000u32).step_by(40_009))
}

#[test]
fn srgb_conversion_is_pinned() {
    let mut f = fold();
    for hex in colours() {
        let c = Color::hex(hex);
        f.add(c.r.to_bits());
        f.add(c.g.to_bits());
        f.add(c.b.to_bits());
        f.add(u32::from_le_bytes(c.to_srgba8()));
    }
    // sRGB to linear must round-trip every byte.
    for b in 0..=255u32 {
        assert_eq!(Color::hex(b).to_srgba8()[2], b as u8);
    }
    assert_eq!(f.0, 0xea60_63db_d5f3_b5b9, "srgb fold {:#x}", f.0);
}

#[test]
fn oklab_and_chroma_are_pinned() {
    let mut f = fold();
    let mut prev = Color::BLACK;
    for hex in colours() {
        let c = Color::hex(hex);
        let lab = oklab(c);
        for v in lab {
            f.add(v.to_bits());
        }
        f.add(chroma(lab).to_bits());
        f.add(ArtBible::oklab_distance(prev, c).to_bits());
        prev = c;
    }
    assert_eq!(f.0, 0x2542_4d74_df86_154a, "oklab fold {:#x}", f.0);
}

#[test]
fn paint_verdicts_are_pinned() {
    // `check_paint` folds lightness, chroma and the chrome hue (`atan2f`) into one
    // verdict; pin which of the colours pass.
    let bible = ArtBible::bundled();
    let mut f = fold();
    for hex in colours() {
        f.add(u32::from(bible.check_paint(Color::hex(hex)).is_none()));
    }
    assert_eq!(f.0, 0x32fd_2c4e_3a4b_c0d4, "paint fold {:#x}", f.0);
}

#[test]
fn decal_reach_is_pinned() {
    let mut f = fold();
    let mut recipe = StationRecipe::lantern_wharf();
    for i in 0..400u32 {
        let x = 0.5 + i as f32 * 0.137;
        let kinds = [
            DecalKind::Stripe {
                length: x,
                width: 1.0 + x * 0.31,
                bands: 3,
            },
            DecalKind::Chevron {
                length: x,
                width: 0.7 + x * 0.19,
            },
            DecalKind::Sigil { size: x * 0.5 },
            DecalKind::Number {
                value: i % 20,
                size: x * 0.4,
            },
        ];
        for kind in kinds {
            recipe.parts.clear();
            recipe.decals = vec![Decal::new(kind, Placement::default())];
            f.add(recipe.footprint().to_bits());
        }
    }
    assert_eq!(f.0, 0x508a_2089_7233_77b2, "decal fold {:#x}", f.0);
}
