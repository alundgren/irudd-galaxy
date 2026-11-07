//! `galaxy-gm test <module>`: runs a module's own Luau tests (`tests/*.luau`,
//! `galaxy_script::testrun`) and its scenario runs (`tests/*.toml`,
//! `galaxy_sim::scenario`), with the modules beside it loaded as `check` loads them, and
//! its bots (`tests/*.bot.toml`, [`crate::bot`]), headless and side by side. Performance
//! scenes (`tests/*.perf.toml`) are not tests: `galaxy-gm perf` runs them ([`crate::perf`]).
//! Reports each; any failure exits 5 with kind `tests_failed`.

use std::path::Path;

use galaxy_api::engine::TICKS_PER_SECOND;
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

/// What `--only <text>` keeps, of tests in `files`: when the text is a whole file's name
/// or path (`trade.bot.toml`, `tests/trade.bot.toml`), that file alone; otherwise each test
/// whose file or name contains it. Called with a file and a test name.
pub fn only_filter<'a>(
    only: Option<&'a str>,
    mut files: impl Iterator<Item = &'a str>,
) -> impl Fn(&str, &str) -> bool + 'a {
    fn names(file: &str, text: &str) -> bool {
        file == text || file.rsplit('/').next() == Some(text)
    }
    let exact = only.is_some_and(|o| files.any(|f| names(f, o)));
    move |file: &str, name: &str| match only {
        None => true,
        Some(o) if exact => names(file, o),
        Some(o) => file.contains(o) || name.contains(o),
    }
}

/// Runs the tests of the module at `dir`. `only` keeps the tests [`only_filter`] keeps.
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
    let keep = only_filter(only, module.files.0.keys().map(String::as_str));

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
        .filter(|(path, _)| !path.ends_with(".bot.toml") && !path.ends_with(".perf.toml"))
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
    let declared: Vec<(&String, &String)> = module
        .files
        .0
        .iter()
        .filter(|(path, _)| {
            path.starts_with("tests/") && path.ends_with(".bot.toml") && keep(path, "")
        })
        .collect();
    let root = if context.is_empty() {
        dir.as_path()
    } else {
        dir.parent().unwrap_or(&dir)
    };
    let bots_started = std::time::Instant::now();
    let bots = run_bots(&declared, |path, text| {
        crate::bot::run(&module, root, path, text)
    });
    let bots_ms = bots_started.elapsed().as_secs_f64() * 1000.0;
    for (report, (path, _)) in bots.iter().zip(&declared) {
        if let Some(error) = report["failure"].as_str() {
            failures.push(format!("{path}: {error}"));
        }
    }
    let failed = tests
        .iter()
        .chain(&scenarios)
        .chain(&bots)
        .filter(|t| t["passed"] != true)
        .count();
    let passed = tests.len() + scenarios.len() + bots.len() - failed;
    if tests.is_empty() && scenarios.is_empty() && bots.is_empty() {
        notes.push(match only {
            Some(o) => format!("no test's file is `{o}`, or its file or name contains it"),
            None => "the module has no tests: add tests/<name>.luau (Luau tests) or \
                     tests/<name>.toml (scenario runs); galaxy-gm guide tests shows both"
                .to_owned(),
        });
    }
    let ok = failed == 0;
    if !bots.is_empty() {
        let game_ms: f64 = bots
            .iter()
            .filter_map(|b| b["tick"].as_u64())
            .map(|t| t as f64 * 1000.0 / f64::from(TICKS_PER_SECOND))
            .sum();
        notes.push(format!(
            "{} bot(s) played {:.1} minutes of game time in {:.1} minutes, headless and side by side",
            bots.len(),
            game_ms / 60_000.0,
            bots_ms / 60_000.0,
        ));
    }
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

/// Runs each declared bot; the reports are in declaration order. Headless bots are
/// independent processes, so they go side by side on half the machine's cores, leaving the
/// rest for whatever else is running.
fn run_bots<'a>(
    declared: &[(&'a String, &'a String)],
    run: impl Fn(&'a str, &'a str) -> Value + Sync,
) -> Vec<Value> {
    let workers = std::thread::available_parallelism()
        .map_or(1, |n| (n.get() / 2).max(1))
        .min(declared.len().max(1));
    if workers <= 1 {
        return declared.iter().map(|(p, t)| run(p, t)).collect();
    }
    // The longest first, so the last to finish is not a long one started late.
    let ticks = |text: &str| {
        toml::from_str::<toml::Table>(text)
            .ok()
            .and_then(|t| t.get("ticks")?.as_integer())
            .unwrap_or(0)
    };
    let mut order: Vec<usize> = (0..declared.len()).collect();
    order.sort_by_key(|&i| std::cmp::Reverse(ticks(declared[i].1)));
    let next = std::sync::atomic::AtomicUsize::new(0);
    let results = std::sync::Mutex::new(vec![Value::Null; declared.len()]);
    std::thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| {
                loop {
                    let k = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    let Some(&i) = order.get(k) else {
                        break;
                    };
                    let (path, text) = declared[i];
                    let report = run(path, text);
                    results.lock().expect("a bot thread panicked")[i] = report;
                }
            });
        }
    });
    results.into_inner().expect("a bot thread panicked")
}

#[cfg(test)]
mod tests {
    use super::only_filter;

    #[test]
    fn only_a_whole_file_name_keeps_that_file_alone() {
        let files = [
            "tests/trade.bot.toml",
            "tests/mixed-trade.bot.toml",
            "tests/fit.luau",
        ];
        let keeps = |only: &str| {
            let keep = only_filter(Some(only), files.iter().copied());
            files.iter().filter(|f| keep(f, "")).count()
        };
        assert_eq!(keeps("trade.bot.toml"), 1);
        assert_eq!(keeps("tests/trade.bot.toml"), 1);
        assert_eq!(
            keeps("trade"),
            2,
            "a part of a name still matches every file with it"
        );
        assert_eq!(keeps(".bot.toml"), 2);
        let keep = only_filter(Some("sells_ore"), files.iter().copied());
        assert!(keep("tests/fit.luau", "sells_ore_first") && !keep("tests/fit.luau", "buys"));
        assert!(only_filter(None, files.iter().copied())("anything", ""));
    }
}
