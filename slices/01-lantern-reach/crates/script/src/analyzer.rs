//! The strict typecheck: `luau-analyze` from the pinned Luau release.
//!
//! The analyzer has no definitions-file flag, but it resolves `require` aliases from
//! `.luaurc`. So the check stages the pack's Luau files in a temporary folder next to
//! a `galaxy.d.luau` generated for this pack's `uses` (undeclared namespaces are not
//! in its `Ctx` type) and a `.luaurc` that maps `@galaxy` to it, then runs
//! `luau-analyze --mode=strict --formatter=gnu` and maps the output back to pack paths.
//!
//! `cargo xtask fetch-luau` puts the binary in `tools/luau/` (git-ignored).

use std::path::{Path, PathBuf};
use std::process::Command;

use galaxy_contract::api;
use galaxy_contract::pack::{Diagnostic, Location, Severity};

use crate::pack::Pack;

/// The pinned Luau release (matches the VM mlua builds: `luau0-src 0.22.0+luau740`).
pub const LUAU_VERSION: &str = "0.740";

/// The analyzer's file name on this OS.
pub fn binary_name() -> &'static str {
    if cfg!(windows) {
        "luau-analyze.exe"
    } else {
        "luau-analyze"
    }
}

/// Finds `luau-analyze`: an explicit path, then `$GALAXY_LUAU_ANALYZE`, then
/// `tools/luau/` in the nearest folder above the working directory or the executable.
pub fn find(explicit: Option<&Path>) -> Option<PathBuf> {
    if let Some(p) = explicit {
        return p.is_file().then(|| p.to_path_buf());
    }
    if let Some(p) = std::env::var_os("GALAXY_LUAU_ANALYZE").map(PathBuf::from) {
        return p.is_file().then_some(p);
    }
    let starts = [
        std::env::current_dir().ok(),
        std::env::current_exe()
            .ok()
            .and_then(|e| e.parent().map(Path::to_path_buf)),
    ];
    for start in starts.into_iter().flatten() {
        for dir in start.ancestors() {
            let candidate = dir.join("tools").join("luau").join(binary_name());
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

/// Typechecks the pack's scripts and modules in strict mode.
pub fn typecheck(pack: &Pack, analyzer: &Path) -> Result<Vec<Diagnostic>, String> {
    let dir = tempfile::Builder::new()
        .prefix("galaxy-typecheck-")
        .tempdir()
        .map_err(|e| format!("can't make a temp folder: {e}"))?;
    let root = dir.path();
    std::fs::write(
        root.join("galaxy.d.luau"),
        api::luau_module(Some(&pack.manifest.uses)),
    )
    .map_err(|e| e.to_string())?;
    std::fs::write(
        root.join(".luaurc"),
        r#"{ "languageMode": "strict", "aliases": { "galaxy": "./galaxy.d" } }"#,
    )
    .map_err(|e| e.to_string())?;
    let mut args = vec!["--mode=strict".to_owned(), "--formatter=gnu".to_owned()];
    for (rel, src) in pack.script_files() {
        let path = root.join("pack").join(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        std::fs::write(&path, src).map_err(|e| e.to_string())?;
        args.push(format!("pack/{rel}"));
    }
    if args.len() == 2 {
        return Ok(Vec::new());
    }
    let out = Command::new(analyzer)
        .args(&args)
        .current_dir(root)
        .output()
        .map_err(|e| format!("can't run {}: {e}", analyzer.display()))?;
    let text =
        String::from_utf8_lossy(&out.stdout).into_owned() + &String::from_utf8_lossy(&out.stderr);
    Ok(parse(&text))
}

/// Parses `path:line.col-line.col: Kind: message` lines.
pub fn parse(text: &str) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        let Some((loc, rest)) = split_gnu(line) else {
            out.push(Diagnostic {
                severity: Severity::Error,
                code: "type_error".into(),
                message: line.to_owned(),
                location: None,
            });
            continue;
        };
        let (kind, message) = rest.split_once(": ").unwrap_or(("Error", rest));
        let (severity, code) = match kind {
            "TypeError" => (Severity::Error, "type_error"),
            "SyntaxError" => (Severity::Error, "syntax_error"),
            _ => (Severity::Warning, "lint"),
        };
        let message = if code == "lint" {
            format!("{kind}: {message}")
        } else {
            message.to_owned()
        };
        out.push(Diagnostic {
            severity,
            code: code.into(),
            message,
            location: Some(loc),
        });
    }
    out
}

fn split_gnu(line: &str) -> Option<(Location, &str)> {
    // The path may contain `:` (a Windows drive), so find `.luau:` first.
    let end = line.find(".luau:")? + ".luau".len();
    let path = line[..end].replace('\\', "/");
    let rest = &line[end + 1..];
    let (pos, msg) = rest.split_once(": ")?;
    let start = pos.split('-').next()?;
    let (l, c) = start.split_once('.')?;
    let file = match path.rfind("pack/") {
        Some(i) => path[i + 5..].to_owned(),
        None => path
            .trim_start_matches("./")
            .rsplit('/')
            .next()
            .unwrap_or(&path)
            .to_owned(),
    };
    Some((
        Location {
            file,
            line: l.parse().ok(),
            column: c.parse().ok(),
        },
        msg,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_gnu_output() {
        let d = parse(
            "./pack/scripts/a.luau:9.22-9.55: TypeError: Table type '{ text: string }' not compatible\n\
             C:/tmp/x/pack/scripts/a.luau:10.13-10.13: LocalUnused: Variable 't' is never used\n",
        );
        assert_eq!(d.len(), 2);
        assert_eq!(d[0].code, "type_error");
        let loc = d[0].location.as_ref().unwrap();
        assert_eq!(
            (loc.file.as_str(), loc.line, loc.column),
            ("scripts/a.luau", Some(9), Some(22))
        );
        assert_eq!(d[1].severity, Severity::Warning);
        assert_eq!(d[1].location.as_ref().unwrap().file, "scripts/a.luau");
    }
}
