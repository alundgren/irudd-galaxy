//! ColorVideoVDP, run outside the engine: a model of how the eye sees differences in
//! video, over space and time (Mantiuk et al., University of Cambridge). It needs Python
//! and PyTorch, so it is not part of the engine: `compare --vdp` writes a small script
//! (`cvvdp.py`, carried inside the binary) to the system's temp folder and runs it with
//! `uv run --script`, which fetches what it needs on first use. Without `uv` the report
//! says how to get it, and the rest of `compare` stands.
//!
//! It scores the test film against the reference in JOD (10 is no visible difference;
//! each point down is a step most viewers would notice) and writes its own distogram.
//! Like FLIP it finds differences, not mistakes.

use std::process::Command;

use serde_json::{Value, json};

use crate::film::Film;

const SCRIPT: &str = include_str!("cvvdp.py");

/// Scores `test` against `reference` (as many frames as both have); the report, or why
/// not. Its files go beside the reference's other reports.
pub fn run(reference: &Film, test: &Film) -> Value {
    match score(reference, test) {
        Ok(v) => v,
        Err(e) => json!({ "skipped": e }),
    }
}

fn score(reference: &Film, test: &Film) -> Result<Value, String> {
    if Command::new("uv").arg("--version").output().is_err() {
        return Err(
            "ColorVideoVDP needs uv (https://docs.astral.sh/uv/) to fetch Python and PyTorch; \
             install it and run again"
                .into(),
        );
    }
    let dir = std::env::temp_dir().join("galaxy-motion");
    std::fs::create_dir_all(&dir).map_err(|e| format!("can't make {}: {e}", dir.display()))?;
    let script = dir.join("cvvdp.py");
    std::fs::write(&script, SCRIPT)
        .map_err(|e| format!("can't write {}: {e}", script.display()))?;
    let reports = reference.dir.join("motion");
    std::fs::create_dir_all(&reports)
        .map_err(|e| format!("can't make {}: {e}", reports.display()))?;
    let out_json = reports.join("cvvdp.json");
    let distogram = reports.join("cvvdp-distogram.png");
    let half = if test.size().0 > 1280 { "1" } else { "0" };
    let output = Command::new("uv")
        .args(["run", "--quiet", "--script"])
        .arg(&script)
        .arg(reference.dir.join("frames"))
        .arg(test.dir.join("frames"))
        .arg(test.manifest.fps.to_string())
        .arg(half)
        .arg(&out_json)
        .arg(&distogram)
        .output()
        .map_err(|e| format!("can't run uv: {e}"))?;
    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        let tail: Vec<&str> = err.lines().rev().take(6).collect();
        return Err(format!(
            "ColorVideoVDP failed: {}",
            tail.into_iter().rev().collect::<Vec<_>>().join(" | ")
        ));
    }
    let text = std::fs::read_to_string(&out_json)
        .map_err(|e| format!("no result from ColorVideoVDP: {e}"))?;
    let mut v: Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    // Frames by their film moment, as the other tools report them.
    if let Some(list) = v["worst_frames"].as_array_mut() {
        for item in list {
            if let Some(i) = item["frame"]
                .as_u64()
                .and_then(|i| test.frames().get(i as usize))
            {
                item["t"] = json!(crate::film::round(i.time, 3));
                item["tick"] = json!(i.tick);
            }
        }
    }
    v["reading"] = json!(format!(
        "ColorVideoVDP gives {} JOD (10 is no visible difference)",
        v["jod"]
    ));
    Ok(v)
}
