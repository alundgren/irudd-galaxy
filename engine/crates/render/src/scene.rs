//! The data-driven scene description the renderer draws each frame.
//!
//! A [`Scene`] is plain data: build one per frame from world state and hand it to the
//! renderer. Heavy data (meshes, backdrops) is uploaded once and referenced by handle.

use crate::camera::Camera;
use crate::defaults;
use crate::style::Ink;
use galaxy_mesh::Color;
use glam::{Mat4, Quat, Vec2, Vec3};
use serde::{Deserialize, Serialize};

/// A mesh uploaded with [`crate::Renderer::upload_mesh`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct MeshHandle {
    pub(crate) index: u32,
    pub(crate) generation: u32,
}

/// A backdrop uploaded with [`crate::Renderer::upload_backdrop`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct BackdropHandle(pub(crate) u32);

/// Everything drawn in one frame.
#[derive(Clone, Debug)]
pub struct Scene {
    pub camera: Camera,
    pub lights: Lights,
    /// Colour behind everything when there is no backdrop.
    pub clear_color: Color,
    pub backdrop: Option<BackdropHandle>,
    pub meshes: Vec<MeshInstance>,
    /// Lanes, borders, rings, beams: ribbons in world space.
    pub lines: Vec<Line>,
    /// Soft additive halos (lanterns, engine glow, beam impact).
    pub glows: Vec<Glow>,
    /// Screen-space UI drawn on top.
    pub overlay: Overlay,
    /// How the inner ink is drawn (creases, colour edges, depth steps).
    pub ink: Ink,
    /// Seconds of presentation time, for gentle shader animation (beam shimmer, twinkle).
    /// The renderer never reads a clock; pass a frozen value for reproducible shots.
    pub time: f32,
}

impl Scene {
    pub fn new(camera: Camera) -> Self {
        Self {
            camera,
            lights: Lights::default(),
            clear_color: defaults::SKY_BOTTOM,
            backdrop: None,
            meshes: Vec::new(),
            lines: Vec::new(),
            glows: Vec::new(),
            overlay: Overlay::default(),
            ink: Ink::default(),
            time: 0.0,
        }
    }
}

/// At most this many point lights are used; extras are ignored.
pub const MAX_POINT_LIGHTS: usize = 8;

/// The light setup: one key "sun", warm ambient fill, a rim light and a few point lights.
#[derive(Clone, Debug, PartialEq)]
pub struct Lights {
    /// Direction *towards* the key light.
    pub sun_direction: Vec3,
    pub sun_color: Color,
    /// Flat fill added to every lit surface.
    pub ambient: Color,
    /// Edge light on surfaces turned away from the camera; alpha is its strength.
    pub rim: Color,
    pub points: Vec<PointLight>,
}

impl Default for Lights {
    fn default() -> Self {
        Self {
            sun_direction: Vec3::new(-0.62, 0.48, 0.52).normalize(),
            sun_color: Color::hex(0xfff1dc),
            ambient: Color::hex(0x2c2240),
            rim: Color::hex(0xffb36b).with_alpha(0.55),
            points: Vec::new(),
        }
    }
}

/// A warm local light with a soft cut-off at `radius`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PointLight {
    pub position: Vec3,
    pub color: Color,
    pub radius: f32,
}

/// One mesh drawn with a transform and a material.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MeshInstance {
    pub mesh: MeshHandle,
    pub transform: Mat4,
    pub material: Material,
    /// How far towards its morph target the mesh is drawn, `0..=1`
    /// ([`crate::MeshData::morph_to`]).
    pub morph: f32,
    /// The uniform scale the hatching is spaced for, or 0 for the instance's own. A
    /// mesh that shrinks (a rock being mined) keeps its full-size spacing, so the
    /// strokes stay on its surface instead of sliding across it.
    pub hatch_scale: f32,
}

impl MeshInstance {
    pub fn new(mesh: MeshHandle, transform: Mat4) -> Self {
        Self {
            mesh,
            transform,
            material: Material::default(),
            morph: 0.0,
            hatch_scale: 0.0,
        }
    }

