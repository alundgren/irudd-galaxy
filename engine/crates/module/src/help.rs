//! The help both binaries print, and the GM's guide: one table of commands, written for an
//! agent that has only the installed binaries.
//!
//! Each [`Command`] says what it answers, its arguments with units and defaults, what each
//! field of its output means, where it writes files, one runnable example and the usual
//! next step. `galaxy --help` and `galaxy-gm --help` print a short overview from the table,
//! `<command> --help` (or `help <command>`) one entry, and `galaxy-gm guide [topic]` walks
//! from "what do you want to do" to the command. Both binaries dispatch through the table
//! ([`resolve`]), so a command without help can't be reached, and the tests check every
//! option a command takes is listed. Help never points at the repository's docs: a GM may
//! run where there is no repository.

use std::fmt::Write as _;

use crate::discover::Tool;

mod gm;
mod motion;

/// One command (or subcommand, or motion tool).
pub struct Command {
    pub tool: Tool,
    /// The words that name it after the binary: `sim run`, `motion flicker`.
    pub path: &'static str,
    /// What follows the path in a synopsis: `<recording> [options]`.
    pub synopsis: &'static str,
    /// One line on what it is for, as the question it answers.
    pub answers: &'static str,
    /// A few lines more, when it needs them; empty if not.
    pub about: &'static str,
    /// Arguments and options: the form (`--seed <n>`, `<film>`) and its meaning, with
    /// units and the default.
    pub args: &'static [(&'static str, &'static str)],
    /// The fields of what it prints, dotted for nested ones, and what each means.
    pub output: &'static [(&'static str, &'static str)],
    /// Where it writes files; empty when it writes none.
    pub writes: &'static str,
    /// One command line that runs.
    pub example: &'static str,
    /// What to do with the result.
    pub next: &'static str,
    /// The guide topic it belongs to.
    pub topic: &'static str,
}

impl Command {
    /// The options it takes, without the leading `--`: `seed` for `--seed <n>`.
    pub fn options(&self) -> Vec<&'static str> {
        let mut out: Vec<&'static str> = Vec::new();
        let words = self
            .args
            .iter()
            .flat_map(|(form, _)| form.split([' ', ',', '|', '[', ']']))
            .filter_map(|w| w.strip_prefix("--"))
            .filter(|w| !w.is_empty());
        for w in words {
            if !out.contains(&w) {
                out.push(w);
            }
        }
        out
    }

    /// The first line of its help: `galaxy sim run --modules <dir> [options]`.
    pub fn usage(&self) -> String {
        format!("{} {} {}", self.tool.name(), self.path, self.synopsis)
            .trim_end()
            .to_owned()
    }

    /// Its whole help.
    pub fn render(&self) -> String {
        let mut s = format!("{}\n\n{}\n", self.usage(), self.answers);
        if !self.about.is_empty() {
            let _ = write!(s, "\n{}\n", self.about.trim_end());
        }
        if !self.args.is_empty() {
            s.push_str("\nArguments:\n");
            table(&mut s, self.args);
        }
        if !self.output.is_empty() {
            s.push_str("\nPrints one JSON object:\n");
            table(&mut s, self.output);
        }
        if !self.writes.is_empty() {
            let _ = write!(s, "\nWrites: {}\n", self.writes);
        }
        let _ = write!(s, "\nExample:\n  {}\n\nNext: {}", self.example, self.next);
        wrap_long(&s)
    }
}

/// Two columns, the second wrapped under itself.
fn table(s: &mut String, rows: &[(&str, &str)]) {
    let width = rows
        .iter()
        .map(|(a, _)| a.len())
        .filter(|&n| n <= 26)
        .max()
        .unwrap_or(0);
    for (a, b) in rows {
        let lines = wrap(b, 92usize.saturating_sub(width + 4).max(40));
        if a.len() > width {
            let _ = writeln!(s, "  {a}");
            for l in &lines {
                let _ = writeln!(s, "  {:width$}  {l}", "");
            }
            continue;
        }
        for (i, l) in lines.iter().enumerate() {
            let left = if i == 0 { *a } else { "" };
            let _ = writeln!(s, "  {left:width$}  {l}");
        }
    }
}

