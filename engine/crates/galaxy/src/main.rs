//! `galaxy`: the game, which runs modules. A stub so far: `doctor` works, and `play`
//! runs the engine version gate on its modules, then says it is not built yet.
//! Exit codes and error kinds: `engine/docs/exit-codes.md`.

use std::path::Path;
use std::process::ExitCode;

use galaxy_module::cli::{self, Output};
use galaxy_module::discover::{self, Tool};
use galaxy_module::{Failure, gate};

const USAGE: &str = "usage: galaxy <command>

  doctor                  the engine, its installed builds and launchers, and the
                          modules near the working folder
  play --modules <dir>    play the modules in <dir>, or the module <dir> is (not built
                          yet: runs the engine version gate)
  --version               print the version
  --help                  print this";

fn main() -> ExitCode {
    cli::main(Tool::Game, USAGE, run)
}

fn run(args: &[String]) -> Result<Output, Failure> {
    match args {
        [command, rest @ ..] if command == "play" => {
            let dir = match rest {
                [flag, dir] if flag == "--modules" => Some(dir.as_str()),
                [flag] => flag.strip_prefix("--modules="),
                _ => None,
            };
            let Some(dir) = dir.filter(|d| !d.is_empty()) else {
                return Err(cli::usage_failure(
                    "play needs one modules folder: galaxy play --modules <dir>",
                    USAGE,
                ));
            };
            let dir = cli::current_dir()?.join(Path::new(&dir));
            if !dir.is_dir() {
                return Err(
                    Failure::bad_input(format!("{} is not a folder", dir.display()))
                        .with("path", dir.display().to_string()),
                );
            }
            let modules = discover::modules_in(&dir);
            if modules.is_empty() {
                return Err(Failure::bad_input(format!(
                    "no modules in {}: a module is a folder with a manifest.toml",
                    dir.display()
                ))
                .with("path", dir.display().to_string()));
            }
            let manifests = gate::open(&modules)?;
            Err(cli::not_built(Tool::Game, "play", &manifests))
        }
        [command, ..] => Err(cli::usage_failure(
            format!("unknown command `{command}`"),
            USAGE,
        )),
        [] => unreachable!("cli::main handles an empty command line"),
    }
}
