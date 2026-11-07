//! `galaxy-gm test <module>`: runs a module's own Luau tests (`tests/*.luau`,
//! `galaxy_script::testrun`) and its scenario runs (`tests/*.toml`,
//! `galaxy_sim::scenario`), with the modules beside it loaded as `check` loads them, and
//! its bots (`tests/*.bot.toml`, [`crate::bot`]), headless; scenarios and bots run side by
//! side ([`side_by_side`]). Performance scenes (`tests/*.perf.toml`) are not tests: `galaxy-gm perf` runs them ([`crate::perf`]).
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
    let scenario_files: Vec<(&String, &String)> = module
        .files
        .0
        .iter()
        .filter(|(path, _)| path.starts_with("tests/") && path.ends_with(".toml"))
        .filter(|(path, _)| !path.ends_with(".bot.toml") && !path.ends_with(".perf.toml"))
        .filter(|(path, _)| keep(path, ""))
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
    // Scenarios and bots share one queue: each is independent of the others, so they run
    // side by side and the reports come back in declaration order.
    let jobs: Vec<Job> = scenario_files
        .iter()
        .map(|&(path, text)| Job::Scenario(path, text))
        .chain(declared.iter().map(|&(path, text)| Job::Bot(path, text)))
        .collect();
    let started = std::time::Instant::now();
    let mut done = side_by_side(&jobs, Job::cost, |job| {
        let report = match *job {
            Job::Scenario(path, text) => scenario_report(&all, module.id(), path, text),
            Job::Bot(path, text) => crate::bot::run(&module, root, path, text),
        };
        (report, started.elapsed())
    });
    let bots_done: Vec<(Value, std::time::Duration)> = done.split_off(scenario_files.len());
    let bots_ms = bots_done
        .iter()
        .map(|(_, at)| at.as_secs_f64() * 1000.0)
        .fold(0.0, f64::max);
    let scenarios: Vec<Value> = done.into_iter().map(|(report, _)| report).collect();
    let bots: Vec<Value> = bots_done.into_iter().map(|(report, _)| report).collect();
    for (report, (path, _)) in scenarios.iter().zip(&scenario_files) {
        for f in report["failures"].as_array().into_iter().flatten() {
            failures.push(format!("{path}: {}", f.as_str().unwrap_or_default()));
        }
    }
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

/// One scenario run or bot, by its file and text.
enum Job<'a> {
    Scenario(&'a str, &'a str),
    Bot(&'a str, &'a str),
}

impl Job<'_> {
    /// About how long it takes, to start the longest first: its ticks, a bot's weighted by
    /// what a headless frame and the replay cost beside a bare tick.
    fn cost(&self) -> u64 {
        let ticks = |text: &str| {
            toml::from_str::<toml::Table>(text)
                .ok()
                .and_then(|t| t.get("ticks")?.as_integer())
                .map_or(0, |n| n.max(0) as u64)
        };
        match *self {
            Job::Scenario(_, text) => ticks(text),
            Job::Bot(_, text) => ticks(text).saturating_mul(BOT_TICK_COST),
        }
    }
}

/// A headless bot tick (the tick, its frame and the replay's tick) against a scenario's.
const BOT_TICK_COST: u64 = 10;

/// A scenario's report. A scenario that is itself wrong (or modules that don't start)
/// fails with that message, and the others still run.
fn scenario_report(all: &[Module], owner: &str, path: &str, text: &str) -> Value {
    match scenario::run_scenario(all, owner, path, text) {
        Ok(r) => json!({
            "file": path,
            "passed": r.passed,
            "ticks": r.ticks,
            "hash": r.hash,
            "failures": r.failures,
        }),
        Err(e) => json!({ "file": path, "passed": false, "failures": [e] }),
    }
}

/// How many tests run at once: half the machine's cores, leaving the rest for whatever
/// else is running (builds, other agents' tests, the game).
fn workers(jobs: usize) -> usize {
    std::thread::available_parallelism()
        .map_or(1, |n| (n.get() / 2).max(1))
        .min(jobs.max(1))
}

/// Runs `run` on each job, [`workers`] at a time, the costliest first so the last to
/// finish is not a long one started late; the results are in the jobs' order. Each job is
/// independent of the others (its own world, Luau states and child process), so the
/// results don't depend on the order or on how many run at once.
fn side_by_side<J: Sync, T: Send>(
    jobs: &[J],
    cost: impl Fn(&J) -> u64,
    run: impl Fn(&J) -> T + Sync,
) -> Vec<T> {
    let workers = workers(jobs.len());
    if workers <= 1 {
        return jobs.iter().map(run).collect();
    }
    let mut order: Vec<usize> = (0..jobs.len()).collect();
    order.sort_by_key(|&i| std::cmp::Reverse(cost(&jobs[i])));
    let next = std::sync::atomic::AtomicUsize::new(0);
    let results: std::sync::Mutex<Vec<Option<T>>> =
        std::sync::Mutex::new((0..jobs.len()).map(|_| None).collect());
    std::thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| {
                loop {
                    let k = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    let Some(&i) = order.get(k) else {
                        break;
                    };
                    let result = run(&jobs[i]);
                    results.lock().expect("a test thread panicked")[i] = Some(result);
                }
            });
        }
    });
    results
        .into_inner()
        .expect("a test thread panicked")
        .into_iter()
        .map(|r| r.expect("every job ran"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{only_filter, side_by_side};

    #[test]
    fn side_by_side_keeps_the_jobs_order_whatever_finishes_first() {
        let jobs: Vec<u64> = (0..24).collect();
        let started = std::sync::Mutex::new(Vec::new());
        // The cheap ones last in the queue finish first; the results stay in job order.
        let results = side_by_side(
            &jobs,
            |&j| j,
            |&j| {
                started.lock().unwrap().push(j);
                std::thread::sleep(std::time::Duration::from_millis(24 - j));
                j * 10
            },
        );
        assert_eq!(results, jobs.iter().map(|j| j * 10).collect::<Vec<_>>());
        let mut started = started.into_inner().unwrap();
        started.sort_unstable();
        assert_eq!(started, jobs, "each job ran once");
    }

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