    /// A mesh on the play plane at `(x, y)`, turned `angle` radians around Z.
    pub fn at(mesh: MeshHandle, pos: Vec2, angle: f32, scale: f32) -> Self {
        Self::new(
            mesh,
            Mat4::from_scale_rotation_translation(
                Vec3::splat(scale),
                Quat::from_rotation_z(angle),
                pos.extend(0.0),
            ),
        )
    }

    pub fn with_material(mut self, material: Material) -> Self {
        self.material = material;
        self
    }

    /// Draws the mesh `w` of the way to its morph target.
    pub fn with_morph(mut self, w: f32) -> Self {
        self.morph = if w.is_finite() {
            w.clamp(0.0, 1.0)
        } else {
            0.0
        };
        self
    }

    /// Spaces the hatching as if the instance were drawn at uniform `scale`.
    pub fn with_hatch_scale(mut self, scale: f32) -> Self {
        self.hatch_scale = if scale.is_finite() {
            scale.max(0.0)
        } else {
            0.0
        };
        self
    }
}

/// Cel-shading parameters for one mesh instance.
///
/// The surface colour is the mesh's vertex paint times `tint`. Light is quantised into
/// `bands` (2 or 3) tone bands: shadow (`albedo * shadow`), mid (`albedo`) and highlight
/// (`albedo * highlight`).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Material {
    pub tint: Color,
    /// Multiplier for the shadow band. Warm violet keeps shadows from going gray.
    pub shadow: Color,
    /// Multiplier for the highlight band.
    pub highlight: Color,
    /// Number of tone bands, 2 or 3.
    pub bands: u32,
    pub outline: Color,
    /// Ink outline width in logical pixels (0 disables it).
    pub outline_px: f32,
    /// Added on top of the shaded colour (selection pulses, docking highlights).
    pub emissive: Color,
    /// Strength of the sketched hatching in the shadow band, `0..=1`.
    pub hatch: f32,
}

impl Default for Material {
    fn default() -> Self {
        Self {
            tint: Color::WHITE,
            shadow: Color::srgb(0.62, 0.52, 0.80),
            highlight: Color::srgb(1.12, 1.06, 0.96),
            bands: 3,
            outline: defaults::INK,
            outline_px: 4.0,
            emissive: Color::TRANSPARENT,
            hatch: 0.6,
        }
    }
}

impl Material {
    pub fn with_outline(mut self, color: Color, px: f32) -> Self {
        self.outline = color;
        self.outline_px = px;
        self
    }

    pub fn with_tint(mut self, tint: Color) -> Self {
        self.tint = tint;
        self
    }

    pub fn with_emissive(mut self, emissive: Color) -> Self {
        self.emissive = emissive;
        self
    }
}

/// How a line is drawn along its length.
#[derive(Clone, Copy, Debug, PartialEq, Default, Serialize, Deserialize)]
pub enum LineStyle {
    /// A solid ribbon with darker ink edges.
    #[default]
    Solid,
    /// Dashes and gaps, in world units.
    Dashed { dash: f32, gap: f32 },
    /// A bright additive beam with a hot core (mining beam, tractor).
    Beam,
}

/// A ribbon through world-space points, facing the camera.
#[derive(Clone, Debug, PartialEq)]
pub struct Line {
    pub points: Vec<Vec3>,
    /// Width in world units.
    pub width: f32,
    pub color: Color,
    pub style: LineStyle,
    pub closed: bool,
    /// Hand-drawn wobble amplitude in world units (0 for a ruler-straight line).
    pub wobble: f32,
}

impl Line {
    pub fn path(points: Vec<Vec3>, width: f32, color: Color) -> Self {
        Self {
            points,
            width,
            color,
            style: LineStyle::Solid,
            closed: false,
            wobble: 0.0,
        }
    }

