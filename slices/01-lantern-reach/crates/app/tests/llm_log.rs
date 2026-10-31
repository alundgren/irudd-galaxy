//! `galaxy llm-log` on a seeded call log: old rows prune into daily rollups, and the
//! queries read rows and rollups together. Offline; the log is written directly.

use std::path::{Path, PathBuf};
use std::process::Command;

use galaxy_mind::calllog::{self, CallId, CallLog, CallRow, Verdict};
use serde_json::Value;

const DAY_MS: i64 = 86_400_000;

fn packs() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packs")
}

fn galaxy(args: &[&str]) -> (i32, Value) {
    let out = Command::new(env!("CARGO_BIN_EXE_galaxy"))
        .args(args)
        .env("GALAXY_PACKS", packs())
        .output()
        .expect("galaxy runs");
    let stdout = String::from_utf8(out.stdout).expect("utf-8");
    let json = serde_json::from_str(&stdout).unwrap_or_else(|e| panic!("{e}: {stdout}"));
    (out.status.code().unwrap_or(-1), json)
}

fn row(i: i64, age_days: i64, purpose: &str, model: &str, verdict: Verdict) -> CallRow {
    CallRow {
        uid: CallId(format!("seed-{i}")),
        ts_ms: calllog::now_ms() - age_days * DAY_MS,
        purpose: purpose.into(),
        requested_model: "openrouter/auto".into(),
        actual_model: Some(model.into()),
        task_type: Some("roleplay_fiction".into()),
        session_id: Some("station.lantern_wharf".into()),
        generation_id: None,
        prompt_tokens: Some(600),
        completion_tokens: Some(40),
        cost_usd: Some(0.0001),
        cost_source: Some("usage".into()),
        latency_ms: 2500,
        attempts: 1,
        http_status: Some(200),
        error_kind: None,
        verdict,
        reason: (verdict == Verdict::Rejected).then(|| "rules: not in the first person".into()),
        prompt: "{}".into(),
        output: "{}".into(),
    }
}

#[test]
fn llm_log_prunes_a_seeded_old_log_and_answers_queries() {
    let dir = std::env::temp_dir().join(format!("galaxy-llm-log-cli-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let save = dir.join("run.sqlite");
    let path = calllog::path_for(&save);
    {
        let log = CallLog::open(&path).unwrap();
        // Twenty days of calls: one a day, every fourth rejected, two models.
        for i in 0..20 {
            let verdict = if i % 4 == 0 {
                Verdict::Rejected
            } else {
                Verdict::Accepted
            };
            let (purpose, model) = if i % 2 == 0 {
                ("banter", "deepseek/deepseek-v4-flash-0731")
            } else {
                ("traffic", "z-ai/glm-5.3-flash")
            };
            log.insert(&row(i, i, purpose, model, verdict)).unwrap();
        }
    }
    let s = save.to_str().unwrap();

    let (code, totals) = galaxy(&["llm-log", "--save", s, "totals"]);
    assert_eq!(code, 0, "{totals}");
    assert_eq!(totals["calls"], 20);
    assert_eq!(totals["rolled_up"], 0);

    let (code, pruned) = galaxy(&["llm-log", "--save", s, "prune"]);
    assert_eq!(code, 0, "{pruned}");
    assert_eq!(pruned["rules"]["keep_days"], 14);
    // Rows 15 to 19 days old roll up (14 days and a few ms is past the cut too).
    let expired = pruned["pruned"]["expired"].as_u64().unwrap();
    assert!((5..=6).contains(&expired), "{pruned}");

    let (_, totals) = galaxy(&["llm-log", "--save", s, "totals"]);
    assert_eq!(totals["calls"], 20, "rollups keep the count");
    assert_eq!(totals["rolled_up"].as_u64().unwrap(), expired);
    assert_eq!(totals["rejected"], 5);
    assert!((totals["cost_usd"].as_f64().unwrap() - 0.002).abs() < 1e-9);

    let (_, rollups) = galaxy(&["llm-log", "--save", s, "rollups"]);
    let rollups = rollups["rollups"].as_array().unwrap();
    assert_eq!(rollups.len() as u64, expired, "one per day here");
    assert!(rollups[0]["day"].as_str().unwrap().len() == 10);

    let (_, by_model) = galaxy(&["llm-log", "--save", s, "cost-by-model"]);
    let models = by_model["models"].as_array().unwrap();
    assert_eq!(models.len(), 2);
    assert_eq!(
        models
            .iter()
            .map(|m| m["calls"].as_i64().unwrap())
            .sum::<i64>(),
        20
    );

    let (_, by_purpose) = galaxy(&["llm-log", "--save", s, "cost-by-purpose"]);
    let banter = by_purpose["purposes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["purpose"] == "banter")
        .unwrap()
        .clone();
    assert_eq!(banter["calls"], 10);
    assert!((banter["rejection_rate"].as_f64().unwrap() - 0.5).abs() < 1e-9);

    let (_, recent) = galaxy(&[
        "llm-log",
        "--save",
        path.to_str().unwrap(),
        "recent-rejections",
        "--limit",
        "2",
    ]);
    let recent = recent["rejections"].as_array().unwrap();
    assert_eq!(recent.len(), 2);
    assert_eq!(recent[0]["uid"], "seed-0", "newest first");
    assert_eq!(recent[0]["reason"], "rules: not in the first person");

    let (code, bad) = galaxy(&["llm-log", "--save", s, "totals", "--since", "yesterday"]);
    assert_eq!(code, 4, "{bad}");
    let (code, missing) = galaxy(&[
        "llm-log",
        "--save",
        dir.join("none.sqlite").to_str().unwrap(),
        "totals",
    ]);
    assert_eq!(code, 4, "{missing}");
    let _ = std::fs::remove_dir_all(&dir);
}
