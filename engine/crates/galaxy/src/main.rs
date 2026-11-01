//! `galaxy`: the game, which runs modules. `play` runs them in a window, `shot` draws one
//! frame headless, `film` draws every frame headless, and `sim run`, `sim load` and `sim replay` run them without drawing.
//! Exit codes and error kinds: `engine/docs/exit-codes.md`.

mod film;
mod notes;
mod play;
mod sim;

use std::path::Path;
use std::process::ExitCode;

use galaxy_module::Failure;
use galaxy_module::cli::{self, Output};
use galaxy_module::discover::{self, Tool};

const USAGE: &str = "usage: galaxy <command>

  doctor                      the engine, its installed builds and launchers, and the
                              modules near the working folder
  play --modules <dir>        play the modules in <dir>, or the module <dir> is, in a
                              window. Esc quits, the wheel zooms, F8 takes a note.
                              Each session records itself into a session folder.
  shot --modules <dir> --out <file.png>
                              run the modules headless and draw one frame
  film --modules <dir> --out <folder>
                              run the modules headless and draw every frame on a fixed
                              clock, with probes of where things are on screen, for
                              galaxy-gm motion
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
sim replay takes --state too.

play takes --seed, --save and --record as above, and:
  --watch           hot-load module folders when their files change, models included
  --size <w>x<h>    the window's size in logical pixels (1280x720 by default)
  --frames <n>      quit after this many frames
  --inputs <file>   play these inputs on their ticks through the window loop (the bot)
  --ticks <n>       quit once the world reaches tick n
It prints frame times when it quits.

shot takes --seed, --ticks (30 by default) and --inputs as above, and --size (in pixels,
1280x720 by default).

film takes --seed and --inputs as above, and:
  --from <tick>     the first tick filmed (0 by default)
  --to <tick>       the last (90 ticks, three seconds, after --from by default)
  --fps <n>         frames a second (60 by default)
  --size <w>x<h>    in pixels (960x540 by default)
  --probes-only     no pictures and no GPU: where things are on screen only
Inputs files may turn the wheel: [[zoom]] tick = <n>, steps = <wheel steps>.";

fn main() -> ExitCode {
    cli::main(Tool::Game, USAGE, run)
}

fn run(args: &[String]) -> Result<Output, Failure> {
    match args {
        [command, rest @ ..] if command == "play" => play::play(rest),
        [command, rest @ ..] if command == "shot" => play::shot(rest),
        [command, rest @ ..] if command == "film" => film::film(rest),
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