/// Words wrapped to `width` columns.
pub fn wrap(text: &str, width: usize) -> Vec<String> {
    let mut out = Vec::new();
    for para in text.split('\n') {
        let mut line = String::new();
        for word in para.split_whitespace() {
            if !line.is_empty() && line.len() + 1 + word.len() > width {
                out.push(std::mem::take(&mut line));
            }
            if !line.is_empty() {
                line.push(' ');
            }
            line.push_str(word);
        }
        out.push(line);
    }
    out
}

/// Every command of both binaries.
pub fn all() -> impl Iterator<Item = &'static Command> {
    GAME.iter().chain(gm::COMMANDS).chain(motion::COMMANDS)
}

/// The commands of one binary.
pub fn of(tool: Tool) -> impl Iterator<Item = &'static Command> {
    all().filter(move |c| c.tool == tool)
}

/// The command `path` names exactly.
pub fn find(tool: Tool, path: &str) -> Option<&'static Command> {
    of(tool).find(|c| c.path == path)
}

/// The command the leading words of `args` name (the longest match), and the arguments
/// after them. None when the first word is no command.
pub fn resolve(tool: Tool, args: &[String]) -> Option<(&'static Command, usize)> {
    let mut best = None;
    for n in 1..=args.len().min(3) {
        let path = args[..n].join(" ");
        if let Some(c) = find(tool, &path) {
            best = Some((c, n));
        }
    }
    best
}

/// Whether the command line asks for help: `--help` or `-h` anywhere, or `help` first.
pub fn asked(args: &[String]) -> bool {
    args.first().is_some_and(|a| a == "help") || args.iter().any(|a| a == "--help" || a == "-h")
}

/// The help a command line asks for: one command's, or the overview.
pub fn for_args(tool: Tool, args: &[String]) -> String {
    let words: Vec<String> = args
        .iter()
        .filter(|a| *a != "--help" && *a != "-h")
        .skip_while(|a| *a == "help")
        .cloned()
        .collect();
    match resolve(tool, &words) {
        Some((c, _)) => c.render(),
        None if words.is_empty() => overview(tool),
        None if group(tool, &words[0]).is_some() => group(tool, &words[0]).expect("checked"),
        None => format!("no command `{}`.\n\n{}", words.join(" "), overview(tool)),
    }
}

/// The commands under a first word that is not a command itself (`sim`, `motion`), or
/// none.
pub fn group(tool: Tool, word: &str) -> Option<String> {
    if find(tool, word).is_some() {
        return None;
    }
    let prefix = format!("{word} ");
    let members: Vec<&Command> = of(tool).filter(|c| c.path.starts_with(&prefix)).collect();
    if members.is_empty() {
        return None;
    }
    if tool == Tool::Gm && word == "motion" {
        return Some(motion_overview());
    }
    let mut s = format!(
        "{} {word} <command>

Commands:
",
        tool.name()
    );
    let rows: Vec<(String, &str)> = members
        .iter()
        .map(|c| (format!("{} {}", c.path, short_synopsis(c)), c.answers))
        .collect();
    let pairs: Vec<(&str, &str)> = rows.iter().map(|(a, b)| (a.as_str(), *b)).collect();
    table(&mut s, &pairs);
    let _ = write!(
        s,
        "
{} {word} <command> --help for its arguments, output and an example.",
        tool.name()
    );
    Some(s)
}

/// The exit codes, as one line for the overviews.
pub const EXIT_CODES: &str = "Exit codes: 0 done; 1 failed while running; 2 can't start (a usage \
    mistake, or the modules need a newer engine); 4 bad input (a file or value is wrong); 5 \
    rejected (findings, a replay that differs, a motion expectation that doesn't hold). A \
    failure prints one JSON object: kind, error, and fields that say more.";

