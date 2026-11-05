//! `galaxy-gm live input`: holds keys and moves the pointer in a running game, through the
//! same path its window's keyboard and mouse take, over one connection. Steps come from
//! the arguments, from stdin a line at a time, or from a file it follows (`--follow`);
//! each line runs as it arrives, so another process can decide the next step from what
//! `live shot` and `live state` show. Whatever the connection still holds when it closes
//! (the steps ended, or this process died) the game lets go of.

use std::io::{BufRead, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::Duration;

use galaxy_api::engine::TICKS_PER_SECOND;
use galaxy_live::client::{self, Connection, Game};
use galaxy_live::protocol::{self, MAX_INPUT_TICKS, MOUSE_LEFT};
use galaxy_live::{Act, Op, Step, WireError};
use galaxy_module::cli::Output;
use galaxy_module::{Failure, discover};
use galaxy_motion::Opts;
use serde_json::{Value, json};

use super::{failure, game_json};

/// How long `tap` and `click` hold their key or button: 0.1 seconds.
const TAP_TICKS: u64 = 3;

/// What `live input` takes its steps from.
enum Source {
    /// The arguments: one timeline.
    Args(Vec<String>),
    /// Stdin, a line at a time.
    Stdin,
    /// A file, followed as lines are added to it, until a line `end`.
    Follow(PathBuf),
}

/// Steps as one request: the steps in order, how many ticks they run, and what they leave
/// held or let go.
#[derive(Debug, Default, PartialEq)]
struct Timeline {
    steps: Vec<Step>,
    ticks: u64,
}

pub(crate) fn run(
    positional: &[String],
    opts: &Opts,
    cwd: &Path,
    usage: impl Fn(String) -> Failure,
) -> Result<Output, Failure> {
    let source = match (positional.is_empty(), opts.text("follow")) {
        (false, Some(_)) => {
            return Err(usage(
                "live input takes steps or --follow <file>, not both".into(),
            ));
        }
        (true, Some(file)) => Source::Follow(discover::resolve(cwd, Path::new(file))),
        (true, None) => Source::Stdin,
        (false, None) => Source::Args(positional.to_vec()),
    };
    // Steps given as arguments are checked before any game is looked for.
    let first = match &source {
        Source::Args(args) => Some(timeline(&args.join(";")).map_err(&usage)?),
        _ => None,
    };
    let games = client::games(&galaxy_live::live_dir());
    let (game, why) = client::choose(games, opts.text("game"), &[], cwd).map_err(failure)?;
    let mut run = Driver::open(&game, why)?;
    match source {
        Source::Args(_) => {
            if let Some(t) = first {
                run.send(t)?;
            }
        }
        Source::Stdin => {
            let stdin = std::io::stdin();
            for line in stdin.lock().lines() {
                let line = line.map_err(|e| run.fail(WireError::runtime(format!("stdin: {e}"))))?;
                if run.line(&line, &usage)? {
                    break;
                }
            }
        }
        Source::Follow(path) => follow(&path, &mut run, &usage)?,
    }
    run.finish()
}

/// Runs each line added to `path` as it comes, until a line `end`. The file may not exist
/// yet; it is read from its start.
fn follow(
    path: &Path,
    run: &mut Driver,
    usage: &impl Fn(String) -> Failure,
) -> Result<(), Failure> {
    let mut at = 0u64;
    let mut partial = String::new();
    loop {
        let mut added = String::new();
        if let Ok(mut file) = std::fs::File::open(path) {
            let len = file.metadata().map_or(0, |m| m.len());
            if len < at {
                return Err(run.fail(
                    WireError::input(format!(
                        "{} got shorter while it was followed; add lines to it, don't rewrite it",
                        path.display()
                    ))
                    .with("path", path.display().to_string()),
                ));
            }
            if len > at {
                file.seek(SeekFrom::Start(at))
                    .and_then(|_| file.read_to_string(&mut added))
                    .map_err(|e| {
                        run.fail(WireError::runtime(format!("{}: {e}", path.display())))
                    })?;
                at += added.len() as u64;
            }
        }
        partial.push_str(&added);
        while let Some(end) = partial.find('\n') {
            let line: String = partial.drain(..=end).collect();
            if run.line(&line, usage)? {
                return Ok(());
            }
        }
        if added.is_empty() {
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

/// One connection to the game and what has happened on it.
struct Driver {
    connection: Connection,
    game: Value,
    from_tick: Option<u64>,
    to_tick: u64,
    hash: Value,
    steps: Vec<Value>,
    unbound: Vec<String>,
    /// Keys this command's steps hold down now.
    held: Vec<String>,
}

impl Driver {
    fn open(game: &Game, why: &str) -> Result<Driver, Failure> {
        let game_json = game_json(game, why);
        let connection = Connection::open(&game.discovery).map_err(|e| {
            let mut f = failure(e);
            f.fields.insert("game".into(), game_json.clone());
            f
        })?;
        Ok(Driver {
            connection,
            game: game_json,
            from_tick: None,
            to_tick: 0,
            hash: Value::Null,
            steps: Vec::new(),
            unbound: Vec::new(),
            held: Vec::new(),
        })
    }

    /// A failure that says which game it was and what ran before it.
    fn fail(&self, e: WireError) -> Failure {
        let mut f = failure(e);
        f.fields.insert("game".into(), self.game.clone());
        f.fields.entry("steps").or_insert_with(|| json!(self.steps));
        f
    }

    /// Runs one line of steps; true when it is `end`.
    fn line(&mut self, line: &str, usage: &impl Fn(String) -> Failure) -> Result<bool, Failure> {
        let line = line.trim();
        if line.eq_ignore_ascii_case("end") {
            return Ok(true);
        }
        if line.is_empty() || line.starts_with('#') {
            return Ok(false);
        }
        let t = timeline(line).map_err(|e| {
            let mut f = usage(e);
            f.fields.insert("game".into(), self.game.clone());
            f.fields.insert("steps".into(), json!(self.steps));
            f
        })?;
        self.send(t)?;
        Ok(false)
    }

    /// Sends one timeline and waits until its ticks have run.
    fn send(&mut self, t: Timeline) -> Result<(), Failure> {
        let op = Op::Input {
            steps: t.steps,
            ticks: t.ticks,
        };
        // Twice real time, and some: the window may drop frames.
        let wait = Duration::from_secs(t.ticks / 15 + 15);
        let reply = self
            .connection
            .call(&op, wait)
            .map_err(|e| self.fail(older_game(e)))?;
        self.from_tick
            .get_or_insert(reply["from_tick"].as_u64().unwrap_or(0));
        self.to_tick = reply["to_tick"].as_u64().unwrap_or(self.to_tick);
        self.hash = reply["hash"].clone();
        if let Some(steps) = reply["steps"].as_array() {
            self.steps.extend(steps.iter().cloned());
        }
        for key in reply["unbound"].as_array().into_iter().flatten() {
            if let Some(k) = key.as_str()
                && !self.unbound.iter().any(|u| u == k)
            {
                self.unbound.push(k.to_owned());
            }
        }
        self.held = reply["held"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|k| k.as_str().map(str::to_owned))
            .collect();
        Ok(())
    }

    /// Lets go of what the steps left held, and reports.
    fn finish(mut self) -> Result<Output, Failure> {
        let released = std::mem::take(&mut self.held);
        if !released.is_empty() {
            let steps = released
                .iter()
                .map(|key| Step {
                    at: 0,
                    act: Act::Key {
                        key: key.clone(),
                        down: false,
                    },
                })
                .collect();
            self.send(Timeline { steps, ticks: 1 })?;
        }
        let note = if self.unbound.is_empty() {
            "the steps reached the game as its own keyboard and mouse would send them, \
             recorded with via live (live history --inputs, galaxy-gm recording)"
        } else {
            "no running module binds the keys in unbound, so holding them sent nothing"
        };
        Ok(Output::Json(json!({
            "ok": true,
            "game": self.game,
            "from_tick": self.from_tick,
            "to_tick": self.to_tick,
            "seconds": self.from_tick.map(|f| {
                (self.to_tick + 1).saturating_sub(f) as f64 / f64::from(TICKS_PER_SECOND)
            }),
            "steps": self.steps,
            "released_at_end": released,
            "unbound": self.unbound,
            "hash": self.hash,
            "note": note,
        })))
    }
}

/// An older game answers an `input` it doesn't know with a usage error: say so plainly.
fn older_game(e: WireError) -> WireError {
    if e.kind == "usage" && e.message.contains("unknown variant `input`") {
        return WireError::usage(
            "this game's build has no live input; start the game with this build \
             (galaxy --build <name> play ...) or use the build that has it",
        );
    }
    e
}

/// Seconds as ticks, 30 a second, rounded.
fn ticks(word: &str, what: &str) -> Result<u64, String> {
    let s: f64 = word
        .parse()
        .ok()
        .filter(|s: &f64| s.is_finite() && *s >= 0.0)
        .ok_or_else(|| format!("{what} takes seconds (such as 0.5), not `{word}`"))?;
    let n = (s * f64::from(TICKS_PER_SECOND)).round() as u64;
    if n > MAX_INPUT_TICKS {
        return Err(format!(
            "{what} {word}: at most {} seconds",
            MAX_INPUT_TICKS / u64::from(TICKS_PER_SECOND)
        ));
    }
    Ok(n)
}

fn key(word: &str) -> Result<String, String> {
    let key = word.to_ascii_lowercase();
    if protocol::is_key(&key) {
        Ok(key)
    } else {
        Err(protocol::not_a_key(word))
    }
}

fn point(words: &[&str], what: &str) -> Result<(f32, f32), String> {
    let joined = words.join(" ").replace(',', " ");
    let parts: Vec<&str> = joined.split_whitespace().collect();
    let number = |w: &str| w.parse::<f32>().ok().filter(|v| v.is_finite());
    match parts.as_slice() {
        [x, y] => match (number(x), number(y)) {
            (Some(x), Some(y)) => Ok((x, y)),
            _ => Err(format!(
                "{what} takes x y in logical pixels, not `{joined}`"
            )),
        },
        _ => Err(format!(
            "{what} takes x y in logical pixels from the window's top left (such as \
             `{what} 640 360`)"
        )),
    }
}

/// Steps separated by `;` as one timeline. `hold` runs alongside the steps after it;
/// `tap`, `click` and `wait` take their time before the next one.
fn timeline(text: &str) -> Result<Timeline, String> {
    let mut steps = Vec::new();
    let (mut t, mut end) = (0u64, 0u64);
    let mut add = |steps: &mut Vec<Step>, at: u64, act: Act| {
        end = end.max(at + 1);
        steps.push(Step { at, act });
    };
    let key_step = |key: String, down: bool| Act::Key { key, down };
    for step in text.split(';').map(str::trim).filter(|s| !s.is_empty()) {
        let words: Vec<&str> = step.split_whitespace().collect();
        match words.as_slice() {
            ["down", k] => add(&mut steps, t, key_step(key(k)?, true)),
            ["up", k] => add(&mut steps, t, key_step(key(k)?, false)),
            ["hold", k, s] => {
                let k = key(k)?;
                let n = ticks(s, "hold")?.max(1);
                add(&mut steps, t, key_step(k.clone(), true));
                add(&mut steps, t + n, key_step(k, false));
            }
            ["tap", k] => {
                let k = key(k)?;
                add(&mut steps, t, key_step(k.clone(), true));
                add(&mut steps, t + TAP_TICKS, key_step(k, false));
                t += TAP_TICKS;
            }
            ["wait", s] => t += ticks(s, "wait")?,
            ["move", rest @ ..] => {
                let (x, y) = point(rest, "move")?;
                add(&mut steps, t, Act::Move { x, y });
            }
            ["click", rest @ ..] => {
                let (x, y) = point(rest, "click")?;
                add(&mut steps, t, Act::Move { x, y });
                add(&mut steps, t, key_step(MOUSE_LEFT.into(), true));
                add(
                    &mut steps,
                    t + TAP_TICKS,
                    key_step(MOUSE_LEFT.into(), false),
                );
                t += TAP_TICKS;
            }
            _ => {
                return Err(format!(
                    "`{step}` is not a step; steps are down <key>, up <key>, hold <key> \
                     <seconds>, tap <key>, wait <seconds>, move <x> <y>, click <x> <y>, \
                     separated by ; or given as separate arguments"
                ));
            }
        }
    }
    let ticks = end.max(t).max(1);
    let op = Op::Input {
        steps: steps.clone(),
        ticks,
    };
    op.check()?;
    Ok(Timeline { steps, ticks })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(t: &Timeline) -> Vec<(u64, String)> {
        t.steps.iter().map(|s| (s.at, s.act.text())).collect()
    }

    #[test]
    fn holds_run_alongside_and_taps_take_their_time() {
        let t = timeline("hold W 2; hold a 0.5").unwrap();
        assert_eq!(
            keys(&t),
            [
                (0, "down w".into()),
                (60, "up w".into()),
                (0, "down a".into()),
                (15, "up a".into()),
            ]
        );
        // W is down for ticks 1 to 60 of the input: two seconds.
        assert_eq!(t.ticks, 61);
        let t = timeline("tap space; wait 1; click 640,360").unwrap();
        assert_eq!(
            keys(&t),
            [
                (0, "down space".into()),
                (3, "up space".into()),
                (33, "move 640 360".into()),
                (33, "down mouse_left".into()),
                (36, "up mouse_left".into()),
            ]
        );
        assert_eq!(t.ticks, 37);
        assert_eq!(timeline("down w").unwrap().ticks, 1);
    }

    #[test]
    fn bad_steps_say_what_is_wrong() {
        assert!(timeline("hold jump 2").unwrap_err().contains("not a key"));
        assert!(timeline("hold w soon").unwrap_err().contains("seconds"));
        assert!(timeline("hold w 900").unwrap_err().contains("at most"));
        assert!(timeline("fly north").unwrap_err().contains("not a step"));
        assert!(timeline("move 10").unwrap_err().contains("x y"));
        assert!(timeline("move -5 10").unwrap_err().contains("0 to"));
    }
}
