//! How a module wants its world drawn: the module's `style.toml`.
//!
//! ```toml
//! [ink]                       # the inner ink, for the whole scene
//! color = "#2a1a2e"
//! inner_px = 2.0              # inner line width in logical pixels
//! crease_degrees = 50.0       # edges sharper than this get a line
//! color_edge = 0.22           # linear paint distance that counts as a border
//! depth_step = 0.55           # smallest depth break that gets a line, world units
//!
//! [light]                     # for the whole scene
//! sun_direction = [-0.62, 0.48, 0.52]
//! sun = "#fff1dc"
//! ambient = "#2c2240"
//! rim = "#ffb36b"
//! rim_strength = 0.55
//!
//! [sky]                       # the backdrop, for the whole scene
//! top = "#4e3672"
//! bottom = "#2a1f48"
//! clouds = ["#c2577a", "#6b4aa0"]
//! cloud_alpha = 0.2
//! cloud_count = 16
//! stars = 260
//!
//! [course]                  # the followed ship's order: dots and a ring
//! show = true
//! color = "#ffcf6b"
//! dot = 5.0                 # dot size, logical pixels
//! spacing = 16.0            # dot to dot, logical pixels
//! ring = 2.0                # ring line width, logical pixels
//! bright = 2.5              # seconds at full strength after an order
//! faint = 0.35              # strength after that (0 to 1)
//!
//! [looks.hull]                # a look a model can wear: `look = "<module>.hull"`
//! outline_px = 4.0            # silhouette width in logical pixels
//! hatch = 0.35                # hatching in the shadow band, 0..1
//! shadow = "#9e84cc"          # multiplies the paint in shadow
//! highlight = "#fff6ea"       # multiplies the paint in the highlight band
//! bands = 3                   # tone bands, 2 or 3
//! ```
//!
//! Every key is optional. The scene-wide tables (`ink`, `light`, `sky`, `course`) may
//! come from one module only; looks are each module's own, named by full ID.

use std::collections::BTreeMap;

use galaxy_mesh::Color;
use glam::Vec3;
use serde::Deserialize;

use crate::backdrop::BackdropParams;
use crate::defaults;
use crate::scene::{Lights, Material};

/// A parsed `style.toml`.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Style {
    pub ink: Option<Ink>,
    pub light: Option<Light>,
    pub sky: Option<Sky>,
    pub course: Option<Course>,
    pub looks: BTreeMap<String, Look>,
}

impl Style {
    pub fn parse(text: &str) -> Result<Style, String> {
        let style: Style = toml::from_str(text).map_err(|e| e.to_string())?;
        if let Some(ink) = &style.ink {
            ink.check().map_err(|e| format!("[ink] {e}"))?;
        }
        for (name, look) in &style.looks {
            look.check().map_err(|e| format!("[looks.{name}] {e}"))?;
        }
        Ok(style)
    }
}

/// The inner ink.
#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Ink {
    pub color: Color,
    pub inner_px: f32,
    pub crease_degrees: f32,
    pub color_edge: f32,
    pub depth_step: f32,
}

impl Default for Ink {
    fn default() -> Self {
        Self {
            color: defaults::INK,
            inner_px: 2.0,
            crease_degrees: 50.0,
            color_edge: 0.22,
            depth_step: 0.55,
        }
    }
}

impl Ink {
    fn check(&self) -> Result<(), String> {
        let ok = |v: f32, lo: f32, hi: f32| v.is_finite() && (lo..=hi).contains(&v);
        if !ok(self.inner_px, 0.0, 8.0) {
            return Err(format!("inner_px is {}; it is 0 to 8", self.inner_px));
        }
        if !ok(self.crease_degrees, 0.0, 180.0) {
            return Err(format!(
                "crease_degrees is {}; it is 0 to 180",
                self.crease_degrees
            ));
        }
        if !ok(self.color_edge, 0.0, 2.0) || !ok(self.depth_step, 0.0, 1000.0) {
            return Err("color_edge and depth_step must be small positive numbers".into());
        }
        Ok(())
    }

    /// The ink pass's parameters: crease cosine, colour-edge distance, depth step, and the
    /// sample radius in pixels (half the inner width, so a line is about `inner_px` wide).
    pub fn uniforms(&self) -> [f32; 4] {
        [
            self.crease_degrees.to_radians().cos(),
            self.color_edge,
            self.depth_step,
            (self.inner_px * 0.5).round().clamp(1.0, 3.0),
        ]
    }
}

/// The scene's light.
#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Light {
    pub sun_direction: [f32; 3],
    pub sun: Color,
    pub ambient: Color,
    pub rim: Color,
    pub rim_strength: f32,
}

impl Default for Light {
    fn default() -> Self {
        let l = Lights::default();
        Self {
            sun_direction: l.sun_direction.into(),
            sun: l.sun_color,
            ambient: l.ambient,
            rim: l.rim.with_alpha(1.0),
            rim_strength: l.rim.a,
        }
    }
}

