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
//! sun_direction = [-0.35, 0.27, 0.90]
//! band_fade_degrees = 25.0   # a face fades between tone bands over this much turn
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
//! halo = 0.6                  # a soft halo round its self-lit paint (lamps, engines)
//!
//! [beams.cutter]              # a beam: `engine.beam { look = "<module>.cutter" }`
//! color = "#7ff0d8"
//! impact = ["<module>.sparks"] # sprayed where it lands
//!
//! [particles.sparks]          # a particle recipe for emitters, bursts and beams
//! shape = "spark"             # dot, spark, puff or chip
//! count = 12                  # in a burst; rate is a second from an emitter
//! ```
//!
//! Every key is optional. The scene-wide tables (`ink`, `light`, `sky`, `course`) may
//! come from one module only; looks, beams and particle recipes are each module's own,
//! named by full ID.

use std::collections::BTreeMap;

use galaxy_mesh::Color;
use glam::Vec3;
use serde::{Deserialize, Serialize};

use crate::backdrop::BackdropParams;
use crate::defaults;
use crate::effects::{BeamStyle, ParticleRecipe};
use crate::scene::{Lights, Material};

/// A parsed `style.toml`.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Style {
    pub ink: Option<Ink>,
    pub light: Option<Light>,
    pub sky: Option<Sky>,
    pub course: Option<Course>,
    pub screen: Option<crate::screen_style::ScreenStyle>,
    pub looks: BTreeMap<String, Look>,
    pub beams: BTreeMap<String, BeamStyle>,
    pub particles: BTreeMap<String, ParticleRecipe>,
}

impl Style {
    pub fn parse(text: &str) -> Result<Style, String> {
        let style: Style = toml::from_str(text).map_err(|e| e.to_string())?;
        if let Some(screen) = &style.screen {
            screen.check()?;
        }
        if let Some(ink) = &style.ink {
            ink.check().map_err(|e| format!("[ink] {e}"))?;
        }
        if let Some(light) = &style.light {
            light.check().map_err(|e| format!("[light] {e}"))?;
        }
        if let Some(sky) = &style.sky {
            sky.check().map_err(|e| format!("[sky] {e}"))?;
        }
        for (name, look) in &style.looks {
            look.check().map_err(|e| format!("[looks.{name}] {e}"))?;
        }
        for (name, beam) in &style.beams {
            beam.check().map_err(|e| format!("[beams.{name}] {e}"))?;
        }
        for (name, recipe) in &style.particles {
            recipe
                .check()
                .map_err(|e| format!("[particles.{name}] {e}"))?;
        }
        Ok(style)
    }
}