/// `galaxy --help` or `galaxy-gm --help`: short and task-oriented.
pub fn overview(tool: Tool) -> String {
    let (intro, groups): (&str, &[(&str, &[&str])]) = match tool {
        Tool::Game => (
            "galaxy: the game. It runs modules (folders with a manifest.toml) in a window or \
             headless. Every command prints one JSON object on stdout.",
            &[
                ("Play and take notes", &["play"]),
                (
                    "Run without a window",
                    &["sim run", "sim load", "sim replay"],
                ),
                ("Draw", &["shot", "film"]),
                ("This install", &["doctor"]),
            ],
        ),
        Tool::Gm => (
            "galaxy-gm: the GM. It checks modules, reads played sessions and their F8 notes, \
             films them and measures how motion looks. Every command prints one JSON object \
             on stdout, except --help, guide and types.",
            &[
                ("Start here", &["guide"]),
                ("Modules", &["check", "types", "fields", "model"]),
                (
                    "Played sessions and notes",
                    &["notes", "recording", "audit", "film"],
                ),
                ("How motion looks", &["film show", "motion", "motion check"]),
                ("This install", &["doctor"]),
            ],
        ),
    };
    let mut s = format!("{intro}\n");
    for (group, paths) in groups {
        let _ = write!(s, "\n{group}:\n");
        let rows: Vec<(String, String)> = paths
            .iter()
            .map(|p| match find(tool, p) {
                Some(c) => (
                    format!("{} {}", c.path, short_synopsis(c)),
                    gist(c.answers).to_owned(),
                ),
                None if *p == "motion" => (
                    "motion <tool> <film>".to_owned(),
                    format!(
                        "Measure one thing about a film: {}. `galaxy-gm motion --help` lists \
                         them.",
                        motion_tools().join(", ")
                    ),
                ),
                None => unreachable!("the overview names commands in the table: {p}"),
            })
            .collect();
        let pairs: Vec<(&str, &str)> = rows.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
        table(&mut s, &pairs);
    }
    let more = match tool {
        Tool::Game => {
            "\nMore: galaxy <command> --help for its arguments, output and an example. \
             galaxy-gm guide maps tasks to commands for both binaries. --version prints the \
             version; galaxy-gm --build <name> ... runs a named installed build."
        }
        Tool::Gm => {
            "\nMore: galaxy-gm <command> --help (or galaxy-gm help <command>) for its \
             arguments, output and an example; galaxy-gm guide <topic> walks through a task. \
             --version prints the version; galaxy-gm --build <name> ... runs a named \
             installed build."
        }
    };
    let _ = write!(s, "{more}\n\n{EXIT_CODES}");
    wrap_long(&s)
}

/// The motion tools the table has help for, in its order (`check` aside).
pub fn motion_tools() -> Vec<&'static str> {
    motion::COMMANDS
        .iter()
        .filter_map(|c| c.path.strip_prefix("motion "))
        .filter(|t| *t != "check")
        .collect()
}

/// The first sentence of `answers`, for the overview.
fn gist(answers: &str) -> &str {
    match answers.find(['?', '.']) {
        Some(i) => &answers[..=i],
        None => answers,
    }
}

/// The synopsis up to its first option, for one-line listings.
fn short_synopsis(c: &Command) -> &'static str {
    let s = c.synopsis;
    let cut = [s.find(" ["), s.find(" (")].into_iter().flatten().min();
    match cut {
        Some(i) if !s.starts_with('[') => &s[..i],
        _ => s,
    }
}

