//! `cargo xtask <task>`: local checks and code generation. There is no CI; run these locally.

use std::process::{Command, ExitCode};

fn main() -> ExitCode {
    let task = std::env::args().nth(1).unwrap_or_default();
    match task.as_str() {
        "check" => run_all(&[
            &["fmt", "--all", "--check"],
            &[
                "clippy",
                "--workspace",
                "--all-targets",
                "--",
                "-D",
                "warnings",
            ],
            &["test", "--workspace"],
        ]),
        _ => {
            eprintln!("usage: cargo xtask <check>");
            ExitCode::FAILURE
        }
    }
}

fn run_all(steps: &[&[&str]]) -> ExitCode {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    for args in steps {
        eprintln!("==> cargo {}", args.join(" "));
        let ok = Command::new(&cargo)
            .args(*args)
            .status()
            .is_ok_and(|s| s.success());
        if !ok {
            return ExitCode::FAILURE;
        }
    }
    ExitCode::SUCCESS
}
