//! The sandbox holds: no clock, no unseeded random, no code loading, frozen globals,
//! `require` inside the pack, a memory limit, and a step budget `pcall` can't dodge.

use std::path::Path;
use std::time::{Duration, Instant};

#[test]
fn sandbox_escape_attempts_fail() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/sandbox_pack");
    let started = Instant::now();
    let report = galaxy_script::run_pack_tests(&dir).expect("the pack reads");
    assert!(
        started.elapsed() < Duration::from_secs(20),
        "cut off, not hung"
    );
    for t in &report.tests {
        if t.name == "budget_survives_pcall" {
            assert!(!t.passed, "an endless loop under pcall is still cut off");
            assert!(
                t.message.as_deref().unwrap_or("").contains("budget"),
                "{t:?}"
            );
        } else {
            assert!(t.passed, "{t:?}");
        }
    }
    assert_eq!(report.tests.len(), 7);
}
