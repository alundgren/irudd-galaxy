//! `galaxy-gm`: the GM, which builds and checks modules. A stub so far: `doctor` works,
//! and `check` runs the engine version gate, then says it is not built yet.
//! Exit codes and error kinds: `engine/docs/exit-codes.md`.

use std::path::Path;
use std::process::ExitCode;

use galaxy_module::cli::{self, Output};
use galaxy_module::discover::{self, Tool};
use galaxy_module::{Failure, gate, manifest};

const USAGE: &str = "usage: galaxy-gm <command>

  doctor            the engine, its installed builds and launchers, and the modules
                    near the working folder
  check <module>    check a module folder (not built yet: runs the engine version gate)
  --version         print the version
  --help            print this";

fn main() -> ExitCode {
    cli::main(Tool::Gm, USAGE, run)
}

fn run(args: &[String]) -> Result<Output, Failure> {
    match args {
        [command, path] if command == "check" => {
            let cwd = cli::current_dir()?;
            let dir = manifest::module_dir(&discover::resolve(&cwd, Path::new(path)));
            let manifests = gate::open(&[dir])?;
            Err(cli::not_built(Tool::Gm, "check", &manifests))
        }
        [command, ..] if command == "check" => Err(cli::usage_failure(
            "check takes one module folder: galaxy-gm check <module>",
            USAGE,
        )),
        [command, ..] => Err(cli::usage_failure(
            format!("unknown command `{command}`"),
            USAGE,
        )),
        [] => unreachable!("cli::main handles an empty command line"),
    }
}
