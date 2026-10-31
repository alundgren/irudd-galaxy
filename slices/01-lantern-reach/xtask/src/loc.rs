//! `cargo xtask loc [--base <git ref>]`: Rust code lines per crate, split into `src`,
//! `unit_tests` (`#[cfg(test)]` items in `src`) and `tests` (the `tests/` folder). A code
//! line is a non-blank line that isn't a `//` comment. With `--base`, the same count at
//! that ref and the change since. JSON on stdout, a table on stderr.

use std::collections::BTreeMap;
use std::process::{Command, ExitCode};

use serde_json::{Value, json};

const RULE: &str = "non-blank lines that aren't // comments; unit_tests: #[cfg(test)] items \
                    outside tests/; tests: the tests/ folder";
const KINDS: [&str; 3] = ["src", "unit_tests", "tests"];

/// Lines per crate, in the order of [`KINDS`].
type Counts = BTreeMap<String, [i64; 3]>;

pub fn run(args: &[std::ffi::OsString]) -> ExitCode {
    let base = match args {
        [] => None,
        [flag, r] if flag == "--base" => Some(r.to_string_lossy().into_owned()),
        _ => {
            eprintln!("usage: cargo xtask loc [--base <git ref>]");
            return ExitCode::FAILURE;
        }
    };
    let counted = count(None).and_then(|now| match &base {
        Some(r) => count(Some(r)).map(|then| (now, Some(then))),
        None => Ok((now, None)),
    });
    let (now, then) = match counted {
        Ok(c) => c,
        Err(e) => {
            eprintln!("xtask loc: {e}");
            return ExitCode::FAILURE;
        }
    };
    let mut report = json!({ "rule": RULE, "crates": to_json(&now) });
    eprintln!(
        "{:<10} {:>7} {:>10} {:>7} {:>7} {:>7}",
        "crate", KINDS[0], KINDS[1], KINDS[2], "total", "change"
    );
    let total = ("TOTAL".to_string(), now.values().copied().fold([0; 3], add));
    for (name, c) in now.clone().into_iter().chain([total]) {
        let before = then.as_ref().map(|t| match name.as_str() {
            "TOTAL" => t.values().copied().fold([0; 3], add),
            _ => t.get(&name).copied().unwrap_or_default(),
        });
        let change = before.map_or(String::new(), |b| {
            format!("{:+}", c.iter().sum::<i64>() - b.iter().sum::<i64>())
        });
        eprintln!(
            "{name:<10} {:>7} {:>10} {:>7} {:>7} {change:>7}",
            c[0],
            c[1],
            c[2],
            c.iter().sum::<i64>()
        );
    }
    if let (Some(r), Some(then)) = (base, then) {
        let mut delta = now.clone();
        for (name, b) in &then {
            let d = delta.entry(name.clone()).or_default();
            *d = add(*d, b.map(|v| -v));
        }
        report["base"] = json!({ "ref": r, "crates": to_json(&then) });
        report["delta"] = json!({ "crates": to_json(&delta) });
    }
    println!(
        "{}",
        serde_json::to_string_pretty(&report).expect("serialises")
    );
    ExitCode::SUCCESS
}

