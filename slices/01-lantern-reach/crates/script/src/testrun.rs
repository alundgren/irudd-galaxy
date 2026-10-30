//! `galaxy pack test`: runs a pack's `tests/*.luau` in the same sandbox as its
//! scripts. A test file returns `{ [name] = function() ... end }`; a test passes if it
//! returns without an error (use `assert`).

use std::path::Path;
use std::rc::Rc;

use galaxy_contract::pack::{Diagnostic, TestReport, TestResult, ValidationReport};

use crate::check::{CheckOptions, check_dir};
use crate::pack::read_pack;
use crate::vm::PackVm;

fn located(d: &Diagnostic) -> String {
    match &d.location {
        Some(l) => format!("{}:{}: {}", l.file, l.line.unwrap_or(0), d.message),
        None => d.message.clone(),
    }
}

/// Runs the pack's tests. A pack whose manifest doesn't read gets its check report.
pub fn run_pack_tests(dir: &Path) -> Result<TestReport, Box<ValidationReport>> {
    let pack = match read_pack(dir) {
        Ok(p) => Rc::new(p),
        Err(_) => return Err(Box::new(check_dir(dir, CheckOptions::default()).report)),
    };
    let mut tests = Vec::new();
    let files: Vec<String> = pack.test_files().map(|(f, _)| f.to_owned()).collect();
    for file in files {
        let failed = |message: String| TestResult {
            file: file.clone(),
            name: "(load)".into(),
            passed: false,
            message: Some(message),
        };
        let vm = match PackVm::bare(pack.clone()) {
            Ok(vm) => vm,
            Err(e) => {
                tests.push(failed(e.to_string()));
                continue;
            }
        };
        match vm.run_tests(&file) {
            Ok(results) => {
                for (name, r) in results {
                    tests.push(TestResult {
                        file: file.clone(),
                        name,
                        passed: r.is_ok(),
                        message: r.err().map(|a| located(&a.diagnostic())),
                    });
                }
            }
            Err(a) => tests.push(failed(located(&a.diagnostic()))),
        }
    }
    Ok(TestReport {
        pack: pack.manifest.id.clone(),
        revision: pack.revision.clone(),
        passed: tests.iter().all(|t| t.passed),
        tests,
    })
}
