//! Per-note receipts beside the recording, independent of installed engine builds.

use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};

use galaxy_module::Failure;
use galaxy_sim::recording::RecEvent;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub(super) struct Handled {
    format: u32,
    event: RecEvent,
    pub reason: String,
    pub change: Option<String>,
}

fn path(recording: &Path, note: usize) -> PathBuf {
    let mut name = recording
        .file_name()
        .expect("a recording file")
        .to_os_string();
    name.push(".handled");
    recording
        .with_file_name(name)
        .join(format!("{note:04}.json"))
}

pub(super) fn read(
    recording: &Path,
    note: usize,
    event: &RecEvent,
) -> Result<Option<Handled>, Failure> {
    let path = path(recording, note);
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(None),
        Err(e) => {
            return Err(Failure::runtime(format!(
                "can't read {}: {e}",
                path.display()
            )));
        }
    };
    let mark: Handled = serde_json::from_slice(&bytes)
        .map_err(|e| Failure::bad_input(format!("invalid handled mark {}: {e}", path.display())))?;
    if mark.format != 1 || mark.reason.trim().is_empty() {
        return Err(Failure::bad_input(format!(
            "unsupported or invalid handled mark {}",
            path.display()
        )));
    }
    // A recording can be replaced at the same path. Never hide a different note.
    Ok((mark.event == *event).then_some(mark))
}

pub(super) fn write(
    recording: &Path,
    note: usize,
    event: &RecEvent,
    reason: &str,
    change: Option<&str>,
) -> Result<PathBuf, Failure> {
    let path = path(recording, note);
    let mark = Handled {
        format: 1,
        event: event.clone(),
        reason: reason.to_owned(),
        change: change.map(str::to_owned),
    };
    let bytes = serde_json::to_vec_pretty(&mark).map_err(|e| Failure::runtime(e.to_string()))?;
    let save = || -> std::io::Result<()> {
        let dir = path.parent().expect("a mark has a parent");
        std::fs::create_dir_all(dir)?;
        let mut tmp = tempfile::NamedTempFile::new_in(dir)?;
        tmp.write_all(&bytes)?;
        tmp.as_file().sync_all()?;
        tmp.persist(&path).map_err(|e| e.error)?;
        Ok(())
    };
    save().map_err(|e| Failure::runtime(format!("can't write {}: {e}", path.display())))?;
    Ok(path)
}

pub(super) fn remove(recording: &Path, note: usize) -> Result<(), Failure> {
    let path = path(recording, note);
    match std::fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(()),
        Err(e) => Err(Failure::runtime(format!(
            "can't remove {}: {e}",
            path.display()
        ))),
    }
}
