//! The native modifiers a ship has from the pilot's traits and fittings and its hull's
//! ranks, added up. The fold lives in `galaxy_contract::modifiers` so the kernel's
//! invariants, the ship and the economy read the same totals without depending on this
//! crate (which depends on `galaxy-core`); it is re-exported here.

pub use galaxy_contract::modifiers::{Modifiers, deed_bonus, for_ship, hold_capacity, pilot};
