//! Meshes and the model contract.
//!
//! A model is a glTF file in a module (`models/<name>.gltf`, buffers embedded). This
//! crate reads it into [`MeshData`]: the triangles, their paint and glow, and what the
//! cel and ink style derives at load (outline normals, part spans, facet spans), plus a
//! morph target for shape changes. It also writes models, for tests and generators. It
//! has no GPU dependency: the renderer uploads what it makes. How it works:
//! `engine/docs/renderer-and-style.md`.

pub mod color;
pub mod gltf;
pub mod mesh;

pub use color::Color;
pub use mesh::{MeshBuilder, MeshData, Vertex, compute_outline_normals};
