//! Structural canon validation. Free prose is not a theorem prover: assertions
//! must declare node IDs; opening and claiming sealed facts is checked again at use.
use crate::Pack;
use galaxy_contract::{canon::CanonStatus, pack::*, prelude::*};
pub fn check(pack: &Pack, world: &WorldState, require_ready: bool) -> StepReport {
    let mut diagnostics = Vec::new();
    let mut error = |msg: String| {
        diagnostics.push(Diagnostic {
            severity: Severity::Error,
            code: "canon".into(),
            message: msg,
            location: None,
        })
    };
    for id in &pack.manifest.opens {
        match world.canon.nodes.get(id) {
            None => error(format!("unknown canon node {id}")),
            Some(n) => {
                let opener = if pack.manifest.id.as_str() == "core" {
                    n.opener.as_deref().is_some_and(|o| o.starts_with("core:"))
                } else {
                    n.opener.as_deref() == Some("pack")
                };
                if !opener || !matches!(n.status, CanonStatus::Sealed | CanonStatus::Open) {
                    error(format!("{} may not open {id}", pack.manifest.id));
                } else if require_ready
                    && let Err(e) = world.canon.may_open(id, &pack.manifest.id, world)
                {
                    error(e);
                }
            }
        }
    }
    for id in &pack.manifest.asserts {
        if !world
            .canon
            .nodes
            .get(id)
            .is_some_and(|n| matches!(n.status, CanonStatus::Locked | CanonStatus::Open))
        {
            error(format!("cannot assert {id}: not revealed canon"));
        }
    }
    StepReport {
        step: ValidationStep::Canon,
        outcome: if diagnostics.is_empty() {
            StepOutcome::Passed
        } else {
            StepOutcome::Failed
        },
        diagnostics,
    }
}
