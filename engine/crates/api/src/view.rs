//! What a script reads: the world as it is when its handler runs.

use crate::command::Fields;

/// A read-only view of the world.
pub trait WorldView {
    /// The tick being run.
    fn tick(&self) -> u64;
    /// A component's fields, if the entity has it.
    fn get(&self, entity: &str, component: &str) -> Option<&Fields>;
    /// Whether the entity has any component.
    fn exists(&self, entity: &str) -> bool;
    /// The entities that have `component`, in ID order.
    fn with_component(&self, component: &str) -> Vec<&str>;
}
