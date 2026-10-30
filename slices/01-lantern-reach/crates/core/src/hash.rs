//! The determinism hash: BLAKE3 over a canonical serialization of the whole state.
//!
//! Canonical because every map in the state is a `BTreeMap` (ordered by key), struct
//! fields serialize in declaration order, and floats serialize as their shortest
//! round-trip decimal. Same build + same seed + same inputs = same hash.

use std::io::Write;

use crate::sim::SimState;

struct HashWriter(blake3::Hasher);

impl Write for HashWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.update(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Hex BLAKE3 of the state.
pub fn state_hash(state: &SimState) -> String {
    let mut w = HashWriter(blake3::Hasher::new());
    serde_json::to_writer(&mut w, state).expect("sim state serializes");
    w.0.finalize().to_hex().to_string()
}
