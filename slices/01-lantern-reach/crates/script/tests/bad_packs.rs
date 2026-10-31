//! The deliberately bad packs are rejected with precise reports, and a script that
//! loops forever at runtime is cut off, discarded and quarantined. Headless.

use crate::common;

use std::time::{Duration, Instant};

use common::*;
use galaxy_contract::history::HistoryQuery;
use galaxy_contract::pack::{Severity, StepOutcome, ValidationReport, ValidationStep};
use galaxy_core::new_game;
use galaxy_script::host::{HostEvent, QUARANTINED};
use galaxy_script::{CheckOptions, ScriptHost, check_dir};

fn check(dir: &std::path::Path, analyzer: Option<&std::path::Path>) -> ValidationReport {
    let state = new_game(&core_data(), 1).unwrap();
    check_dir(
        dir,
        CheckOptions {
            world: Some(&state.world),
            history: Some(&state.history),
            analyzer,
            require_typecheck: analyzer.is_some(),
            ..Default::default()
        },
    )
    .report
}

/// `(step, code, file, line)` of every error.
fn errors(r: &ValidationReport) -> Vec<(ValidationStep, String, String, Option<u32>)> {
    r.steps
        .iter()
        .flat_map(|s| s.diagnostics.iter().map(move |d| (s.step, d)))
        .filter(|(_, d)| d.severity == Severity::Error)
        .map(|(step, d)| {
            let loc = d
                .location
                .clone()
                .unwrap_or(galaxy_contract::pack::Location {
                    file: String::new(),
                    line: None,
                    column: None,
                });
            (step, d.code.clone(), loc.file, loc.line)
        })
        .collect()
}

fn has(r: &ValidationReport, step: ValidationStep, code: &str, file: &str, line: Option<u32>) {
    assert!(!r.accepted(), "{r:#?}");
    let e = errors(r);
    assert!(
        e.iter()
            .any(|(s, c, f, l)| *s == step && c == code && f == file && *l == line),
        "expected {step:?} {code} at {file}:{line:?}, got {e:#?}"
    );
}

#[test]
fn the_good_pack_is_accepted() {
    let r = check(&kettle_dir(), None);
    assert!(r.accepted(), "{r:#?}");
    let types = r
        .steps
        .iter()
        .find(|s| s.step == ValidationStep::Types)
        .unwrap();
    assert_eq!(types.outcome, StepOutcome::Skipped, "no analyzer passed");
    let smoke = r
        .steps
        .iter()
        .find(|s| s.step == ValidationStep::Smoke)
        .unwrap();
    assert_eq!(smoke.outcome, StepOutcome::Passed);
}

#[test]
fn undeclared_hook_is_rejected() {
    let r = check(&bad("undeclared_hook"), None);
    has(
        &r,
        ValidationStep::Hooks,
        "undeclared_hook",
        "scripts/grow_on_dock.luau",
        Some(10),
    );
    has(
        &r,
        ValidationStep::Hooks,
        "undeclared_hook",
        "scripts/grow_on_dock.luau",
        Some(11),
    );
}

#[test]
fn infinite_loop_is_rejected() {
    let started = Instant::now();
    let r = check(&bad("infinite_loop"), None);
    has(
        &r,
        ValidationStep::Smoke,
        "over_budget",
        "scripts/spin.luau",
        Some(11),
    );
    assert!(
        started.elapsed() < Duration::from_secs(20),
        "cut off, not hung"
    );
}

#[test]
fn clock_api_is_rejected() {
    let r = check(&bad("os_time"), None);
    has(
        &r,
        ValidationStep::Invariants,
        "clock_api",
        "scripts/night_shift.luau",
        Some(8),
    );
}

#[test]
fn uncited_talk_is_rejected() {
    let r = check(&bad("uncited_talk"), None);
    has(
        &r,
        ValidationStep::Invariants,
        "uncited_line",
        "scripts/chatter.luau",
        Some(9),
    );
}

#[test]
fn unpaired_shock_is_rejected() {
    let r = check(&bad("unpaired_shock"), None);
    has(
        &r,
        ValidationStep::Invariants,
        "unpaired_shock",
        "manifest.toml",
        None,
    );
}

#[test]
fn time_trigger_is_rejected() {
    let r = check(&bad("timer_trigger"), None);
    has(
        &r,
        ValidationStep::Hooks,
        "bad_trigger",
        "scripts/later.luau",
        Some(7),
    );
    assert_eq!(errors(&r).len(), 1, "{:#?}", errors(&r));
}

