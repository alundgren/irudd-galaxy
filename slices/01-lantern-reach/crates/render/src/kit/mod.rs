//! The procedural parametric kit: CPU-side generators that turn parameters and a seed
//! into meshes (and a backdrop). Same inputs, same output, on every platform.
//!
//! - [`hull`]: ship hulls (Wren, Tandem, Heron), and [`fitted_hull`] wearing their parts.
//! - [`Cradle`]: a ship drawn large in the hullwright's cradle, with its sockets.
//! - [`station`]: stations assembled from a serde [`StationRecipe`] of parts.
//! - [`rock`]: ice, iron and glassite rocks that shrink and crack as they're mined.
//! - [`backdrop`]: a quiet nebula and star field.
//! - [`life`]: residents, stock piles, lamp posts and the cargo crane of a living station.

mod backdrop;
pub mod life;
mod parts;
mod rng;
mod rock;
mod ship;
mod station;

pub use backdrop::{Backdrop, BackdropParams, Blob, backdrop};
pub use parts::{
    CRADLE_SHADOW, Cradle, CradlePose, CradleSocket, FITTED_GOLD, SEAM_LIGHT, SKY_BLUE, SocketRing,
    cradle_sockets, fitted_hull, ghost_material, part, part_ghost, seam_lamp_light,
};
pub use rng::Rng;
pub use rock::{RockKind, RockParams, rock, rock_morph, rock_scale};
pub use ship::{HullClass, HullColors, HullParams, HullSockets, LookId, ShipFitLook, Socket, hull};
pub use station::{
    Anchor, Decal, DecalKind, MAX_PARTS, MAX_STATION_RADIUS, MODULE_KINDS, PART_KINDS, Part,
    PartKind, Placement, RecipeError, StationLight, StationPalette, StationRecipe, anchors,
    docking_ring_center, module, module_part, station, station_lights,
};
