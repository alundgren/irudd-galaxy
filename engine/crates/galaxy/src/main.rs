//! `galaxy`: the game, which runs modules. Headless so far: `sim run`, `sim load` and
//! `sim replay` run modules without a window; `play` runs the engine version gate on its
//! modules, then says it is not built yet. Exit codes and error kinds:
//! `engine/docs/exit-codes.md`.

mod sim;

use std::path::Path;
use std::process::ExitCode;

use galaxy_module::cli::{self, Output};
use galaxy_module::discover::{self, Tool};
use galaxy_module::{Failure, gate};

const USAGE: &str = "usage: galaxy <command>

  doctor                      the engine, its installed builds and launchers, and the
                              modules near the working folder
  play --modules <dir>        play the modules in <dir>, or the module <dir> is (not built
                              yet: runs the engine version gate)
  sim run --modules <dir>     run a new world from the modules' start states, headless
  sim load <save> --modules <dir>
                              load a save and carry on with the modules
  sim replay <recording>      run a recording again and compare the world it ends with
  --version                   print the version
  --help                      print this

sim run and sim load take:
  --seed <n>        the world's seed (run only; 1 by default)
  --ticks <n>       ticks to run (30 a second; run: 300 by default, load: 0)
  --inputs <file>   inputs, notes and hot-loads by tick (engine/docs/saves-and-replay.md)
  --save <file>     write a save when done
  --record <file>   write a recording of the run
  --realtime        run at 30 ticks a second instead of as fast as possible
  --watch           hot-load module folders when their files change (implies --realtime)
  --state           print the whole world when done
sim replay takes --state too.";

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
            let manifests = gate::open(&module_dirs(dir)?)?;
            Err(cli::not_built(Tool::Game, "play", &manifests))
        }
        [command, rest @ ..] if command == "sim" => sim::run(rest),
        [command, ..] => Err(cli::usage_failure(
            format!("unknown command `{command}`"),
            USAGE,
        )),
        [] => unreachable!("cli::main handles an empty command line"),
    }
}

/// The module folders `--modules <dir>` names: `<dir>` if it is a module, else every
/// module in it.
fn module_dirs(dir: &str) -> Result<Vec<std::path::PathBuf>, Failure> {
    let dir = discover::resolve(&cli::current_dir()?, Path::new(dir));
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
    Ok(modules)
}
