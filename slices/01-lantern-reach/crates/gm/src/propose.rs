//! `galaxy gm propose`: the full validation pipeline, ending in a report the caller
//! stages.
//!
//! 1. The fast checks of `galaxy pack check` (manifest, types, hooks, references,
//!    invariants, smoke) against the world the pack would join.
//! 2. `canon`: skipped; the slice has no canon graph yet (an info line says so).
//! 3. `sandbox`: [`crate::sandbox`] forks the base save and flies the activity loop
//!    with and without the pack in two child processes, then compares telemetry,
//!    handler health and rejected batches.
//! 4. `legibility`: from the same runs, the pack's changes came with lines that fired.
//! 5. `stage` is appended by whoever stages the pack ([`stage_step`]).
//!
//! A failed step skips the later ones; the report says which and why.

use std::path::{Path, PathBuf};

use galaxy_contract::pack::{
    Diagnostic, ReportStatus, Severity, StepOutcome, StepReport, ValidationReport, ValidationStep,
};
use galaxy_contract::prelude::*;
use galaxy_core::Save;
use galaxy_script::{CheckOptions, check_dir};

use crate::sandbox::{self, SandboxRules};

/// How to validate.
#[derive(Clone, Debug)]
pub struct ProposeOptions {
    /// The `galaxy` binary the sandbox children run.
    pub exe: PathBuf,
    /// `luau-analyze` for the strict typecheck.
    pub analyzer: Option<PathBuf>,
    /// Reject when the analyzer is missing (otherwise skip the typecheck with a warning).
    pub require_typecheck: bool,
    pub rules: SandboxRules,
}

fn info(code: &str, message: impl Into<String>) -> Diagnostic {
    Diagnostic {
        severity: Severity::Info,
        code: code.into(),
        message: message.into(),
        location: None,
    }
}

fn skipped(step: ValidationStep, why: &str) -> StepReport {
    StepReport {
        step,
        outcome: StepOutcome::Skipped,
        diagnostics: vec![info("skipped", why)],
    }
}

/// Sets the status from the steps: rejected if any failed.
pub fn settle(report: &mut ValidationReport) {
    let failed = report
        .steps
        .iter()
        .any(|s| s.outcome == StepOutcome::Failed);
    if failed {
        report.status = ReportStatus::Rejected;
    } else if report.status == ReportStatus::Rejected {
        report.status = ReportStatus::Accepted;
    }
}

/// Validates the pack in `dir` against the save `base` (a fork of the running game, or
/// the save itself), on which `running` packs already run.
pub fn validate(
    dir: &Path,
    base: &Path,
    running: &[(PackId, PathBuf)],
    opts: &ProposeOptions,
) -> ValidationReport {
    let state = Save::open(base).and_then(|s| s.load());
    let state = match state {
        Ok(s) => s,
        Err(e) => {
            return ValidationReport {
                pack: PackId::from_static("unknown"),
                revision: String::new(),
                status: ReportStatus::Rejected,
                steps: vec![StepReport {
                    step: ValidationStep::Manifest,
                    outcome: StepOutcome::Failed,
                    diagnostics: vec![Diagnostic {
                        severity: Severity::Error,
                        code: "no_base".into(),
                        message: format!("can't read the save to validate against: {e}"),
                        location: None,
                    }],
                }],
            };
        }
    };
    let checked = check_dir(
        dir,
        CheckOptions {
            world: Some(&state.world),
            history: Some(&state.history),
            analyzer: opts.analyzer.as_deref(),
            require_typecheck: opts.require_typecheck,
        },
    );
    let mut report = checked.report;
    drop(checked.vm);
    report.steps.push(skipped(
        ValidationStep::Canon,
        "the slice has no canon graph yet; nothing to check against",
    ));
    if !report.accepted() {
        report
            .steps
            .push(skipped(ValidationStep::Sandbox, "an earlier step failed"));
        report.steps.push(skipped(
            ValidationStep::Legibility,
            "an earlier step failed",
        ));
        settle(&mut report);
        return report;
    }
    match sandbox::sandbox(&opts.exe, base, running, (&report.pack, dir), &opts.rules) {
        Ok(pair) => {
            let (s, l) = sandbox::compare(&pair, &report.pack, &opts.rules);
            report.steps.push(s);
            report.steps.push(l);
        }
        Err(e) => {
            report.steps.push(StepReport {
                step: ValidationStep::Sandbox,
                outcome: StepOutcome::Failed,
                diagnostics: vec![Diagnostic {
                    severity: Severity::Error,
                    code: "sandbox_failed".into(),
                    message: e,
                    location: None,
                }],
            });
            report.steps.push(skipped(
                ValidationStep::Legibility,
                "the sandbox run failed",
            ));
        }
    }
    settle(&mut report);
    report
}

/// The `stage` step: `diagnostics` say what happened; any error fails it.
pub fn stage_step(diagnostics: Vec<Diagnostic>) -> StepReport {
    let failed = diagnostics.iter().any(|d| d.severity == Severity::Error);
    StepReport {
        step: ValidationStep::Stage,
        outcome: if failed {
            StepOutcome::Failed
        } else {
            StepOutcome::Passed
        },
        diagnostics,
    }
}

/// An info diagnostic for the stage step.
pub fn stage_info(code: &str, message: impl Into<String>) -> Diagnostic {
    info(code, message)
}

/// An error diagnostic for the stage step.
pub fn stage_error(code: &str, message: impl Into<String>) -> Diagnostic {
    Diagnostic {
        severity: Severity::Error,
        code: code.into(),
        message: message.into(),
        location: None,
    }
}

/// Appends a skipped stage step (the pack was rejected before it).
pub fn skip_stage(report: &mut ValidationReport) {
    report.steps.push(skipped(
        ValidationStep::Stage,
        "the pack was rejected; nothing staged",
    ));
}
