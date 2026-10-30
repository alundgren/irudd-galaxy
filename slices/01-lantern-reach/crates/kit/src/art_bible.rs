//! The art bible: palettes, ink weights and part proportions as data.
//!
//! Bundled from `assets/art-bible.toml`. Station recipes are checked with
//! [`crate::station::StationRecipe::validate_art`]. The game master uses the same file.

use crate::color::Color;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::sync::OnceLock;

/// Which silhouette weight and hatch a mesh should wear.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InkRole {
    Station,
    Ship,
    Rock,
    Prop,
}

/// A loaded art bible.
#[derive(Clone, Debug, Deserialize)]
pub struct ArtBible {
    pub ink: InkRules,
    pub hatch: Hatch,
    pub ranges: Ranges,
    pub budget: Budget,
    /// `kind -> parameter -> [min, max]`.
    pub proportions: BTreeMap<String, BTreeMap<String, [f32; 2]>>,
    pub factions: BTreeMap<String, FactionStyle>,
    pub regions: BTreeMap<String, RegionStyle>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct InkRules {
    pub color: Color,
    pub station_px: [f32; 2],
    pub ship_px: [f32; 2],
    pub rock_px: [f32; 2],
    pub prop_px: [f32; 2],
    pub inner_px: [f32; 2],
    pub crease_degrees: f32,
    pub color_edge: f32,
    pub depth_step: f32,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Hatch {
    pub station: f32,
    pub ship: f32,
    pub rock: f32,
    pub prop: f32,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Ranges {
    pub hull_l: [f32; 2],
    pub trim_l: [f32; 2],
    pub accent_l: [f32; 2],
    pub glow_l: [f32; 2],
    pub glass_l: [f32; 2],
    pub leaf_l: [f32; 2],
    pub chroma: [f32; 2],
    pub glow_chroma: [f32; 2],
    pub hull_above_sky: f32,
    pub faction_distance: f32,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Budget {
    pub min_parts: usize,
    pub max_parts: usize,
    pub max_decals: usize,
    pub max_of_kind: u32,
    pub kind: BTreeMap<String, u32>,
}

/// One faction's colour pair, mark, motif and the station paint that carries them.
#[derive(Clone, Debug, Deserialize)]
pub struct FactionStyle {
    pub name: String,
    pub primary: Color,
    pub secondary: Color,
    pub mark: String,
    pub motif: String,
    pub hull: Color,
    pub trim: Color,
    pub accent: Color,
    pub glow: Color,
    pub glass: Color,
    pub leaf: Color,
}

/// A region's sky. Region tint belongs on the backdrop, not on hull paint.
#[derive(Clone, Debug, Deserialize)]
pub struct RegionStyle {
    pub name: String,
    pub sky_top: Color,
    pub sky_bottom: Color,
    pub clouds: Vec<Color>,
    #[serde(default = "default_cloud_alpha")]
    pub cloud_alpha: f32,
}

fn default_cloud_alpha() -> f32 {
    0.2
}

/// Why a colour was refused. `role` is the palette slot, when it has one.
#[derive(Clone, Debug, PartialEq)]
pub struct ColorIssue {
    pub role: Option<&'static str>,
    pub message: String,
}

impl ArtBible {
    /// The bible compiled into the renderer.
    pub fn bundled() -> &'static Self {
        static BIBLE: OnceLock<ArtBible> = OnceLock::new();
        BIBLE.get_or_init(|| {
            Self::parse(include_str!("../../../assets/art-bible.toml"))
                .expect("the bundled art bible parses")
        })
    }

    /// Parses a bible from TOML text.
    pub fn parse(text: &str) -> Result<Self, String> {
        toml::from_str(text).map_err(|e| e.to_string())
    }

    pub fn faction(&self, id: &str) -> Option<&FactionStyle> {
        self.factions.get(id)
    }

    pub fn region(&self, id: &str) -> Option<&RegionStyle> {
        self.regions.get(id)
    }

    /// OKLab L of the lighter end of a region's sky, for the hull-gap check.
    pub fn sky_lightness(&self, region: &str) -> Option<f32> {
        let r = self.region(region)?;
        Some(oklab_l(r.sky_top).max(oklab_l(r.sky_bottom)))
    }

    /// How many of `kind` a recipe may contain.
    pub fn kind_cap(&self, kind: &str) -> u32 {
        self.budget
            .kind
            .get(kind)
            .copied()
            .unwrap_or(self.budget.max_of_kind)
    }

    /// `None` when `value` sits in the bible's range for `kind.param`.
    pub fn proportion_issue(&self, kind: &str, param: &str, value: f32) -> Option<String> {
        let Some(range) = self.proportions.get(kind).and_then(|p| p.get(param)) else {
            return Some(format!("no proportion range for {kind}.{param}"));
        };
        if value.is_finite() && value >= range[0] && value <= range[1] {
            None
        } else {
            Some(format!(
                "{kind}.{param} is {value}; expected {}..={}",
                range[0], range[1]
            ))
        }
    }

    /// Uniforms for the inner-ink pass: crease cosine, colour-edge distance,
    /// depth step, sample radius in pixels. The radius is half the inner width,
    /// so a line is about `inner_px` wide.
    pub fn ink_uniforms(&self) -> [f32; 4] {
        [
            self.ink.crease_degrees.to_radians().cos(),
            self.ink.color_edge,
            self.ink.depth_step,
            (mid(self.ink.inner_px) * 0.5).round().clamp(1.0, 3.0),
        ]
    }

    /// Checks one paint colour against a palette role.
    pub fn check_role(&self, role: &'static str, color: Color) -> Option<ColorIssue> {
        let (l_range, c_range) = match role {
            "hull" => (self.ranges.hull_l, self.ranges.chroma),
            "trim" => (self.ranges.trim_l, self.ranges.chroma),
            "accent" => (self.ranges.accent_l, self.ranges.chroma),
            "glow" => (self.ranges.glow_l, self.ranges.glow_chroma),
            "glass" => (self.ranges.glass_l, self.ranges.chroma),
            "leaf" => (self.ranges.leaf_l, self.ranges.chroma),
            _ => (self.ranges.trim_l, self.ranges.chroma),
        };
        self.color_issue(Some(role), color, l_range, c_range)
    }

    /// A free paint colour (a part override, a decal): inside the widest band, and
    /// neither grey nor blue-grey chrome.
    pub fn check_paint(&self, color: Color) -> Option<ColorIssue> {
        let l = [
            self.ranges
                .trim_l
                .into_iter()
                .chain(self.ranges.hull_l)
                .chain(self.ranges.glow_l)
                .fold(1.0, f32::min),
            self.ranges
                .trim_l
                .into_iter()
                .chain(self.ranges.hull_l)
                .chain(self.ranges.glow_l)
                .fold(0.0, f32::max),
        ];
        let c = [self.ranges.chroma[0], self.ranges.glow_chroma[1]];
        self.color_issue(None, color, l, c)
    }

    fn color_issue(
        &self,
        role: Option<&'static str>,
        color: Color,
        l_range: [f32; 2],
        c_range: [f32; 2],
    ) -> Option<ColorIssue> {
        let lab = oklab(color);
        let c = chroma(lab);
        let fail = |message: String| {
            Some(ColorIssue {
                role,
                message: format!("{message} ({})", describe(color)),
            })
        };
        if !lab[0].is_finite() || !c.is_finite() {
            return fail("colour is not finite".into());
        }
        if lab[0] < 0.04 {
            return fail("too close to black".into());
        }
        if !(l_range[0]..=l_range[1]).contains(&lab[0]) {
            return fail(format!(
                "lightness {:.2} outside {:.2}..={:.2}",
                lab[0], l_range[0], l_range[1]
            ));
        }
        if !(c_range[0]..=c_range[1]).contains(&c) {
            return fail(format!(
                "chroma {c:.3} outside {:.3}..={:.3}",
                c_range[0], c_range[1]
            ));
        }
        if cold_chrome(lab, c) {
            return fail("blue-grey chrome".into());
        }
        None
    }

    /// OKLab distance between two colours.
    pub fn oklab_distance(a: Color, b: Color) -> f32 {
        let a = oklab(a);
        let b = oklab(b);
        let d = [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
        d[0].hypot(d[1]).hypot(d[2])
    }
}

fn mid(r: [f32; 2]) -> f32 {
    (r[0] + r[1]) * 0.5
}

/// Linear sRGB to OKLab. `l` is lightness, `a` green–red, `b` blue–yellow.
///
/// The matrix is Björn Ottosson's. It is computed in f64 so the published
/// coefficients survive; the result is f32, which is plenty for a palette check.
pub fn oklab(color: Color) -> [f32; 3] {
    let (r, g, b) = (color.r as f64, color.g as f64, color.b as f64);
    let l = 0.412_221_470_8 * r + 0.536_332_536_3 * g + 0.051_445_992_9 * b;
    let m = 0.211_903_498_2 * r + 0.680_699_545_1 * g + 0.107_396_956_6 * b;
    let s = 0.088_302_461_9 * r + 0.281_718_837_6 * g + 0.629_978_700_5 * b;
    let l_ = l.cbrt();
    let m_ = m.cbrt();
    let s_ = s.cbrt();
    [
        (0.210_454_255_3 * l_ + 0.793_617_785_0 * m_ - 0.004_072_046_8 * s_) as f32,
        (1.977_998_495_1 * l_ - 2.428_592_205_0 * m_ + 0.450_593_709_9 * s_) as f32,
        (0.025_904_037_1 * l_ + 0.782_771_766_2 * m_ - 0.808_675_766_0 * s_) as f32,
    ]
}

pub fn oklab_l(color: Color) -> f32 {
    oklab(color)[0]
}

pub fn chroma(lab: [f32; 3]) -> f32 {
    lab[1].hypot(lab[2])
}

fn describe(color: Color) -> String {
    let lab = oklab(color);
    format!("{} L={:.3} C={:.3}", color.to_hex(), lab[0], chroma(lab))
}

/// Low-chroma steel blue in the mid values: the chrome the palette refuses.
fn cold_chrome(lab: [f32; 3], c: f32) -> bool {
    if c >= 0.06 || !(0.35..=0.78).contains(&lab[0]) {
        return false;
    }
    // OKLab +b is yellow and -b is blue. Chrome sits near hue -90 degrees with little red.
    let hue = lab[2].atan2(lab[1]);
    (-2.2..=-0.7).contains(&hue)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_bible_has_the_three_factions_and_regions() {
        let b = ArtBible::bundled();
        assert!(b.ink.crease_degrees > 30.0);
        for id in [
            "faction.wharf_cooperative",
            "faction.tollway_compact",
            "faction.lamplighter_society",
        ] {
            let f = b.faction(id).unwrap_or_else(|| panic!("missing {id}"));
            assert!(b.check_role("hull", f.hull).is_none(), "{id} hull");
            assert!(
                b.check_role("trim", f.trim).is_none(),
                "{id} trim {:?}",
                b.check_role("trim", f.trim)
            );
            assert!(b.check_role("accent", f.accent).is_none(), "{id} accent");
            assert!(b.check_role("glow", f.glow).is_none(), "{id} glow");
            assert!(
                ArtBible::oklab_distance(f.trim, f.primary) < 0.02
                    || ArtBible::oklab_distance(f.accent, f.primary) < 0.02
                    || ArtBible::oklab_distance(f.hull, f.primary) < 0.02,
                "{id} should carry its primary"
            );
        }
        for id in [
            "region.lantern_shallows",
            "region.cinder_drift",
            "region.brassmouth_narrows",
        ] {
            assert!(b.sky_lightness(id).is_some(), "{id}");
        }
    }

    #[test]
    fn grey_and_chrome_are_refused() {
        let b = ArtBible::bundled();
        assert!(b.check_paint(Color::hex(0x808080)).is_some(), "grey");
        assert!(b.check_paint(Color::hex(0x8899aa)).is_some(), "chrome");
        assert!(b.check_paint(Color::hex(0x111111)).is_some(), "black");
        assert!(
            b.check_role("hull", Color::hex(0x3a2d5c)).is_some(),
            "dark hull"
        );
    }
}
