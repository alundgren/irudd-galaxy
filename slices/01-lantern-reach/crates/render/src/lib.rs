//! The Lantern Reach renderer.
//!
//! A small 3D renderer on wgpu with a cel-shaded, ink-outlined look, plus a procedural
//! [`kit`] of parametric meshes. It knows nothing about the simulation: the app turns
//! world state into a [`Scene`] each frame and hands it to a [`Renderer`].

pub mod camera;
pub mod color;
pub mod kit;
pub mod mesh;
pub mod palette;
pub mod scene;

pub use camera::{Camera, Projection};
pub use color::Color;
pub use glam;
pub use mesh::{MeshBuilder, MeshData, Vertex};
pub use scene::*;
