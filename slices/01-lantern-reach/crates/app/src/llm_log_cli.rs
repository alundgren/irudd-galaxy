//! `galaxy llm-log --save PATH <query>`: reads the LLM call log beside a save
//! (`<stem>.llm-log.sqlite`, see `galaxy_mind::calllog`) and prints JSON. For agents
//! and developers; the player never sees any of it.

use std::path::{Path, PathBuf};

use clap::{Args, Subcommand};
use galaxy_mind::MindConfig;
use galaxy_mind::calllog::{self, CallLog, Group, LogRules, Range};
use serde_json::{Value, json};

#[derive(Args)]
pub struct LlmLogArgs {
    /// The save (its log is `<stem>.llm-log.sqlite` beside it), or the log file itself.
    #[arg(long)]
    save: PathBuf,
    #[command(subcommand)]
    query: Query,
}

#[derive(Subcommand)]
enum Query {
    /// The latest rejected or failed calls, newest first.
    RecentRejections {
        #[arg(long, default_value_t = 20)]
        limit: u32,
    },
    /// Calls, cost and rejections per model (the one that answered), rollups included.
    CostByModel(Dates),
    /// Calls, cost and rejections per purpose (banter, traffic, decision ...).
    CostByPurpose(Dates),
    /// Call count, total cost, tokens and rejections over a date range.
    Totals(Dates),
    /// The permanent daily rollups (date, model, purpose, count, cost, rejection rate).
    Rollups(Dates),
    /// Prunes now, as the game does on startup: rows past `log.keep_days` roll into the
    /// daily rollups, and the file is brought under `log.max_mb` (from `mind.toml`).
    Prune {
        /// The packs folder, for `mind.toml` (default: as for the other commands).
        #[arg(long)]
        packs: Option<PathBuf>,
    },
}

#[derive(Args)]
struct Dates {
    /// First day, inclusive (YYYY-MM-DD, UTC).
    #[arg(long)]
    since: Option<String>,
    /// Last day, inclusive (YYYY-MM-DD, UTC).
    #[arg(long)]
    until: Option<String>,
}

impl Dates {
    fn range(&self) -> Result<Range, (u8, String)> {
        let date = |d: &Option<String>| {
            d.as_deref()
                .map(Range::date)
                .transpose()
                .map_err(|e| (4, e))
        };
        Ok(Range {
            since: date(&self.since)?,
            until: date(&self.until)?,
        })
    }
}

/// The log for `save`: the file itself if it is a log, else the one beside it.
fn log_path(save: &Path) -> PathBuf {
    let name = save.file_name().and_then(|n| n.to_str()).unwrap_or("");
    if name.ends_with(".llm-log.sqlite") {
        save.to_path_buf()
    } else {
        calllog::path_for(save)
    }
}

pub fn run(args: LlmLogArgs) -> Result<Value, (u8, String)> {
    let path = log_path(&args.save);
    if !path.is_file() {
        return Err((
            4,
            format!(
                "no call log at {} (the game writes one beside the save once it calls OpenRouter)",
                path.display()
            ),
        ));
    }
    let runtime = |e: calllog::CallLogError| (1, e.to_string());
    let log = || CallLog::open_read(&path).map_err(runtime);
    let shown = path.display().to_string();
    match args.query {
        Query::RecentRejections { limit } => Ok(json!({
            "log": shown,
            "rejections": log()?.recent_rejections(limit).map_err(runtime)?,
        })),
        Query::CostByModel(d) => Ok(json!({
            "log": shown,
            "models": log()?.cost_by(Group::Model, &d.range()?).map_err(runtime)?,
        })),
        Query::CostByPurpose(d) => Ok(json!({
            "log": shown,
            "purposes": log()?.cost_by(Group::Purpose, &d.range()?).map_err(runtime)?,
        })),
        Query::Totals(d) => {
            let mut v = log()?.totals(&d.range()?).map_err(runtime)?;
            v["log"] = json!(shown);
            Ok(v)
        }
        Query::Rollups(d) => Ok(json!({
            "log": shown,
            "rollups": log()?.rollups(&d.range()?).map_err(runtime)?,
        })),
        Query::Prune { packs } => {
            let rules = match crate_packs(packs) {
                Some(dir) => {
                    MindConfig::load_dir(&dir.join("core").join("data"))
                        .map_err(|e| (4, e.to_string()))?
                        .log
                }
                None => LogRules::default(),
            };
            let mut log = CallLog::open(&path).map_err(runtime)?;
            let report = log.prune(calllog::now_ms(), &rules).map_err(runtime)?;
            Ok(json!({ "log": shown, "rules": rules, "pruned": report }))
        }
    }
}

fn crate_packs(flag: Option<PathBuf>) -> Option<PathBuf> {
    galaxy_app::game::packs_dir(flag)
}
