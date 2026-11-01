//! `galaxy`: the game, which runs modules. `play` runs them in a window, `shot` draws one
//! frame headless, `film` draws every frame headless, and `sim run`, `sim load` and `sim replay` run them without drawing.
//! Each command's help is in `galaxy_module::help`, which dispatch goes through.
//! Exit codes and error kinds: `engine/docs/exit-codes.md`.

mod film;
mod notes;
mod play;
mod sim;

use std::path::Path;
use std::process::ExitCode;
use std::sync::OnceLock;

use galaxy_module::Failure;
use galaxy_module::cli::{self, Output};
use galaxy_module::discover::{self, Tool};
use galaxy_module::help;

/// The command being run, for usage errors.
static CURRENT: OnceLock<&'static help::Command> = OnceLock::new();

fn main() -> ExitCode {
    cli::main(Tool::Game, run)
}

fn run(command: &'static help::Command, args: &[String]) -> Result<Output, Failure> {
    let _ = CURRENT.set(command);
    match command.path {
        "play" => play::play(args),
        "shot" => play::shot(args),
        "film" => film::film(args),
        "sim run" => sim::run(args),
        "sim load" => sim::load(args),
        "sim replay" => sim::replay(args),
        other => unreachable!("galaxy has help for `{other}` but does not run it"),
    }
}

/// Exit 2, kind `usage`, for the command being run.
pub(crate) fn usage(message: impl Into<String>) -> Failure {
    match CURRENT.get() {
        Some(c) => cli::command_usage(c, message),
        None => Failure::usage(message),
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Every option a command's parser takes is in its help, and the other way round.
    #[test]
    fn help_lists_the_options_each_command_takes() {
        let load: Vec<&str> = sim::RUN_OPTIONS
            .iter()
            .copied()
            .filter(|o| *o != "--seed")
            .collect();
        let table: [(&str, &[&str]); 6] = [
            ("play", play::PLAY_OPTIONS),
            ("shot", play::SHOT_OPTIONS),
            ("film", film::OPTIONS),
            ("sim run", sim::RUN_OPTIONS),
            ("sim load", &load),
            ("sim replay", sim::REPLAY_OPTIONS),
        ];
        for (path, takes) in table {
            let c = help::find(Tool::Game, path).expect("help");
            let mut documented = c.options();
            documented.sort_unstable();
            let mut takes: Vec<&str> = takes.iter().map(|o| &o[2..]).collect();
            takes.sort_unstable();
            assert_eq!(documented, takes, "galaxy {path}");
        }
        let paths: Vec<&str> = help::of(Tool::Game).map(|c| c.path).collect();
        assert_eq!(
            paths,
            [
                "play",
                "shot",
                "film",
                "sim run",
                "sim load",
                "sim replay",
                "doctor"
            ]
        );
    }
}
