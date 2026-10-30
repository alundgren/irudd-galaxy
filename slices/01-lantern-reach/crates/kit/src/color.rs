//! Colours. Stored in linear space (what the shaders want); authored as sRGB hex.

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;

/// A linear-space RGBA colour.
///
/// Author colours with [`Color::hex`] or [`Color::srgb`], which convert from sRGB the way
/// a colour picker shows them. Serde reads and writes `"#rrggbb"` / `"#rrggbbaa"` strings so
/// recipes stay readable for people and game-master agents.
#[derive(Clone, Copy, Debug, PartialEq, Default, schemars::JsonSchema)]
#[schemars(with = "String")]
pub struct Color {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

fn linear_to_srgb(c: f32) -> f32 {
    let c = c.clamp(0.0, 1.0);
    if c <= 0.003_130_8 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

impl Color {
    pub const WHITE: Color = Color::linear(1.0, 1.0, 1.0, 1.0);
    pub const BLACK: Color = Color::linear(0.0, 0.0, 0.0, 1.0);
    pub const TRANSPARENT: Color = Color::linear(0.0, 0.0, 0.0, 0.0);

    /// A colour from linear components.
    pub const fn linear(r: f32, g: f32, b: f32, a: f32) -> Self {
        Self { r, g, b, a }
    }

    /// An opaque colour from sRGB components in `0..=1`.
    pub fn srgb(r: f32, g: f32, b: f32) -> Self {
        Self::linear(srgb_to_linear(r), srgb_to_linear(g), srgb_to_linear(b), 1.0)
    }

    /// An opaque colour from a `0xRRGGBB` sRGB value.
    pub fn hex(rgb: u32) -> Self {
        let c = |shift: u32| ((rgb >> shift) & 0xff) as f32 / 255.0;
        Self::srgb(c(16), c(8), c(0))
    }

    /// The same colour with a different alpha.
    pub fn with_alpha(self, a: f32) -> Self {
        Self { a, ..self }
    }

    /// Linear interpolation in linear space.
    pub fn lerp(self, other: Color, t: f32) -> Self {
        Self::linear(
            self.r + (other.r - self.r) * t,
            self.g + (other.g - self.g) * t,
            self.b + (other.b - self.b) * t,
            self.a + (other.a - self.a) * t,
        )
    }

    /// Multiplies the RGB channels, keeping alpha.
    pub fn scale(self, k: f32) -> Self {
        Self::linear(self.r * k, self.g * k, self.b * k, self.a)
    }

    pub fn to_array(self) -> [f32; 4] {
        [self.r, self.g, self.b, self.a]
    }

    /// sRGB bytes, as written to an 8-bit image or used by the text renderer.
    pub fn to_srgba8(self) -> [u8; 4] {
        let q = |c: f32| (linear_to_srgb(c) * 255.0).round() as u8;
        [
            q(self.r),
            q(self.g),
            q(self.b),
            (self.a.clamp(0.0, 1.0) * 255.0).round() as u8,
        ]
    }

    /// Parses `#rgb`, `#rrggbb` or `#rrggbbaa` (the `#` is optional).
    pub fn parse_hex(s: &str) -> Result<Self, ColorParseError> {
        let h = s.trim().trim_start_matches('#');
        let err = || ColorParseError(s.to_string());
        if !h.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(err());
        }
        let byte = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).map_err(|_| err());
        let (rgb, a) = match h.len() {
            3 => {
                let n = |i: usize| u8::from_str_radix(&h[i..=i], 16).map(|v| v * 17);
                let v = |i| n(i).map_err(|_| err());
                ([v(0)?, v(1)?, v(2)?], 255)
            }
            6 => ([byte(0)?, byte(2)?, byte(4)?], 255),
            8 => ([byte(0)?, byte(2)?, byte(4)?], byte(6)?),
            _ => return Err(err()),
        };
        let f = |b: u8| b as f32 / 255.0;
        Ok(Self::srgb(f(rgb[0]), f(rgb[1]), f(rgb[2])).with_alpha(f(a)))
    }

    /// `#rrggbb`, or `#rrggbbaa` when not opaque.
    pub fn to_hex(self) -> String {
        let [r, g, b, a] = self.to_srgba8();
        if a == 255 {
            format!("#{r:02x}{g:02x}{b:02x}")
        } else {
            format!("#{r:02x}{g:02x}{b:02x}{a:02x}")
        }
    }
}

/// A string that isn't a valid hex colour.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColorParseError(pub String);

impl fmt::Display for ColorParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "not a hex colour: {:?}", self.0)
    }
}

impl std::error::Error for ColorParseError {}

impl Serialize for Color {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_hex())
    }
}

impl<'de> Deserialize<'de> for Color {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Color::parse_hex(&s).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_round_trips_through_linear() {
        for s in ["#f2c14e", "#2b1e2f", "#000000", "#ffffff", "#12345678"] {
            assert_eq!(Color::parse_hex(s).unwrap().to_hex(), s);
        }
        assert_eq!(Color::parse_hex("fff").unwrap().to_hex(), "#ffffff");
        assert!(Color::parse_hex("#ggg").is_err());
        assert!(Color::parse_hex("#12345").is_err());
    }

    #[test]
    fn serde_uses_hex_strings() {
        let c = Color::hex(0xe07a5f);
        let json = serde_json::to_string(&c).unwrap();
        assert_eq!(json, "\"#e07a5f\"");
        let back: Color = serde_json::from_str(&json).unwrap();
        assert_eq!(back.to_hex(), "#e07a5f");
    }
}
