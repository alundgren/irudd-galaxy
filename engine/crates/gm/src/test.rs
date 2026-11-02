//! `galaxy-gm test <module>`: runs a module's own Luau tests (`tests/*.luau`,
//! `galaxy_script::testrun`) and its scenario runs (`tests/*.toml`,
//! `galaxy_sim::scenario`), with the modules beside it loaded as `check` loads them, and
//! reports each. Any failure exits 5 with kind `tests_failed`.

use std::path::Path;

use galaxy_module::cli::Output;
use galaxy_module::discover;
use galaxy_module::failure::code;
use galaxy_module::module::Module;
use galaxy_module::{Failure, gate, manifest};
use galaxy_script::testrun;
use galaxy_sim::scenario;
use serde_json::{Value, json};

/// Options `test` takes.
pub const OPTIONS: &[&str] = &["only"];

/// Runs the tests of the module at `dir`. `only` keeps the tests whose file or name
/// contains it.
pub fn run(dir: &Path, only: Option<&str>) -> Result<Output, Failure> {
    let dir = manifest::module_dir(dir);
    gate::open(std::slice::from_ref(&dir))?;
    let module = Module::read(&dir)?;
    let mut all = Vec::new();
    let mut notes = Vec::new();
    if let Some(parent) = dir.parent() {
        for other in discover::modules_in(parent) {
            if other == dir {
                continue;
            }
            match Module::read(&other) {
                Ok(m) if m.id() != module.id() => all.push(m),
                Ok(_) => {}
                Err(f) => notes.push(format!("skipped {}: {}", other.display(), f.message)),
            }
        }
    }
    let context: Vec<String> = all.iter().map(|m| m.id().to_owned()).collect();
    all.push(module.clone());
    let keep = |file: &str, name: &str| only.is_none_or(|o| file.contains(o) || name.contains(o));

    let mut failures = Vec::new();
    let tests: Vec<Value> = testrun::run_luau_tests(&module)
        .into_iter()
        .filter(|t| keep(&t.file, &t.name))
        .map(|t| {
            if let Some(f) = &t.failure {
                failures.push(format!("{} {}: {f}", t.file, t.name));
            }
            json!({ "file": t.file, "name": t.name, "passed": t.passed, "failure": t.failure })
        })
        .collect();
    let scenarios: Vec<Value> = module
        .files
        .0
        .iter()
        .filter(|(path, _)| path.starts_with("tests/") && path.ends_with(".toml"))
        .filter(|(path, _)| !path.ends_with(".bot.toml"))
        .filter(|(path, _)| keep(path, ""))
        .map(|(path, text)| {
            match scenario::run_scenario(&all, module.id(), path, text) {
                Ok(r) => {
                    failures.extend(r.failures.iter().map(|f| format!("{path}: {f}")));
                    json!({
                        "file": path,
                        "passed": r.passed,
                        "ticks": r.ticks,
                        "hash": r.hash,
                        "failures": r.failures,
                    })
                }
                // The scenario itself is wrong (or the modules don't start): it fails,
                // and the others still run.
                Err(e) => {
                    failures.push(format!("{path}: {e}"));
                    json!({ "file": path, "passed": false, "failures": [e] })
                }
            }
        })
        .collect();
    let bots: Vec<Value> = module
        .files
        .0
        .iter()
        .filter(|(path, _)| {
            path.starts_with("tests/") && path.ends_with(".bot.toml") && keep(path, "")
        })
        .map(|(path, text)| {
            let root = if context.is_empty() {
                dir.as_path()
            } else {
                dir.parent().unwrap_or(&dir)
            };
            let report = crate::bot::run(&module, root, path, text);
            if let Some(error) = report["failure"].as_str() {
                failures.push(format!("{path}: {error}"));
            }
            report
        })
        .collect();
    let failed = tests
        .iter()
        .chain(&scenarios)
        .chain(&bots)
        .filter(|t| t["passed"] != true)
        .count();
    let passed = tests.len() + scenarios.len() + bots.len() - failed;
    if tests.is_empty() && scenarios.is_empty() && bots.is_empty() {
        notes.push(match only {
            Some(o) => format!("no test's file or name contains `{o}`"),
            None => "the module has no tests: add tests/<name>.luau (Luau tests) or \
                     tests/<name>.toml (scenario runs); galaxy-gm guide tests shows both"
                .to_owned(),
        });
    }
    let ok = failed == 0;
    let out = json!({
        "ok": ok,
        "module": module.id(),
        "version": module.manifest.version.to_string(),
        "context_modules": context,
        "notes": notes,
        "passed": passed,
        "failed": failed,
        "failures": failures,
        "tests": tests,
        "scenarios": scenarios,
        "bots": bots,
    });
    if ok {
        return Ok(Output::Json(out));
    }
    let mut failure = Failure::new(
        code::REJECTED,
        "tests_failed",
        format!(
            "{}: {failed} of {} test(s) failed",
            module.id(),
            passed + failed
        ),
    );
    if let Value::Object(fields) = out {
        failure.fields = fields;
    }
    Err(failure)
}
