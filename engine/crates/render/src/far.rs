//! Distant things behind the play layer, drawn after the backdrop and before the play
//! meshes, far to near: fields of dust, specks or hatch strokes, landmark shapes (a
//! planet's limb, a nebula wash) and meshes (a far station's silhouette).
//!
//! The renderer never decides where they are: the view places each one on the play
//! plane from the camera and its parallax, so the renderer only draws. A field is drawn
//! per pixel from a hashed grid, so it never repeats and needs no tile to wrap.

use crate::scene::MeshInstance;
use galaxy_mesh::Color;
use glam::{Vec2, Vec3};

/// What each mark of a [`Field`] looks like.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mark {
    /// A soft round puff.
    Dust,
    /// A crisp round dot.
    Speck,
    /// A short ink stroke, all at the field's angle, like hatching.
    Hatch,
}

/// A field of marks on an endless grid. Each grid cell holds at most one mark, at a
/// place, size and brightness hashed from the cell and the field's seed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Field {
    pub mark: Mark,
    /// The grid point under the camera's target, in world units: the camera's place
    /// times the layer's parallax. A field whose offset follows the camera fully moves
    /// with the play layer; one whose offset stays put is fixed on screen.
    pub offset: Vec2,
    /// Grid cell edge, world units on the play plane.
    pub cell: f32,
    /// The share of cells that hold a mark, 0 to 1.
    pub chance: f32,
    /// The smallest and largest mark: radius (dust, speck) or stroke length (hatch), in
    /// logical pixels. A mark never grows past half a cell, so it never needs a
    /// neighbour's cell to draw.
    pub size: [f32; 2],
    /// Colour; alpha is the brightest mark's opacity.
    pub color: Color,
    /// A hatch stroke's direction, radians anticlockwise from screen right.
    pub angle: f32,
    pub seed: u32,
    /// The colour distance fades into; alpha is how far.
    pub haze: Color,
}

/// A landmark's shape.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outline {
    /// A lit ball in tone bands with an ink rim and hatched shadow: a planet or moon.
    Disc,
    /// A soft, cloudy, lightly hatched streak: a nebula.
    Wash,
}

/// A big shape far away.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shape {
    pub outline: Outline,
    /// Its centre on the play plane, where the view placed it for this frame.
    pub center: Vec3,
    /// Disc radius, or a wash's half length, in world units on the play plane.
    pub radius: f32,
    /// A wash's width as a share of its length.
    pub stretch: f32,
    /// A wash's direction, radians anticlockwise from screen right.
    pub angle: f32,
    /// Paint; alpha is its opacity.
    pub color: Color,
    /// Multiplies a disc's paint in its shadow band.
    pub shadow: Color,
    pub ink: Color,
    /// A disc's ink rim, logical pixels.
    pub outline_px: f32,
    /// Hatching strength, 0 to 1.
    pub hatch: f32,
    pub seed: u32,
    /// The colour distance fades into; alpha is how far.
    pub haze: Color,
}

/// One distant thing, in the order drawn.
#[derive(Clone, Debug, PartialEq)]
pub enum Far {
    Field(Field),
    Shape(Shape),
    /// Meshes drawn in the cel and ink style, but behind every play mesh and before the
    /// far things after them; a mesh's [`MeshInstance::haze`] fades it into the sky.
    Meshes(Vec<MeshInstance>),
}

/// The ratio of a field's grid that holds marks: cells are sized so that `density`
/// marks fall in each 100 by 100 square.
pub const FIELD_CHANCE: f32 = 0.7;

/// The cell edge for `density` marks per 100 by 100 square, in that square's units
/// (the view uses logical pixels at the camera's own zoom); `None` for none.
pub fn field_cell(density: f32) -> Option<f32> {
    (density.is_finite() && density > 0.0).then(|| 100.0 * (FIELD_CHANCE / density).sqrt())
}

/// Where a field's grid point under the screen's centre is, for a camera at `camera`
/// on the plane and a layer at `parallax`.
pub fn offset(camera: Vec2, parallax: f32) -> Vec2 {
    camera * parallax
}

/// Where a far thing at `at` (relative to the camera's target when the camera is at the
/// world origin) is drawn on the play plane this frame: `at`, plus the share of the
/// camera's travel it does not keep up with.
pub fn placed(target: Vec2, at: Vec2, parallax: f32) -> Vec2 {
    target + at - target * parallax
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_layer_at_zero_is_fixed_and_one_moves_with_play() {
        let at = Vec2::new(100.0, 50.0);
        for target in [Vec2::ZERO, Vec2::new(300.0, -40.0), Vec2::new(-1e4, 7e3)] {
            // Fixed: always `at` from the screen's centre.
            assert_eq!(placed(target, at, 0.0) - target, at);
            // Play layer: fixed in the world.
            assert!((placed(target, at, 1.0) - at).length() < 1e-2);
        }
        // A slow layer slides a tenth as far as the camera.
        let a = placed(Vec2::ZERO, at, 0.1);
        let b = placed(Vec2::new(1000.0, 0.0), at, 0.1);
        let screen_shift = (b - Vec2::new(1000.0, 0.0)) - a;
        assert!((screen_shift - Vec2::new(-100.0, 0.0)).length() < 1e-3);
    }

    #[test]
    fn density_sizes_the_cells() {
        assert_eq!(field_cell(0.0), None);
        assert_eq!(field_cell(f32::NAN), None);
        let cell = field_cell(1.0).unwrap();
        // One mark in each 100 by 100 on average.
        let marks = (100.0 / cell).powi(2) * FIELD_CHANCE;
        assert!((marks - 1.0).abs() < 1e-4);
    }
}
