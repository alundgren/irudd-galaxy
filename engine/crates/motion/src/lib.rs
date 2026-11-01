//! Films and the tools that read them.
//!
//! A film ([`film`]) is a session drawn frame by frame on a fixed clock, with probes of
//! where everything was on screen. Each tool answers one question about how motion looks
//! with a few numbers, the timestamps of suspicious moments and at most one small
//! picture, so an agent can check its work without reading dozens of frames. Everything
//! here runs on the CPU. How it works, and which tool answers which question:
//! `engine/docs/motion-tools.md`.

pub mod check;
pub mod compare;
pub mod dots;
pub mod film;
pub mod flicker;
pub mod flip;
pub mod flow;
pub mod image;
pub mod opts;
mod plot;
pub mod sheet;
pub mod strip;
pub mod tone;
pub mod track;
pub mod vdp;
pub mod warp;

use std::path::Path;

use serde_json::Value;

pub use film::Film;
pub use opts::Opts;

/// The tools, in the order an agent usually reaches for them.
pub const TOOLS: &[&str] = &[
    "track", "dots", "strip", "sheet", "flicker", "tone", "flow", "compare",
];

/// The options `tool` takes, besides `--image`; none for a tool that doesn't exist.
pub fn options(tool: &str) -> Option<&'static [&'static str]> {
    Some(match tool {
        "track" => track::OPTIONS,
        "dots" => dots::OPTIONS,
        "strip" => strip::OPTIONS,
        "sheet" => sheet::OPTIONS,
        "flicker" => flicker::OPTIONS,
        "tone" => tone::OPTIONS,
        "flow" => flow::OPTIONS,
        "compare" => compare::OPTIONS,
        _ => return None,
    })
}

/// Runs `tool` on the film in `dir`.
pub fn run(tool: &str, dir: &Path, opts: &Opts) -> Result<Value, String> {
    let film = Film::read(dir)?;
    match tool {
        "track" => track::run(&film, opts),
        "dots" => dots::run(&film, opts),
        "strip" => strip::run(&film, opts),
        "sheet" => sheet::run(&film, opts),
        "flicker" => flicker::run(&film, opts),
        "tone" => tone::run(&film, opts),
        "flow" => flow::run(&film, opts),
        "compare" => compare::run(&film, opts),
        other => Err(format!(
            "no motion tool `{other}`; the tools are {}",
            TOOLS.join(", ")
        )),
    }
}