    /// A circle on the plane `z = center.z`.
    pub fn circle(center: Vec3, radius: f32, width: f32, color: Color) -> Self {
        let n = ((radius * 0.8) as usize).clamp(24, 160);
        let points = (0..n)
            .map(|i| {
                let a = i as f32 / n as f32 * std::f32::consts::TAU;
                center + Vec3::new(a.cos(), a.sin(), 0.0) * radius
            })
            .collect();
        Self {
            closed: true,
            ..Self::path(points, width, color)
        }
    }

    pub fn beam(from: Vec3, to: Vec3, width: f32, color: Color) -> Self {
        Self {
            style: LineStyle::Beam,
            ..Self::path(vec![from, to], width, color)
        }
    }

    pub fn dashed(mut self, dash: f32, gap: f32) -> Self {
        self.style = LineStyle::Dashed { dash, gap };
        self
    }

    pub fn wobbly(mut self, amplitude: f32) -> Self {
        self.wobble = amplitude;
        self
    }
}

/// A soft round halo, added on top of the scene.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Glow {
    pub position: Vec3,
    /// Radius in world units.
    pub radius: f32,
    pub color: Color,
}

/// A rectangle in logical pixels, origin top-left.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }

    pub fn center(&self) -> Vec2 {
        Vec2::new(self.x + self.w * 0.5, self.y + self.h * 0.5)
    }
}

/// A rounded UI panel with an ink border and a soft drop shadow.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Panel {
    pub rect: Rect,
    pub radius: f32,
    pub fill: Color,
    pub border: Color,
    pub border_px: f32,
    pub shadow: bool,
    /// Dash length plus gap of the border in logical pixels; 0 draws it solid.
    pub dash: f32,
    /// Turn around the centre, radians, clockwise on screen.
    pub rotate: f32,
    pub shape: PanelShape,
}

/// What a [`Panel`] draws inside its rectangle.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum PanelShape {
    /// A box with rounded corners.
    #[default]
    Box,
    /// A triangle pointing up the screen, its base the rectangle's bottom edge (corner
    /// radius rounds it).
    Triangle,
}

impl Panel {
    /// The default parchment panel.
    pub fn new(rect: Rect) -> Self {
        Self {
            rect,
            radius: 14.0,
            fill: defaults::PAPER,
            border: defaults::INK,
            border_px: 3.0,
            shadow: true,
            dash: 0.0,
            rotate: 0.0,
            shape: PanelShape::Box,
        }
    }
}

/// Horizontal text alignment relative to [`Text::pos`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Align {
    #[default]
    Left,
    Center,
    Right,
}

/// A run of UI text in the bundled rounded font.
#[derive(Clone, Debug, PartialEq)]
pub struct Text {
    pub text: String,
    /// Anchor in logical pixels: the top of the first line, at the aligned edge.
    pub pos: Vec2,
    /// Font size in logical pixels.
    pub size: f32,
    pub color: Color,
    /// Font weight, 300 (light) to 700 (bold).
    pub weight: u16,
    pub align: Align,
    /// Wrap width in logical pixels.
    pub max_width: Option<f32>,
}

impl Text {
    pub fn new(text: impl Into<String>, pos: Vec2, size: f32) -> Self {
        Self {
            text: text.into(),
            pos,
            size,
            color: defaults::INK,
            weight: 500,
            align: Align::Left,
            max_width: None,
        }
    }

    pub fn color(mut self, color: Color) -> Self {
        self.color = color;
        self
    }

    pub fn weight(mut self, weight: u16) -> Self {
        self.weight = weight;
        self
    }

    pub fn align(mut self, align: Align) -> Self {
        self.align = align;
        self
    }

    pub fn wrap(mut self, max_width: f32) -> Self {
        self.max_width = Some(max_width);
        self
    }
}

/// One overlay layer: its panels draw first, then its text.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct UiLayer {
    pub panels: Vec<Panel>,
    pub texts: Vec<Text>,
}

/// Screen-space UI. Panels draw first, then text; then each `upper` layer the same way.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Overlay {
    pub panels: Vec<Panel>,
    pub texts: Vec<Text>,
    pub upper: [UiLayer; 3],
}