#[test]
fn turning_the_typecheck_off_is_rejected() {
    // Without the analyzer: the directive alone is an error.
    let r = check(&bad("nocheck"), None);
    has(
        &r,
        ValidationStep::Types,
        "typecheck_off",
        "scripts/unchecked.luau",
        Some(1),
    );
    // And the first line isn't `--!strict`.
    has(
        &r,
        ValidationStep::Types,
        "not_strict",
        "scripts/unchecked.luau",
        Some(1),
    );
    assert_eq!(errors(&r).len(), 2, "{:#?}", errors(&r));
}

#[test]
fn bare_iteration_is_rejected() {
    let r = check(&bad("bare_iteration"), None);
    has(
        &r,
        ValidationStep::Invariants,
        "unordered_iteration",
        "scripts/tally.luau",
        Some(9),
    );
    assert_eq!(errors(&r).len(), 1, "{:#?}", errors(&r));
}

#[test]
fn strict_typecheck_with_the_pinned_analyzer() {
    let Some(analyzer) = galaxy_script::analyzer::find(None) else {
        println!(
            "SKIPPED: luau-analyze not found; run `cargo xtask fetch-luau` (cargo xtask check does)"
        );
        return;
    };
    let good = check(&kettle_dir(), Some(&analyzer));
    let types = good
        .steps
        .iter()
        .find(|s| s.step == ValidationStep::Types)
        .unwrap();
    assert_eq!(types.outcome, StepOutcome::Passed, "{types:#?}");
    assert!(good.accepted(), "{good:#?}");

    let r = check(&bad("uncited_talk"), Some(&analyzer));
    has(
        &r,
        ValidationStep::Types,
        "type_error",
        "scripts/chatter.luau",
        Some(9),
    );
    // The analyzer alone would accept `--!nocheck` code; the directive is rejected.
    let r = check(&bad("nocheck"), Some(&analyzer));
    has(
        &r,
        ValidationStep::Types,
        "typecheck_off",
        "scripts/unchecked.luau",
        Some(1),
    );
    let r = check(&bad("bare_iteration"), Some(&analyzer));
    let types = r
        .steps
        .iter()
        .find(|s| s.step == ValidationStep::Types)
        .unwrap();
    assert_eq!(types.outcome, StepOutcome::Passed, "{types:#?}");
    let r = check(&bad("undeclared_hook"), Some(&analyzer));
    has(
        &r,
        ValidationStep::Types,
        "type_error",
        "scripts/grow_on_dock.luau",
        Some(10),
    );
    has(
        &r,
        ValidationStep::Types,
        "type_error",
        "scripts/grow_on_dock.luau",
        Some(11),
    );
}

#[test]
fn a_runtime_infinite_loop_is_cut_off_discarded_and_quarantined() {
    let mut sim = new_sim(1);
    let host = ScriptHost::new(None);
    host.load_unchecked(&bad("infinite_loop")).unwrap();
    host.attach(&mut sim);
    let started = Instant::now();
    for _ in 0..3 {
        dock(&mut sim, &KETTLE);
        settle(&mut sim);
        assert_eq!(stage(&sim), 0, "the write before the loop was discarded");
    }
    assert!(
        started.elapsed() < Duration::from_secs(20),
        "cut off, not hung"
    );

    let events = host.take_events();
    let aborts: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            HostEvent::Aborted { diagnostic, .. } => Some(diagnostic),
            _ => None,
        })
        .collect();
    assert_eq!(aborts.len(), 3);
    assert_eq!(aborts[0].code, "over_budget");
    assert_eq!(
        aborts[0].location.as_ref().unwrap().file,
        "scripts/spin.luau"
    );
    assert!(
        events
            .iter()
            .any(|e| matches!(e, HostEvent::Quarantined { script, .. } if script == "spin"))
    );

    let health = &sim
        .world()
        .packs
        .get("gm.bad_infinite_loop")
        .unwrap()
        .health["spin"];
    assert!(health.quarantined);
    assert_eq!(health.aborts, 3);
    let fact = sim
        .history()
        .last(&HistoryQuery::kind(QUARANTINED))
        .expect("quarantine fact");
    assert!(fact.amount == Some(3));

    // Quarantined: further dockings don't even call it.
    dock(&mut sim, &KETTLE);
    settle(&mut sim);
    assert!(host.take_events().is_empty());
}
