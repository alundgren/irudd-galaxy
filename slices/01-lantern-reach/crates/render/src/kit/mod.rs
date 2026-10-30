//! The procedural parametric kit: CPU-side generators that turn parameters and a seed
//! into meshes (and a backdrop). Same inputs, same output, on every platform.
//!
//! - [`hull`]: ship hulls (Wren, Tandem, Heron).
//! - [`station`]: stations assembled from a serde [`StationRecipe`] of parts.
//! - [`rock`]: ice, iron and glassite rocks that shrink and crack as they're mined.
//! - [`backdrop`]: a quiet nebula and star field.

mod backdrop;
mod rng;
mod rock;
mod ship;
mod station;

pub use backdrop::{Backdrop, BackdropParams, Blob, backdrop};
pub use rng::Rng;
pub use rock::{RockKind, RockParams, rock, rock_morph, rock_scale};
pub use ship::{HullClass, HullColors, HullParams, hull};
pub use station::{
    Decal, DecalKind, MAX_PARTS, MAX_STATION_RADIUS, PART_KINDS, Part, PartKind, Placement,
    RecipeError, StationLight, StationPalette, StationRecipe, docking_ring_center, station,
    station_lights,
};
