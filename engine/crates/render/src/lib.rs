//! The renderer: glTF models in the cel and ink style.
//!
//! A small 3D renderer on wgpu. Models are lit in two or three flat tone bands, hatched in
//! shadow and outlined in ink: a silhouette pushed out along welded outline normals, and
//! inner lines where creases, paint borders or depth steps show. It knows nothing about
//! the world: the game turns world state into a [`Scene`] each frame and hands it to a
//! [`Renderer`]. Meshes come from [`galaxy_mesh`]; how a module wants things drawn comes
//! from its [`style::Style`]. How it works: `engine/docs/renderer-and-style.md`.

pub mod backdrop;
pub mod camera;
pub mod defaults;
mod renderer;
mod rng;
pub mod scene;
pub mod style;

pub use backdrop::{Backdrop, BackdropParams, backdrop};
pub use camera::{Camera, Projection};
pub use galaxy_mesh::{Color, MeshBuilder, MeshData, Vertex};
pub use glam;
pub use renderer::{RenderError, Renderer, RgbaImage};
pub use scene::*;
pub use style::{Ink, Look, Style};