/// Every key `style.toml` takes: its table (`looks.<name>` for a look), the key, the
/// values it takes (empty for a colour or a switch) and what it does. `galaxy-gm fields
/// style` prints it with each key's default from [`defaults`]; a test keeps it to the
/// structs.
pub const KEYS: &[(&str, &str, &str, &str)] = &[
    (
        "screen",
        "paper",
        "opaque colour",
        "paper colour; text pairs need 4.5:1 contrast, handles on track 3:1",
    ),
    (
        "screen",
        "panel",
        "opaque colour",
        "panel colour; text pairs need 4.5:1 contrast, handles on track 3:1",
    ),
    (
        "screen",
        "button",
        "opaque colour",
        "button colour; text pairs need 4.5:1 contrast, handles on track 3:1",
    ),
    (
        "screen",
        "text",
        "opaque colour",
        "text colour; text pairs need 4.5:1 contrast, handles on track 3:1",
    ),
    (
        "screen",
        "gain",
        "opaque colour",
        "gain colour; text pairs need 4.5:1 contrast, handles on track 3:1",
    ),
    (
        "screen",
        "loss",
        "opaque colour",
        "loss colour; text pairs need 4.5:1 contrast, handles on track 3:1",
    ),
    (
        "screen",
        "border",
        "opaque colour",
        "border colour; text pairs need 4.5:1 contrast, handles on track 3:1",
    ),
    (
        "screen",
        "bubble",
        "opaque colour",
        "bubble colour; text pairs need 4.5:1 contrast, handles on track 3:1",
    ),
    (
        "screen",
        "sign",
        "opaque colour",
        "sign colour; text pairs need 4.5:1 contrast, handles on track 3:1",
    ),
    (
        "screen",
        "held",
        "opaque colour",
        "held colour; text pairs need 4.5:1 contrast, handles on track 3:1",
    ),
    (
        "screen",
        "socket",
        "opaque colour",
        "socket colour; text pairs need 4.5:1 contrast, handles on track 3:1",
    ),
    (
        "screen",
        "accept",
        "opaque colour",
        "accept colour, behind Drop here and its note; text pairs need 4.5:1 contrast, handles on track 3:1",
    ),
    (
        "screen",
        "refuse",
        "opaque colour",
        "refuse colour; text pairs need 4.5:1 contrast, handles on track 3:1",
    ),
    (
        "screen",
        "track",
        "opaque colour",
        "track colour; text pairs need 4.5:1 contrast, handles on track 3:1",
    ),
    (
        "screen",
        "handle",
        "opaque colour",
        "handle colour; text pairs need 4.5:1 contrast, handles on track 3:1",
    ),
    (
        "screen",
        "handle_active",
        "opaque colour",
        "handle active colour; text pairs need 4.5:1 contrast, handles on track 3:1",
    ),
    (
        "screen",
        "radius",
        "0 to 24",
        "corner rounding, logical pixels",
    ),
    (
        "screen",
        "border_px",
        "0 to 4",
        "frame width, logical pixels",
    ),
    (
        "screen",
        "frame_padding",
        "12 to 32",
        "default panel and target padding; explicit widget pad wins, logical pixels",
    ),
    (
        "screen",
        "target_min",
        "44 to 64",
        "minimum target size, logical pixels",
    ),
    (
        "screen",
        "handle_width",
        "24 to 48",
        "scroll lane and handle width, logical pixels",
    ),
    (
        "screen",
        "handle_min",
        "44 to 80",
        "minimum handle height, logical pixels",
    ),
    (
        "screen",
        "handle_radius",
        "0 to 24",
        "handle corner rounding, logical pixels",
    ),
    (
        "screen",
        "socket_dash",
        "0 to 16",
        "empty socket dash period, logical pixels",
    ),
    (
        "screen",
        "resting",
        "opaque colour",
        "resting button fill; unset is panel; text needs 4.5:1, its dashed border 3:1",
    ),
    (
        "screen",
        "resting_dash",
        "0 to 16",
        "resting button frame dash period, logical pixels; 0 is solid, then resting must differ from button",
    ),
    (
        "screen",
        "hint_border_px",
        "0 to 4",
        "drop hint frame width, logical pixels",
    ),
    (
        "screen",
        "highlight",
        "opaque colour",
        "outline of a hovered ui.target's model",
    ),
    (
        "screen",
        "highlight_px",
        "2 to 12",
        "width of that outline, logical pixels",
    ),
    (
        "screen",
        "bubble_title",
        "16 to 40",
        "talk bubble heading (the speaker's name) size, logical pixels",
    ),
    (
        "screen",
        "bubble_title_weight",
        "400 to 700",
        "talk bubble heading font weight; lines are 500",
    ),
    (
        "screen",
        "bubble_subtitle",
        "16 to 32",
        "talk bubble subtitle size, logical pixels; drawn at weight 400",
    ),
    (
        "screen",
        "bubble_text",
        "16 to 32",
        "talk bubble line and answer button size, logical pixels",
    ),
    (
        "screen",
        "bubble_marker",
        "16 to 32",
        "talk bubble Continue/Done marker size, logical pixels; drawn at weight 400",
    ),
    (
        "ink",
        "color",
        "",
        "the ink of inner lines, outlines and text",
    ),
    (
        "ink",
        "inner_px",
        "0 to 8",
        "inner line width, logical pixels",
    ),
    (
        "ink",
        "crease_degrees",
        "0 to 180",
        "edges sharper than this get an inner line",
    ),
    (
        "ink",
        "color_edge",
        "0 to 2",
        "how different two paints must be (linear colour distance) for a line between them",
    ),
    (
        "ink",
        "depth_step",
        "0 to 1000",
        "the smallest jump in depth that gets a line, world units",
    ),
    (
        "light",
        "sun_direction",
        "[x, y, z]",
        "towards the sun; z is up, normalised",
    ),
    ("light", "sun", "", "the sun's colour"),
    (
        "light",
        "ambient",
        "",
        "flat fill added to every lit surface",
    ),
    (
        "light",
        "rim",
        "",
        "edge light on surfaces turned away from the camera",
    ),
    (
        "light",
        "rim_strength",
        "0 to 1",
        "how strong the rim light is",
    ),
    (
        "light",
        "band_fade_degrees",
        "0 to 90",
        "how far a flat face turns while it fades from one tone band to the next; 0 is a hard step, which flips whole faces of a turning thing in one frame",
    ),
    (
        "sky",
        "top",
        "",
        "the backdrop's colour at the top of the screen",
    ),
    ("sky", "bottom", "", "at the bottom"),
    ("sky", "clouds", "[colours]", "the colours of nebula clouds"),
    ("sky", "cloud_alpha", "0 to 1", "how strong clouds are"),
    ("sky", "cloud_count", "0 to 64", "how many clouds"),
    ("sky", "stars", "0 to 4000", "how many stars"),
    (
        "sky",
        "star_layers",
        "[numbers]",
        "1 to 4 star layers, each the logical pixels it scrolls per world unit the camera moves (0 to 20): [0.25, 0.6, 1.2] puts far stars creeping behind near ones; the stars are shared among them",
    ),
    (
        "course",
        "show",
        "true or false",
        "draw the followed thing's order: dots to where it goes and a ring at the end",
    ),
    ("course", "color", "", "the course's colour"),
    ("course", "dot", "logical pixels", "dot size"),
    ("course", "spacing", "logical pixels", "dot to dot"),
    (
        "course",
        "ring",
        "logical pixels",
        "the end ring's line width",
    ),
    (
        "course",
        "bright",
        "seconds",
        "how long the course stays at full strength after an order",
    ),
    ("course", "faint", "0 to 1", "its strength after that"),
    (
        "looks.<name>",
        "outline_px",
        "0 to 16",
        "silhouette width, logical pixels",
    ),
    (
        "looks.<name>",
        "hatch",
        "0 to 1",
        "hatching in the shadow band",
    ),
    (
        "looks.<name>",
        "shadow",
        "",
        "multiplies the paint in shadow",
    ),
    (
        "looks.<name>",
        "highlight",
        "",
        "multiplies the paint in the highlight band",
    ),
    ("looks.<name>", "bands", "2 or 3", "tone bands"),
    (
        "looks.<name>",
        "halo",
        "0 to 4",
        "a soft additive halo round each patch of self-lit paint (lamps, windows, engines); 0 is none",
    ),
    (
        "looks.<name>",
        "halo_size",
        "0 to 8",
        "the halo's radius, times the size of the lit patch",
    ),
    ("beams.<name>", "color", "", "the beam's colour"),
    (
        "beams.<name>",
        "width",
        "0 to 8",
        "width, world units; 0 hides it",
    ),
    (
        "beams.<name>",
        "glow",
        "0 to 4",
        "the soft glow round the beam and the halos at its ends; 0 leaves the hot core",
    ),
    (
        "beams.<name>",
        "glow_size",
        "0 to 32",
        "radius of the halo where it lands, world units; the one at its source is half",
    ),
    (
        "beams.<name>",
        "flicker",
        "0 to 1",
        "how much its brightness flickers",
    ),
    (
        "beams.<name>",
        "flicker_rate",
        "0 to 30",
        "flickers a second",
    ),
    (
        "beams.<name>",
        "impact",
        "[recipe IDs]",
        "particle recipes sprayed where the beam lands, each at its own rate (sparks, chips)",
    ),
    (
        "particles.<name>",
        "shape",
        "dot, spark, puff or chip",
        "a soft dot, a streak along its flight, a cel puff or a turning chip (puff and chip are inked)",
    ),
    (
        "particles.<name>",
        "blend",
        "add or paint",
        "add glows over what is beneath; paint covers it",
    ),
    ("particles.<name>", "color", "", "colour at birth"),
    (
        "particles.<name>",
        "end_color",
        "",
        "colour at the end of life; it fades out as it gets there",
    ),
    (
        "particles.<name>",
        "count",
        "0 to 256",
        "particles in one burst",
    ),
    (
        "particles.<name>",
        "rate",
        "0 to 400",
        "particles a second from an emitter or a beam's landing",
    ),
    ("particles.<name>", "life", "0 to 4", "seconds each lives"),
    (
        "particles.<name>",
        "life_jitter",
        "0 to 1",
        "up to this share of a life is cut short at random",
    ),
    (
        "particles.<name>",
        "speed",
        "0 to 200",
        "starting speed, world units a second",
    ),
    (
        "particles.<name>",
        "speed_jitter",
        "0 to 1",
        "up to this share of the speed is taken off at random",
    ),
    (
        "particles.<name>",
        "direction",
        "-360 to 360",
        "degrees from the emitter's facing (180 is behind it); at a beam's landing, from the way back up the beam",
    ),
    (
        "particles.<name>",
        "spread",
        "0 to 360",
        "how wide they fan out round that way, degrees",
    ),
    (
        "particles.<name>",
        "size",
        "0 to 16",
        "diameter at birth, world units",
    ),
    (
        "particles.<name>",
        "end_size",
        "0 to 16",
        "diameter at the end of life, world units",
    ),
    (
        "particles.<name>",
        "drag",
        "0 to 20",
        "share of speed lost a second",
    ),
    (
        "particles.<name>",
        "inherit",
        "0 to 1",
        "share of the emitter's velocity they keep; 0 stays where born (a trail)",
    ),
];

