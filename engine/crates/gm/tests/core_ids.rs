//! The core-ID scan: no ID the core module declares appears in engine or GM source.
//!
//! It reads the IDs `game/modules/core` declares through the module loader and looks for
//! each, full and local, as a whole word in the engine's crates, its test modules and
//! xtask (`galaxy_module::idscan`). The module's own name is not looked for. Without a
//! `game/` folder next to `engine/` (an engine-only checkout) it says it skipped, on
//! stderr past the test harness's capture, so the skip is visible.

use std::io::Write as _;
use std::path::Path;

use galaxy_module::idscan;
use galaxy_module::module::Module;

#[test]
fn no_core_id_appears_in_engine_or_gm_source() {
    let engine = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let core = engine.join("../game/modules/core");
    let mut stderr = std::io::stderr();
    if !core.join("manifest.toml").is_file() {
        let _ = writeln!(
            stderr,
            "core-ID scan: SKIPPED: there is no game/modules/core next to engine/"
        );
        return;
    }
    let module = Module::read(&core).expect("the core module loads");
    let declared = module.declared_ids();
    let dirs = ["crates", "tests", "xtask"].map(|d| engine.join(d));
    let hits = idscan::scan(&dirs, module.id(), &declared);
    let listed: Vec<String> = hits
        .iter()
        .map(|h| {
            let file = h.file.strip_prefix(&engine).unwrap_or(&h.file);
            format!(
                "{}:{}: `{}` ({} {})",
                file.display(),
                h.line,
                h.matched,
                h.id.what,
                h.id.full
            )
        })
        .collect();
    assert!(
        listed.is_empty(),
        "IDs the core module declares appear in engine or GM source. The engine knows no \
         game nouns: reword the engine source, or raise it with the owner if the engine \
         needs the word.\n{}",
        listed.join("\n")
    );
    let _ = writeln!(
        stderr,
        "core-ID scan: {} declared IDs of `{}` checked, none in engine or GM source",
        declared.len(),
        module.id()
    );
}
