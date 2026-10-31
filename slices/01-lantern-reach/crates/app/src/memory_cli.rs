//! `galaxy memory`: search a save's world memory (issue #47, 4.4), the summarised
//! history per station, person and faction that `galaxy_core::memory` keeps in the
//! save with an FTS5 index. Prints JSON like the other commands; for agents and the
//! game master, never the player.

use std::path::PathBuf;

use clap::{Args, Subcommand};
use galaxy_core::Save;
use galaxy_core::memory::{self, Filter, Source};
use serde_json::{Value, json};

#[derive(Subcommand)]
pub enum MemoryCmd {
    /// Entries whose text has every word of QUERY, best match first; with no QUERY, the
    /// filter's entries, newest first.
    Search(SearchArgs),
}

#[derive(Args)]
pub struct SearchArgs {
    #[arg(long)]
    save: PathBuf,
    /// Words to find (stemmed: "sold ice" finds "Sold 30 ice at Kettle Dock").
    #[arg(default_value = "")]
    query: String,
    /// Only entries about this station (`station.kettle_dock`).
    #[arg(long)]
    station: Option<String>,
    /// Only entries about this person (`npc.odo_fenn`).
    #[arg(long)]
    person: Option<String>,
    /// Only entries about this faction (`faction.wharf_cooperative`).
    #[arg(long)]
    faction: Option<String>,
    /// Only entries from this source: authored, game_master or live_layer.
    #[arg(long)]
    source: Option<String>,
    /// Most entries (default 20).
    #[arg(long)]
    limit: Option<u32>,
}

/// Runs a `memory` command. `Err` is (exit code, message): 4 for bad input, 1 for a
/// runtime failure.
pub fn run(cmd: MemoryCmd) -> Result<Value, (u8, String)> {
    let MemoryCmd::Search(a) = cmd;
    let source = match a.source.as_deref() {
        None => None,
        Some(s) => Some(Source::parse(s).ok_or_else(|| {
            (
                4,
                format!("--source `{s}` should be authored, game_master or live_layer"),
            )
        })?),
    };
    let save = Save::open(&a.save).map_err(|e| {
        let input = matches!(
            e,
            galaxy_core::SaveError::Missing(_) | galaxy_core::SaveError::Format { .. }
        );
        (if input { 4 } else { 1 }, e.to_string())
    })?;
    let filter = Filter {
        station: a.station,
        person: a.person,
        faction: a.faction,
        source,
        limit: a.limit,
    };
    let runtime = |e: galaxy_core::SaveError| (1, e.to_string());
    let entries = memory::search(&save, &a.query, &filter).map_err(runtime)?;
    let total = memory::count(&save).map_err(runtime)?;
    Ok(json!({
        "query": a.query,
        "total": total,
        "entries": entries,
    }))
}