/// Each table's keys with their defaults, as TOML text (`"#2a1a2e"`, `2.0`), from the
/// structs' own defaults.
pub fn defaults() -> Vec<(&'static str, Vec<(String, String)>)> {
    fn table<T: Serialize>(name: &'static str, value: &T) -> (&'static str, Vec<(String, String)>) {
        let keys = match toml::Value::try_from(value) {
            Ok(toml::Value::Table(t)) => t.into_iter().map(|(k, v)| (k, short(&v))).collect(),
            _ => Vec::new(),
        };
        (name, keys)
    }
    /// TOML text with floats at the precision they were stored in (`0.35`, not
    /// `0.3499999940395355`).
    fn short(v: &toml::Value) -> String {
        match v {
            toml::Value::Float(f) => {
                let s = (*f as f32).to_string();
                if s.contains('.') { s } else { format!("{s}.0") }
            }
            toml::Value::Array(a) => {
                format!("[{}]", a.iter().map(short).collect::<Vec<_>>().join(", "))
            }
            other => other.to_string(),
        }
    }
    vec![
        table("ink", &Ink::default()),
        table("light", &Light::default()),
        table("sky", &Sky::default()),
        table("course", &Course::default()),
        table("screen", &{
            // Unset, a resting button uses the panel colour: list that as its default.
            let mut s = crate::screen_style::ScreenStyle::default();
            s.resting = Some(s.panel);
            s
        }),
        table("looks.<name>", &Look::default()),
        table("beams.<name>", &BeamStyle::default()),
        table("particles.<name>", &ParticleRecipe::default()),
    ]
}

