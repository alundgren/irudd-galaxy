//! The client `galaxy-gm live` uses: find the running games, choose one, send one request
//! and read its reply.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

use serde_json::{Value, json};

use crate::protocol::{Discovery, Op, PROTOCOL, Request, Response, WireError};

/// A running game, found by its discovery file.
#[derive(Clone, Debug)]
pub struct Game {
    pub discovery: Discovery,
    pub file: PathBuf,
}

/// The games serving in `dir`, newest first. A discovery file whose game can't be reached
/// (it crashed, or was killed) is removed.
pub fn games(dir: &Path) -> Vec<Game> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_none_or(|e| e != "json") {
            continue;
        }
        let Ok(discovery) = Discovery::read(&path) else {
            continue;
        };
        match discovery.endpoint.connect() {
            Ok(stream) => drop(stream),
            Err(_) => {
                let _ = std::fs::remove_file(&path);
                continue;
            }
        }
        out.push(Game {
            discovery,
            file: path,
        });
    }
    out.sort_by(|a, b| {
        (b.discovery.started, b.discovery.pid).cmp(&(a.discovery.started, a.discovery.pid))
    });
    out
}

/// The game a command works on, and why it was chosen. `want` is `--game` (a pid or a
/// session folder's name); `modules` are module IDs the game must run (a load names
/// them); `cwd` breaks a tie for the game started from modules in this folder.
pub fn choose(
    games: Vec<Game>,
    want: Option<&str>,
    modules: &[String],
    cwd: &Path,
) -> Result<(Game, &'static str), WireError> {
    let list = |gs: &[Game]| Value::Array(gs.iter().map(|g| g.discovery.public()).collect());
    if games.is_empty() {
        return Err(WireError::new(
            2,
            "no_game",
            "no game is running: start one with `galaxy play --modules <dir>` (play serves \
             the live socket unless --no-live), then run this again",
        ));
    }
    if let Some(want) = want {
        let found: Vec<Game> = games
            .iter()
            .filter(|g| g.discovery.pid.to_string() == want || g.discovery.session == want)
            .cloned()
            .collect();
        return match found.len() {
            1 => Ok((found.into_iter().next().expect("one"), "named by --game")),
            _ => Err(WireError::new(
                2,
                "no_game",
                format!(
                    "no running game is `{want}`; --game takes a pid or a session name from \
                     `games`"
                ),
            )
            .with("games", list(&games))),
        };
    }
    let running = |g: &Game, id: &str| g.discovery.modules.iter().any(|m| m.id == id);
    let fitting: Vec<Game> = games
        .iter()
        .filter(|g| modules.iter().all(|id| running(g, id)))
        .cloned()
        .collect();
    if fitting.is_empty() {
        return Err(WireError::new(
            2,
            "no_game",
            format!(
                "no running game runs {}; start one that does, or name another module",
                modules.join(" and ")
            ),
        )
        .with("games", list(&games)));
    }
    if fitting.len() == 1 {
        let why = if games.len() == 1 {
            "the only game running"
        } else {
            "the only game running these modules"
        };
        return Ok((fitting.into_iter().next().expect("one"), why));
    }
    let here: Vec<&Game> = fitting
        .iter()
        .filter(|g| g.discovery.modules.iter().any(|m| m.dir.starts_with(cwd)))
        .collect();
    if let [one] = here.as_slice() {
        return Ok((
            (*one).clone(),
            "the only game playing modules in this folder",
        ));
    }
    Err(WireError::new(
        2,
        "several_games",
        format!(
            "{} games are running; say which with --game <pid or session>",
            fitting.len()
        ),
    )
    .with("games", list(&fitting)))
}

/// Sends one request and waits up to `timeout` for its reply.
pub fn call(game: &Discovery, op: &Op, timeout: Duration) -> Result<Value, WireError> {
    if game.protocol != PROTOCOL {
        return Err(WireError::new(
            2,
            "live_protocol",
            format!(
                "the running game (build {}) speaks live protocol {}, and this galaxy-gm \
                 speaks {PROTOCOL}; run the game's own: galaxy-gm --build {} live ...",
                game.build, game.protocol, game.build
            ),
        ));
    }
    let wire = serde_json::to_value(op).expect("ops serialize");
    let request = Request {
        id: 1,
        token: game.token.clone(),
        op: wire["op"].as_str().unwrap_or_default().to_owned(),
        args: wire.get("args").cloned().unwrap_or(json!({})),
    };
    let stream = game.endpoint.connect().map_err(|e| {
        WireError::new(
            2,
            "no_game",
            format!("the game (pid {}) stopped answering: {e}", game.pid),
        )
    })?;
    let line = serde_json::to_string(&request).expect("requests serialize");
    (&stream)
        .write_all(format!("{line}\n").as_bytes())
        .and_then(|()| (&stream).flush())
        .map_err(|e| WireError::runtime(format!("can't send to the game: {e}")))?;
    // Named pipes have no read timeout, so the read runs on a thread of its own.
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut reply = String::new();
        let read = BufReader::new(&stream).read_line(&mut reply);
        let _ = tx.send(read.map(|_| reply));
    });
    let reply = match rx.recv_timeout(timeout) {
        Ok(Ok(reply)) if !reply.trim().is_empty() => reply,
        Ok(Ok(_)) | Ok(Err(_)) | Err(mpsc::RecvTimeoutError::Disconnected) => {
            return Err(WireError::runtime(
                "the game closed the connection without answering (did it quit?)",
            ));
        }
        Err(mpsc::RecvTimeoutError::Timeout) => {
            return Err(WireError::runtime(format!(
                "the game did not answer within {} s. Is it paused at a note (F8), or \
                 minimized so its ticks stopped?",
                timeout.as_secs()
            )));
        }
    };
    let response: Response = serde_json::from_str(&reply)
        .map_err(|e| WireError::runtime(format!("the game's reply is not one: {e}")))?;
    match (response.ok, response.result, response.error) {
        (true, Some(result), _) => Ok(result),
        (_, _, Some(error)) => Err(error),
        _ => Err(WireError::runtime(
            "the game's reply has neither result nor error",
        )),
    }
}
