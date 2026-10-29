//! The event bus for code outside the tick (the app, the game-master bridge).
//!
//! Systems inside the tick read `TickCtx::events` instead: the events committed at
//! the end of the previous tick. The bus is not saved; subscribers resubscribe.

use std::collections::{BTreeMap, VecDeque};

use galaxy_contract::events::EventEnvelope;

/// A subscriber handle.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Subscriber(String);

#[derive(Debug, Default)]
pub struct EventBus {
    log: VecDeque<EventEnvelope>,
    /// Sequence number of `log[0]`.
    base: u64,
    cursors: BTreeMap<String, u64>,
}

impl EventBus {
    /// Subscribes from now on. Re-subscribing a name resets it to now.
    pub fn subscribe(&mut self, name: impl Into<String>) -> Subscriber {
        let name = name.into();
        let end = self.base + self.log.len() as u64;
        self.cursors.insert(name.clone(), end);
        Subscriber(name)
    }

    pub fn unsubscribe(&mut self, sub: &Subscriber) {
        self.cursors.remove(&sub.0);
        self.trim();
    }

    /// Everything published since this subscriber last drained.
    pub fn drain(&mut self, sub: &Subscriber) -> Vec<EventEnvelope> {
        let end = self.base + self.log.len() as u64;
        let Some(cursor) = self.cursors.get_mut(&sub.0) else {
            return Vec::new();
        };
        let from = (*cursor - self.base) as usize;
        *cursor = end;
        let out = self.log.iter().skip(from).cloned().collect();
        self.trim();
        out
    }

    pub(crate) fn publish(&mut self, events: &[EventEnvelope]) {
        if self.cursors.is_empty() {
            self.base += events.len() as u64;
            return;
        }
        self.log.extend(events.iter().cloned());
    }

    fn trim(&mut self) {
        let min = self
            .cursors
            .values()
            .min()
            .copied()
            .unwrap_or(self.base + self.log.len() as u64);
        while self.base < min && self.log.pop_front().is_some() {
            self.base += 1;
        }
    }
}
