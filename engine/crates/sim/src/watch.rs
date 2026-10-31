//! Hot-loading from disk: watch module folders and report which modules changed. Carried
//! over from the slice; the session reloads changed modules between ticks.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, channel};

use notify::{RecommendedWatcher, RecursiveMode, Watcher};

/// Watches module folders. [`ModuleWatcher::changed`] drains what happened since the last
/// call; the host reloads those modules between ticks.
pub struct ModuleWatcher {
    _watcher: RecommendedWatcher,
    rx: Receiver<notify::Result<notify::Event>>,
    roots: Vec<(String, PathBuf)>,
}

impl ModuleWatcher {
    pub fn new(roots: Vec<(String, PathBuf)>) -> notify::Result<Self> {
        let (tx, rx) = channel();
        let mut watcher = notify::recommended_watcher(move |e| {
            let _ = tx.send(e);
        })?;
        let mut known = Vec::new();
        for (id, root) in roots {
            watcher.watch(&root, RecursiveMode::Recursive)?;
            if let Ok(canon) = root.canonicalize() {
                known.push((id.clone(), canon));
            }
            known.push((id, root));
        }
        Ok(Self {
            _watcher: watcher,
            rx,
            roots: known,
        })
    }

    /// Modules with a created, changed or removed file since the last call.
    pub fn changed(&self) -> BTreeSet<String> {
        let mut out = BTreeSet::new();
        while let Ok(event) = self.rx.try_recv() {
            let Ok(event) = event else { continue };
            if event.kind.is_access() {
                continue;
            }
            for path in &event.paths {
                for (id, root) in &self.roots {
                    if path.starts_with(root) {
                        out.insert(id.clone());
                    }
                }
            }
        }
        out
    }
}