/// The inner ink.
#[derive(Clone, Copy, Debug, PartialEq, Deserialize, Serialize)]
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
#[derive(Clone, Copy, Debug, PartialEq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Light {
    pub sun_direction: [f32; 3],
    pub sun: Color,
    pub ambient: Color,
    pub rim: Color,
    pub rim_strength: f32,
    /// How far a flat face turns, in degrees, while it fades from one tone band to the
    /// next. Small is a hard cel step; a turning ship then flips whole faces in a frame.
    pub band_fade_degrees: f32,
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
            band_fade_degrees: l.band_fade_degrees,
        }
    }
}

impl Light {
    fn check(&self) -> Result<(), String> {
        if !(self.band_fade_degrees.is_finite() && (0.0..=90.0).contains(&self.band_fade_degrees)) {
            return Err(format!(
                "band_fade_degrees is {}; it is 0 to 90",
                self.band_fade_degrees
            ));
        }
        Ok(())
    }

    pub fn lights(&self) -> Lights {
        Lights {
            sun_direction: Vec3::from(self.sun_direction).normalize_or(Vec3::Z),
            sun_color: self.sun,
            ambient: self.ambient,
            rim: self.rim.with_alpha(self.rim_strength.clamp(0.0, 1.0)),
            band_fade_degrees: self.band_fade_degrees,
            points: Vec::new(),
        }
    }
}

