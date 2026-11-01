//! `galaxy-gm check` and `types` through the real executable.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

fn engine() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn gm(cwd: &Path, args: &[&str]) -> (i32, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_engine-galaxy-gm"))
        .args(args)
        .current_dir(cwd)
        .env_remove("GALAXY_INSTALL_ROOT")
        .output()
        .unwrap();
    (
        out.status.code().unwrap(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
    )
}

fn json(text: &str) -> Value {
    serde_json::from_str(text).unwrap_or(Value::Null)
}

#[test]
fn the_test_modules_pass_check() {
    for module in [
        "tests/modules/stroll",
        "tests/modules/tally",
        "tests/play/drift",
        "tests/play/screens",
        "tests/play/rich-screens/rich",
    ] {
        let (code, out) = gm(&engine(), &["check", module]);
        let out = json(&out);
        assert_eq!(code, 0, "{out}");
        assert!(module.ends_with(out["module"].as_str().unwrap()), "{out}");
    }
    let (_, out) = gm(&engine(), &["check", "tests/modules/stroll"]);
    assert_eq!(json(&out)["context_modules"], serde_json::json!(["tally"]));
}

#[test]
fn check_reports_unused_missing_glyphs_missing_icons_and_unreadable_grids() {
    for (catalogue, widget, expected) in [
        (
            "[icons]\nunused='\u{10ffff}'",
            "ui.text{text='Readable'}",
            "font cannot draw",
        ),
        (
            "[icons]\nunused=42",
            "ui.text{text='Readable'}",
            "glyph string",
        ),
        ("[icons]", "ui.icon{icon='absent'}", "missing module icon"),
        (
            "[icons]",
            "ui.grid{width=10,columns=2,children={ui.text{text='Wide'},ui.text{text='W'}}}",
            "clipped",
        ),
        (
            "[icons]",
            "ui.drop_target{id='target',on='drop',text='',accepts={small=''}}",
            "empty text",
        ),
    ] {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("example");
        std::fs::create_dir(&dir).unwrap();
        std::fs::write(dir.join("manifest.toml"), "id='example'\nversion='0.1.0'\nmin_engine_version='0.3.0'\nscripts=['screen.luau']\nevents=['drop']").unwrap();
        std::fs::write(dir.join("icons.toml"), catalogue).unwrap();
        std::fs::write(
            dir.join("screen.luau"),
            format!("return {{screen=function(_,ui) return {widget} end}}"),
        )
        .unwrap();
        let (code, out) = gm(tmp.path(), &["check", "example"]);
        assert_eq!(code, 5, "{out}");
        assert!(
            messages(&json(&out)).iter().any(|m| m.contains(expected)),
            "{out}"
        );
    }
}

/// A module folder `dir/<id>` with these files beside its manifest.
fn write_module(dir: &Path, id: &str, writes: &str, files: &[(&str, &str)]) {
    let root = dir.join(id);
    std::fs::create_dir_all(&root).unwrap();
    let manifest = format!(
        "id = \"{id}\"\nversion = \"0.1.0\"\nmin_engine_version = \"{}\"\nwrites = [{writes}]\n",
        env!("CARGO_PKG_VERSION")
    );
    std::fs::write(root.join("manifest.toml"), manifest).unwrap();
    for (path, text) in files {
        let path = root.join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
}

fn messages(out: &Value) -> Vec<String> {
    out["findings"]
        .as_array()
        .map(|list| {
            list.iter()
                .map(|f| f["message"].as_str().unwrap().to_owned())
                .collect()
        })
        .unwrap_or_default()
}

#[test]
fn missing_models_and_looks_and_broken_models_are_findings() {
    let tmp = tempfile::tempdir().unwrap();
    write_module(
        tmp.path(),
        "shapes",
        r#""engine.position", "engine.model""#,
        &[
            (
                "start.toml",
                "[entities.a]\n\"engine.position\" = {}\n\
                 \"engine.model\" = { model = \"shapes.cube\", look = \"shapes.shiny\" }\n",
            ),
            ("models/broken.gltf", "{"),
            ("style.toml", "[looks.dull]\nhatch = 0.5\n"),
        ],
    );
    let (code, out) = gm(tmp.path(), &["check", "shapes"]);
    let out = json(&out);
    assert_eq!(code, 5, "{out}");
    let messages = messages(&out);
    assert_eq!(messages.len(), 3, "{messages:?}");
    assert!(messages[0].contains("models/broken.gltf"), "{messages:?}");
    assert!(
        messages[1].contains("the model `shapes.cube`"),
        "{messages:?}"
    );
    assert!(
        messages[2].contains("the look `shapes.shiny`"),
        "{messages:?}"
    );
    assert!(messages.iter().all(|m| !m.contains("  ")), "{messages:?}");
}

#[test]
fn a_shared_style_conflict_is_found_on_the_modules_in_it_only() {
    let tmp = tempfile::tempdir().unwrap();
    write_module(tmp.path(), "aa", "", &[]);
    write_module(
        tmp.path(),
        "aa-one",
        "",
        &[("style.toml", "[sky]\nstars = 1\n")],
    );
    write_module(
        tmp.path(),
        "aa-two",
        "",
        &[("style.toml", "[sky]\nstars = 2\n")],
    );
    let (code, out) = gm(tmp.path(), &["check", "aa"]);
    assert_eq!(code, 0, "aa sets no sky: {out}");
    for id in ["aa-two", "aa-one"] {
        let (code, out) = gm(tmp.path(), &["check", id]);
        let out = json(&out);
        assert_eq!(code, 5, "{out}");
        assert_eq!(
            messages(&out),
            [
                "[sky] is set by aa-one, aa-two; one module sets the whole scene's sky, and the \
              last one (aa-two) is used"
            ],
        );
    }
}

#[test]
fn a_module_with_problems_is_rejected_with_findings() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("broken");
    std::fs::create_dir_all(dir.join("scripts")).unwrap();
    std::fs::write(
        dir.join("manifest.toml"),
        format!(
            "id = \"broken\"\nversion = \"0.1.0\"\nmin_engine_version = \"{}\"\n\
             scripts = [\"scripts/main.luau\"]\n",
            env!("CARGO_PKG_VERSION")
        ),
    )
    .unwrap();
    std::fs::write(
        dir.join("scripts/main.luau"),
        "return { on = { nothing = function() end } }",
    )
    .unwrap();
    let (code, out) = gm(tmp.path(), &["check", "broken"]);
    let out = json(&out);
    assert_eq!((code, out["kind"].as_str()), (5, Some("findings")), "{out}");
    assert_eq!(out["findings"][0]["kind"], "script");
}

#[test]
fn types_print() {
    let (code, types) = gm(&engine(), &["types"]);
    assert_eq!(code, 0);
    assert!(types.contains("export type Ctx"), "{types}");
}

#[test]
fn screen_check_is_headless_and_reports_widget_paths() {
    let (code, out) = gm(&engine(), &["check", "tests/play/screens-unreadable"]);
    assert_eq!(code, 5, "{out}");
    let report = json(&out);
    let findings = report["findings"].as_array().unwrap();
    assert!(
        findings
            .iter()
            .any(|p| p["kind"] == "screen" && p["message"].as_str().unwrap().contains("/tiny[1]"))
    );
    assert!(
        findings
            .iter()
            .any(|p| p["message"].as_str().unwrap().contains("/outside[2]"))
    );
}