/// Paragraph lines longer than the terminal, wrapped; tables are left as they are.
fn wrap_long(s: &str) -> String {
    s.lines()
        .flat_map(|l| {
            if l.len() > 92 && !l.starts_with(' ') {
                wrap(l, 92)
            } else {
                vec![l.to_owned()]
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// `galaxy-gm motion --help`: the tools, which question each answers.
pub fn motion_overview() -> String {
    let mut s = String::from(
        "galaxy-gm motion <tool> <film> [options]\n\nMeasure one thing about how motion looks \
         in a film, with a sentence (`reading`), a few numbers, timestamps and one picture. A \
         film is a folder from `galaxy film` (a scripted run) or `galaxy-gm film` (a played \
         session). Times are seconds of presentation time (`t`); ticks are the sim's, 30 a \
         second.\n\nTools:\n",
    );
    let rows: Vec<(String, &str)> = motion::COMMANDS
        .iter()
        .map(|c| (format!("{} {}", c.path, short_synopsis(c)), c.answers))
        .collect();
    let pairs: Vec<(&str, &str)> = rows.iter().map(|(a, b)| (a.as_str(), *b)).collect();
    table(&mut s, &pairs);
    s.push_str(
        "\nHow to work a motion question: film a short span round the moment (60 fps, at the \
         player's frame size); ask the tool that fits and read its `reading`; look at its \
         picture; then `sheet --at <times>` for the flagged frames. When it is fixed, keep it \
         fixed with a `motion check` spec.\n\ngalaxy-gm motion <tool> --help for a tool's \
         options and output; galaxy-gm guide motion for the whole workflow.",
    );
    wrap_long(&s)
}

/// A guide topic.
pub struct Topic {
    pub name: &'static str,
    pub title: &'static str,
    /// What to know first, a few lines.
    pub about: &'static str,
    /// What you may want to do, and the command (a table path) that does it.
    pub tasks: &'static [(&'static str, &'static str)],
    /// Recipes: a question that takes several commands, and the steps, each a command
    /// line (starting `galaxy` or `galaxy-gm`, checked against the table) or a line of
    /// what to do or read.
    pub recipes: &'static [Recipe],
}

/// A question that takes several commands, step by step.
pub struct Recipe {
    pub title: &'static str,
    pub steps: &'static [&'static str],
}

pub use gm::TOPICS;

/// `galaxy-gm guide`: which command does what, by task, for both binaries.
pub fn guide() -> String {
    let mut s = String::from(
        "What do you want to do? Each line names the command; `<command> --help` says more, \
         and `galaxy-gm guide <topic>` walks through a topic with examples.\n",
    );
    for t in TOPICS {
        let _ = write!(s, "\n{} (guide {}):\n", t.title, t.name);
        let rows: Vec<(&str, String)> = t
            .tasks
            .iter()
            .map(|(want, path)| (*want, command_name(path)))
            .collect();
        let width = rows.iter().map(|(w, _)| w.len()).max().unwrap_or(0);
        for (want, cmd) in rows {
            let _ = writeln!(s, "  {want:width$}  {cmd}");
        }
    }
    let names: Vec<&str> = TOPICS.iter().map(|t| t.name).collect();
    let _ = write!(s, "\nTopics: {}.", names.join(", "));
    let recipes: Vec<String> = TOPICS
        .iter()
        .flat_map(|t| {
            t.recipes
                .iter()
                .map(move |r| format!("{} (guide {})", r.title, t.name))
        })
        .collect();
    if !recipes.is_empty() {
        let _ = write!(s, "\nRecipes: {}.", recipes.join("; "));
    }
    wrap_long(&s)
}

/// `galaxy-gm guide <topic>`.
pub fn guide_topic(name: &str) -> Option<String> {
    let t = TOPICS.iter().find(|t| t.name == name)?;
    let mut s = format!("{}\n\n{}\n", t.title, t.about.trim_end());
    for (want, path) in t.tasks {
        let c = command(path);
        let _ = write!(
            s,
            "\n{want}:\n  {}\n  {}\n",
            c.example,
            wrap(c.answers, 88).join("\n  ")
        );
    }
    for r in t.recipes {
        let _ = write!(s, "\nRecipe: {}\n", r.title);
        for (k, step) in r.steps.iter().enumerate() {
            let _ = writeln!(s, "  {}. {}", k + 1, wrap(step, 86).join("\n     "));
        }
    }
    let _ = write!(
        s,
        "\nEach command's --help lists its options and what its output means."
    );
    Some(wrap_long(&s))
}

/// A guide task's command: `galaxy-gm audit` for `gm audit`.
fn command(path: &str) -> &'static Command {
    let (tool, rest) = split(path);
    find(tool, rest).unwrap_or_else(|| panic!("the guide names a command not in the table: {path}"))
}

fn command_name(path: &str) -> String {
    let c = command(path);
    format!("{} {} {}", c.tool.name(), c.path, short_synopsis(c))
        .trim_end()
        .to_owned()
}

/// `gm audit` is galaxy-gm's audit, `game play` galaxy's play.
fn split(path: &str) -> (Tool, &str) {
    match path.split_once(' ') {
        Some(("gm", rest)) => (Tool::Gm, rest),
        Some(("game", rest)) => (Tool::Game, rest),
        _ => panic!("a guide task names `gm <command>` or `game <command>`, not {path}"),
    }
}

/// galaxy's commands.
static GAME: &[Command] = &[
    Command {
        tool: Tool::Game,
        path: "play",
        synopsis: "--modules <dir> [options]",
        answers: "Play the modules in a window. Esc quits, the wheel zooms, F8 takes a note.",
        about: "Every session records itself into a new folder under the sessions folder: \
                recording.json (written at every note and when the window closes), save.db \
                (the world at the end) and notes/0001.png (the frame shown at each note). F8 \
                pauses on the current tick and opens a note box over that frame; Enter keeps \
                the note, Esc drops it. `galaxy-gm notes` lists the notes later.",
        args: &[
            ("--modules <dir>", "a module folder, or a folder of modules"),
            ("--seed <n>", "the world's seed; 1 by default"),
            (
                "--size <w>x<h>",
                "the window in logical pixels; 1280x720 by default",
            ),
            (
                "--watch",
                "hot-load module folders when their files change, models included",
            ),
            (
                "--inputs <file>",
                "a bot: play these inputs, clicks, notes and zooms on their ticks \
              through the window loop. [[click]] takes tick and either x/y logical pixels \
              or widget = a full path or unique id (follows moving anchors). \
              `galaxy-gm fields inputs` lists the format; `galaxy-gm guide screens` gives a recipe",
            ),
            (
                "--ticks <n>",
                "quit once the world reaches tick n (30 ticks a second)",
            ),
            ("--frames <n>", "quit after this many frames"),
            ("--save <file>", "also write the world here when it quits"),
            ("--record <file>", "also write the recording here"),
        ],
        output: &[
            ("tick, hash", "the last tick and the world's hash there"),
            ("session", "the session folder"),
            ("notes", "how many notes were taken"),
            (
                "frame_time",
                "frame times in milliseconds: median, p95, worst",
            ),
            ("problems", "model, look or screen readability problems"),
            ("hot_load_failures", "hot-loads --watch refused, with why"),
            ("log", "what scripts printed"),
        ],
        writes: "a session folder under the sessions folder (`galaxy doctor` names it; \
                 GALAXY_SESSIONS moves it), and --save and --record if given",
        example: "galaxy play --modules game/modules",
        next: "galaxy-gm notes lists the notes; galaxy-gm audit <session>/recording.json \
               draws each noted moment again.",
        topic: "play",
    },
    Command {
        tool: Tool::Game,
        path: "shot",
        synopsis: "--modules <dir> --out <file.png> [options]",
        answers: "Draw one frame of the modules headless, after some ticks: what does it look \
                  like now?",
        about: "A still: drawn on the tick (no interpolation, presentation time 0). For how \
                something moves, film it (`galaxy film`).",
        args: &[
            ("--modules <dir>", "a module folder, or a folder of modules"),
            ("--out <file.png>", "the picture to write"),
            (
                "--ticks <n>",
                "ticks to run first, 30 a second; 30 by default",
            ),
            ("--seed <n>", "the world's seed; 1 by default"),
            (
                "--inputs <file>",
                "inputs and zooms by tick (`galaxy-gm fields inputs`)",
            ),
            ("--size <w>x<h>", "in pixels; 1280x720 by default"),
            (
                "--crop <thing>",
                "write only the square round this thing (by ID or bare name), enlarged in \
              whole pixel blocks to at least 512 px, for a before and after of one thing",
            ),
        ],
        output: &[
            ("out", "the picture"),
            (
                "crop",
                "with --crop: the thing, box_px (x, y, w, h in the full frame), how many \
              times it was enlarged, and a note when the thing reaches past the frame",
            ),
            ("tick, hash", "the tick drawn and the world's hash"),
            (
                "camera",
                "the camera: eye, target, view height in world units",
            ),
            ("models_drawn", "how many models the scene drew"),
            ("problems", "model, look or screen readability problems"),
            ("log", "what scripts printed"),
        ],
        writes: "--out",
        example: "galaxy shot --modules game/modules --ticks 90 --out shot.png",
        next: "Open the picture. For motion, `galaxy film` the same run.",
        topic: "modules",
    },
    Command {
        tool: Tool::Game,
        path: "film",
        synopsis: "--modules <dir> --out <folder> [options]",
        answers: "Film a scripted run headless, frame by frame on a fixed clock as the window \
                  would show it, with probes of where everything is on screen, for the motion \
                  tools.",
        about: "Frame k is shown at k/fps seconds; by then the sim has run floor(k*30/fps) \
                ticks and the frame is drawn between the last two, as `galaxy play` draws at \
                that rate. Scenes are built from ten seconds before the first frame, so effects \
                are under way. For a played session use `galaxy-gm film <recording>`.\n\
                --builds a,b films the same run with two installed builds (an older one is \
                asked only for what every build since films began takes; this build adds the \
                rest) into <out>/<build> each, and compares them.\n\
                Catching defects: thin lines breaking, one-frame pops and flicker show at 60 fps \
                and at the size the player had (1280x720 or more); at 30 fps or 960x540 they can \
                be missed.",
        args: &[
            ("--modules <dir>", "a module folder, or a folder of modules"),
            (
                "--out <folder>",
                "a new or empty folder, or an earlier film to replace",
            ),
            (
                "--inputs <file>",
                "inputs and zooms by tick (`galaxy-gm fields inputs`)",
            ),
            ("--from <tick>", "the first tick filmed; 0 by default"),
            (
                "--to <tick>",
                "the last; 90 ticks (three seconds) after --from by default",
            ),
            ("--fps <n>", "frames a second, 1 to 240; 60 by default"),
            ("--size <w>x<h>", "in pixels; 960x540 by default"),
            ("--seed <n>", "the world's seed; 1 by default"),
            (
                "--probes-only",
                "no pictures and no GPU: where things are on screen only \
              (enough for track and dots)",
            ),
            (
                "--builds <a,b>",
                "film with two installed builds and compare: each `this`, a build's \
              name, or a part only one has (its commit)",
            ),
        ],
        output: &[
            ("out", "the film folder"),
            (
                "frames, fps, size",
                "how many frames, at what rate and size",
            ),
            ("from, to", "the first and last frame: t (seconds) and tick"),
            ("drawn", "false for a probes-only film"),
            ("hash", "the world's hash at the last frame"),
            (
                "films, behaviour, compare",
                "with --builds: each film; how the followed thing moved on each build, \
              side by side (path, top speed, arrival, end) and whether it is the same; and \
              where the pictures differ",
            ),
            ("problems", "model, look or screen readability problems"),
        ],
        writes: "<out>/film.json (how it was filmed, small), frames.jsonl (a frame a line: \
                 where every thing is), lines.jsonl (a frame a line: the lines and dots drawn), \
                 frames/00000.png (lossless, one a frame; about 15 MB a second at 960x540). \
                 The motion tools put their pictures in <out>/motion/.",
        example: "galaxy film --modules game/modules --inputs route.toml --from 4 --to 94 \
                  --size 1280x720 --out films/route",
        next: "galaxy-gm film show <folder> for the probes, or a motion tool: galaxy-gm motion \
               --help lists them.",
        topic: "motion",
    },
    Command {
        tool: Tool::Game,
        path: "sim run",
        synopsis: "--modules <dir> [options]",
        answers: "Run a new world from the modules' start states, headless, as fast as it \
                  can: what does the world do?",
        about: "",
        args: &[
            ("--modules <dir>", "a module folder, or a folder of modules"),
            ("--seed <n>", "the world's seed; 1 by default"),
            ("--ticks <n>", "ticks to run, 30 a second; 300 by default"),
            (
                "--inputs <file>",
                "inputs, notes and hot-loads by tick \
              (`galaxy-gm fields inputs`)",
            ),
            ("--save <file>", "write a save (SQLite) when done"),
            ("--record <file>", "write a recording of the run"),
            ("--realtime", "run at 30 ticks a second"),
            (
                "--watch",
                "hot-load module folders when their files change (implies \
              --realtime)",
            ),
            (
                "--state",
                "print the world when done: a line an entity (where it is, its speed, \
              its model, its components) and the facts counted by kind",
            ),
            (
                "--entity <id>",
                "with --state: only this entity, by ID or bare name (ship for \
              drift.ship); the facts are left out",
            ),
            (
                "--component <name>",
                "with --state: only this component, by full name or bare (position \
              for engine.position); the facts are left out",
            ),
            (
                "--all",
                "with --state: the whole world, every field of every entity, the facts \
              and causes",
            ),
        ],
        output: &[
            ("tick, hash", "the last tick and the world's hash there"),
            ("entities, facts", "how many there are"),
            (
                "script_failures",
                "handler calls that were aborted, with the error",
            ),
            (
                "hot_loads, watch_failures",
                "modules hot-loaded, and those refused",
            ),
            ("log", "what scripts printed"),
            (
                "state",
                "with --state: a line an entity and the facts by kind; with --entity or \
              --component, those entities' and components' fields; with --all, every \
              field, the facts and causes",
            ),
        ],
        writes: "--save and --record, if given",
        example: "galaxy sim run --modules game/modules --ticks 300 --state --entity ship \
                  --component position",
        next: "galaxy sim replay <recording> checks a recording runs the same; galaxy-gm \
               recording <file> lists its inputs and orders.",
        topic: "replay",
    },
    Command {
        tool: Tool::Game,
        path: "sim load",
        synopsis: "<save> --modules <dir> [options]",
        answers: "Load a save and carry on with the modules.",
        about: "The save must list the same modules; their versions may differ if every \
                stored component still fits.",
        args: &[
            ("<save>", "a save file, from --save or a session's save.db"),
            ("--modules <dir>", "a module folder, or a folder of modules"),
            ("--ticks <n>", "ticks to run after loading; 0 by default"),
            ("--inputs <file>", "inputs, notes and hot-loads by tick"),
            ("--save <file>", "write a save when done"),
            ("--record <file>", "write a recording from the save on"),
            ("--realtime", "run at 30 ticks a second"),
            ("--watch", "hot-load module folders when their files change"),
            (
                "--state",
                "print the world when done: a line an entity, and the facts by kind",
            ),
            (
                "--entity <id>",
                "with --state: only this entity, by ID or bare name (ship for \
              drift.ship); the facts are left out",
            ),
            (
                "--component <name>",
                "with --state: only this component, by full name or bare (position \
              for engine.position); the facts are left out",
            ),
            (
                "--all",
                "with --state: the whole world, every field of every entity, the facts \
              and causes",
            ),
        ],
        output: &[
            ("tick, hash", "the last tick and the world's hash there"),
            ("script_failures, log", "as for sim run"),
            (
                "state",
                "with --state: a line an entity; --entity, --component or --all for fields",
            ),
        ],
        writes: "--save and --record, if given",
        example: "galaxy sim load save.db --modules game/modules --ticks 30 --state",
        next: "Read `state` for the components you care about.",
        topic: "replay",
    },
    Command {
        tool: Tool::Game,
        path: "sim replay",
        synopsis: "<recording> [--state [--entity <id>] [--component <name>] [--all]]",
        answers: "Run a recording again from its start with its own module files: does it \
                  end in the same world?",
        about: "Which build runs it: without --build, the launcher runs the build that made \
                the recording (when it is installed); `galaxy --build <name> sim replay ...` \
                runs that one instead. `builds` says which recorded it and which replayed it, \
                and whether they are the same. A different end exits 5 with kind \
                replay_diverged.",
        args: &[
            (
                "<recording>",
                "a recording file (a session's recording.json)",
            ),
            (
                "--state",
                "print the world at the end: a line an entity, and the facts by kind",
            ),
            (
                "--entity <id>",
                "with --state: only this entity, by ID or bare name (ship for \
              drift.ship); the facts are left out",
            ),
            (
                "--component <name>",
                "with --state: only this component, by full name or bare (position \
              for engine.position); the facts are left out",
            ),
            (
                "--all",
                "with --state: the whole world, every field of every entity, the facts \
              and causes",
            ),
        ],
        output: &[
            (
                "identical",
                "the replay ended with the recorded world's hash",
            ),
            (
                "tick, hash, recorded_hash",
                "where it ended, and both hashes",
            ),
            (
                "builds",
                "recorded (the build that made it), replayed (this one), same (true, \
              false, or null when it can't be told), note (what that means, and how to run \
              the recording's build when they differ)",
            ),
            ("notes", "each note's tick, text and the world's hash there"),
            (
                "state",
                "with --state: the world at the end, a line an entity; --entity, \
              --component or --all for fields",
            ),
        ],
        writes: "",
        example: "galaxy sim replay recording.json",
        next: "galaxy-gm audit <recording> draws each noted moment again; galaxy-gm \
               recording <recording> inputs lists what the player did.",
        topic: "replay",
    },
    Command {
        tool: Tool::Game,
        path: "doctor",
        synopsis: "",
        answers: "Which engine is this, which builds and launchers are installed, and which \
                  modules are near the working folder?",
        about: "",
        args: &[],
        output: &[
            ("engine, build", "this engine's version and installed build"),
            (
                "install_root, installed_builds",
                "where builds are installed, and each one",
            ),
            (
                "launchers",
                "the launcher folder, which build made it, whether it is on PATH",
            ),
            (
                "modules",
                "modules near the working folder, with their minimum engine",
            ),
        ],
        writes: "",
        example: "galaxy doctor",
        next: "galaxy-gm --build <name> <command> runs a named build, such as a dev build.",
        topic: "install",
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_command_has_what_an_agent_needs() {
        for c in all() {
            let name = format!("{} {}", c.tool.name(), c.path);
            assert!(!c.answers.is_empty(), "{name}: answers");
            assert!(
                c.example
                    .starts_with(&format!("{} {}", c.tool.name(), c.path)),
                "{name}: the example runs the command: {}",
                c.example
            );
            assert!(!c.next.is_empty(), "{name}: next");
            assert!(
                TOPICS.iter().any(|t| t.name == c.topic),
                "{name}: topic {}",
                c.topic
            );
            let text = c.render();
            for banned in ["engine/docs", ".md", "crates/"] {
                assert!(!text.contains(banned), "{name} points at {banned}");
            }
            // Every option the example uses is one it takes.
            let options = c.options();
            for word in c.example.split_whitespace() {
                if let Some(o) = word.strip_prefix("--") {
                    assert!(
                        options.contains(&o),
                        "{name}: the example's --{o} is not listed"
                    );
                }
            }
        }
    }

    #[test]
    fn paths_are_unique_and_the_guide_names_real_commands() {
        let mut seen = std::collections::BTreeSet::new();
        for c in all() {
            assert!(seen.insert((c.tool.name(), c.path)), "{} twice", c.path);
        }
        for t in TOPICS {
            for (_, path) in t.tasks {
                command(path);
            }
            assert!(guide_topic(t.name).is_some());
        }
        // Every command is in some topic's tasks, so the guide reaches it.
        for c in all() {
            let path = format!(
                "{} {}",
                if c.tool == Tool::Gm { "gm" } else { "game" },
                c.path
            );
            assert!(
                TOPICS
                    .iter()
                    .any(|t| t.tasks.iter().any(|(_, p)| *p == path)),
                "the guide never names {path}"
            );
        }
        let g = guide();
        assert!(g.lines().count() < 80, "the guide stays short:\n{g}");
    }

    #[test]
    fn screens_guide_covers_authoring_and_the_playtest_loop() {
        let text = guide_topic("screens").expect("screens guide topic");
        for required in [
            "ui.button",
            "engine.anchor",
            "galaxy-gm check",
            "galaxy shot",
            "widget",
            "galaxy play",
            "galaxy sim replay",
            "galaxy-gm audit",
        ] {
            assert!(text.contains(required), "missing {required}");
        }
        assert!(for_args(Tool::Gm, &["guide".into()]).contains("screens"));
        assert!(for_args(Tool::Game, &["play".into()]).contains("widget"));
    }

    /// Every command line in a recipe runs a command in the table with options it takes.
    #[test]
    fn recipes_run_real_commands() {
        let mut lines = 0;
        for t in TOPICS {
            for r in t.recipes {
                for step in r.steps {
                    let words: Vec<String> = step.split_whitespace().map(str::to_owned).collect();
                    let tool = match words.first().map(String::as_str) {
                        Some("galaxy") => Tool::Game,
                        Some("galaxy-gm") => Tool::Gm,
                        _ => continue,
                    };
                    lines += 1;
                    let (c, _) = resolve(tool, &words[1..])
                        .unwrap_or_else(|| panic!("{}: no command in `{step}`", r.title));
                    let options = c.options();
                    for w in &words {
                        if let Some(o) = w.strip_prefix("--") {
                            assert!(options.contains(&o), "{}: `{step}`: --{o}", r.title);
                        }
                    }
                }
            }
            if !t.recipes.is_empty() {
                let text = guide_topic(t.name).unwrap();
                assert!(text.contains("Recipe: "), "{text}");
            }
        }
        assert!(lines >= 4, "the recipes give their commands");
    }

    #[test]
    fn help_is_found_from_any_command_line() {
        let args = |s: &str| s.split_whitespace().map(str::to_owned).collect::<Vec<_>>();
        assert!(asked(&args("motion flicker film --help")));
        assert!(asked(&args("help film")));
        assert!(!asked(&args("film rec.json")));
        let h = for_args(Tool::Gm, &args("motion flicker films/a --help"));
        assert!(h.starts_with("galaxy-gm motion flicker"), "{h}");
        let h = for_args(Tool::Gm, &args("film rec.json --help"));
        assert!(h.starts_with("galaxy-gm film <recording>"), "{h}");
        let h = for_args(Tool::Game, &args("help sim replay"));
        assert!(h.starts_with("galaxy sim replay"), "{h}");
        assert!(for_args(Tool::Game, &args("--help")).contains("sim run"));
        assert!(for_args(Tool::Gm, &args("nope --help")).starts_with("no command `nope`"));
    }

    #[test]
    fn overviews_stay_short() {
        for tool in [Tool::Game, Tool::Gm] {
            let o = overview(tool);
            assert!(o.lines().count() < 45, "{}:\n{o}", tool.name());
        }
    }
}