/// The backdrop behind everything.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Sky {
    pub top: Color,
    pub bottom: Color,
    pub clouds: Vec<Color>,
    pub cloud_alpha: f32,
    pub cloud_count: u32,
    pub stars: u32,
    pub star_layers: Vec<f32>,
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
            star_layers: b.star_layers,
        }
    }
}

impl Sky {
    fn check(&self) -> Result<(), String> {
        use crate::backdrop::{MAX_STAR_LAYERS, MAX_STAR_PARALLAX};
        let n = self.star_layers.len();
        if !(1..=MAX_STAR_LAYERS).contains(&n) {
            return Err(format!(
                "star_layers has {n} layers; a sky has 1 to {MAX_STAR_LAYERS}"
            ));
        }
        if let Some(p) = self
            .star_layers
            .iter()
            .find(|p| !p.is_finite() || !(0.0..=MAX_STAR_PARALLAX).contains(*p))
        {
            return Err(format!(
                "star_layers has {p}; each layer scrolls 0 to {MAX_STAR_PARALLAX} logical pixels per world unit"
            ));
        }
        Ok(())
    }

    pub fn params(&self) -> BackdropParams {
        BackdropParams {
            top: self.top,
            bottom: self.bottom,
            cloud_colors: self.clouds.clone(),
            cloud_alpha: self.cloud_alpha.clamp(0.0, 1.0),
            clouds: self.cloud_count.min(64),
            stars: self.stars.min(4000),
            star_layers: self.star_layers.clone(),
            ..BackdropParams::default()
        }
    }
}

