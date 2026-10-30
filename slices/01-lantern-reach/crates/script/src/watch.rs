//! Dev hot reload: watch pack folders, report which packs changed.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, channel};

use galaxy_contract::ids::PackId;
use notify::{RecommendedWatcher, RecursiveMode, Watcher};

/// Watches pack folders. [`PackWatcher::changed`] drains what happened since the last
/// call; the host reloads those packs between ticks.
pub struct PackWatcher {
    _watcher: RecommendedWatcher,
    rx: Receiver<notify::Result<notify::Event>>,
    roots: Vec<(PackId, PathBuf)>,
}

impl PackWatcher {
    pub fn new(roots: Vec<(PackId, PathBuf)>) -> notify::Result<Self> {
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

    /// Reports changes under `from`'s folders as `to`'s from now on: a reload found the
    /// folder's manifest naming a new ID.
    pub fn rename(&mut self, from: &PackId, to: &PackId) {
        for (id, _) in &mut self.roots {
            if id == from {
                *id = to.clone();
            }
        }
    }

    /// Packs with a created, changed or removed file since the last call.
    pub fn changed(&self) -> BTreeSet<PackId> {
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
