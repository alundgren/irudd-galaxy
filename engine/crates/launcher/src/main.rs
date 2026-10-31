//! The launcher: a small executable that `cargo xtask install` copies into the launcher
//! folder twice, as `galaxy` and `galaxy-gm`. It reads its own name to know which tool it
//! stands for, picks an installed build and runs that build's executable with the same
//! arguments, exit code included.
//!
//! Which build:
//! - `--build <name>` before the command: exactly that build folder (the only way to run a
//!   `--dev` build). Recordings will name their build the same way.
//! - Otherwise the newest non-dev build that is at least the highest minimum engine version
//!   of the modules the arguments name (`check <path>`, `play --modules <dir>`, `doctor`:
//!   the working folder). If none is new enough, the newest build, whose version gate
//!   refuses with exit 2 and kind `engine_version`.
//!
//! See `engine/docs/modules-and-install.md`.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use galaxy_module::discover::{self, Tool};
use galaxy_module::install::{self, Want};
use galaxy_module::{Failure, manifest};

fn main() -> ExitCode {
    match launch() {
        Ok(code) => code,
        Err(failure) => failure.report(),
    }
}

fn launch() -> Result<ExitCode, Failure> {
    let exe = std::env::current_exe()
        .map_err(|e| Failure::runtime(format!("can't find the launcher's own path: {e}")))?;
    let stem = exe.file_stem().and_then(|s| s.to_str()).unwrap_or_default();
    let tool = Tool::from_name(stem).ok_or_else(|| {
        Failure::usage(format!(
            "the launcher must be named galaxy or galaxy-gm, not `{stem}`; install it with \
             `cargo xtask install` in engine/"
        ))
    })?;
    let root = root_of(&exe);
    let (named, args) = split_build_arg(std::env::args_os().skip(1).collect())?;
    let want = match named {
        Some(name) => Want::Named(name),
        None => {
            let cwd = std::env::current_dir().unwrap_or_default();
            let minimum = discover::modules_for(tool, &args, &cwd)
                .iter()
                .filter_map(|dir| manifest::read_minimum(dir).ok())
                .map(|m| m.min_engine_version)
                .max();
            Want::Newest(minimum)
        }
    };
    let builds = install::list(&root);
    let build = install::select(&builds, &want, &root)?;
    let target = build.dir.join(install::exe(tool.name()));
    run(&target, &args)
}

/// The install root: the folder above `bin/` when the launcher sits in an install, else
/// the configured one.
fn root_of(exe: &Path) -> PathBuf {
    exe.parent()
        .filter(|dir| dir.file_name().is_some_and(|n| n == "bin"))
        .and_then(Path::parent)
        .filter(|root| install::builds_dir(root).is_dir())
        .map_or_else(install::root, Path::to_path_buf)
}

/// Takes a leading `--build <name>` or `--build=<name>` off the arguments.
fn split_build_arg(mut args: Vec<OsString>) -> Result<(Option<String>, Vec<OsString>), Failure> {
    let first = args.first().and_then(|a| a.to_str()).map(str::to_owned);
    let name = match first.as_deref() {
        Some("--build") => {
            if args.len() < 2 {
                return Err(Failure::usage("--build needs a build name: --build <name>"));
            }
            let name = args.remove(1);
            args.remove(0);
            Some(name.to_string_lossy().into_owned())
        }
        Some(arg) => match arg.strip_prefix("--build=") {
            Some(name) => {
                let name = name.to_owned();
                args.remove(0);
                Some(name)
            }
            None => None,
        },
        None => None,
    };
    Ok((name, args))
}

#[cfg(unix)]
fn run(target: &Path, args: &[OsString]) -> Result<ExitCode, Failure> {
    use std::os::unix::process::CommandExt as _;
    // Replaces this process, so signals and the exit code are the build's own.
    let error = Command::new(target).args(args).exec();
    Err(start_failure(target, error))
}

#[cfg(not(unix))]
fn run(target: &Path, args: &[OsString]) -> Result<ExitCode, Failure> {
    let status = Command::new(target)
        .args(args)
        .status()
        .map_err(|e| start_failure(target, e))?;
    let code = status.code().unwrap_or(1);
    Ok(ExitCode::from(u8::try_from(code).unwrap_or(1)))
}

fn start_failure(target: &Path, error: std::io::Error) -> Failure {
    Failure::runtime(format!("can't start {}: {error}", target.display()))
        .with("path", target.display().to_string())
}

#[cfg(test)]
mod tests {
    use super::split_build_arg;
    use std::ffi::OsString;

    fn split(list: &[&str]) -> (Option<String>, Vec<String>) {
        let args = list.iter().map(OsString::from).collect();
        let (name, rest) = split_build_arg(args).unwrap();
        (
            name,
            rest.into_iter().map(|a| a.into_string().unwrap()).collect(),
        )
    }

    #[test]
    fn a_leading_build_choice_is_taken_off() {
        assert_eq!(
            split(&["--build", "0.1.0-abc-dev", "doctor"]),
            (Some("0.1.0-abc-dev".into()), vec!["doctor".to_string()])
        );
        assert_eq!(
            split(&["--build=0.1.0-abc", "check", "x"]),
            (Some("0.1.0-abc".into()), vec!["check".into(), "x".into()])
        );
        assert_eq!(
            split(&["check", "--build", "x"]),
            (None, vec!["check".into(), "--build".into(), "x".into()])
        );
        assert!(split_build_arg(vec!["--build".into()]).is_err());
    }
}