fn add(a: [i64; 3], b: [i64; 3]) -> [i64; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

/// Per crate and a `TOTAL`, each with its total.
fn to_json(counts: &Counts) -> Value {
    let total = (
        "TOTAL".to_string(),
        counts.values().copied().fold([0; 3], add),
    );
    let rows = counts.iter().map(|(n, c)| (n.clone(), *c)).chain([total]);
    rows.map(|(name, c)| {
        let mut row: serde_json::Map<String, Value> = KINDS
            .iter()
            .map(|k| k.to_string())
            .zip(c.map(Value::from))
            .collect();
        row.insert("total".into(), c.iter().sum::<i64>().into());
        (name, Value::Object(row))
    })
    .collect::<serde_json::Map<_, _>>()
    .into()
}

/// Counts the Rust files under `crates/` and `xtask/`: the working tree's (tracked and
/// new, not ignored), or those at a git ref.
fn count(at: Option<&str>) -> Result<Counts, String> {
    let git = |args: &[&str]| -> Result<String, String> {
        let out = Command::new("git")
            .args(args)
            .current_dir(crate::workspace_root())
            .output()
            .map_err(|e| format!("can't run git: {e}"))?;
        if !out.status.success() {
            let err = String::from_utf8_lossy(&out.stderr);
            return Err(format!("git {}: {}", args.join(" "), err.trim()));
        }
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    };
    let dirs = ["--", "crates", "xtask"];
    let listing = match at {
        None => git(&[
            &[
                "ls-files",
                "-z",
                "--cached",
                "--others",
                "--exclude-standard",
            ],
            &dirs[..],
        ]
        .concat())?,
        Some(r) => git(&[&["ls-tree", "-r", "-z", "--name-only", r], &dirs[..]].concat())?,
    };
    let mut counts = Counts::new();
    for path in listing.split('\0').filter(|p| p.ends_with(".rs")) {
        let parts: Vec<&str> = path.split('/').collect();
        let (name, rest) = match parts.as_slice() {
            ["crates", name, rest @ ..] => (*name, rest),
            ["xtask", rest @ ..] => ("xtask", rest),
            _ => continue,
        };
        let text = match at {
            // A tracked file deleted in the working tree is skipped.
            None => match std::fs::read_to_string(crate::workspace_root().join(path)) {
                Ok(text) => text,
                Err(_) => continue,
            },
            Some(r) => git(&["show", &format!("{r}:./{path}")])?,
        };
        let (code, test) = split(&text);
        let c = counts.entry(name.to_string()).or_default();
        *c = add(
            *c,
            if rest.first() == Some(&"tests") {
                [0, 0, code + test]
            } else {
                [code, test, 0]
            },
        );
    }
    Ok(counts)
}

/// Code lines of a source file as `(outside tests, inside #[cfg(test)] items)`.
fn split(text: &str) -> (i64, i64) {
    let mut scan = Scanner::default();
    let (mut code, mut test) = (0, 0);
    // The depth a `#[cfg(test)]` item started at, and whether it has opened a block.
    let mut item: Option<(i32, bool)> = None;
    for line in text.lines() {
        let trimmed = line.trim();
        let start = scan.depth;
        if item.is_none() && trimmed.starts_with("#[cfg(test)]") {
            item = Some((start, false));
        }
        let max = scan.line(line);
        let is_code = !trimmed.is_empty() && !trimmed.starts_with("//");
        match &mut item {
            Some((d0, opened)) => {
                test += i64::from(is_code);
                *opened |= max > *d0;
                let ended = if *opened {
                    scan.depth <= *d0
                } else {
                    !trimmed.starts_with("#[") && trimmed.ends_with(';')
                };
                if ended {
                    item = None;
                }
            }
            None => code += i64::from(is_code),
        }
    }
    (code, test)
}

/// Tracks `{}` depth across lines, skipping comments, strings and char literals.
#[derive(Default)]
struct Scanner {
    depth: i32,
    block_comment: u32,
    /// Inside a string: `Some(0)` for `"..."`, `Some(n)` for a raw string with `n` hashes
    /// plus one (so `r"..."` is `Some(1)`).
    string: Option<usize>,
}

impl Scanner {
    /// Scans one line and returns the deepest depth it reached.
    fn line(&mut self, line: &str) -> i32 {
        let c: Vec<char> = line.chars().collect();
        let mut max = self.depth;
        let mut i = 0;
        while i < c.len() {
            let next = c.get(i + 1).copied();
            if self.block_comment > 0 {
                if c[i] == '*' && next == Some('/') {
                    self.block_comment -= 1;
                    i += 1;
                } else if c[i] == '/' && next == Some('*') {
                    self.block_comment += 1;
                    i += 1;
                }
            } else if let Some(raw) = self.string {
                if raw == 0 && c[i] == '\\' {
                    i += 1;
                } else if c[i] == '"'
                    && (raw == 0 || c[i + 1..].iter().take_while(|&&h| h == '#').count() >= raw - 1)
                {
                    self.string = None;
                    i += raw.saturating_sub(1);
                }
            } else {
                match c[i] {
                    '/' if next == Some('/') => break,
                    '/' if next == Some('*') => {
                        self.block_comment = 1;
                        i += 1;
                    }
                    '"' => self.string = Some(0),
                    'r' if !c[..i]
                        .last()
                        .is_some_and(|p| p.is_alphanumeric() || *p == '_') =>
                    {
                        let hashes = c[i + 1..].iter().take_while(|&&h| h == '#').count();
                        if c.get(i + 1 + hashes) == Some(&'"') {
                            self.string = Some(hashes + 1);
                            i += 1 + hashes;
                        }
                    }
                    '\'' if next == Some('\\') => {
                        // An escaped char literal: skip to its closing quote.
                        i += 2 + c[i + 2..].iter().position(|&q| q == '\'').unwrap_or(0);
                    }
                    '\'' if c.get(i + 2) == Some(&'\'') => i += 2,
                    '{' => {
                        self.depth += 1;
                        max = max.max(self.depth);
                    }
                    '}' => self.depth -= 1,
                    _ => {}
                }
            }
            i += 1;
        }
        max
    }
}

#[cfg(test)]
mod tests {
    use super::split;

    #[test]
    fn cfg_test_items_count_as_unit_tests() {
        let text = r##"// a comment
fn a() {
    let s = "{ not a brace";
    let r = r#"} "nor this" {"#;
    let c = '{';
}

#[cfg(test)]
use std::fmt;

#[cfg(test)]
mod tests {
    #[test]
    fn t() { let x = '}'; }
}
fn b<'a>(x: &'a str) {}
"##;
        // Code: fn a (5 lines) and fn b. Tests: the use (2 lines) and the module (5).
        assert_eq!(split(text), (6, 7));
    }
}