/// How the followed thing's order is shown: round dots from it to where it is going,
/// through the waypoints left on a route, and a ring round where it ends. Sizes are in
/// logical pixels so the course reads the same at any zoom.
#[derive(Clone, Copy, Debug, PartialEq, Deserialize, Serialize)]
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
#[derive(Clone, Copy, Debug, PartialEq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Look {
    pub outline_px: f32,
    pub hatch: f32,
    pub shadow: Color,
    pub highlight: Color,
    pub bands: u32,
    /// A soft halo round each patch of self-lit paint, 0 for none.
    pub halo: f32,
    /// The halo's radius, times the size of the patch it surrounds.
    pub halo_size: f32,
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
            halo: 0.0,
            halo_size: 2.5,
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
        if !(self.halo.is_finite() && (0.0..=4.0).contains(&self.halo)) {
            return Err(format!("halo is {}; it is 0 to 4", self.halo));
        }
        if !(self.halo_size.is_finite() && (0.0..=8.0).contains(&self.halo_size)) {
            return Err(format!("halo_size is {}; it is 0 to 8", self.halo_size));
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
        assert!(
            Style::parse("[light]\nband_fade_degrees = 120.0")
                .unwrap_err()
                .contains("[light] band_fade_degrees")
        );
    }

    #[test]
    fn every_key_is_described_and_its_range_is_accepted() {
        for (table, keys) in defaults() {
            assert!(!keys.is_empty(), "{table}");
            for (key, default) in &keys {
                let doc = KEYS.iter().find(|k| k.0 == table && k.1 == key);
                assert!(doc.is_some(), "style key [{table}] {key} is not in KEYS");
                let header = table.replace("<name>", "a");
                assert!(
                    Style::parse(&format!(
                        "[{header}]
{key} = {default}"
                    ))
                    .is_ok(),
                    "[{table}] {key} = {default}"
                );
            }
        }
        for (table, key, range, _) in KEYS {
            assert!(
                defaults()
                    .iter()
                    .any(|(t, keys)| t == table && keys.iter().any(|(k, _)| k == key)),
                "KEYS names [{table}] {key}, which style.toml doesn't take"
            );
            // "lo to hi": both ends are accepted.
            if let Some((lo, hi)) = range.split_once(" to ") {
                let header = table.replace("<name>", "a");
                for end in [lo, hi] {
                    let value = if key.ends_with("count") || *key == "stars" {
                        end.to_owned()
                    } else {
                        format!("{end}.0")
                    };
                    assert!(
                        Style::parse(&format!(
                            "[{header}]
{key} = {value}"
                        ))
                        .is_ok(),
                        "[{table}] {key} = {value}"
                    );
                }
            }
        }
    }

    #[test]
    fn star_layers_are_read_and_kept_in_range() {
        let s = Style::parse("[sky]\nstar_layers = [0.25, 0.6, 1.2]").unwrap();
        assert_eq!(s.sky.unwrap().params().star_layers, [0.25, 0.6, 1.2]);
        for bad in ["[]", "[1.0, 2.0, 3.0, 4.0, 5.0]", "[-1.0]", "[30.0]"] {
            let e = Style::parse(&format!("[sky]\nstar_layers = {bad}")).unwrap_err();
            assert!(e.contains("[sky] star_layers"), "{bad}: {e}");
        }
    }

    #[test]
    fn beams_and_particle_recipes_are_read_and_checked() {
        let s = Style::parse(
            "[beams.cutter]\nwidth = 0.8\nimpact = [\"m.sparks\", \"m.chips\"]\n\
             [particles.puff]\nshape = \"puff\"\nblend = \"paint\"\ncount = 30\n",
        )
        .unwrap();
        assert_eq!(s.beams["cutter"].width, 0.8);
        assert_eq!(s.beams["cutter"].impact, ["m.sparks", "m.chips"]);
        let puff = s.particles["puff"];
        assert_eq!(puff.shape, crate::scene::ParticleShape::Puff);
        assert_eq!(puff.blend, crate::effects::Blend::Paint);
        assert_eq!(puff.count, 30);
        for (bad, key) in [
            ("[particles.a]\ncount = 9999", "[particles.a] count"),
            ("[particles.a]\nrate = 1e9", "[particles.a] rate"),
            ("[particles.a]\nshape = \"smoke\"", "smoke"),
            ("[beams.a]\nwidth = -1.0", "[beams.a] width"),
            ("[looks.a]\nhalo = 9.0", "[looks.a] halo"),
        ] {
            let e = Style::parse(bad).unwrap_err();
            assert!(e.contains(key), "{bad}: {e}");
        }
    }

    #[test]
    fn the_band_fade_reaches_the_lights() {
        let s = Style::parse("[light]\nband_fade_degrees = 10.0").unwrap();
        assert_eq!(s.light.unwrap().lights().band_fade_degrees, 10.0);
    }
}
