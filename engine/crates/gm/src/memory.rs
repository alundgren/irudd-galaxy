//! `galaxy-gm memory <modules>`: the GM's memory of one game, a short Markdown file the GM
//! reads at the start of an unattended run and edits at its end. It holds the player's
//! preferences and the long-running directions not yet in the game. It lives in the
//! per-user game data folder (`install::memory_dir`), one file a game, named by the game's
//! module IDs, so it survives reinstalls and is kept whether the player accepts or rolls
//! back a change. The player may read and edit it. It never holds secrets: a memory that
//! holds the OpenRouter key is refused unprinted.

use std::io::Write as _;
use std::path::Path;

use galaxy_module::cli::Output;
use galaxy_module::{Failure, discover, install};
use serde_json::json;

/// What a new memory starts as. `{game}` is the module IDs.
const TEMPLATE: &str = "# GM memory: {game}

The GM reads this at the start of every unattended run and updates it at the end. You may
edit it: the GM follows what you write here. Keep it short, and only what the game does
not already say. Never put keys, passwords or other secrets here.

## The player's preferences

## Directions not yet in the game

## Handed over, waiting for the player
";

/// The environment variable the GM's one key comes from.
const KEY_VAR: &str = "OPENROUTER_API_KEY";

pub fn run(dir: &Path) -> Result<Output, Failure> {
    let ids = discover::game_ids(dir)?;
    let game = ids.join("+");
    let path = install::memory_dir().join(format!("{game}.md"));
    let io = |e: std::io::Error| {
        Failure::runtime(format!("can't open {}: {e}", path.display()))
            .with("path", path.display().to_string())
    };
    std::fs::create_dir_all(path.parent().expect("a file in a folder")).map_err(io)?;
    // Made only when missing: create_new never replaces the player's edits.
    let created = match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
    {
        Ok(mut file) => {
            file.write_all(TEMPLATE.replace("{game}", &ids.join(", ")).as_bytes())
                .map_err(io)?;
            true
        }
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => false,
        Err(e) => return Err(io(e)),
    };
    let text = std::fs::read_to_string(&path).map_err(io)?;
    if holds_secret(&text, std::env::var(KEY_VAR).ok().as_deref()) {
        return Err(Failure::bad_input(format!(
            "{} holds an API key; delete it from the file (the memory never holds secrets), \
             then run memory again",
            path.display()
        ))
        .with("path", path.display().to_string()));
    }
    Ok(Output::Json(json!({
        "ok": true,
        "command": "memory",
        "game": ids,
        "path": path.display().to_string(),
        "created": created,
        "lines": text.lines().count(),
        "text": text,
        "next": "Read it before choosing work; at the end of the run edit the file itself \
                 (galaxy-gm guide unattended).",
    })))
}

/// Whether `text` holds the key from the environment, or anything shaped like an
/// OpenRouter key.
fn holds_secret(text: &str, key: Option<&str>) -> bool {
    let key = key.map(str::trim).filter(|k| k.len() >= 8);
    key.is_some_and(|k| text.contains(k)) || text.contains("sk-or-")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_memory_with_a_key_is_refused() {
        assert!(!holds_secret(TEMPLATE, Some("abcdefgh12345")));
        assert!(holds_secret("prefers abcdefgh12345", Some("abcdefgh12345")));
        assert!(holds_secret("key sk-or-v1-0000", None));
        // A short or empty variable never matches ordinary words.
        assert!(!holds_secret("a quiet garden", Some("a")));
        assert!(!holds_secret("a quiet garden", Some("  ")));
    }
}