impl Light {
    pub fn lights(&self) -> Lights {
        Lights {
            sun_direction: Vec3::from(self.sun_direction).normalize_or(Vec3::Z),
            sun_color: self.sun,
            ambient: self.ambient,
            rim: self.rim.with_alpha(self.rim_strength.clamp(0.0, 1.0)),
            points: Vec::new(),
        }
    }
}

/// The backdrop behind everything.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Sky {
    pub top: Color,
    pub bottom: Color,
    pub clouds: Vec<Color>,
    pub cloud_alpha: f32,
    pub cloud_count: u32,
    pub stars: u32,
}

impl Default for Sky {
    fn default() -> Self {
        let b = BackdropParams::default();
        Self {
            top: b.top,
            bottom: b.bottom,
            clouds: b.cloud_colors,
            cloud_alpha: b.cloud_alpha,
            cloud_count: b.clouds,
            stars: b.stars,
        }
    }
}

impl Sky {
    pub fn params(&self) -> BackdropParams {
        BackdropParams {
            top: self.top,
            bottom: self.bottom,
            cloud_colors: self.clouds.clone(),
            cloud_alpha: self.cloud_alpha.clamp(0.0, 1.0),
            clouds: self.cloud_count.min(64),
            stars: self.stars.min(4000),
            ..BackdropParams::default()
        }
    }
}

/// How the followed thing's order is shown: round dots from it to where it is going,
/// through the waypoints left on a route, and a ring round where it ends. Sizes are in
/// logical pixels so the course reads the same at any zoom.
#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Course {
    pub show: bool,
    pub color: Color,
    pub dot: f32,
    pub spacing: f32,
    pub ring: f32,
    pub bright: f32,
    pub faint: f32,
}

impl Default for Course {
    fn default() -> Self {
        Self {
            show: true,
            color: Color::hex(0xffcf6b),
            dot: 5.0,
            spacing: 16.0,
            ring: 2.0,
            bright: 2.5,
            faint: 0.35,
        }
    }
}

/// How a model is shaded and inked.
#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Look {
    pub outline_px: f32,
    pub hatch: f32,
    pub shadow: Color,
    pub highlight: Color,
    pub bands: u32,
}

impl Default for Look {
    fn default() -> Self {
        let m = Material::default();
        Self {
            outline_px: m.outline_px,
            hatch: m.hatch,
            shadow: m.shadow,
            highlight: m.highlight,
            bands: m.bands,
        }
    }
}

impl Look {
    fn check(&self) -> Result<(), String> {
        if !(self.outline_px.is_finite() && (0.0..=16.0).contains(&self.outline_px)) {
            return Err(format!("outline_px is {}; it is 0 to 16", self.outline_px));
        }
        if !(self.hatch.is_finite() && (0.0..=1.0).contains(&self.hatch)) {
            return Err(format!("hatch is {}; it is 0 to 1", self.hatch));
        }
        if !(2..=3).contains(&self.bands) {
            return Err(format!("bands is {}; it is 2 or 3", self.bands));
        }
        Ok(())
    }

    /// The material for this look, inked in `ink`'s colour.
    pub fn material(&self, ink: &Ink) -> Material {
        Material {
            shadow: self.shadow,
            highlight: self.highlight,
            bands: self.bands,
            outline: ink.color,
            outline_px: self.outline_px,
            hatch: self.hatch,
            ..Material::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_style_is_the_engine_default() {
        assert_eq!(Style::parse("").unwrap(), Style::default());
    }

    #[test]
    fn a_style_reads_its_tables() {
        let s = Style::parse(
            r##"
            [ink]
            color = "#102030"
            [looks.hull]
            outline_px = 5.0
            bands = 2
            "##,
        )
        .unwrap();
        assert_eq!(s.ink.unwrap().color.to_hex(), "#102030");
        assert_eq!(s.ink.unwrap().crease_degrees, 50.0);
        let hull = s.looks["hull"];
        assert_eq!((hull.outline_px, hull.bands), (5.0, 2));
        assert_eq!(hull.material(&Ink::default()).outline_px, 5.0);
    }

    #[test]
    fn the_course_can_be_hidden_or_recoloured() {
        let s = Style::parse(
            "[course]
show = false
color = \"#3fa7a0\"
",
        )
        .unwrap();
        let course = s.course.unwrap();
        assert!(!course.show);
        assert_eq!(course.color.to_hex(), "#3fa7a0");
        assert_eq!(course.spacing, Course::default().spacing);
    }

    #[test]
    fn bad_styles_name_the_key() {
        assert!(
            Style::parse("[looks.a]\nbands = 7")
                .unwrap_err()
                .contains("looks.a")
        );
        assert!(
            Style::parse("[ink]\ninner_px = 99.0")
                .unwrap_err()
                .contains("inner_px")
        );
        assert!(
            Style::parse("[ink]\nwidth = 1.0")
                .unwrap_err()
                .contains("width")
        );
    }
}
