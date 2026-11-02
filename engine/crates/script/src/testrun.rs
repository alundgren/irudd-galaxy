//! A module's own Luau tests: `tests/*.luau` (any depth under `tests/`). Carried over from
//! the slice's `galaxy pack test`.
//!
//! A test file returns a table of named functions; a test passes if its function returns
//! without an error (`assert(cond, "message")` fails it):
//!
//! ```lua
//! local rules = require("../scripts/rules")
//! return {
//!     deck_needs_height = function()
//!         assert(not rules.reached_deck({ layer = 0, height = 1 }), "1 is below the deck")
//!     end,
//! }
//! ```
//!
//! Tests run in the same sandbox and budgets as the module's scripts, and `require` sees
//! the same files. There is no `ctx`: these test pure Luau (rules, tables, maths); what
//! a script does to the world is a scenario test's job. Each test runs in a fresh state,
//! so tests share nothing, and they run in name order.

use std::rc::Rc;

use galaxy_module::module::Module;
use mlua::{Function, Value};

use crate::vm::{Abort, Code, LIMITS, Location, Vm};

/// How one test went.
#[derive(Clone, Debug, PartialEq)]
pub struct LuauTest {
    /// The test file, `tests/rules.luau`.
    pub file: String,
    /// The test's name, or `(load)` when the file itself failed to load.
    pub name: String,
    pub passed: bool,
    /// Why it failed, with the file and line: `tests/rules.luau:7: 1 is below the deck`.
    pub failure: Option<String>,
}

/// The module's Luau test files, in path order.
pub fn test_files(module: &Module) -> Vec<&str> {
    module
        .files
        .0
        .keys()
        .filter(|p| p.starts_with("tests/") && p.ends_with(".luau"))
        .map(String::as_str)
        .collect()
}

/// Runs every test in the module's `tests/*.luau`, in file then name order.
pub fn run_luau_tests(module: &Module) -> Vec<LuauTest> {
    let files = Rc::new(module.files.clone());
    let code = Code::default();
    let mut out = Vec::new();
    for file in test_files(module) {
        let result = |name: &str, failure: Option<String>| LuauTest {
            file: file.to_owned(),
            name: name.to_owned(),
            passed: failure.is_none(),
            failure,
        };
        let names = match test_names(&files, &code, file) {
            Ok(names) => names,
            Err(e) => {
                out.push(result("(load)", Some(e)));
                continue;
            }
        };
        if names.is_empty() {
            out.push(result(
                "(load)",
                Some(format!(
                    "{file}: returns no tests; a test file returns {{ name = function() ... end }}"
                )),
            ));
        }
        for name in names {
            out.push(result(&name, run_one(&files, &code, file, &name).err()));
        }
    }
    out
}

fn fresh(files: &Rc<galaxy_module::module::ModuleFiles>, code: &Code) -> Result<Vm, String> {
    Vm::new(files.clone(), code, LIMITS).map_err(|e| format!("can't make a Luau state: {e}"))
}

/// Loads `file` and returns the names of its test functions, sorted.
fn test_names(
    files: &Rc<galaxy_module::module::ModuleFiles>,
    code: &Code,
    file: &str,
) -> Result<Vec<String>, String> {
    let vm = fresh(files, code)?;
    let table = vm
        .budgeted(|| vm.run_file(file))
        .map_err(|a| located(&a, file))?;
    let Value::Table(table) = table else {
        return Err(format!(
            "{file}: a test file returns a table of test functions: \
             return {{ name = function() ... end }}"
        ));
    };
    let mut names = Vec::new();
    let mut problems = Vec::new();
    for pair in table.pairs::<Value, Value>() {
        let (key, value) = pair.map_err(|e| format!("{file}: {e}"))?;
        let name = match &key {
            Value::String(s) => s.to_string_lossy(),
            _ => {
                problems.push("a test's key is its name, a string".to_owned());
                continue;
            }
        };
        match value {
            Value::Function(_) => names.push(name),
            _ => problems.push(format!("`{name}` is not a function")),
        }
    }
    if !problems.is_empty() {
        return Err(format!("{file}: {}", problems.join("; ")));
    }
    names.sort();
    Ok(names)
}

/// Runs one test in a fresh state.
fn run_one(
    files: &Rc<galaxy_module::module::ModuleFiles>,
    code: &Code,
    file: &str,
    name: &str,
) -> Result<(), String> {
    let vm = fresh(files, code)?;
    vm.budgeted(|| {
        let Value::Table(table) = vm.run_file(file)? else {
            return Err(mlua::Error::runtime(
                "the test file no longer returns a table",
            ));
        };
        let f: Function = table.get(name)?;
        f.call::<()>(())
    })
    .map_err(|a| located(&a, file))
}

/// An abort as one line, with its place; a bare message gets the test file.
fn located(a: &Abort, file: &str) -> String {
    let at = a.location.clone().unwrap_or(Location {
        file: file.to_owned(),
        line: None,
    });
    let message = a.message.trim();
    let code = match a.code.as_str() {
        "runtime_error" => String::new(),
        other => format!(" ({other})"),
    };
    format!("{at}: {message}{code}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn module(files: &[(&str, &str)]) -> Module {
        let mut all = vec![(
            "manifest.toml",
            "id = \"t\"\nversion = \"0.1.0\"\nmin_engine_version = \"0.1.0\"\n",
        )];
        all.extend_from_slice(files);
        let files = galaxy_module::module::ModuleFiles(
            all.iter()
                .map(|(p, t)| ((*p).to_owned(), (*t).to_owned()))
                .collect(),
        );
        Module::from_files(std::path::Path::new("t"), files).unwrap()
    }

    #[test]
    fn tests_pass_fail_and_say_where() {
        let m = module(&[
            (
                "scripts/lib.luau",
                "return { double = function(x) return x * 2 end }",
            ),
            (
                "tests/lib.luau",
                "local lib = require(\"../scripts/lib\")\nreturn {\n  b_doubles = function() assert(lib.double(2) == 4, 'two') end,\n  a_wrong = function()\n    assert(lib.double(2) == 5, 'double(2) is not 5')\n  end,\n}\n",
            ),
        ]);
        let r = run_luau_tests(&m);
        assert_eq!(r.len(), 2, "{r:?}");
        assert_eq!((r[0].name.as_str(), r[0].passed), ("a_wrong", false));
        let failure = r[0].failure.as_deref().unwrap();
        assert!(
            failure.contains("tests/lib.luau:5") && failure.contains("double(2) is not 5"),
            "{failure}"
        );
        assert_eq!((r[1].name.as_str(), r[1].passed), ("b_doubles", true));
    }

    #[test]
    fn endless_loops_and_bad_files_fail_their_test() {
        let m = module(&[
            (
                "tests/a.luau",
                "return { spin = function() while true do end end }",
            ),
            ("tests/b.luau", "return 3"),
            ("tests/c.luau", "return {"),
        ]);
        let r = run_luau_tests(&m);
        assert_eq!(r.len(), 3, "{r:?}");
        assert!(
            r[0].failure.as_deref().unwrap().contains("over_budget"),
            "{r:?}"
        );
        assert_eq!(r[1].name, "(load)");
        assert!(r[1].failure.as_deref().unwrap().contains("returns a table"));
        assert_eq!(r[2].name, "(load)");
        assert!(r.iter().all(|t| !t.passed));
    }
}
