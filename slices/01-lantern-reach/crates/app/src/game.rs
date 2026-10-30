//! The game's composition: which systems run, in which order, and where the packs are.

use std::path::{Path, PathBuf};

use galaxy_core::{Sim, SimState, System};

/// The game's systems, in tick order: the kernel's, the ship's (buy and sell orders go
/// through the economy's desk), then the economy's (pulses, scouting, re-pricing and
/// the player's dock-screen trades), which reads the dockings the ships made, then
/// progress (station services, faction deeds, pilot traits and hull ranks).
pub fn game(state: SimState) -> Sim {
    galaxy_core::game(state)
        .with_systems(galaxy_ship::systems(Box::new(galaxy_ship::EconomyDesk)))
        .with_systems([
            Box::new(galaxy_economy::EconomySystem) as Box<dyn System>,
            Box::new(galaxy_progress::ProgressSystem),
        ])
}

/// Finds the packs folder: the flag, `GALAXY_PACKS`, or the nearest `packs/core` above
/// the working directory or the executable.
pub fn packs_dir(flag: Option<PathBuf>) -> Option<PathBuf> {
    if let Some(p) = flag.or_else(|| std::env::var_os("GALAXY_PACKS").map(PathBuf::from)) {
        return Some(p);
    }
    let starts = [
        std::env::current_dir().ok(),
        std::env::current_exe()
            .ok()
            .and_then(|e| e.parent().map(Path::to_path_buf)),
    ];
    for start in starts.into_iter().flatten() {
        for dir in start.ancestors() {
            let packs = dir.join("packs");
            if packs.join("core").is_dir() {
                return Some(packs);
            }
        }
    }
    None
}

/// The core pack's data folder inside `packs`.
pub fn core_data(packs: &Path) -> PathBuf {
    packs.join("core").join("data")
}
