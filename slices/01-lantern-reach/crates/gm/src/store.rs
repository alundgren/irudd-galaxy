//! The save's game-master folder: `<save>.gm/` next to the save file.
//!
//! ```text
//! saves/lantern-1.sqlite
//! saves/lantern-1.gm/
//!     state.json                 staged, applied and quarantined packs (GmState)
//!     packs/<pack>/<revision>/   an exact copy of each pack revision ever staged
//!     live.json                  the live socket's discovery file, while the game runs
//!     inbox/                     inbox entries the game has claimed
//! saves/gm-inbox/                drop a pack folder here; a running game picks it up
//! ```
//!
//! Pack copies are content-addressed (the revision is the hash of the files), so a game
//! master can keep editing the folder it proposed from: what runs is what was checked.
//! While the game runs it is the only writer of `state.json`; the save-file commands
//! write it only when no game answers on the save.

use std::path::{Path, PathBuf};

use galaxy_contract::pack::ValidationReport;
use galaxy_contract::prelude::*;
use galaxy_script::Pack;
use serde::{Deserialize, Serialize};

pub const STATE_FILE: &str = "state.json";
pub const INBOX: &str = "gm-inbox";

/// Everything the bridge remembers about a save's game-master packs.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GmState {
    /// The last `stage.N` handed out.
    pub last_stage: u64,
    /// Validated and waiting for a calm moment, oldest first.
    pub staged: Vec<StagedPack>,
    /// Applied at a calm moment, oldest first. The newest entry per pack is the one
    /// that runs (unless it was rolled back).
    pub applied: Vec<AppliedPack>,
    /// Staged packs that failed their checks against the world at the calm moment.
    pub quarantined: Vec<QuarantinedPack>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StagedPack {
    /// `stage.N`.
    pub stage: String,
    pub pack: PackId,
    pub revision: String,
    pub name: String,
    /// The copy, relative to the game-master folder.
    pub dir: PathBuf,
    /// Where it came from: `cli` or `inbox:<folder>`.
    pub source: String,
    pub report: ValidationReport,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AppliedPack {
    pub stage: String,
    pub pack: PackId,
    pub revision: String,
    pub name: String,
    pub dir: PathBuf,
    pub source: String,
    /// The tick it was applied after.
    pub tick: Tick,
    /// The snapshot taken just before (`Save::load_snapshot` restores it).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snapshot: Option<i64>,
    /// The change ID: the cause of the batch that applied it. `None` until that batch
    /// commits (the first tick after the calm moment; after unpausing if paused).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub change: Option<CauseId>,
    pub report: ValidationReport,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct QuarantinedPack {
    pub stage: String,
    pub pack: PackId,
    pub revision: String,
    pub tick: Tick,
    pub reason: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub report: Option<ValidationReport>,
}

impl GmState {
    pub fn next_stage(&mut self) -> String {
        self.last_stage += 1;
        format!("stage.{}", self.last_stage)
    }

    /// The applied entry that runs for each pack: the newest one.
    pub fn current(&self) -> Vec<&AppliedPack> {
        let mut out: Vec<&AppliedPack> = Vec::new();
        for a in self.applied.iter().rev() {
            if !out.iter().any(|b| b.pack == a.pack) {
                out.push(a);
            }
        }
        out.reverse();
        out
    }

    /// The applied entry with this stage or change ID.
    pub fn find_applied(&self, change: &str) -> Option<&AppliedPack> {
        self.applied
            .iter()
            .find(|a| a.stage == change || a.change.is_some_and(|c| c.to_string() == change))
    }
}

/// A save's game-master folder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GmStore {
    save: PathBuf,
    dir: PathBuf,
}

impl GmStore {
    /// `<save stem>.gm` next to the save.
    pub fn for_save(save: &Path) -> Self {
        let stem = save
            .file_stem()
            .map_or_else(|| "save".into(), |s| s.to_string_lossy().into_owned());
        let parent = save.parent().unwrap_or(Path::new(""));
        Self {
            save: save.to_path_buf(),
            dir: parent.join(format!("{stem}.gm")),
        }
    }

    pub fn save(&self) -> &Path {
        &self.save
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// The `gm-inbox/` folder next to the save.
    pub fn inbox(&self) -> PathBuf {
        self.save.parent().unwrap_or(Path::new("")).join(INBOX)
    }

    /// Where the game moves inbox entries it claims.
    pub fn claimed(&self) -> PathBuf {
        self.dir.join("inbox")
    }

    pub fn load(&self) -> Result<GmState, String> {
        let path = self.dir.join(STATE_FILE);
        match std::fs::read_to_string(&path) {
            Ok(text) => serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(GmState::default()),
            Err(e) => Err(format!("{}: {e}", path.display())),
        }
    }

    /// Writes `state.json` (to a temp file, then renamed over the old one).
    pub fn write(&self, state: &GmState) -> Result<(), String> {
        std::fs::create_dir_all(&self.dir).map_err(|e| format!("{}: {e}", self.dir.display()))?;
        let path = self.dir.join(STATE_FILE);
        let tmp = self.dir.join("state.json.tmp");
        let text = serde_json::to_string_pretty(state).expect("gm state serializes");
        std::fs::write(&tmp, text).map_err(|e| format!("{}: {e}", tmp.display()))?;
        std::fs::rename(&tmp, &path).map_err(|e| format!("{}: {e}", path.display()))
    }

    /// The copy's folder for a revision, relative to the game-master folder.
    pub fn rel_dir(pack: &PackId, revision: &str) -> PathBuf {
        PathBuf::from("packs").join(pack.as_str()).join(revision)
    }

    pub fn abs(&self, rel: &Path) -> PathBuf {
        self.dir.join(rel)
    }

    /// Copies a pack's files into the store (once per revision). Returns the copy's
    /// relative folder. The copy has the same revision: it holds exactly the files the
    /// revision hashes.
    pub fn import(&self, pack: &Pack) -> Result<PathBuf, String> {
        let rel = Self::rel_dir(&pack.manifest.id, &pack.revision);
        let dest = self.abs(&rel);
        if dest.join("manifest.toml").is_file() {
            return Ok(rel);
        }
        let tmp = dest.with_extension("tmp");
        let _ = std::fs::remove_dir_all(&tmp);
        for (file, text) in &pack.files {
            let to = tmp.join(file);
            if let Some(parent) = to.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| format!("{}: {e}", parent.display()))?;
            }
            std::fs::write(&to, text).map_err(|e| format!("{}: {e}", to.display()))?;
        }
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
        }
        match std::fs::rename(&tmp, &dest) {
            Ok(()) => Ok(rel),
            // Someone else stored the same revision meanwhile.
            Err(_) if dest.join("manifest.toml").is_file() => {
                let _ = std::fs::remove_dir_all(&tmp);
                Ok(rel)
            }
            Err(e) => Err(format!("{}: {e}", dest.display())),
        }
    }

    /// Reads a stored copy back and checks it is still the revision it claims.
    pub fn read_copy(&self, rel: &Path, revision: &str) -> Result<Pack, String> {
        let pack = galaxy_script::read_pack(&self.abs(rel)).map_err(|d| {
            d.first()
                .map_or_else(|| "unreadable pack copy".to_owned(), |d| d.message.clone())
        })?;
        if pack.revision != revision {
            return Err(format!(
                "the stored copy in {} is revision {}, not {revision}",
                rel.display(),
                pack.revision
            ));
        }
        Ok(pack)
    }
}
